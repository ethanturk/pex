import {
  updateState,
  downloadAndInstall,
  dismissUpdate,
} from "@/lib/updater";

function formatMB(bytes: number): string {
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}

export function UpdateBanner() {
  const s = updateState.value;
  if (s.kind === "idle" || s.kind === "checking") return null;

  // Common shell — keeps the banner height stable across states so the layout
  // doesn't jump when downloading starts.
  return (
    <div class="bg-accent/10 border-b border-accent/30 px-4 py-2 flex items-center gap-3 text-sm">
      {s.kind === "available" && (
        <>
          <span class="text-accent">↑</span>
          <span class="flex-1">
            <strong>Pex v{s.update.version}</strong> is available
            {s.update.body ? ` — ${s.update.body.slice(0, 100)}` : ""}
          </span>
          <button
            onClick={downloadAndInstall}
            class="px-3 py-1 bg-accent hover:bg-accent-hover text-white rounded text-xs font-medium"
          >
            Install and restart
          </button>
          <button
            onClick={dismissUpdate}
            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none px-1"
            title="Dismiss"
            aria-label="Dismiss update notice"
          >
            ×
          </button>
        </>
      )}

      {s.kind === "downloading" && (
        <>
          <span class="animate-spin w-3 h-3 border-2 border-accent/30 border-t-accent rounded-full" />
          <span class="flex-1 flex items-center gap-3">
            <span>Downloading v{s.update.version}…</span>
            {s.total != null && (
              <span class="flex-1 max-w-xs h-1.5 bg-accent/20 rounded-full overflow-hidden">
                <span
                  class="block h-full bg-accent transition-all duration-150"
                  style={{
                    width: `${Math.min(100, Math.round((s.downloaded / s.total) * 100))}%`,
                  }}
                />
              </span>
            )}
            <span class="text-xs text-gray-500 tabular-nums">
              {formatMB(s.downloaded)}
              {s.total != null ? ` / ${formatMB(s.total)}` : ""}
            </span>
          </span>
        </>
      )}

      {s.kind === "ready" && (
        <>
          <span class="text-accent">✓</span>
          <span class="flex-1">Update installed — restarting…</span>
        </>
      )}

      {s.kind === "error" && (
        <>
          <span class="text-red-500">✘</span>
          <span class="flex-1 text-red-600 dark:text-red-400">
            Update failed: {s.message}
          </span>
          <button
            onClick={dismissUpdate}
            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none px-1"
            title="Dismiss"
          >
            ×
          </button>
        </>
      )}
    </div>
  );
}
