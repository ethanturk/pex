import { diffView } from "@/lib/signals";

export function DiffViewToggle() {
  const toggle = () => {
    diffView.value = diffView.value === "inline" ? "split" : "inline";
  };

  const isSplit = diffView.value === "split";
  const icon = isSplit ? "⇋" : "≡";
  const title = isSplit
    ? "Diff layout: side-by-side (click for inline)"
    : "Diff layout: inline (click for side-by-side)";

  return (
    <button
      onClick={toggle}
      class="text-sm px-1.5 py-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700"
      title={title}
      aria-label={title}
    >
      {icon}
    </button>
  );
}
