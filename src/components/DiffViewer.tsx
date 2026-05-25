import { useState, useRef, useEffect, useCallback } from "preact/hooks";
import type { CommentThread } from "@/lib/api";
import { getFileLines } from "@/lib/api";

interface Props {
  html: string;
  path: string;
  threads: CommentThread[];
  onComment: (
    filePath: string,
    lineStart: number,
    lineEnd: number,
    content: string,
  ) => Promise<void>;
  projectId: string;
  repoId: string;
  sourceCommit: string;
  baseCommit: string | null;
}

const EXPAND_CHUNK = 10;

interface Range {
  start: number;
  end: number; // inclusive
}

interface PopupPosition {
  top: number; // px, relative to selection container
  left: number;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function renderEqualLines(lines: string[], startNewLine: number): string {
  return lines
    .map((line, i) => {
      const ln = startNewLine + i;
      return `<div class="diff-line" data-line="${ln}"><span class="diff-lineno">${ln}</span><span class="diff-sign">  </span><span class="diff-content">${escapeHtml(line)}\n</span></div>`;
    })
    .join("");
}

function normalize(r: Range): Range {
  return r.start <= r.end ? r : { start: r.end, end: r.start };
}

export function DiffViewer({
  html,
  path,
  threads,
  onComment,
  projectId,
  repoId,
  sourceCommit,
  baseCommit: _baseCommit,
}: Props) {
  const diffRef = useRef<HTMLDivElement>(null);
  const dragAnchorRef = useRef<number | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // The committed selection (set on mouseup) — popup is anchored to its end line.
  const [range, setRange] = useState<Range | null>(null);
  const [popupPos, setPopupPos] = useState<PopupPosition | null>(null);
  const [commentText, setCommentText] = useState("");
  const [posting, setPosting] = useState(false);
  const [postError, setPostError] = useState("");

  // ---- selection highlight on the DOM (cheap; avoids re-rendering injected HTML)
  const setHighlight = useCallback((lo: number, hi: number) => {
    const root = diffRef.current;
    if (!root) return;
    root.querySelectorAll<HTMLElement>("[data-line]").forEach((el) => {
      const ln = Number(el.getAttribute("data-line"));
      if (ln >= lo && ln <= hi) el.classList.add("diff-line--selected");
      else el.classList.remove("diff-line--selected");
    });
  }, []);

  const clearHighlight = useCallback(() => {
    diffRef.current
      ?.querySelectorAll<HTMLElement>(".diff-line--selected")
      .forEach((el) => el.classList.remove("diff-line--selected"));
  }, []);

  const closePopup = useCallback(() => {
    setRange(null);
    setPopupPos(null);
    setCommentText("");
    setPostError("");
    dragAnchorRef.current = null;
    clearHighlight();
  }, [clearHighlight]);

  // Reposition the popup whenever the selection or DOM changes (e.g. after expander reveal).
  useEffect(() => {
    if (!range || !diffRef.current) return;
    const last = diffRef.current.querySelector<HTMLElement>(
      `[data-line="${range.end}"]`,
    );
    if (!last) return;
    setPopupPos({
      top: last.offsetTop + last.offsetHeight,
      left: 32,
    });
  }, [range, html]);

  // ---- mouse handlers wired to the diff container.
  const lineFromTarget = (target: EventTarget | null): number | null => {
    if (!(target instanceof HTMLElement)) return null;
    const el = target.closest<HTMLElement>("[data-line]");
    if (!el) return null;
    const ln = Number(el.getAttribute("data-line"));
    return Number.isFinite(ln) && ln > 0 ? ln : null;
  };

  const onMouseDown = (e: MouseEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    // Defer to existing controls (expander buttons, etc.) and to clicks inside the popup itself.
    if (target.closest(".diff-expander-btn")) return;
    if (target.closest(".comment-popup")) return;
    const ln = lineFromTarget(target);
    if (ln === null) return;
    // Prevent native text selection while we own the drag.
    e.preventDefault();
    dragAnchorRef.current = ln;
    setRange({ start: ln, end: ln });
    setPopupPos(null);
    setCommentText("");
    setPostError("");
    setHighlight(ln, ln);
  };

  const onMouseMove = (e: MouseEvent) => {
    if (dragAnchorRef.current === null) return;
    const ln = lineFromTarget(e.target);
    if (ln === null) return;
    const anchor = dragAnchorRef.current;
    const r = normalize({ start: anchor, end: ln });
    setHighlight(r.start, r.end);
    setRange(r);
  };

  // mouseup needs to fire even when the cursor leaves the diff area.
  useEffect(() => {
    const handleUp = (e: MouseEvent) => {
      if (dragAnchorRef.current === null) return;
      dragAnchorRef.current = null;
      // Range is already current in state from the last mousemove (or mousedown for a click).
      const target = e.target as HTMLElement | null;
      // If mouseup lands inside the popup, don't close — user is clicking a button there.
      if (target?.closest(".comment-popup")) return;
      // Force a re-render so the popup position effect runs even when range
      // hasn't changed (single-click case).
      setRange((r) => (r ? { ...r } : r));
      // The popup mounts on the next render — focus the textarea so the user
      // can start typing immediately without an extra click.
      requestAnimationFrame(() => {
        textareaRef.current?.focus();
      });
    };
    window.addEventListener("mouseup", handleUp);
    return () => window.removeEventListener("mouseup", handleUp);
  }, []);

  // ---- expander (unchanged behavior)
  const readRange = (el: HTMLElement) => ({
    oldStart: Number(el.getAttribute("data-old-start") || "0"),
    oldEnd: Number(el.getAttribute("data-old-end") || "0"),
    newStart: Number(el.getAttribute("data-new-start") || "0"),
    newEnd: Number(el.getAttribute("data-new-end") || "0"),
  });

  const expandRange = async (
    expander: HTMLElement,
    action: "up" | "down" | "all",
  ) => {
    if (!sourceCommit) return;
    const r = readRange(expander);
    const total = Math.max(r.newEnd - r.newStart + 1, r.oldEnd - r.oldStart + 1);
    if (total <= 0) return;
    let fetchStart: number;
    let fetchEnd: number;
    if (action === "all" || total <= EXPAND_CHUNK) {
      fetchStart = r.newStart;
      fetchEnd = r.newEnd;
    } else if (action === "up") {
      fetchStart = Math.max(r.newStart, r.newEnd - EXPAND_CHUNK + 1);
      fetchEnd = r.newEnd;
    } else {
      fetchStart = r.newStart;
      fetchEnd = Math.min(r.newEnd, r.newStart + EXPAND_CHUNK - 1);
    }
    expander.setAttribute("data-loading", "1");
    try {
      const lines = await getFileLines(projectId, repoId, sourceCommit, path, fetchStart, fetchEnd);
      const linesHtml = renderEqualLines(lines, fetchStart);
      const consumed = fetchEnd - fetchStart + 1;
      const remainingTotal = total - consumed;
      if (remainingTotal <= 0) {
        expander.outerHTML = linesHtml;
      } else if (action === "up") {
        expander.setAttribute("data-old-end", String(r.oldEnd - consumed));
        expander.setAttribute("data-new-end", String(r.newEnd - consumed));
        const allBtn = expander.querySelector(".diff-expander-all");
        if (allBtn) allBtn.textContent = `${remainingTotal} hidden lines`;
        expander.insertAdjacentHTML("afterend", linesHtml);
      } else {
        expander.setAttribute("data-old-start", String(r.oldStart + consumed));
        expander.setAttribute("data-new-start", String(r.newStart + consumed));
        const allBtn = expander.querySelector(".diff-expander-all");
        if (allBtn) allBtn.textContent = `${remainingTotal} hidden lines`;
        expander.insertAdjacentHTML("beforebegin", linesHtml);
      }
    } catch (e) {
      console.error("Failed to expand context:", e);
      expander.removeAttribute("data-loading");
    }
  };

  const onClickContainer = (e: MouseEvent) => {
    const target = e.target as HTMLElement;
    const btn = target.closest(".diff-expander-btn") as HTMLElement | null;
    if (!btn) return;
    const expander = btn.closest(".diff-expander") as HTMLElement | null;
    const action = btn.getAttribute("data-action") as "up" | "down" | "all" | null;
    if (expander && action && !expander.hasAttribute("data-loading")) {
      expandRange(expander, action);
    }
  };

  // ---- submit comment
  const handlePost = async () => {
    if (!commentText.trim() || !range) return;
    setPosting(true);
    setPostError("");
    try {
      await onComment(path, range.start, range.end, commentText);
      closePopup();
    } catch (e: any) {
      setPostError(typeof e === "string" ? e : e?.message ?? String(e));
    } finally {
      setPosting(false);
    }
  };

  const rangeLabel = range
    ? range.start === range.end
      ? `line ${range.start}`
      : `lines ${range.start}–${range.end}`
    : "";

  return (
    <div class="overflow-x-auto">
      {/* File header */}
      <div class="diff-header sticky top-0 z-10">{path}</div>

      {/* Selection + popup live inside this relative container so the popup
          can be positioned with absolute coords against the diff content. */}
      <div class="relative">
        <div
          ref={diffRef}
          dangerouslySetInnerHTML={{ __html: html }}
          onMouseDown={onMouseDown}
          onMouseMove={onMouseMove}
          onClick={onClickContainer}
        />

        {range && popupPos && (
          <div
            class="comment-popup absolute z-20 w-[min(520px,calc(100%-2rem))] shadow-lg rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900 p-3"
            style={{ top: `${popupPos.top + 4}px`, left: `${popupPos.left}px` }}
            // Keep mouse interactions inside the popup from triggering selection.
            onMouseDown={(e) => e.stopPropagation()}
            onMouseMove={(e) => e.stopPropagation()}
          >
            <div class="text-xs text-gray-500 mb-1">
              Comment on {rangeLabel} of <code>{path}</code>
            </div>
            <textarea
              ref={textareaRef}
              autofocus
              value={commentText}
              onInput={(e) => setCommentText(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.preventDefault();
                  closePopup();
                } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                  e.preventDefault();
                  handlePost();
                }
              }}
              class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent resize-none"
              rows={3}
              placeholder="Leave a comment (Markdown supported)  •  ⌘/Ctrl+Enter to post, Esc to cancel"
            />
            {postError && (
              <div class="mt-2 text-xs text-red-600 dark:text-red-400 break-words">
                {postError}
              </div>
            )}
            <div class="flex gap-2 mt-2 items-center">
              <button
                onClick={handlePost}
                disabled={!commentText.trim() || posting}
                class="px-3 py-1 bg-accent hover:bg-accent-hover text-white rounded text-xs font-medium disabled:opacity-50"
              >
                {posting ? "Posting…" : "Comment"}
              </button>
              <button
                onClick={closePopup}
                class="px-3 py-1 text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Existing threads (unchanged) */}
      {threads.map((t) => (
        <div key={t.id} class="border-t border-gray-100 dark:border-gray-800 p-3">
          <div class="text-xs text-gray-400 mb-1">
            Thread on line {t.lineStart === t.lineEnd ? t.lineStart : `${t.lineStart}-${t.lineEnd}`}
          </div>
          {t.comments.map((c) => (
            <div
              key={c.id}
              class="text-sm mb-2 pl-3 border-l-2 border-gray-200 dark:border-gray-700"
            >
              <span class="font-medium text-xs">{c.author}</span>
              {c.publishedDate && (
                <span class="text-xs text-gray-400 ml-2">{c.publishedDate}</span>
              )}
              <div class="mt-1 text-gray-700 dark:text-gray-300">{c.content}</div>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
