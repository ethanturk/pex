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

/** True on a tablet — touch-capable and large enough for the multi-panel
 *  layout. Covers iPad (which reports as "Macintosh" with touch on iPadOS 13+)
 *  and Android tablets. Excludes touchscreen Windows/Linux desktops, which have
 *  touch + a wide viewport but should keep the desktop layout. */
export function isTablet(): boolean {
  if (navigator.maxTouchPoints === 0 || window.innerWidth < 768) return false;
  const ua = navigator.userAgent;
  return /iPad|Macintosh/.test(ua) || /Android/.test(ua);
}

/** @deprecated Use {@link isTablet}. Retained for backwards compatibility. */
export const isIPad = isTablet;

/** Returns the current active platform category.
 *  Used to switch between layout strategies. */
export type Platform = "desktop" | "ipad" | "mobile";
export function getPlatform(): Platform {
  if (isMobile()) return "mobile";
  // On Tauri desktop there's no touch, so isMobile() returns false.
  // Tablets (iPad / Android tablet) are touch-capable with a wide viewport.
  if (isTablet()) return "ipad";
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
