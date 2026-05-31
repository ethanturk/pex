// Shared glyphs/colors for a file's change status, used by both the file tree
// and the main-area tab strip so they stay visually consistent.
export const STATUS_ICON: Record<string, string> = {
  add: "+",
  edit: "~",
  delete: "−",
  rename: "→",
};

export const STATUS_COLOR: Record<string, string> = {
  add: "text-green-500",
  edit: "text-yellow-500",
  delete: "text-red-500",
  rename: "text-blue-500",
};
