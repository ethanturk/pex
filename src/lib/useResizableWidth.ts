import { useEffect, useRef, useState } from "preact/hooks";

interface Options {
  /** localStorage key for the persisted width (pixels). */
  storageKey: string;
  /** Default width when no value is stored or stored value is invalid. */
  defaultWidth: number;
  min: number;
  max: number;
  /**
   * Which edge of the element the drag handle sits on.
   *  - "right": handle is on the element's right edge (e.g. a left sidebar).
   *    Dragging right grows the element.
   *  - "left":  handle is on the element's left edge (e.g. a right sidebar).
   *    Dragging left grows the element.
   */
  side: "left" | "right";
}

interface DragState {
  startX: number;
  startWidth: number;
}

/**
 * Drives a draggable resize handle for a panel that controls its own width.
 * Returns the current width and a mouse-down handler to attach to the handle.
 * Width is clamped to `[min, max]` during drag and persisted to localStorage
 * after each render — cheap enough that we don't bother debouncing.
 */
export function useResizableWidth(opts: Options): {
  width: number;
  onMouseDown: (e: MouseEvent) => void;
} {
  const { storageKey, defaultWidth, min, max, side } = opts;

  const [width, setWidth] = useState(() => {
    try {
      const raw = localStorage.getItem(storageKey);
      if (raw) {
        const n = parseInt(raw, 10);
        if (Number.isFinite(n)) return Math.max(min, Math.min(max, n));
      }
    } catch {
      // localStorage may be unavailable (e.g. private mode quirks) — fall through.
    }
    return defaultWidth;
  });

  // Cleans up any in-flight listeners on unmount, so navigating away mid-drag
  // doesn't leave a dangling cursor override or document-level handlers.
  const dragRef = useRef<DragState | null>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  useEffect(() => () => cleanupRef.current?.(), []);

  const onMouseDown = (e: MouseEvent) => {
    e.preventDefault();
    dragRef.current = { startX: e.clientX, startWidth: width };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";

    const onMove = (ev: MouseEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const dx = ev.clientX - drag.startX;
      const next = side === "right" ? drag.startWidth + dx : drag.startWidth - dx;
      setWidth(Math.max(min, Math.min(max, next)));
    };
    const cleanup = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      dragRef.current = null;
      cleanupRef.current = null;
    };
    const onUp = () => {
      cleanup();
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    cleanupRef.current = cleanup;
  };

  useEffect(() => {
    try {
      localStorage.setItem(storageKey, String(width));
    } catch {
      // ignore
    }
  }, [width, storageKey]);

  return { width, onMouseDown };
}
