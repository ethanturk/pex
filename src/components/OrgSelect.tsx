import { activeOrg, currentView, savedOrgs } from "@/lib/signals";
import { removeOrg } from "@/lib/api";

export function OrgSelect() {
  return (
    <div class="flex items-center justify-center h-full">
      <div class="w-full max-w-sm space-y-4">
        <h2 class="text-xl font-semibold">Choose an Organization</h2>
        <div class="space-y-2">
          {savedOrgs.value.map((org) => (
            <div
              key={org.orgUrl}
              class="flex items-center justify-between px-4 py-3 rounded-lg border border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
            >
              <button
                class="text-left flex-1"
                onClick={() => {
                  activeOrg.value = org;
                  currentView.value = { kind: "pr-list" };
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
