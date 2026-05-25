import { selectedFile } from "@/lib/signals";
import type { FileEntry } from "@/lib/signals";

interface Props {
  files: FileEntry[];
  onToggleViewed: (path: string, viewed: boolean) => void;
}

const STATUS_ICON: Record<string, string> = {
  add: "+",
  edit: "~",
  delete: "−",
  rename: "→",
};

const STATUS_COLOR: Record<string, string> = {
  add: "text-green-500",
  edit: "text-yellow-500",
  delete: "text-red-500",
  rename: "text-blue-500",
};

export function FileTree({ files, onToggleViewed }: Props) {
  if (files.length === 0) {
    return <div class="p-3 text-xs text-gray-400">No files changed.</div>;
  }

  const activeFile = selectedFile.value;

  return (
    <div class="py-1">
      {files.map((f) => (
        <div
          key={f.path}
          class={`file-tree-item ${f.viewed ? "file-tree-item--viewed" : ""} ${activeFile === f.path ? "file-tree-item--active" : ""}`}
        >
          <span
            class={`text-xs font-mono w-4 text-center shrink-0 ${STATUS_COLOR[f.status] || ""}`}
            title={f.status}
          >
            {STATUS_ICON[f.status] || "?"}
          </span>
          <button
            class="flex-1 text-left truncate text-[13px]"
            onClick={() => (selectedFile.value = f.path)}
          >
            {f.path}
          </button>
          <button
            class="shrink-0 text-xs px-1 text-gray-300 dark:text-gray-600 hover:text-gray-500 dark:hover:text-gray-400"
            onClick={(e) => {
              e.stopPropagation();
              onToggleViewed(f.path, !f.viewed);
            }}
            title={f.viewed ? "Mark unviewed" : "Mark viewed"}
          >
            {f.viewed ? "👁" : "◌"}
          </button>
        </div>
      ))}
    </div>
  );
}
