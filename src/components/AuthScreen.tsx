import { currentView, activeOrg, savedOrgs } from "@/lib/signals";
import { loginPat, loginOAuth, getSavedOrgs, activateOrg } from "@/lib/api";
import { useState } from "preact/hooks";

export function AuthScreen() {
  // If we were routed here because a saved org's credential was missing
  // (e.g. after an upgrade from a build that didn't persist PATs to the
  // keyring), activeOrg will already be set — pre-fill the URL so re-signing
  // in is a single paste.
  const prefilledOrg = activeOrg.value?.orgUrl ?? "";
  const [orgUrl, setOrgUrl] = useState(prefilledOrg);
  const [pat, setPat] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(
    prefilledOrg ? "Your saved session has expired. Sign in again to continue." : "",
  );
  const [oauthOpen, setOauthOpen] = useState(false);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  // Force the form view even when there are saved orgs (used to bypass the
  // account picker when a saved org's credential is missing).
  const [forceForm, setForceForm] = useState(!!prefilledOrg);

  const handleOAuthLogin = async () => {
    if (!orgUrl) {
      setError("Enter your org URL above first.");
      return;
    }
    if (!clientId || !clientSecret) {
      setError("Enter both Client ID and Client Secret.");
      return;
    }
    setError("");
    setLoading(true);
    try {
      await loginOAuth(orgUrl, clientId, clientSecret);
      activeOrg.value = { orgUrl, name: new URL(orgUrl).hostname, tokenType: "oauth" };
      const orgs = await getSavedOrgs();
      savedOrgs.value = orgs;
      currentView.value = { kind: "pr-list" };
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setLoading(false);
    }
  };

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
  if (existingOrgs.length > 0 && !forceForm) {
    return (
      <div class="flex items-center justify-center h-full">
        <div class="w-full max-w-sm space-y-4">
          <h2 class="text-xl font-semibold">Connect to Azure DevOps</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">Select an account or add a new one.</p>
          {error && (
            <div class="px-3 py-2 rounded-lg border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20 text-sm text-red-700 dark:text-red-300 break-words">
              {error}
            </div>
          )}
          <div class="space-y-2">
            {existingOrgs.map((org) => (
              <button
                key={org.orgUrl}
                class="w-full text-left px-4 py-3 rounded-lg border border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
                onClick={async () => {
                  setError("");
                  try {
                    await activateOrg(org.orgUrl);
                    activeOrg.value = org;
                    currentView.value = { kind: "pr-list" };
                  } catch (e: any) {
                    const msg = typeof e === "string" ? e : e?.message ?? String(e);
                    // Missing credential (e.g. upgraded from a build that
                    // didn't persist the PAT) → drop into the form with the
                    // org URL pre-filled so re-signing in is one paste.
                    if (/no saved (pat|oauth)/i.test(msg)) {
                      activeOrg.value = org;
                      setOrgUrl(org.orgUrl);
                      setError("Your saved session has expired. Sign in again to continue.");
                      setForceForm(true);
                    } else {
                      setError(msg);
                    }
                  }
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
              {renderPatForm()}
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
          Sign in with a Personal Access Token or OAuth.
        </p>
        {renderPatForm()}
        <div class="relative">
          <div class="absolute inset-0 flex items-center">
            <div class="w-full border-t border-gray-200 dark:border-gray-700"></div>
          </div>
          <div class="relative flex justify-center text-xs">
            <span class="px-2 bg-white dark:bg-gray-950 text-gray-400">or</span>
          </div>
        </div>
        <button
          onClick={() => setOauthOpen((v) => !v)}
          class="w-full py-2 px-4 border border-gray-300 dark:border-gray-600 rounded-lg text-sm font-medium hover:bg-gray-50 dark:hover:bg-gray-800"
        >
          Sign in with browser (OAuth)
        </button>
        {oauthOpen && (
          <div class="space-y-3 pt-1">
            <input
              type="text"
              placeholder="Client ID (Azure AD app registration)"
              value={clientId}
              onInput={(e) => setClientId(e.currentTarget.value)}
              class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
            />
            <input
              type="password"
              placeholder="Client Secret"
              value={clientSecret}
              onInput={(e) => setClientSecret(e.currentTarget.value)}
              class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
            />
            <button
              onClick={handleOAuthLogin}
              disabled={loading || !orgUrl || !clientId || !clientSecret}
              class="w-full py-2 px-4 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {loading ? "Opening browser..." : "Continue with OAuth"}
            </button>
          </div>
        )}
      </div>
    </div>
  );

  function renderPatForm() {
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
