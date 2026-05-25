import type { FileDiff } from "@/lib/api";
import { useState, useEffect } from "preact/hooks";
import { getThreads, postComment, postReply, type CommentThread } from "@/lib/api";

interface Props {
  diff: FileDiff;
}

export function DiffViewer({ diff }: Props) {
  const [threads, setThreads] = useState<CommentThread[]>([]);
  const [commentLine, setCommentLine] = useState<number | null>(null);
  const [commentText, setCommentText] = useState("");

  // Load existing threads for this file
  useEffect(() => {
    // TODO: get projectId/repoId/prId from context
    setThreads([]);
  }, [diff.path]);

  const handleLineClick = (line: number) => {
    setCommentLine(line);
  };

  const handlePost = async () => {
    if (!commentText.trim()) return;
    // TODO: invoke postComment with proper params
    setCommentLine(null);
    setCommentText("");
  };

  return (
    <div class="overflow-x-auto">
      {/* File header */}
      <div class="diff-header sticky top-0 z-10">
        {diff.path}
      </div>

      {/* Diff content — rendered as HTML from the Rust backend */}
      <div
        class="text-[13px] leading-5"
        dangerouslySetInnerHTML={{ __html: diff.html }}
        onClick={(e) => {
          const target = e.target as HTMLElement;
          const lineEl = target.closest("[data-line]");
          if (lineEl) {
            const ln = Number(lineEl.getAttribute("data-line"));
            if (!isNaN(ln)) handleLineClick(ln);
          }
        }}
      />

      {/* Inline comment form */}
      {commentLine !== null && (
        <div class="border-t border-gray-200 dark:border-gray-700 p-3 bg-gray-50 dark:bg-gray-800/50">
          <div class="text-xs text-gray-500 mb-1">Comment on line {commentLine}</div>
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
              disabled={!commentText.trim()}
              class="px-3 py-1 bg-accent hover:bg-accent-hover text-white rounded text-xs font-medium disabled:opacity-50"
            >
              Comment
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
        <div key={t.id} class="border-t border-gray-200 dark:border-gray-700 p-3">
          {t.comments.map((c) => (
            <div key={c.id} class="text-sm mb-2">
              <span class="font-medium text-xs">{c.author}</span>
              <span class="text-xs text-gray-400 ml-2">{c.publishedDate}</span>
              <div class="mt-1 text-gray-700 dark:text-gray-300">{c.content}</div>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
