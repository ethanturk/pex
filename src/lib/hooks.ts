import { useEffect } from "preact/hooks";

/** Like useEffect(fn, []) but type-safe and fires exactly once. */
export function useEffectOnce(fn: () => void | (() => void)) {
  useEffect(fn, []);
}
