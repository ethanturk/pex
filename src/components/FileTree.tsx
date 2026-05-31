import { useState, useMemo, useEffect, useRef } from "preact/hooks";
import { selectedFile, fileTreeMode, visibleFilePaths, openPreviewTab, pinTab } from "@/lib/signals";
import type { FileEntry } from "@/lib/signals";
import { STATUS_ICON, STATUS_COLOR } from "@/lib/fileStatus";

interface Props {
  files: FileEntry[];
  onToggleViewed: (path: string, viewed: boolean) => void;
}

interface FolderNode {
  kind: "folder";
  name: string;
  path: string; // folder path, "/"-joined, no leading slash
  children: TreeNode[];
}
interface FileNode {
  kind: "file";
  name: string;
  file: FileEntry;
}
type TreeNode = FolderNode | FileNode;

function buildTree(files: FileEntry[]): FolderNode {
  const root: FolderNode = { kind: "folder", name: "", path: "", children: [] };
  for (const file of files) {
    const parts = file.path.split("/").filter((p) => p.length > 0);
    let cursor = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const segment = parts[i];
      const folderPath = parts.slice(0, i + 1).join("/");
      let next = cursor.children.find(
        (c): c is FolderNode => c.kind === "folder" && c.name === segment,
      );
      if (!next) {
        next = { kind: "folder", name: segment, path: folderPath, children: [] };
        cursor.children.push(next);
      }
      cursor = next;
    }
    cursor.children.push({ kind: "file", name: parts[parts.length - 1] ?? file.path, file });
  }
  // Sort: folders first, then files; alphabetical within each group.
  const sort = (node: FolderNode) => {
    node.children.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === "folder" ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    for (const child of node.children) if (child.kind === "folder") sort(child);
  };
  sort(root);
  return root;
}

function FileRow({
  file,
  depth,
  showFullPath,
  onToggleViewed,
}: {
  file: FileEntry;
  depth: number;
  showFullPath: boolean;
  onToggleViewed: Props["onToggleViewed"];
}) {
  const activeFile = selectedFile.value;
  const isActive = activeFile === file.path;
  const label = showFullPath ? file.path : file.path.split("/").pop() || file.path;
  const rowRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (isActive && rowRef.current) {
      rowRef.current.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }, [isActive]);
  return (
    <div
      ref={rowRef}
      class={`file-tree-item ${file.viewed ? "file-tree-item--viewed" : ""} ${isActive ? "file-tree-item--active" : ""}`}
      style={{ paddingLeft: `${depth * 12 + 8}px` }}
      onClick={() => openPreviewTab(file.path)}
      onDblClick={() => pinTab(file.path)}
      title={file.path}
    >
      <span
        class={`text-xs font-mono w-4 text-center shrink-0 ${STATUS_COLOR[file.status] || ""}`}
        title={file.status}
      >
        {STATUS_ICON[file.status] || "?"}
      </span>
      <span class="flex-1 text-left truncate text-[13px]">
        {label}
      </span>
      <button
        class="shrink-0 text-xs px-1 text-gray-300 dark:text-gray-600 hover:text-gray-500 dark:hover:text-gray-400"
        onClick={(e) => {
          e.stopPropagation();
          onToggleViewed(file.path, !file.viewed);
        }}
        title={file.viewed ? "Mark unviewed" : "Mark viewed"}
      >
        {file.viewed ? "👁" : "◌"}
      </button>
    </div>
  );
}

function TreeNodes({
  nodes,
  depth,
  collapsed,
  toggleFolder,
  onToggleViewed,
}: {
  nodes: TreeNode[];
  depth: number;
  collapsed: Set<string>;
  toggleFolder: (path: string) => void;
  onToggleViewed: Props["onToggleViewed"];
}) {
  return (
    <>
      {nodes.map((node) =>
        node.kind === "folder" ? (
          <FolderRow
            key={`d:${node.path}`}
            node={node}
            depth={depth}
            collapsed={collapsed}
            toggleFolder={toggleFolder}
            onToggleViewed={onToggleViewed}
          />
        ) : (
          <FileRow
            key={`f:${node.file.path}`}
            file={node.file}
            depth={depth}
            showFullPath={false}
            onToggleViewed={onToggleViewed}
          />
        ),
      )}
    </>
  );
}

function FolderRow({
  node,
  depth,
  collapsed,
  toggleFolder,
  onToggleViewed,
}: {
  node: FolderNode;
  depth: number;
  collapsed: Set<string>;
  toggleFolder: (path: string) => void;
  onToggleViewed: Props["onToggleViewed"];
}) {
  const isCollapsed = collapsed.has(node.path);
  return (
    <>
      <button
        class="file-tree-item w-full text-left"
        style={{ paddingLeft: `${depth * 12 + 4}px` }}
        onClick={() => toggleFolder(node.path)}
      >
        <span class="w-4 text-center text-xs text-gray-400 shrink-0">
          {isCollapsed ? "▸" : "▾"}
        </span>
        <span class="flex-1 truncate text-[13px] text-gray-600 dark:text-gray-300">
          {node.name}
        </span>
      </button>
      {!isCollapsed && (
        <TreeNodes
          nodes={node.children}
          depth={depth + 1}
          collapsed={collapsed}
          toggleFolder={toggleFolder}
          onToggleViewed={onToggleViewed}
        />
      )}
    </>
  );
}

function flattenTree(nodes: TreeNode[], collapsed: Set<string>, out: string[]) {
  for (const node of nodes) {
    if (node.kind === "file") {
      out.push(node.file.path);
    } else {
      if (!collapsed.has(node.path)) {
        flattenTree(node.children, collapsed, out);
      }
    }
  }
}

export function FileTree({ files, onToggleViewed }: Props) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const trimmedFilter = filter.trim().toLowerCase();
  const filteredFiles = useMemo(() => {
    if (!trimmedFilter) return files;
    return files.filter((f) => {
      const name = f.path.split("/").pop() ?? f.path;
      return name.toLowerCase().includes(trimmedFilter);
    });
  }, [files, trimmedFilter]);
  const tree = useMemo(() => buildTree(filteredFiles), [filteredFiles]);

  const toggleFolder = (path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  // When the selected file changes (e.g. via "jump to finding" from the PR
  // review sidebar), expand any collapsed ancestor folders so the row exists
  // in the DOM and the active highlight is actually visible.
  const active = selectedFile.value;
  useEffect(() => {
    if (!active) return;
    const parts = active.split("/").filter((p) => p.length > 0);
    if (parts.length < 2) return;
    setCollapsed((prev) => {
      let changed = false;
      const next = new Set(prev);
      for (let i = 0; i < parts.length - 1; i++) {
        const folderPath = parts.slice(0, i + 1).join("/");
        if (next.delete(folderPath)) changed = true;
      }
      return changed ? next : prev;
    });
  }, [active]);

  const mode = fileTreeMode.value;

  useEffect(() => {
    if (mode === "flat") {
      visibleFilePaths.value = filteredFiles.map((f) => f.path);
    } else {
      const out: string[] = [];
      flattenTree(tree.children, collapsed, out);
      visibleFilePaths.value = out;
    }
  }, [filteredFiles, tree, collapsed, mode]);

  return (
    <div class="flex flex-col">
      <div class="flex items-center justify-between px-2 py-1 border-b border-gray-200 dark:border-gray-800 sticky top-0 bg-white dark:bg-gray-950 z-10">
        <span class="text-[11px] uppercase tracking-wide text-gray-400">
          Files ({trimmedFilter ? `${filteredFiles.length}/${files.length}` : files.length})
        </span>
        <button
          class="text-sm px-1.5 py-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-700"
          onClick={() =>
            (fileTreeMode.value = mode === "flat" ? "tree" : "flat")
          }
          title={
            mode === "flat"
              ? "View: flat list (click for folder tree)"
              : "View: folder tree (click for flat list)"
          }
          aria-label="Toggle file view"
        >
          {mode === "flat" ? "🗂" : "📄"}
        </button>
      </div>

      <div class="px-2 py-1 border-b border-gray-200 dark:border-gray-800 sticky top-[27px] bg-white dark:bg-gray-950 z-10">
        <div class="relative">
          <input
            type="text"
            value={filter}
            onInput={(e) => setFilter(e.currentTarget.value)}
            placeholder="Filter by filename..."
            class="w-full text-xs px-2 py-1 pr-6 rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 focus:outline-none focus:ring-1 focus:ring-accent"
          />
          {filter && (
            <button
              onClick={() => setFilter("")}
              class="absolute right-1 top-1/2 -translate-y-1/2 w-4 h-4 flex items-center justify-center text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 text-sm leading-none"
              title="Clear filter"
              aria-label="Clear filter"
            >
              ×
            </button>
          )}
        </div>
      </div>

      {files.length === 0 ? (
        <div class="p-3 text-xs text-gray-400">No files changed.</div>
      ) : filteredFiles.length === 0 ? (
        <div class="p-3 text-xs text-gray-400">No files match "{filter}".</div>
      ) : mode === "flat" ? (
        <div class="py-1">
          {filteredFiles.map((f) => (
            <FileRow
              key={f.path}
              file={f}
              depth={0}
              showFullPath={true}
              onToggleViewed={onToggleViewed}
            />
          ))}
        </div>
      ) : (
        <div class="py-1">
          <TreeNodes
            nodes={tree.children}
            depth={0}
            collapsed={collapsed}
            toggleFolder={toggleFolder}
            onToggleViewed={onToggleViewed}
          />
        </div>
      )}
    </div>
  );
}
