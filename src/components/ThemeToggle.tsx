import { theme, applyTheme, type Theme } from "@/lib/signals";

export function ThemeToggle() {
  const cycle = () => {
    const order: Theme[] = ["system", "light", "dark"];
    const idx = order.indexOf(theme.value);
    theme.value = order[(idx + 1) % order.length];
    applyTheme(theme.value);
  };

  const icon = theme.value === "dark" ? "🌙" : theme.value === "light" ? "☀️" : "💻";

  return (
    <button
      onClick={cycle}
      class="text-sm px-1.5 py-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700"
      title={`Theme: ${theme.value} (click to cycle)`}
    >
      {icon}
    </button>
  );
}
