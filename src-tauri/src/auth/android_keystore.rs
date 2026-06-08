//! Android credential backend.
//!
//! The `keyring` crate has no Android provider, so on Android we store secrets
//! in an AES-256-GCM encrypted file inside the app-private data directory. The
//! 256-bit data-encryption key (DEK) is itself wrapped (envelope encryption) by
//! a key held in the hardware-backed **Android Keystore** (`AndroidKeyStore`),
//! reached over JNI. The wrapped DEK never leaves disk in the clear and the
//! key-encryption key (KEK) never leaves the TEE.
//!
//! Layout under `<app_data_dir>/secrets/`:
//!   - `dek.bin`         — the wrapped (or, in the fallback, raw) DEK
//!   - `credentials.enc` — AES-256-GCM(`{account: secret}` JSON) under the DEK
//!
//! If the Keystore JNI path is unavailable (e.g. an emulator image without a
//! keystore, or an unexpected JNI error) we fall back to storing the DEK as a
//! raw file in the same app-private directory and log a prominent warning. That
//! still keeps the credentials file encrypted at rest and the directory is
//! OS-sandboxed to this app, but the DEK is no longer hardware-protected.

use crate::AppError;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// App-private base directory for secret storage, set once at startup from the
/// Tauri-resolved app data dir (see `lib.rs::run`).
static STORE_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Serializes all read-modify-write access to the on-disk credentials map.
static FILE_LOCK: Mutex<()> = Mutex::new(());
/// In-memory cache of the unwrapped DEK, so we touch the Keystore at most once.
static DEK: OnceLock<[u8; 32]> = OnceLock::new();

const KEK_ALIAS: &str = "pex_dek_kek";
const DEK_VERSION_KEYSTORE: u8 = 1;
const DEK_VERSION_RAW: u8 = 0;

/// Initialize the secret store directory. Called once during app setup.
pub fn init(app_data_dir: PathBuf) {
    let dir = app_data_dir.join("secrets");
    let _ = std::fs::create_dir_all(&dir);
    let _ = STORE_DIR.set(dir);
}

fn store_dir() -> Result<&'static PathBuf, AppError> {
    STORE_DIR
        .get()
        .ok_or_else(|| AppError::Auth("Android secret store not initialized".into()))
}

// ---------------------------------------------------------------------------
// Public backend primitives (mirror the keyring backend in keyring_store.rs)
// ---------------------------------------------------------------------------

pub fn get(account: &str) -> Result<Option<String>, AppError> {
    let _guard = FILE_LOCK.lock().unwrap();
    let map = read_map()?;
    Ok(map.get(account).cloned())
}

pub fn set(account: &str, secret: &str) -> Result<(), AppError> {
    let _guard = FILE_LOCK.lock().unwrap();
    let mut map = read_map()?;
    map.insert(account.to_string(), secret.to_string());
    write_map(&map)
}

pub fn delete(account: &str) -> Result<(), AppError> {
    let _guard = FILE_LOCK.lock().unwrap();
    let mut map = read_map()?;
    if map.remove(account).is_some() {
        write_map(&map)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Encrypted credentials file (AES-256-GCM under the DEK)
// ---------------------------------------------------------------------------

fn read_map() -> Result<BTreeMap<String, String>, AppError> {
    let path = store_dir()?.join("credentials.enc");
    let blob = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(AppError::Auth(format!("read credentials: {e}"))),
    };
    if blob.len() < 12 {
        return Ok(BTreeMap::new());
    }
    let (nonce, ct) = blob.split_at(12);
    let plaintext = aes_decrypt(nonce, ct)?;
    serde_json::from_slice(&plaintext)
        .map_err(|e| AppError::Auth(format!("Invalid credentials store: {e}")))
}

fn write_map(map: &BTreeMap<String, String>) -> Result<(), AppError> {
    let plaintext =
        serde_json::to_vec(map).map_err(|e| AppError::Auth(format!("serialize secrets: {e}")))?;
    let mut nonce_bytes = [0u8; 12];
    fill_random(&mut nonce_bytes)?;
    let ct = aes_encrypt(&nonce_bytes, &plaintext)?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);

    // Write to a temp file then rename for crash-atomicity.
    let dir = store_dir()?;
    let tmp = dir.join("credentials.enc.tmp");
    let final_path = dir.join("credentials.enc");
    std::fs::write(&tmp, &out).map_err(|e| AppError::Auth(format!("write credentials: {e}")))?;
    std::fs::rename(&tmp, &final_path)
        .map_err(|e| AppError::Auth(format!("commit credentials: {e}")))
}

fn cipher() -> Result<Aes256Gcm, AppError> {
    let dek = dek()?;
    let key = Key::<Aes256Gcm>::from_slice(dek);
    Ok(Aes256Gcm::new(key))
}

fn aes_encrypt(nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AppError> {
    cipher()?
        .encrypt(Nonce::from_slice(nonce), plaintext)
        .map_err(|_| AppError::Auth("encrypt failed".into()))
}

fn aes_decrypt(nonce: &[u8], ct: &[u8]) -> Result<Vec<u8>, AppError> {
    cipher()?
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| AppError::Auth("decrypt failed (store may be corrupt)".into()))
}

// ---------------------------------------------------------------------------
// Data-encryption key (DEK) lifecycle
// ---------------------------------------------------------------------------

fn dek() -> Result<&'static [u8; 32], AppError> {
    if let Some(k) = DEK.get() {
        return Ok(k);
    }
    let k = load_or_create_dek()?;
    Ok(DEK.get_or_init(|| k))
}

fn load_or_create_dek() -> Result<[u8; 32], AppError> {
    let path = store_dir()?.join("dek.bin");
    if let Ok(blob) = std::fs::read(&path) {
        if let Some(dek) = unwrap_dek(&blob) {
            return Ok(dek);
        }
        // Corrupt/unreadable wrapper: fall through and regenerate. Existing
        // credentials become unreadable, which is the correct failure mode for
        // a lost key (the user re-enters their PAT/AI keys).
        eprintln!("pex: DEK wrapper unreadable; regenerating (saved secrets will be reset)");
    }

    let mut dek = [0u8; 32];
    fill_random(&mut dek)?;
    let blob = wrap_dek(&dek);
    std::fs::write(&path, &blob).map_err(|e| AppError::Auth(format!("write DEK: {e}")))?;
    Ok(dek)
}

/// Wrap the DEK with the Android Keystore KEK; fall back to a raw on-disk DEK if
/// the Keystore is unavailable.
fn wrap_dek(dek: &[u8; 32]) -> Vec<u8> {
    match keystore::kek_wrap(dek) {
        Ok((iv, ct)) => {
            let mut blob = Vec::with_capacity(2 + iv.len() + ct.len());
            blob.push(DEK_VERSION_KEYSTORE);
            blob.push(iv.len() as u8);
            blob.extend_from_slice(&iv);
            blob.extend_from_slice(&ct);
            blob
        }
        Err(e) => {
            eprintln!(
                "pex: Android Keystore unavailable ({e}); storing data key in app-private \
                 storage WITHOUT hardware protection (credentials file is still encrypted)"
            );
            let mut blob = Vec::with_capacity(1 + dek.len());
            blob.push(DEK_VERSION_RAW);
            blob.extend_from_slice(dek);
            blob
        }
    }
}

fn unwrap_dek(blob: &[u8]) -> Option<[u8; 32]> {
    match blob.first().copied()? {
        DEK_VERSION_KEYSTORE => {
            let iv_len = *blob.get(1)? as usize;
            let iv = blob.get(2..2 + iv_len)?;
            let ct = blob.get(2 + iv_len..)?;
            let dek = keystore::kek_unwrap(iv, ct).ok()?;
            dek.try_into().ok()
        }
        DEK_VERSION_RAW => blob.get(1..33)?.try_into().ok(),
        _ => None,
    }
}

fn fill_random(buf: &mut [u8]) -> Result<(), AppError> {
    getrandom::getrandom(buf).map_err(|e| AppError::Auth(format!("RNG failure: {e}")))
}

// ---------------------------------------------------------------------------
// Android Keystore (AES-256-GCM KEK) over JNI
// ---------------------------------------------------------------------------

mod keystore {
    use jni::objects::{JByteArray, JObject, JObjectArray, JValue};
    use jni::JavaVM;

    /// Encrypt `data` with the Keystore KEK. Returns `(iv, ciphertext_with_tag)`.
    pub fn kek_wrap(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        with_env(|env| {
            ensure_kek(env)?;
            let key = get_kek(env)?;

            // Cipher c = Cipher.getInstance("AES/GCM/NoPadding"); c.init(ENCRYPT_MODE, key);
            let cipher_cls = "javax/crypto/Cipher";
            let transform = env.new_string("AES/GCM/NoPadding").j()?;
            let cipher = env
                .call_static_method(
                    cipher_cls,
                    "getInstance",
                    "(Ljava/lang/String;)Ljavax/crypto/Cipher;",
                    &[JValue::Object(&transform)],
                )
                .and_then(|v| v.l())
                .j()?;
            env.call_method(
                &cipher,
                "init",
                "(ILjava/security/Key;)V",
                &[JValue::Int(1), JValue::Object(&key)], // ENCRYPT_MODE = 1
            )
            .and_then(|v| v.v())
            .j()?;

            let iv_obj = env
                .call_method(&cipher, "getIV", "()[B", &[])
                .and_then(|v| v.l())
                .j()?;
            let iv = jbytes(env, iv_obj)?;

            let input = env.byte_array_from_slice(data).j()?;
            let ct_obj = env
                .call_method(&cipher, "doFinal", "([B)[B", &[JValue::Object(&input)])
                .and_then(|v| v.l())
                .j()?;
            let ct = jbytes(env, ct_obj)?;

            Ok((iv, ct))
        })
    }

    /// Decrypt `ct` produced by [`kek_wrap`] using `iv`.
    pub fn kek_unwrap(iv: &[u8], ct: &[u8]) -> Result<Vec<u8>, String> {
        with_env(|env| {
            let key = get_kek(env)?;

            // GCMParameterSpec spec = new GCMParameterSpec(128, iv);
            let iv_arr = env.byte_array_from_slice(iv).j()?;
            let spec = env
                .new_object(
                    "javax/crypto/spec/GCMParameterSpec",
                    "(I[B)V",
                    &[JValue::Int(128), JValue::Object(&iv_arr)],
                )
                .j()?;

            let transform = env.new_string("AES/GCM/NoPadding").j()?;
            let cipher = env
                .call_static_method(
                    "javax/crypto/Cipher",
                    "getInstance",
                    "(Ljava/lang/String;)Ljavax/crypto/Cipher;",
                    &[JValue::Object(&transform)],
                )
                .and_then(|v| v.l())
                .j()?;
            // c.init(DECRYPT_MODE, key, spec);
            env.call_method(
                &cipher,
                "init",
                "(ILjava/security/Key;Ljava/security/spec/AlgorithmParameterSpec;)V",
                &[JValue::Int(2), JValue::Object(&key), JValue::Object(&spec)], // DECRYPT_MODE = 2
            )
            .and_then(|v| v.v())
            .j()?;

            let input = env.byte_array_from_slice(ct).j()?;
            let out = env
                .call_method(&cipher, "doFinal", "([B)[B", &[JValue::Object(&input)])
                .and_then(|v| v.l())
                .j()?;
            jbytes(env, out)
        })
    }

    /// The process `JavaVM`, captured in [`JNI_OnLoad`] when the JVM loads our
    /// shared library. We can't use `ndk_context` here: nothing in this app
    /// (tao/wry don't depend on it) ever initializes that global, so
    /// `ndk_context::android_context()` panics. `JNI_OnLoad` is the canonical,
    /// dependency-free way to obtain the VM.
    static JVM: std::sync::OnceLock<JavaVM> = std::sync::OnceLock::new();

    /// Called once by the Android runtime when `System.loadLibrary("pex_lib")`
    /// loads our `.so` (the generated `TauriActivity` does this). Stashes the
    /// `JavaVM` so the credential store can attach threads for Keystore JNI.
    #[no_mangle]
    #[allow(non_snake_case)]
    pub extern "C" fn JNI_OnLoad(
        vm: *mut jni::sys::JavaVM,
        _reserved: *mut std::ffi::c_void,
    ) -> jni::sys::jint {
        if let Ok(vm) = unsafe { JavaVM::from_raw(vm) } {
            let _ = JVM.set(vm);
        }
        jni::sys::JNI_VERSION_1_6
    }

    /// Run `f` with an attached `JNIEnv`, clearing any pending Java exception on
    /// the way out so a failure here never poisons later JNI calls.
    fn with_env<T>(f: impl FnOnce(&mut jni::JNIEnv) -> Result<T, String>) -> Result<T, String> {
        let vm = JVM
            .get()
            .ok_or("JavaVM unavailable (JNI_OnLoad was not called)")?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let result = f(&mut env);
        if result.is_err() {
            if let Ok(true) = env.exception_check() {
                let _ = env.exception_clear();
            }
        }
        result
    }

    /// Resolve the `SecretKey` for our alias from the AndroidKeyStore.
    fn get_kek<'a>(env: &mut jni::JNIEnv<'a>) -> Result<JObject<'a>, String> {
        let ks = open_keystore(env)?;
        let alias = env.new_string(super::KEK_ALIAS).j()?;
        env.call_method(
            &ks,
            "getKey",
            "(Ljava/lang/String;[C)Ljava/security/Key;",
            &[JValue::Object(&alias), JValue::Object(&JObject::null())],
        )
        .and_then(|v| v.l())
        .j()
    }

    /// Create the KEK if our alias does not yet exist in the keystore.
    fn ensure_kek(env: &mut jni::JNIEnv) -> Result<(), String> {
        let ks = open_keystore(env)?;
        let alias = env.new_string(super::KEK_ALIAS).j()?;
        let exists = env
            .call_method(
                &ks,
                "containsAlias",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&alias)],
            )
            .and_then(|v| v.z())
            .j()?;
        if exists {
            return Ok(());
        }

        // KeyGenParameterSpec.Builder b =
        //   new KeyGenParameterSpec.Builder(alias, ENCRYPT|DECRYPT)  // 1|2 = 3
        //     .setBlockModes("GCM").setEncryptionPaddings("NoPadding").setKeySize(256);
        let builder_cls = "android/security/keystore/KeyGenParameterSpec$Builder";
        let alias2 = env.new_string(super::KEK_ALIAS).j()?;
        let builder = env
            .new_object(
                builder_cls,
                "(Ljava/lang/String;I)V",
                &[JValue::Object(&alias2), JValue::Int(3)],
            )
            .j()?;

        let gcm = string_array(env, "GCM")?;
        let builder = env
            .call_method(
                &builder,
                "setBlockModes",
                "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
                &[JValue::Object(&gcm)],
            )
            .and_then(|v| v.l())
            .j()?;

        let nopad = string_array(env, "NoPadding")?;
        let builder = env
            .call_method(
                &builder,
                "setEncryptionPaddings",
                "([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
                &[JValue::Object(&nopad)],
            )
            .and_then(|v| v.l())
            .j()?;

        let builder = env
            .call_method(
                &builder,
                "setKeySize",
                "(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;",
                &[JValue::Int(256)],
            )
            .and_then(|v| v.l())
            .j()?;

        let spec = env
            .call_method(
                &builder,
                "build",
                "()Landroid/security/keystore/KeyGenParameterSpec;",
                &[],
            )
            .and_then(|v| v.l())
            .j()?;

        // KeyGenerator kg = KeyGenerator.getInstance("AES", "AndroidKeyStore");
        // kg.init(spec); kg.generateKey();
        let aes = env.new_string("AES").j()?;
        let provider = env.new_string("AndroidKeyStore").j()?;
        let kg = env
            .call_static_method(
                "javax/crypto/KeyGenerator",
                "getInstance",
                "(Ljava/lang/String;Ljava/lang/String;)Ljavax/crypto/KeyGenerator;",
                &[JValue::Object(&aes), JValue::Object(&provider)],
            )
            .and_then(|v| v.l())
            .j()?;
        env.call_method(
            &kg,
            "init",
            "(Ljava/security/spec/AlgorithmParameterSpec;)V",
            &[JValue::Object(&spec)],
        )
        .and_then(|v| v.v())
        .j()?;
        env.call_method(&kg, "generateKey", "()Ljavax/crypto/SecretKey;", &[])
            .and_then(|v| v.l())
            .j()?;
        Ok(())
    }

    /// `KeyStore ks = KeyStore.getInstance("AndroidKeyStore"); ks.load(null);`
    fn open_keystore<'a>(env: &mut jni::JNIEnv<'a>) -> Result<JObject<'a>, String> {
        let provider = env.new_string("AndroidKeyStore").j()?;
        let ks = env
            .call_static_method(
                "java/security/KeyStore",
                "getInstance",
                "(Ljava/lang/String;)Ljava/security/KeyStore;",
                &[JValue::Object(&provider)],
            )
            .and_then(|v| v.l())
            .j()?;
        env.call_method(
            &ks,
            "load",
            "(Ljava/security/KeyStore$LoadStoreParameter;)V",
            &[JValue::Object(&JObject::null())],
        )
        .and_then(|v| v.v())
        .j()?;
        Ok(ks)
    }

    /// Build a single-element `String[]` (for the Builder varargs setters).
    fn string_array<'a>(
        env: &mut jni::JNIEnv<'a>,
        value: &str,
    ) -> Result<JObjectArray<'a>, String> {
        let s = env.new_string(value).j()?;
        env.new_object_array(1, "java/lang/String", &s)
            .map_err(|e| e.to_string())
    }

    /// Read a Java `byte[]` (held as a `JObject`) into a `Vec<u8>`.
    fn jbytes(env: &mut jni::JNIEnv, obj: JObject) -> Result<Vec<u8>, String> {
        let arr = unsafe { JByteArray::from_raw(obj.into_raw()) };
        env.convert_byte_array(&arr).j()
    }

    /// Map `jni::errors::Error` to `String`, the error type the envelope layer
    /// uses to decide whether to fall back to a raw on-disk DEK.
    trait JResult<T> {
        fn j(self) -> Result<T, String>;
    }
    impl<T> JResult<T> for Result<T, jni::errors::Error> {
        fn j(self) -> Result<T, String> {
            self.map_err(|e| e.to_string())
        }
    }
}
