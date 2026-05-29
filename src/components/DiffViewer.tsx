import { useState, useRef, useEffect, useCallback, useMemo } from "preact/hooks";
import { marked } from "marked";
import type { CommentThread } from "@/lib/api";
import { getFileLines } from "@/lib/api";
import { pendingScrollLine, type DiffView } from "@/lib/signals";

interface Props {
  html: string;
  path: string;
  status: string;
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
  view: DiffView;
  oldContent: string;
  newContent: string;
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

function renderEqualLinesInline(lines: string[], startNewLine: number): string {
  return lines
    .map((line, i) => {
      const ln = startNewLine + i;
      return `<div class="diff-line" data-line="${ln}"><span class="diff-lineno">${ln}</span><span class="diff-sign"></span><span class="diff-content">${escapeHtml(line)}\n</span></div>`;
    })
    .join("");
}

function renderEqualLinesSplit(
  lines: string[],
  startNewLine: number,
  startOldLine: number,
): string {
  return lines
    .map((line, i) => {
      const newLn = startNewLine + i;
      const oldLn = startOldLine + i;
      const escaped = escapeHtml(line);
      const oldCell = `<div class="diff-cell diff-line"><span class="diff-lineno">${oldLn}</span><span class="diff-sign"></span><span class="diff-content">${escaped}\n</span></div>`;
      const newCell = `<div class="diff-cell diff-line" data-line="${newLn}"><span class="diff-lineno">${newLn}</span><span class="diff-sign"></span><span class="diff-content">${escaped}\n</span></div>`;
      return `<div class="diff-row">${oldCell}${newCell}</div>`;
    })
    .join("");
}

function normalize(r: Range): Range {
  return r.start <= r.end ? r : { start: r.end, end: r.start };
}

function isMarkdownPath(path: string): boolean {
  return /\.(md|markdown|mdown|mkd|mkdn)$/i.test(path);
}

function renderMarkdown(markdown: string): string {
  return marked.parse(markdown, {
    async: false,
    breaks: true,
    gfm: true,
  }) as string;
}

export function DiffViewer({
  html,
  path,
  status,
  threads,
  onComment,
  projectId,
  repoId,
  sourceCommit,
  baseCommit: _baseCommit,
  view,
  oldContent,
  newContent,
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
  const [showMarkdownPreview, setShowMarkdownPreview] = useState(false);

  const [jumpOpen, setJumpOpen] = useState(false);
  const [jumpInput, setJumpInput] = useState("");
  const [jumpError, setJumpError] = useState("");
  const [jumpTotal, setJumpTotal] = useState(0);
  const jumpInputRef = useRef<HTMLInputElement>(null);

  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchIdx, setSearchIdx] = useState(0);
  const [searchHits, setSearchHits] = useState(0);
  const searchInputRef = useRef<HTMLInputElement>(null);
  // Matches are recomputed on every query change; held in a ref so prev/next
  // navigation doesn't trigger a re-scan.
  const searchMatchesRef = useRef<HTMLElement[]>([]);
  const isMarkdown = isMarkdownPath(path);
  const markdownPreviewHtml = useMemo(() => {
    if (!isMarkdown) return "";
    return renderMarkdown(status === "delete" ? oldContent : newContent);
  }, [isMarkdown, status, newContent, oldContent]);

  useEffect(() => {
    setShowMarkdownPreview(isMarkdown);
  }, [isMarkdown, path]);

  // ---- selection highlight on the DOM (cheap; avoids re-rendering injected HTML)
  const setHighlight = useCallback((lo: number, hi: number) => {
    const root = diffRef.current;
    if (!root) return;
    root.querySelectorAll<HTMLElement>("[data-line]").forEach((el) => {
      const ln = Number(el.getAttribute("data-line"));
      const row = el.closest<HTMLElement>(".diff-row");
      if (ln >= lo && ln <= hi) {
        el.classList.add("diff-line--selected");
        row?.classList.add("diff-row--selected");
      } else {
        el.classList.remove("diff-line--selected");
        row?.classList.remove("diff-row--selected");
      }
    });
  }, []);

  const clearHighlight = useCallback(() => {
    diffRef.current
      ?.querySelectorAll<HTMLElement>(".diff-line--selected")
      .forEach((el) => el.classList.remove("diff-line--selected"));
    diffRef.current
      ?.querySelectorAll<HTMLElement>(".diff-row--selected")
      .forEach((el) => el.classList.remove("diff-row--selected"));
  }, []);

  const closePopup = useCallback(() => {
    setRange(null);
    setPopupPos(null);
    setCommentText("");
    setPostError("");
    dragAnchorRef.current = null;
    clearHighlight();
  }, [clearHighlight]);

  // Jump to a specific line when something (e.g. a review finding) requests it.
  // We re-run on `html` so the scroll lands after the new diff is injected.
  useEffect(() => {
    const ln = pendingScrollLine.value;
    if (ln === null || !diffRef.current) return;
    const el = diffRef.current.querySelector<HTMLElement>(`[data-line="${ln}"]`);
    if (!el) return;
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    setHighlight(ln, ln);
    pendingScrollLine.value = null;
  }, [pendingScrollLine.value, html, setHighlight]);

  // Reposition the popup whenever the selection or DOM changes (e.g. after expander reveal).
  useEffect(() => {
    if (!range || !diffRef.current) return;
    const last = diffRef.current.querySelector<HTMLElement>(
      `[data-line="${range.end}"]`,
    );
    if (!last) return;
    // In split view, [data-line] is on a cell inside .diff-row; offsetTop is
    // relative to the row, not the diff container. Anchor to the row.
    const anchorEl = last.closest<HTMLElement>(".diff-row") ?? last;
    setPopupPos({
      top: anchorEl.offsetTop + anchorEl.offsetHeight,
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
    // Only the gutter (line-number column with the "+" affordance) starts a comment.
    if (!target.closest(".diff-lineno")) return;
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

  // Compute the largest known new-side line number visible in this diff. This
  // includes rendered lines (data-line) and the far end of any unexpanded gap
  // (data-new-end on .diff-expander). It's the best total available without
  // an extra API call.
  const computeTotalLines = useCallback((): number => {
    const root = diffRef.current;
    if (!root) return 0;
    let max = 0;
    root.querySelectorAll<HTMLElement>("[data-line]").forEach((el) => {
      const n = Number(el.getAttribute("data-line"));
      if (Number.isFinite(n) && n > max) max = n;
    });
    root.querySelectorAll<HTMLElement>(".diff-expander").forEach((el) => {
      const n = Number(el.getAttribute("data-new-end") || "0");
      if (Number.isFinite(n) && n > max) max = n;
    });
    return max;
  }, []);

  const openJumpDialog = useCallback(() => {
    setJumpTotal(computeTotalLines());
    setJumpInput("");
    setJumpError("");
    setJumpOpen(true);
    requestAnimationFrame(() => {
      jumpInputRef.current?.focus();
      jumpInputRef.current?.select();
    });
  }, [computeTotalLines]);

  // Ctrl+G / Cmd+G opens the jump-to-line dialog.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "g") {
        e.preventDefault();
        openJumpDialog();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openJumpDialog]);

  // Find a match's containing data-line element, then its row (for split view).
  const scrollToMatch = useCallback((idx: number) => {
    const matches = searchMatchesRef.current;
    if (matches.length === 0) return;
    const safe = ((idx % matches.length) + matches.length) % matches.length;
    const el = matches[safe];
    const lineEl = el.closest<HTMLElement>("[data-line]");
    const row = (lineEl?.closest<HTMLElement>(".diff-row")) ?? lineEl ?? el;
    row.scrollIntoView({ block: "center", behavior: "smooth" });
    if (lineEl) {
      const n = Number(lineEl.getAttribute("data-line"));
      if (Number.isFinite(n) && n > 0) setHighlight(n, n);
    }
    setSearchIdx(safe);
  }, [setHighlight]);

  // Recompute matches whenever the query changes. Hidden context lines (inside
  // unexpanded gaps) are not in the DOM, so they're not searchable until the
  // user expands them — same limitation as Cmd+G.
  const runSearch = useCallback((q: string) => {
    const root = diffRef.current;
    if (!root || !q) {
      searchMatchesRef.current = [];
      setSearchHits(0);
      setSearchIdx(0);
      return;
    }
    const needle = q.toLowerCase();
    const matches: HTMLElement[] = [];
    root.querySelectorAll<HTMLElement>(".diff-content").forEach((el) => {
      if ((el.textContent ?? "").toLowerCase().includes(needle)) {
        matches.push(el);
      }
    });
    searchMatchesRef.current = matches;
    setSearchHits(matches.length);
    if (matches.length > 0) scrollToMatch(0);
    else setSearchIdx(0);
  }, [scrollToMatch]);

  const openSearch = useCallback(() => {
    setSearchOpen(true);
    requestAnimationFrame(() => {
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
    });
  }, []);

  const closeSearch = useCallback(() => {
    setSearchOpen(false);
    searchMatchesRef.current = [];
    setSearchHits(0);
    setSearchIdx(0);
  }, []);

  // Ctrl+F / Cmd+F opens the search bar.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        openSearch();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openSearch]);

  // Re-run search when the diff html changes (e.g. after expanding context)
  // so the hit count stays accurate; preserves the current query.
  useEffect(() => {
    if (searchOpen && searchQuery) runSearch(searchQuery);
  }, [html]);

  const handleJumpSubmit = () => {
    const n = Number(jumpInput.trim());
    if (!Number.isInteger(n) || n <= 0) {
      setJumpError("Enter a positive line number.");
      return;
    }
    const root = diffRef.current;
    if (!root) return;
    const el = root.querySelector<HTMLElement>(`[data-line="${n}"]`);
    if (!el) {
      setJumpError(
        `Line ${n} is not currently shown. Expand context to reveal it.`,
      );
      return;
    }
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    setHighlight(n, n);
    setJumpOpen(false);
  };

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
      // Reveal the top of the hidden range (adjacent to the hunk above / file start).
      fetchStart = r.newStart;
      fetchEnd = Math.min(r.newEnd, r.newStart + EXPAND_CHUNK - 1);
    } else {
      // Reveal the bottom of the hidden range (adjacent to the hunk below).
      fetchStart = Math.max(r.newStart, r.newEnd - EXPAND_CHUNK + 1);
      fetchEnd = r.newEnd;
    }
    // The old-side line number to start at, aligned with the new-side fetch range.
    const fetchOldStart = r.oldStart + (fetchStart - r.newStart);
    expander.setAttribute("data-loading", "1");
    try {
      const lines = await getFileLines(projectId, repoId, sourceCommit, path, fetchStart, fetchEnd);
      const linesHtml =
        view === "split"
          ? renderEqualLinesSplit(lines, fetchStart, fetchOldStart)
          : renderEqualLinesInline(lines, fetchStart);
      const consumed = fetchEnd - fetchStart + 1;
      const remainingTotal = total - consumed;
      if (remainingTotal <= 0) {
        expander.outerHTML = linesHtml;
      } else if (action === "up") {
        // Consumed lines were at the TOP of the gap; they sit above the
        // expander and the remaining hidden range starts further down.
        expander.setAttribute("data-old-start", String(r.oldStart + consumed));
        expander.setAttribute("data-new-start", String(r.newStart + consumed));
        const allBtn = expander.querySelector(".diff-expander-all");
        if (allBtn) allBtn.textContent = `${remainingTotal} hidden lines`;
        expander.insertAdjacentHTML("beforebegin", linesHtml);
        expander.removeAttribute("data-loading");
      } else {
        // Consumed lines were at the BOTTOM of the gap; they sit below the
        // expander and the remaining hidden range ends further up.
        expander.setAttribute("data-old-end", String(r.oldEnd - consumed));
        expander.setAttribute("data-new-end", String(r.newEnd - consumed));
        const allBtn = expander.querySelector(".diff-expander-all");
        if (allBtn) allBtn.textContent = `${remainingTotal} hidden lines`;
        expander.insertAdjacentHTML("afterend", linesHtml);
        expander.removeAttribute("data-loading");
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
    <div class="h-full flex flex-col overflow-hidden relative">
      {isMarkdown && (
        <button
          onClick={() => setShowMarkdownPreview((v) => !v)}
          class="absolute top-2 right-3 z-30 px-3 py-1.5 rounded bg-white/75 dark:bg-gray-900/75 border border-gray-300/70 dark:border-gray-600/70 shadow-sm backdrop-blur text-xs font-medium text-gray-700 dark:text-gray-200 hover:bg-white/90 dark:hover:bg-gray-900/90"
        >
          {showMarkdownPreview ? "View Raw" : "View Preview"}
        </button>
      )}
      {/* The diff scrolls inside this container so the comments bar below
          remains visible without scrolling. */}
      <div class="flex-1 overflow-auto relative">
      {/* File header */}
      <div class="diff-header sticky top-0 z-10">{path}</div>

      {searchOpen && (
        <div
          class="sticky top-0 z-20 ml-auto mr-2 mt-1 w-[min(420px,calc(100%-1rem))] float-right shadow-lg rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900 p-2 flex items-center gap-2"
          // Stop diff-area mouse handlers from owning clicks in the bar.
          onMouseDown={(e) => e.stopPropagation()}
        >
          <input
            ref={searchInputRef}
            type="text"
            value={searchQuery}
            onInput={(e) => {
              const q = e.currentTarget.value;
              setSearchQuery(q);
              runSearch(q);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") { e.preventDefault(); closeSearch(); }
              else if (e.key === "Enter") {
                e.preventDefault();
                if (searchMatchesRef.current.length > 0) {
                  scrollToMatch(searchIdx + (e.shiftKey ? -1 : 1));
                }
              }
            }}
            placeholder="Find in file… (Enter / Shift+Enter)"
            class="flex-1 min-w-0 px-2 py-1 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
          />
          <span class="text-xs text-gray-500 whitespace-nowrap tabular-nums">
            {searchHits === 0
              ? searchQuery ? "0/0" : ""
              : `${searchIdx + 1}/${searchHits}`}
          </span>
          <button
            onClick={() => scrollToMatch(searchIdx - 1)}
            disabled={searchHits === 0}
            title="Previous (Shift+Enter)"
            class="px-1.5 py-0.5 text-xs text-gray-500 hover:text-gray-800 dark:hover:text-gray-200 disabled:opacity-40"
          >▲</button>
          <button
            onClick={() => scrollToMatch(searchIdx + 1)}
            disabled={searchHits === 0}
            title="Next (Enter)"
            class="px-1.5 py-0.5 text-xs text-gray-500 hover:text-gray-800 dark:hover:text-gray-200 disabled:opacity-40"
          >▼</button>
          <button
            onClick={closeSearch}
            title="Close (Esc)"
            class="px-1.5 py-0.5 text-xs text-gray-500 hover:text-gray-800 dark:hover:text-gray-200"
          >×</button>
        </div>
      )}

      {/* Selection + popup live inside this relative container so the popup
          can be positioned with absolute coords against the diff content. */}
      <div class="relative">
        {showMarkdownPreview ? (
          <div
            class="diff-markdown-preview"
            dangerouslySetInnerHTML={{ __html: markdownPreviewHtml }}
          />
        ) : (
          <div
            ref={diffRef}
            dangerouslySetInnerHTML={{ __html: html }}
            onMouseDown={onMouseDown}
            onMouseMove={onMouseMove}
            onClick={onClickContainer}
          />
        )}

        {!showMarkdownPreview && range && popupPos && (
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

      {jumpOpen && (
        <div
          class="fixed inset-0 z-30 flex items-start justify-center pt-32 bg-black/40"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) setJumpOpen(false);
          }}
        >
          <div class="w-80 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900 p-4 shadow-xl">
            <div class="text-sm font-medium mb-1">Jump to line</div>
            <div class="text-xs text-gray-500 mb-2">
              {jumpTotal > 0
                ? `File has ${jumpTotal} line${jumpTotal === 1 ? "" : "s"}`
                : "Line count unavailable"}
            </div>
            <input
              ref={jumpInputRef}
              type="number"
              min={1}
              value={jumpInput}
              onInput={(e) => {
                setJumpInput(e.currentTarget.value);
                if (jumpError) setJumpError("");
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  handleJumpSubmit();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  setJumpOpen(false);
                }
              }}
              placeholder="Line number"
              class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
            />
            {jumpError && (
              <div class="mt-2 text-xs text-red-600 dark:text-red-400">
                {jumpError}
              </div>
            )}
            <div class="flex gap-2 mt-3 justify-end">
              <button
                onClick={() => setJumpOpen(false)}
                class="px-3 py-1 text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
              >
                Cancel
              </button>
              <button
                onClick={handleJumpSubmit}
                class="px-3 py-1 bg-accent hover:bg-accent-hover text-white rounded text-xs font-medium"
              >
                Jump
              </button>
            </div>
          </div>
        </div>
      )}

      </div>
      {/* Comments bar — sits below the scrolling diff so it's always visible
          when there are comments. Hidden entirely when empty. */}
      {threads.length > 0 && (
        <div class="shrink-0 border-t border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 max-h-[40vh] overflow-y-auto">
          <div class="text-xs font-medium text-gray-500 dark:text-gray-400 px-3 py-1.5 border-b border-gray-100 dark:border-gray-800 bg-gray-50 dark:bg-gray-800/60">
            Comments ({threads.length})
          </div>
          {threads.map((t) => (
            <div key={t.id} class="border-t border-gray-100 dark:border-gray-800 p-3">
              <div class="text-xs text-gray-400 mb-1">
                {t.lineStart > 0
                  ? `Thread on line ${t.lineStart === t.lineEnd ? t.lineStart : `${t.lineStart}-${t.lineEnd}`}`
                  : "File-level thread"}
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
                  <div class="mt-1 text-gray-700 dark:text-gray-300 whitespace-pre-wrap break-words">
                    {c.content || <span class="italic text-gray-400">(no content)</span>}
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
