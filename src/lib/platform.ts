/** Platform detection utilities for adaptive UI. */

/** True when running inside Tauri (desktop or mobile).
 *  Never true in a regular browser or Safari. */
export function isTauri(): boolean {
  return "__TAURI__" in window;
}

/** True when the device has a touch screen AND a narrow viewport (phone).
 *  Tablets (iPad) are NOT considered mobile — they get the desktop layout. */
export function isMobile(): boolean {
  return navigator.maxTouchPoints > 0 && window.innerWidth < 768;
}

/** True on iPad — touch-capable but large enough for multi-panel layout. */
export function isIPad(): boolean {
  return (
    navigator.maxTouchPoints > 0 &&
    window.innerWidth >= 768 &&
    /iPad|Macintosh/.test(navigator.userAgent) &&
    !("ontouchend" in document) === false
  );
}

/** Returns the current active platform category.
 *  Used to switch between layout strategies. */
export type Platform = "desktop" | "ipad" | "mobile";
export function getPlatform(): Platform {
  if (isMobile()) return "mobile";
  // On Tauri desktop there's no touch, so isMobile() returns false
  // iPad detection: touch-capable + wider screen
  if (isIPad()) return "ipad";
  return "desktop";
}

/** React to platform changes (orientation, window resize). */
export function onPlatformChange(fn: (p: Platform) => void) {
  let last = getPlatform();
  const check = () => {
    const now = getPlatform();
    if (now !== last) {
      last = now;
      fn(now);
    }
  };
  window.addEventListener("resize", check);
  // orientationchange fires before resize on iOS — gives faster response
  window.addEventListener("orientationchange", check);
  return () => {
    window.removeEventListener("resize", check);
    window.removeEventListener("orientationchange", check);
  };
}
