import { currentView, activeOrg, savedOrgs } from "@/lib/signals";
import { loginPat, loginOAuth, getSavedOrgs, activateOrg } from "@/lib/api";
import { useState } from "preact/hooks";

type Provider = "ado" | "github";

// Mirror of the backend's `canonical_org_url` for GitHub so the URL we store in
// `activeOrg` matches the saved-org row the backend persists (and that
// `activateOrg` looks up on restart). Blank host → github.com.
function githubCanonicalUrl(host: string): string {
  const trimmed = host.trim().replace(/\/+$/, "");
  const hostOnly = trimmed
    .replace(/^https?:\/\//, "")
    .replace(/^www\./, "");
  if (trimmed === "" || hostOnly === "github.com") return "https://github.com";
  return trimmed.startsWith("http") ? trimmed : `https://${trimmed}`;
}

export function AuthScreen() {
  // If we were routed here because a saved org's credential was missing
  // (e.g. after an upgrade from a build that didn't persist PATs to the
  // keyring), activeOrg will already be set — pre-fill so re-signing in is a
  // single paste.
  const prefilledOrg = activeOrg.value?.orgUrl ?? "";
  const prefilledProvider: Provider = activeOrg.value?.provider ?? "ado";
  const [provider, setProvider] = useState<Provider>(prefilledProvider);
  const [orgUrl, setOrgUrl] = useState(prefilledProvider === "ado" ? prefilledOrg : "");
  // GitHub Enterprise Server host (blank = github.com).
  const [ghHost, setGhHost] = useState(
    prefilledProvider === "github" && prefilledOrg !== "https://github.com" ? prefilledOrg : "",
  );
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
      activeOrg.value = { orgUrl, name: new URL(orgUrl).hostname, tokenType: "oauth", provider: "ado" };
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
      const effectiveUrl = provider === "github" ? githubCanonicalUrl(ghHost) : orgUrl;
      const ok = await loginPat(provider, effectiveUrl, pat);
      if (ok) {
        const orgs = await getSavedOrgs();
        savedOrgs.value = orgs;
        // Prefer the freshly-persisted row (carries the correct display name,
        // e.g. the GitHub login) over a locally-built credential.
        const match = orgs.find((o) => o.orgUrl === effectiveUrl);
        activeOrg.value =
          match ?? {
            orgUrl: effectiveUrl,
            name: new URL(effectiveUrl).hostname,
            tokenType: "pat",
            provider,
          };
        currentView.value = { kind: "pr-list" };
      } else {
        setError("Invalid credentials. Check your URL and token.");
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
      <div class="flex items-center justify-center h-full p-4 safe-bottom">
        <div class="w-full max-w-sm space-y-4">
          <h2 class="text-xl font-semibold">Connect a repository host</h2>
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
                      setProvider(org.provider);
                      if (org.provider === "github") {
                        setGhHost(org.orgUrl === "https://github.com" ? "" : org.orgUrl);
                      } else {
                        setOrgUrl(org.orgUrl);
                      }
                      setError("Your saved session has expired. Sign in again to continue.");
                      setForceForm(true);
                    } else {
                      setError(msg);
                    }
                  }
                }}
              >
                <div class="font-medium text-sm flex items-center justify-between">
                  <span>{org.name}</span>
                  <span class="text-[10px] uppercase tracking-wide text-gray-400">
                    {org.provider === "github" ? "GitHub" : "Azure DevOps"}
                  </span>
                </div>
                <div class="text-xs text-gray-400">{org.orgUrl}</div>
              </button>
            ))}
          </div>
          <details class="text-sm">
            <summary class="cursor-pointer text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300">
              + Add another account
            </summary>
            <div class="mt-3 space-y-3">
              {renderProviderToggle()}
              {renderPatForm()}
            </div>
          </details>
        </div>
      </div>
    );
  }

  return (
    <div class="flex items-center justify-center h-full p-4 safe-bottom">
      <div class="w-full max-w-sm space-y-4">
        <h2 class="text-xl font-semibold">
          {provider === "github" ? "Connect to GitHub" : "Connect to Azure DevOps"}
        </h2>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          {provider === "github"
            ? "Sign in with a GitHub Personal Access Token."
            : "Sign in with a Personal Access Token or OAuth."}
        </p>
        {renderProviderToggle()}
        {renderPatForm()}
        {provider === "ado" && (
          <>
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
          </>
        )}
        {provider === "github" && (
          <p class="text-xs text-gray-400">
            Browser (OAuth) sign-in for GitHub is coming soon. Use a Personal Access
            Token with <code>repo</code> scope (and <code>read:org</code> to list
            organization repositories).
          </p>
        )}
      </div>
    </div>
  );

  function renderProviderToggle() {
    const tab = (value: Provider, label: string) => (
      <button
        type="button"
        onClick={() => {
          setProvider(value);
          setError("");
        }}
        class={`flex-1 py-1.5 text-sm font-medium rounded-md transition-colors ${
          provider === value
            ? "bg-white dark:bg-gray-700 shadow text-gray-900 dark:text-gray-100"
            : "text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
        }`}
      >
        {label}
      </button>
    );
    return (
      <div class="flex gap-1 p-1 rounded-lg bg-gray-100 dark:bg-gray-800">
        {tab("ado", "Azure DevOps")}
        {tab("github", "GitHub")}
      </div>
    );
  }

  function renderPatForm() {
    return (
      <div class="space-y-3">
        {provider === "github" ? (
          <input
            type="text"
            placeholder="Enterprise Server host (optional, blank = github.com)"
            value={ghHost}
            onInput={(e) => setGhHost(e.currentTarget.value)}
            class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
          />
        ) : (
          <input
            type="text"
            inputmode="url"
            autocomplete="url"
            placeholder="https://dev.azure.com/your-org"
            value={orgUrl}
            onInput={(e) => setOrgUrl(e.currentTarget.value)}
            class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
          />
        )}
        <input
          type="password"
          autocomplete="current-password"
          placeholder={
            provider === "github"
              ? "GitHub Personal Access Token (repo scope)"
              : "Personal Access Token"
          }
          value={pat}
          onInput={(e) => setPat(e.currentTarget.value)}
          class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
        />
        {error && <p class="text-sm text-red-500">{error}</p>}
        <button
          onClick={handlePatLogin}
          disabled={loading || !pat || (provider === "ado" && !orgUrl)}
          class="w-full py-2 px-4 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {loading ? "Connecting..." : "Connect"}
        </button>
      </div>
    );
  }
}
