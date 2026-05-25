import { useState } from "preact/hooks";
import type { CommentThread } from "@/lib/api";
import { getFileLines } from "@/lib/api";

interface Props {
  html: string;
  path: string;
  threads: CommentThread[];
  onComment: (filePath: string, line: number, content: string) => Promise<void>;
  projectId: string;
  repoId: string;
  sourceCommit: string;
  baseCommit: string | null;
}

const EXPAND_CHUNK = 10;

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
  const [commentLine, setCommentLine] = useState<number | null>(null);
  const [commentText, setCommentText] = useState("");
  const [posting, setPosting] = useState(false);

  const handleLineClick = (line: number) => {
    setCommentLine(line === commentLine ? null : line);
    setCommentText("");
  };

  // Read numeric data-* attributes off an expander element.
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
    // Unchanged-gap regions have matching widths on both sides, so we fetch from
    // the source commit using the new-side line numbers — what the user sees.
    const total = Math.max(r.newEnd - r.newStart + 1, r.oldEnd - r.oldStart + 1);
    if (total <= 0) return;

    let fetchStart: number;
    let fetchEnd: number;
    if (action === "all" || total <= EXPAND_CHUNK) {
      fetchStart = r.newStart;
      fetchEnd = r.newEnd;
    } else if (action === "up") {
      // Reveal context just before the hunk *below* this expander.
      fetchStart = Math.max(r.newStart, r.newEnd - EXPAND_CHUNK + 1);
      fetchEnd = r.newEnd;
    } else {
      // "down": context just after the hunk *above* this expander.
      fetchStart = r.newStart;
      fetchEnd = Math.min(r.newEnd, r.newStart + EXPAND_CHUNK - 1);
    }

    expander.setAttribute("data-loading", "1");
    try {
      const lines = await getFileLines(
        projectId,
        repoId,
        sourceCommit,
        path,
        fetchStart,
        fetchEnd,
      );
      const linesHtml = renderEqualLines(lines, fetchStart);

      // Recompute remaining hidden ranges and either replace or shrink the expander.
      const consumed = fetchEnd - fetchStart + 1;
      const remainingTotal = total - consumed;
      if (remainingTotal <= 0) {
        expander.outerHTML = linesHtml;
      } else if (action === "up") {
        // We took the tail; shrink the expander's end on both sides.
        const newOldEnd = r.oldEnd - consumed;
        const newNewEnd = r.newEnd - consumed;
        expander.setAttribute("data-old-end", String(newOldEnd));
        expander.setAttribute("data-new-end", String(newNewEnd));
        const allBtn = expander.querySelector(".diff-expander-all");
        if (allBtn) allBtn.textContent = `${remainingTotal} hidden lines`;
        expander.insertAdjacentHTML("afterend", linesHtml);
      } else {
        // "down" or "all" with leftover (shouldn't usually happen): took the head.
        const newOldStart = r.oldStart + consumed;
        const newNewStart = r.newStart + consumed;
        expander.setAttribute("data-old-start", String(newOldStart));
        expander.setAttribute("data-new-start", String(newNewStart));
        const allBtn = expander.querySelector(".diff-expander-all");
        if (allBtn) allBtn.textContent = `${remainingTotal} hidden lines`;
        expander.insertAdjacentHTML("beforebegin", linesHtml);
      }
    } catch (e) {
      console.error("Failed to expand context:", e);
      expander.removeAttribute("data-loading");
    }
  };

  const handlePost = async () => {
    if (!commentText.trim() || commentLine === null) return;
    setPosting(true);
    try {
      await onComment(path, commentLine, commentText);
      setCommentLine(null);
      setCommentText("");
    } finally {
      setPosting(false);
    }
  };

  return (
    <div class="overflow-x-auto">
      {/* File header */}
      <div class="diff-header sticky top-0 z-10">{path}</div>

      {/* Diff content — rendered as HTML from the Rust backend */}
      <div
        dangerouslySetInnerHTML={{ __html: html }}
        onClick={(e) => {
          const target = e.target as HTMLElement;
          // Expander click takes precedence: the button lives inside the .diff-expander row.
          const btn = target.closest(".diff-expander-btn") as HTMLElement | null;
          if (btn) {
            const expander = btn.closest(".diff-expander") as HTMLElement | null;
            const action = btn.getAttribute("data-action") as "up" | "down" | "all" | null;
            if (expander && action && !expander.hasAttribute("data-loading")) {
              expandRange(expander, action);
            }
            return;
          }
          const lineEl = target.closest("[data-line]");
          if (lineEl) {
            const ln = Number(lineEl.getAttribute("data-line"));
            if (!isNaN(ln) && ln > 0) handleLineClick(ln);
          }
        }}
      />

      {/* Inline comment form */}
      {commentLine !== null && (
        <div class="border-t border-gray-200 dark:border-gray-700 p-3 bg-gray-50 dark:bg-gray-800/50">
          <div class="text-xs text-gray-500 mb-1">Comment on line {commentLine} in {path}</div>
          <textarea
            value={commentText}
            onInput={(e) => setCommentText(e.currentTarget.value)}
            class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent resize-none"
            rows={3}
            placeholder="Leave a comment (Markdown supported)"
          />
          <div class="flex gap-2 mt-2">
            <button
              onClick={handlePost}
              disabled={!commentText.trim() || posting}
              class="px-3 py-1 bg-accent hover:bg-accent-hover text-white rounded text-xs font-medium disabled:opacity-50"
            >
              {posting ? "Posting..." : "Comment"}
            </button>
            <button
              onClick={() => { setCommentLine(null); setCommentText(""); }}
              class="px-3 py-1 text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Existing threads */}
      {threads.map((t) => (
        <div key={t.id} class="border-t border-gray-100 dark:border-gray-800 p-3">
          <div class="text-xs text-gray-400 mb-1">
            Thread on line {t.lineStart === t.lineEnd ? t.lineStart : `${t.lineStart}-${t.lineEnd}`}
          </div>
          {t.comments.map((c) => (
            <div key={c.id} class="text-sm mb-2 pl-3 border-l-2 border-gray-200 dark:border-gray-700">
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
