import { useState, useEffect } from "preact/hooks";
import { currentView, type View } from "@/lib/signals";
import { AuthScreen } from "@/components/AuthScreen";
import { OrgSelect } from "@/components/OrgSelect";
import { PRList } from "@/components/PRList";
import { PRDetail } from "@/components/PRDetail";
import { AiSettings } from "@/components/AiSettings";
import { MobileReviewActions } from "@/components/ApprovalBar";
import { useEffectOnce } from "@/lib/hooks";
import { getSavedOrgs, activateOrg } from "@/lib/api";
import { activeOrg, savedOrgs } from "@/lib/signals";
import { initReviewBus } from "@/lib/reviewBus";
import { startUpdateCheck } from "@/lib/updater";
import { UpdateBanner } from "@/components/UpdateBanner";
import { getPlatform } from "@/lib/platform";

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

// ──── Mobile Tab Bar ────
type MobileTab = "prs" | "settings";

function MobileTabBar({
  active,
  onSelect,
}: {
  active: MobileTab;
  onSelect: (t: MobileTab) => void;
}) {
  return (
    <nav class="safe-bottom flex items-center justify-around border-t border-gray-200 dark:border-gray-800 bg-gray-50 dark:bg-gray-900 pb-safe shrink-0">
      <button
        class={`flex flex-col items-center gap-0.5 px-4 py-2 text-xs font-medium transition-colors ${
          active === "prs"
            ? "text-accent"
            : "text-gray-500 dark:text-gray-400"
        }`}
        onClick={() => onSelect("prs")}
      >
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
            d="M9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2M9 5a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2M9 5h6" />
        </svg>
        PRs
      </button>
      <button
        class={`flex flex-col items-center gap-0.5 px-4 py-2 text-xs font-medium transition-colors ${
          active === "settings"
            ? "text-accent"
            : "text-gray-500 dark:text-gray-400"
        }`}
        onClick={() => onSelect("settings")}
      >
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
            d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 0 0 1.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 0 0-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 0 0-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 0 0-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 0 0-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 0 0 1.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
            d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0z" />
        </svg>
        Settings
      </button>
    </nav>
  );
}

// ──── Desktop Shell ────
function DesktopShell({ onOpenSettings }: { onOpenSettings: () => void }) {
  return (
    <div class="flex flex-col h-screen">
      <UpdateBanner />
      {/* Header */}
      <header class="flex items-center justify-between px-4 py-2 border-b border-gray-200 dark:border-gray-800 bg-gray-50 dark:bg-gray-900 shrink-0">
        <div class="flex items-center gap-3">
          <h1 class="text-lg font-bold tracking-tight">Pex</h1>
          <span class="text-xs text-gray-400 dark:text-gray-500 hidden sm:inline">
            Azure DevOps PR Reviewer
          </span>
        </div>
        <div class="flex items-center gap-2">
          <button
            class="text-xl px-2 py-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 leading-none"
            onClick={onOpenSettings}
            title="Settings"
            aria-label="Settings"
          >
            ⚙
          </button>
          {activeOrg.value && (
            <button
              class="text-xs px-2 py-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-500 dark:text-gray-400"
              onClick={() => (currentView.value = { kind: "org-select" })}
            >
              {activeOrg.value.name}
            </button>
          )}
        </div>
      </header>
      <main class="flex-1 overflow-hidden">{viewComponent(currentView.value)}</main>
    </div>
  );
}

// ──── Mobile Shell ────
function MobileShell({ tab, onTab }: { tab: MobileTab; onTab: (t: MobileTab) => void }) {
  return (
    <div class="flex flex-col h-screen safe-top">
      {/* Minimal mobile header */}
      <header class="flex items-center justify-between px-3 py-2 border-b border-gray-200 dark:border-gray-800 bg-gray-50 dark:bg-gray-900 shrink-0">
        <h1 class="text-base font-bold tracking-tight">
          {tab === "prs" ? "Pex" : "Settings"}
        </h1>
        <div class="flex items-center gap-1">
          {tab === "prs" && (
            <>
              {activeOrg.value && (
                <button
                  class="text-xs px-2 py-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-500 dark:text-gray-400"
                  onClick={() => (currentView.value = { kind: "org-select" })}
                >
                  {activeOrg.value.name}
                </button>
              )}
            </>
          )}
          {tab === "prs" && currentView.value.kind === "pr-detail" && <MobileReviewActions />}
        </div>
      </header>

      {/* Content area */}
      <main class="flex-1 overflow-hidden scroll-ios">
        {tab === "prs" ? viewComponent(currentView.value) : <AiSettings open onClose={() => {}} standalone />}
      </main>

      <MobileTabBar active={tab} onSelect={onTab} />
    </div>
  );
}

// ──── App Root ────
export function App() {
  const [showAiSettings, setShowAiSettings] = useState(false);
  const [platform, setPlatform] = useState(() => getPlatform());
  const [mobileTab, setMobileTab] = useState<MobileTab>("prs");

  // Track platform changes (orientation, resize)
  useEffect(() => {
    const check = () => setPlatform(getPlatform());
    window.addEventListener("resize", check);
    window.addEventListener("orientationchange", check);
    return () => {
      window.removeEventListener("resize", check);
      window.removeEventListener("orientationchange", check);
    };
  }, []);

  // Start-up org hydration (same as before)
  useEffectOnce(() => {
    initReviewBus();
    startUpdateCheck();
    // Check if we have saved orgs — if so, go to org-select instead of auth
    getSavedOrgs().then(async (orgs) => {
      savedOrgs.value = orgs;
      if (orgs.length === 1) {
        try {
          await activateOrg(orgs[0].orgUrl);
          activeOrg.value = orgs[0];
          currentView.value = { kind: "pr-list" };
        } catch (e) {
          console.error("Failed to activate saved org:", e);
          activeOrg.value = orgs[0];
          currentView.value = { kind: "auth" };
        }
      } else if (orgs.length > 1) {
        currentView.value = { kind: "org-select" };
      }
    });
  });

  const isDesktop = platform === "desktop";

  return (
    <>
      {isDesktop ? (
        <>
          <DesktopShell onOpenSettings={() => setShowAiSettings(true)} />
          <AiSettings
            open={showAiSettings}
            onClose={() => setShowAiSettings(false)}
          />
        </>
      ) : (
        <MobileShell tab={mobileTab} onTab={setMobileTab} />
      )}
    </>
  );
}
