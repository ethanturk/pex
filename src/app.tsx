import { currentView, type View } from "@/lib/signals";
import { AuthScreen } from "@/components/AuthScreen";
import { OrgSelect } from "@/components/OrgSelect";
import { PRList } from "@/components/PRList";
import { PRDetail } from "@/components/PRDetail";
import { ThemeToggle } from "@/components/ThemeToggle";
import { useEffectOnce } from "@/lib/hooks";
import { getSavedOrgs } from "@/lib/api";
import { activeOrg, savedOrgs } from "@/lib/signals";

function viewComponent(view: View) {
  switch (view.kind) {
    case "auth":
      return <AuthScreen />;
    case "org-select":
      return <OrgSelect />;
    case "pr-list":
      return <PRList />;
    case "pr-detail":
      return <PRDetail prId={view.prId} />;
  }
}

export function App() {
  useEffectOnce(() => {
    // Check if we have saved orgs — if so, go to org-select instead of auth
    getSavedOrgs().then((orgs) => {
      savedOrgs.value = orgs;
      if (orgs.length === 1) {
        // Single org: skip org-select, go straight to PR list
        activeOrg.value = orgs[0];
        currentView.value = { kind: "pr-list" };
      } else if (orgs.length > 1) {
        currentView.value = { kind: "org-select" };
      }
    });
  });

  return (
    <div class="flex flex-col h-screen">
      {/* Header */}
      <header class="flex items-center justify-between px-4 py-2 border-b border-gray-200 dark:border-gray-800 bg-gray-50 dark:bg-gray-900 shrink-0">
        <div class="flex items-center gap-3">
          <h1 class="text-lg font-bold tracking-tight">Pex</h1>
          <span class="text-xs text-gray-400 dark:text-gray-500 hidden sm:inline">
            Azure DevOps PR Reviewer
          </span>
        </div>
        <div class="flex items-center gap-2">
          {activeOrg.value && (
            <button
              class="text-xs px-2 py-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-500 dark:text-gray-400"
              onClick={() => (currentView.value = { kind: "org-select" })}
            >
              {activeOrg.value.name}
            </button>
          )}
          <ThemeToggle />
        </div>
      </header>

      {/* Main content */}
      <main class="flex-1 overflow-hidden">{viewComponent(currentView.value)}</main>
    </div>
  );
}
