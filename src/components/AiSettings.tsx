import { useState, useEffect } from "preact/hooks";
import {
  getAiSettings,
  saveAiSettings,
  testAiConnection,
  checkPurist,
  getPuristPath,
  savePuristPath,
  getAiPrompts,
  saveAiPrompt,
  resetAiPrompt,
  getPuristConfig,
  savePuristConfig,
  setPuristConfigPath,
  type AiPromptInfo,
  type PuristConfigPayload,
} from "@/lib/api";
import { puristConfigRevision } from "@/lib/signals";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Tab = "ai" | "prompts" | "purist";

export function AiSettings({ open, onClose }: Props) {
  const [tab, setTab] = useState<Tab>("ai");

  // ---- AI tab ----
  const [provider, setProvider] = useState("openai");
  const [endpoint, setEndpoint] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [requestTimeoutSecs, setRequestTimeoutSecs] = useState(120);
  const [hunkConcurrency, setHunkConcurrency] = useState(1);
  const [standardsMaxChars, setStandardsMaxChars] = useState(8000);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);
  const [testing, setTesting] = useState(false);

  // ---- Prompts tab ----
  const [prompts, setPrompts] = useState<AiPromptInfo[]>([]);
  const [promptDrafts, setPromptDrafts] = useState<Record<string, string>>({});
  const [promptStatus, setPromptStatus] = useState<Record<string, { text: string; ok: boolean } | null>>({});

  // ---- Purist tab ----
  const [puristPath, setPuristPath] = useState("");
  const [puristCheck, setPuristCheck] = useState<string | null>(null);
  const [config, setConfig] = useState<PuristConfigPayload | null>(null);
  const [configDraft, setConfigDraft] = useState<string>("");
  const [configPathDraft, setConfigPathDraft] = useState<string>("");
  const [configError, setConfigError] = useState<string | null>(null);
  const [configStatus, setConfigStatus] = useState<{ text: string; ok: boolean } | null>(null);

  useEffect(() => {
    if (open) {
      loadSettings();
    }
  }, [open]);

  // Reload Purist config whenever the user switches to the Purist tab, since
  // it can change behind the dialog's back (e.g. Purist itself rewriting it).
  useEffect(() => {
    if (open && tab === "purist") {
      loadPuristConfig();
    }
  }, [open, tab]);

  const loadSettings = async () => {
    try {
      const [settings, pp, ps] = await Promise.all([
        getAiSettings(),
        getPuristPath(),
        getAiPrompts(),
      ]);
      setProvider(settings.provider);
      setEndpoint(settings.endpoint);
      setModel(settings.model);
      setRequestTimeoutSecs(settings.requestTimeoutSecs || 120);
      setHunkConcurrency(settings.hunkConcurrency || 1);
      setStandardsMaxChars(settings.standardsMaxChars || 8000);
      setApiKey("");
      if (pp) setPuristPath(pp);
      setPrompts(ps);
      setPromptDrafts(Object.fromEntries(ps.map((p) => [p.key, p.value])));
      setPromptStatus({});
    } catch {
      // defaults are fine
    }
  };

  const loadPuristConfig = async () => {
    setConfigError(null);
    setConfigStatus(null);
    try {
      const c = await getPuristConfig();
      setConfig(c);
      setConfigDraft(c.content);
      setConfigPathDraft(c.isCustomPath ? c.path : "");
    } catch (e: any) {
      setConfig(null);
      setConfigDraft("");
      setConfigError(String(e));
    }
  };

  const handleSavePrompt = async (key: string) => {
    const value = promptDrafts[key] ?? "";
    setPromptStatus((prev) => ({ ...prev, [key]: null }));
    try {
      await saveAiPrompt(key, value);
      const refreshed = await getAiPrompts();
      setPrompts(refreshed);
      setPromptDrafts((prev) => ({ ...prev, [key]: value }));
      setPromptStatus((prev) => ({ ...prev, [key]: { text: "Prompt saved.", ok: true } }));
    } catch (e: any) {
      setPromptStatus((prev) => ({ ...prev, [key]: { text: String(e), ok: false } }));
    }
  };

  const handleResetPrompt = async (key: string, label: string) => {
    const confirmed = window.confirm(
      `Reset "${label}" to its default? Your customizations for this prompt will be lost.`,
    );
    if (!confirmed) return;
    setPromptStatus((prev) => ({ ...prev, [key]: null }));
    try {
      await resetAiPrompt(key);
      const refreshed = await getAiPrompts();
      setPrompts(refreshed);
      const restored = refreshed.find((p) => p.key === key);
      setPromptDrafts((prev) => ({ ...prev, [key]: restored?.value ?? "" }));
      setPromptStatus((prev) => ({ ...prev, [key]: { text: "Reset to default.", ok: true } }));
    } catch (e: any) {
      setPromptStatus((prev) => ({ ...prev, [key]: { text: String(e), ok: false } }));
    }
  };

  const handleSaveAi = async () => {
    setSaving(true);
    setMessage(null);
    try {
      await saveAiSettings(provider, endpoint, model, apiKey, requestTimeoutSecs, hunkConcurrency, standardsMaxChars);
      setMessage({ text: "AI settings saved.", ok: true });
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setSaving(false);
    }
  };

  const handleSavePurist = async () => {
    setSaving(true);
    setMessage(null);
    try {
      await savePuristPath(puristPath);
      puristConfigRevision.value++;
      setMessage({ text: "Purist path saved.", ok: true });
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setSaving(false);
    }
  };

  const handleCheckPurist = async () => {
    setPuristCheck(null);
    try {
      const r = await checkPurist(puristPath);
      setPuristCheck(r.message);
      if (r.ok) {
        await savePuristPath(puristPath);
        puristConfigRevision.value++;
      }
    } catch (e: any) {
      setPuristCheck(String(e));
    }
  };

  const handleTestConnection = async () => {
    // Save settings first so the backend can configure the provider
    setTesting(true);
    setMessage(null);
    try {
      await saveAiSettings(provider, endpoint, model, apiKey, requestTimeoutSecs, hunkConcurrency, standardsMaxChars);
      const result = await testAiConnection();
      setMessage({ text: result, ok: true });
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setTesting(false);
    }
  };

  const handleSaveConfigPath = async () => {
    setConfigError(null);
    setConfigStatus(null);
    try {
      await setPuristConfigPath(configPathDraft);
      await loadPuristConfig();
      setConfigStatus({ text: "Config path updated.", ok: true });
    } catch (e: any) {
      setConfigStatus({ text: String(e), ok: false });
    }
  };

  const handleSaveConfig = async () => {
    setConfigError(null);
    setConfigStatus(null);
    try {
      const c = await savePuristConfig(configDraft);
      setConfig(c);
      setConfigDraft(c.content);
      setConfigStatus({ text: `Saved to ${c.path}`, ok: true });
    } catch (e: any) {
      setConfigStatus({ text: String(e), ok: false });
    }
  };

  const handleReloadConfig = async () => {
    await loadPuristConfig();
    setConfigStatus({ text: "Reloaded from disk.", ok: true });
  };

  if (!open) return null;

  const configDirty = config !== null && configDraft !== config.content;

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div
        class="bg-white dark:bg-gray-900 rounded-xl shadow-xl border border-gray-200 dark:border-gray-700 w-full max-w-2xl mx-4 max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div class="flex items-center justify-between px-5 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 class="text-base font-semibold">AI Settings</h2>
          <button
            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none"
            onClick={onClose}
          >
            ×
          </button>
        </div>

        {/* Tabs */}
        <div class="flex border-b border-gray-200 dark:border-gray-700 px-5">
          <TabButton label="AI" active={tab === "ai"} onClick={() => setTab("ai")} />
          <TabButton label="Prompts" active={tab === "prompts"} onClick={() => setTab("prompts")} />
          <TabButton label="Purist" active={tab === "purist"} onClick={() => setTab("purist")} />
        </div>

        <div class="px-5 py-4 space-y-5">
          {tab === "ai" && (
            <section>
              <div class="space-y-3">
                <Field label="Provider">
                  <select
                    value={provider}
                    onChange={(e) => setProvider(e.currentTarget.value)}
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  >
                    <option value="openai">OpenAI-compatible</option>
                    <option value="anthropic">Anthropic-compatible</option>
                  </select>
                </Field>

                <Field label="Endpoint URL">
                  <input
                    type="url"
                    value={endpoint}
                    onInput={(e) => setEndpoint(e.currentTarget.value)}
                    placeholder={provider === "openai" ? "https://api.openai.com" : "https://api.anthropic.com"}
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                </Field>

                <Field label="Model">
                  <input
                    type="text"
                    value={model}
                    onInput={(e) => setModel(e.currentTarget.value)}
                    placeholder={provider === "openai" ? "gpt-4.1" : "claude-sonnet-4-20250514"}
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                </Field>

                <Field label="API Key">
                  <input
                    type="password"
                    value={apiKey}
                    onInput={(e) => setApiKey(e.currentTarget.value)}
                    placeholder="Enter API key"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                </Field>

                <Field label="Request timeout (seconds)">
                  <input
                    type="number"
                    min={1}
                    max={3600}
                    value={requestTimeoutSecs}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      setRequestTimeoutSecs(Number.isFinite(n) && n > 0 ? n : 120);
                    }}
                    placeholder="120"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Maximum time to wait for the model to respond. Increase this for slower local models.
                  </p>
                </Field>

                <Field label="Hunk review concurrency">
                  <input
                    type="number"
                    min={1}
                    max={16}
                    value={hunkConcurrency}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      setHunkConcurrency(Number.isFinite(n) && n >= 1 ? Math.min(n, 16) : 1);
                    }}
                    placeholder="1"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Maximum number of hunks "Review All" sends to the model at once. Default 1 = sequential. Increase only if your provider can handle parallel requests.
                  </p>
                </Field>

                <Field label="Review context size limit (chars)">
                  <input
                    type="number"
                    min={500}
                    max={65535}
                    value={standardsMaxChars}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      if (!Number.isFinite(n)) {
                        setStandardsMaxChars(8000);
                      } else {
                        setStandardsMaxChars(Math.min(65535, Math.max(500, n)));
                      }
                    }}
                    placeholder="8000"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Per-file cap (in characters) for AGENTS.md / STYLE.md content injected into Review prompts. Anything beyond this is truncated with a visible marker.
                  </p>
                </Field>
              </div>

              <button
                onClick={handleSaveAi}
                disabled={saving || !endpoint || !model}
                class="mt-3 px-4 py-2 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50"
              >
                {saving ? "Saving..." : "Save AI Settings"}
              </button>
              <button
                onClick={handleTestConnection}
                disabled={testing || !endpoint || !model || !apiKey}
                class="mt-3 ml-2 px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg text-sm font-medium hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
              >
                {testing ? "Testing..." : "Test Connection"}
              </button>
            </section>
          )}

          {tab === "prompts" && (
            <section>
              <p class="text-xs text-gray-500 dark:text-gray-400 mb-3">
                Customize the system prompts used by AI features. Changes are saved per prompt.
              </p>

              <div class="space-y-5">
                {prompts.map((p) => {
                  const draft = promptDrafts[p.key] ?? "";
                  const status = promptStatus[p.key];
                  const dirty = draft !== p.value;
                  return (
                    <div key={p.key}>
                      <div class="flex items-center justify-between mb-1">
                        <span class="text-xs text-gray-500 dark:text-gray-400">
                          {p.label}
                          {p.isCustomized && (
                            <span class="ml-2 text-[10px] uppercase tracking-wide text-accent">
                              customized
                            </span>
                          )}
                        </span>
                      </div>
                      <textarea
                        value={draft}
                        onInput={(e) =>
                          setPromptDrafts((prev) => ({
                            ...prev,
                            [p.key]: e.currentTarget.value,
                          }))
                        }
                        rows={8}
                        class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-xs font-mono outline-none focus:ring-2 focus:ring-accent resize-y"
                      />
                      <p class="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                        {p.description}
                      </p>
                      <div class="flex items-center gap-2 mt-2">
                        <button
                          onClick={() => handleSavePrompt(p.key)}
                          disabled={!dirty || draft.trim().length === 0}
                          class="px-3 py-1.5 bg-accent hover:bg-accent-hover text-white rounded-lg text-xs font-medium disabled:opacity-50"
                        >
                          Save
                        </button>
                        <button
                          onClick={() => handleResetPrompt(p.key, p.label)}
                          disabled={!p.isCustomized}
                          title={
                            p.isCustomized
                              ? "Restore the default prompt"
                              : "Already using the default"
                          }
                          class="px-3 py-1.5 border border-gray-300 dark:border-gray-600 rounded-lg text-xs font-medium hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50 disabled:cursor-not-allowed"
                        >
                          Reset to default
                        </button>
                        {status && (
                          <span
                            class={`text-xs ${status.ok ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}`}
                          >
                            {status.text}
                          </span>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </section>
          )}

          {tab === "purist" && (
            <section class="space-y-5">
              {/* Purist path */}
              <div>
                <Field label="Purist path">
                  <div class="flex gap-2">
                    <input
                      type="text"
                      value={puristPath}
                      onInput={(e) => setPuristPath(e.currentTarget.value)}
                      placeholder="~/repos/purist"
                      class="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                    />
                    <button
                      onClick={handleCheckPurist}
                      class="px-3 py-2 text-xs border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-800"
                    >
                      Check
                    </button>
                  </div>
                </Field>

                {puristCheck && (
                  <p class={`text-xs mt-1.5 ${puristCheck.startsWith("Purist found") ? "text-green-600 dark:text-green-400" : "text-orange-600 dark:text-orange-400"}`}>
                    {puristCheck}
                  </p>
                )}

                <button
                  onClick={handleSavePurist}
                  disabled={saving || !puristPath}
                  class="mt-3 px-4 py-2 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50"
                >
                  Save Purist Path
                </button>
              </div>

              <hr class="border-gray-200 dark:border-gray-700" />

              {/* Config editor */}
              <div>
                <div class="flex items-center justify-between mb-2">
                  <h3 class="text-sm font-medium text-gray-700 dark:text-gray-300">Config (YAML)</h3>
                  <button
                    onClick={handleReloadConfig}
                    class="text-xs px-2 py-1 border border-gray-300 dark:border-gray-600 rounded hover:bg-gray-50 dark:hover:bg-gray-800"
                    title="Reload from disk"
                  >
                    Reload
                  </button>
                </div>

                <Field label="Config file path">
                  <div class="flex gap-2">
                    <input
                      type="text"
                      value={configPathDraft}
                      onInput={(e) => setConfigPathDraft(e.currentTarget.value)}
                      placeholder={
                        config && !config.isCustomPath
                          ? `${config.path}  (default)`
                          : "/path/to/purist/config.yaml"
                      }
                      class="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm font-mono outline-none focus:ring-2 focus:ring-accent"
                    />
                    <button
                      onClick={handleSaveConfigPath}
                      class="px-3 py-2 text-xs border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-800"
                    >
                      Use path
                    </button>
                  </div>
                </Field>
                <p class="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                  Leave blank to use the default location next to the Purist clone (
                  <code>&lt;purist path&gt;/config.yaml</code>). Click "Use path" to apply.
                </p>

                {configError && (
                  <div class="mt-3 text-xs px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300">
                    {configError}
                  </div>
                )}

                {config && (
                  <>
                    <div class="mt-3 flex items-center justify-between text-[11px] text-gray-500 dark:text-gray-400">
                      <span class="font-mono truncate">{config.path}</span>
                      <span>
                        {config.exists ? "exists" : "will be created on save"}
                        {config.isCustomPath && " · custom path"}
                      </span>
                    </div>
                    <textarea
                      value={configDraft}
                      onInput={(e) => setConfigDraft(e.currentTarget.value)}
                      rows={18}
                      spellcheck={false}
                      placeholder={
                        config.exists
                          ? ""
                          : "# Purist config not found yet. Edit and save to create it."
                      }
                      class="mt-2 w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-xs font-mono outline-none focus:ring-2 focus:ring-accent resize-y"
                    />
                    <div class="flex items-center gap-2 mt-2">
                      <button
                        onClick={handleSaveConfig}
                        disabled={!configDirty}
                        class="px-3 py-1.5 bg-accent hover:bg-accent-hover text-white rounded-lg text-xs font-medium disabled:opacity-50"
                      >
                        Save Config
                      </button>
                      {configDirty && (
                        <span class="text-[11px] text-gray-500 dark:text-gray-400">
                          Unsaved changes
                        </span>
                      )}
                      {configStatus && (
                        <span
                          class={`text-xs ${configStatus.ok ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}`}
                        >
                          {configStatus.text}
                        </span>
                      )}
                    </div>
                  </>
                )}
              </div>
            </section>
          )}

          {/* Status message (AI tab only) */}
          {tab === "ai" && message && (
            <div class={`text-sm px-3 py-2 rounded-lg ${message.ok ? "bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-300" : "bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300"}`}>
              {message.text}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function TabButton({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      class={`px-3 py-2 text-sm font-medium border-b-2 -mb-px ${
        active
          ? "border-accent text-accent"
          : "border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
      }`}
    >
      {label}
    </button>
  );
}

function Field({ label, children }: { label: string; children: preact.ComponentChildren }) {
  return (
    <label class="block">
      <span class="text-xs text-gray-500 dark:text-gray-400 mb-1 block">{label}</span>
      {children}
    </label>
  );
}
