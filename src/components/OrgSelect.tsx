import { activeOrg, currentView, savedOrgs } from "@/lib/signals";
import { removeOrg, activateOrg } from "@/lib/api";
import { useState } from "preact/hooks";

export function OrgSelect() {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  return (
    <div class="flex items-center justify-center h-full">
      <div class="w-full max-w-sm space-y-4">
        <h2 class="text-xl font-semibold">Choose an Organization</h2>
        {error && (
          <div class="px-3 py-2 rounded-lg border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20 text-sm text-red-700 dark:text-red-300 break-words">
            {error}
          </div>
        )}
        <div class="space-y-2">
          {savedOrgs.value.map((org) => (
            <div
              key={org.orgUrl}
              class="flex items-center justify-between px-4 py-3 rounded-lg border border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
            >
              <button
                class="text-left flex-1 disabled:opacity-50"
                disabled={busy === org.orgUrl}
                onClick={async () => {
                  setError("");
                  setBusy(org.orgUrl);
                  try {
                    await activateOrg(org.orgUrl);
                    activeOrg.value = org;
                    currentView.value = { kind: "pr-list" };
                  } catch (e: any) {
                    setError(
                      `Couldn't sign in to ${org.name}: ${
                        typeof e === "string" ? e : e?.message ?? String(e)
                      }`
                    );
                  } finally {
                    setBusy(null);
                  }
                }}
              >
                <div class="font-medium text-sm">{org.name}</div>
                <div class="text-xs text-gray-400">{org.orgUrl}</div>
              </button>
              <button
                class="text-xs text-red-500 hover:text-red-700 px-2 py-1"
                onClick={async () => {
                  await removeOrg(org.orgUrl);
                  savedOrgs.value = savedOrgs.value.filter((o) => o.orgUrl !== org.orgUrl);
                  if (savedOrgs.value.length === 0) {
                    currentView.value = { kind: "auth" };
                  }
                }}
              >
                Remove
              </button>
            </div>
          ))}
        </div>
        <button
          class="text-sm text-accent hover:underline"
          onClick={() => (currentView.value = { kind: "auth" })}
        >
          + Add another account
        </button>
      </div>
    </div>
  );
}
