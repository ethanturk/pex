import { useState } from "preact/hooks";
import type { CommentThread } from "@/lib/api";

interface Props {
  html: string;
  path: string;
  threads: CommentThread[];
  onComment: (filePath: string, line: number, content: string) => Promise<void>;
}

export function DiffViewer({ html, path, threads, onComment }: Props) {
  const [commentLine, setCommentLine] = useState<number | null>(null);
  const [commentText, setCommentText] = useState("");
  const [posting, setPosting] = useState(false);

  const handleLineClick = (line: number) => {
    setCommentLine(line === commentLine ? null : line);
    setCommentText("");
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
