import { currentView, activeOrg, savedOrgs } from "@/lib/signals";
import { loginPat, getSavedOrgs, removeOrg } from "@/lib/api";
import { useState } from "preact/hooks";

export function AuthScreen() {
  const [orgUrl, setOrgUrl] = useState("");
  const [pat, setPat] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const handlePatLogin = async () => {
    setError("");
    setLoading(true);
    try {
      const ok = await loginPat(orgUrl, pat);
      if (ok) {
        activeOrg.value = { orgUrl, name: new URL(orgUrl).hostname, tokenType: "pat" };
        const orgs = await getSavedOrgs();
        savedOrgs.value = orgs;
        currentView.value = { kind: "pr-list" };
      } else {
        setError("Invalid credentials. Check your org URL and PAT.");
      }
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setLoading(false);
    }
  };

  const existingOrgs = savedOrgs.value;
  if (existingOrgs.length > 0) {
    return (
      <div class="flex items-center justify-center h-full">
        <div class="w-full max-w-sm space-y-4">
          <h2 class="text-xl font-semibold">Connect to Azure DevOps</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">Select an account or add a new one.</p>
          <div class="space-y-2">
            {existingOrgs.map((org) => (
              <button
                key={org.orgUrl}
                class="w-full text-left px-4 py-3 rounded-lg border border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
                onClick={async () => {
                  activeOrg.value = org;
                  currentView.value = { kind: "pr-list" };
                }}
              >
                <div class="font-medium text-sm">{org.name}</div>
                <div class="text-xs text-gray-400">{org.orgUrl}</div>
              </button>
            ))}
          </div>
          <details class="text-sm">
            <summary class="cursor-pointer text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300">
              + Add another account
            </summary>
            <div class="mt-3 space-y-3">
              {renderForm()}
            </div>
          </details>
        </div>
      </div>
    );
  }

  return (
    <div class="flex items-center justify-center h-full">
      <div class="w-full max-w-sm space-y-4">
        <h2 class="text-xl font-semibold">Connect to Azure DevOps</h2>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Enter your organization URL and a Personal Access Token with Code (Read & Write) scope.
        </p>
        {renderForm()}
      </div>
    </div>
  );

  function renderForm() {
    return (
      <>
        <div class="space-y-3">
          <input
            type="text"
            placeholder="https://dev.azure.com/your-org"
            value={orgUrl}
            onInput={(e) => setOrgUrl(e.currentTarget.value)}
            class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
          />
          <input
            type="password"
            placeholder="Personal Access Token"
            value={pat}
            onInput={(e) => setPat(e.currentTarget.value)}
            class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
          />
          {error && <p class="text-sm text-red-500">{error}</p>}
          <button
            onClick={handlePatLogin}
            disabled={loading || !orgUrl || !pat}
            class="w-full py-2 px-4 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {loading ? "Connecting..." : "Connect"}
          </button>
        </div>
      </>
    );
  }
}
