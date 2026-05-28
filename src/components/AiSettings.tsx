import { useState, useEffect } from "preact/hooks";
import {
  getAiSettings,
  saveAiSettings,
  getAiPrompts,
  saveAiPrompt,
  resetAiPrompt,
  saveAiPromptModel,
  listAiModels,
  type AiPromptInfo,
} from "@/lib/api";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Tab = "ai" | "prompts";

const DEFAULT_STANDARDS_MAX_CHARS = 8000;
const MIN_STANDARDS_MAX_CHARS = 500;
const MAX_STANDARDS_MAX_CHARS = 65535;

function normalizeStandardsMaxChars(value: string | number): number {
  const n = typeof value === "number" ? value : parseInt(value, 10);
  if (!Number.isFinite(n)) return DEFAULT_STANDARDS_MAX_CHARS;
  return Math.min(MAX_STANDARDS_MAX_CHARS, Math.max(MIN_STANDARDS_MAX_CHARS, n));
}

export function AiSettings({ open, onClose }: Props) {
  const [tab, setTab] = useState<Tab>("ai");

  // ---- AI tab ----
  const [provider, setProvider] = useState("openai");
  const [endpoint, setEndpoint] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [connectTimeoutSecs, setConnectTimeoutSecs] = useState(10);
  const [readTimeoutSecs, setReadTimeoutSecs] = useState(60);
  const [hunkConcurrency, setHunkConcurrency] = useState(1);
  const [standardsMaxChars, setStandardsMaxChars] = useState(String(DEFAULT_STANDARDS_MAX_CHARS));
  const [retryCount, setRetryCount] = useState(1);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);
  const [testing, setTesting] = useState(false);
  const [backdropMouseDown, setBackdropMouseDown] = useState(false);

  // ---- Prompts tab ----
  const [prompts, setPrompts] = useState<AiPromptInfo[]>([]);
  const [promptDrafts, setPromptDrafts] = useState<Record<string, string>>({});
  const [promptStatus, setPromptStatus] = useState<Record<string, { text: string; ok: boolean } | null>>({});
  // Available models from the configured provider's /models endpoint.
  // `null` distinguishes "not yet attempted" from "fetched but empty".
  const [availableModels, setAvailableModels] = useState<string[] | null>(null);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [modelsRefreshing, setModelsRefreshing] = useState(false);

  useEffect(() => {
    if (open) {
      loadSettings();
    }
  }, [open]);

  const loadSettings = async () => {
    try {
      const [settings, ps] = await Promise.all([
        getAiSettings(),
        getAiPrompts(),
      ]);
      setProvider(settings.provider);
      setEndpoint(settings.endpoint);
      setModel(settings.model);
      setConnectTimeoutSecs(settings.connectTimeoutSecs || 10);
      setReadTimeoutSecs(settings.readTimeoutSecs || 60);
      setHunkConcurrency(settings.hunkConcurrency || 1);
      setStandardsMaxChars(String(settings.standardsMaxChars || DEFAULT_STANDARDS_MAX_CHARS));
      // retryCount of 0 is valid ("no retries"), so don't fall back to a default
      // when the user has explicitly chosen 0.
      setRetryCount(
        Number.isFinite(settings.retryCount) ? settings.retryCount : 1,
      );
      setApiKey("");
      setPrompts(ps);
      setPromptDrafts(Object.fromEntries(ps.map((p) => [p.key, p.value])));
      setPromptStatus({});
      // Fire-and-forget: populate the model dropdown from the cached list if
      // there is one, so the picker shows real options without blocking the
      // dialog. A refresh button gives the user explicit control over hitting
      // the live /models endpoint.
      listAiModels(false)
        .then((m) => {
          setAvailableModels(m);
          setModelsError(null);
        })
        .catch((e: unknown) => {
          setAvailableModels([]);
          setModelsError(String(e));
        });
    } catch {
      // defaults are fine
    }
  };

  const handleRefreshModels = async () => {
    setModelsRefreshing(true);
    setModelsError(null);
    try {
      const m = await listAiModels(true);
      setAvailableModels(m);
      if (m.length > 0 && model && !m.includes(model)) {
        setModel(m[0]);
      }
    } catch (e: unknown) {
      setModelsError(String(e));
    } finally {
      setModelsRefreshing(false);
    }
  };

  const handleChangePromptModel = async (key: string, model: string) => {
    try {
      await saveAiPromptModel(key, model);
      const refreshed = await getAiPrompts();
      setPrompts(refreshed);
      setPromptStatus((prev) => ({
        ...prev,
        [key]: {
          text: model ? `Model set to ${model}.` : "Model set to default.",
          ok: true,
        },
      }));
    } catch (e: any) {
      setPromptStatus((prev) => ({ ...prev, [key]: { text: String(e), ok: false } }));
    }
  };

  const handleSavePrompt = async (key: string) => {
    const value = promptDrafts[key] ?? "";
    setPromptStatus((prev) => ({ ...prev, [key]: null }));
    try {
      await saveAiPrompt(key, value);
      const refreshed = await getAiPrompts();
      setPrompts(refreshed);
      setPromptDrafts((prev) => ({ ...prev, [key]: value }));
      setPromptStatus((prev) => ({ ...prev, [key]: { text: "Prompt saved.", ok: true } }));
    } catch (e: any) {
      setPromptStatus((prev) => ({ ...prev, [key]: { text: String(e), ok: false } }));
    }
  };

  const handleResetPrompt = async (key: string, label: string) => {
    const confirmed = window.confirm(
      `Reset "${label}" to its default? Your customizations for this prompt will be lost.`,
    );
    if (!confirmed) return;
    setPromptStatus((prev) => ({ ...prev, [key]: null }));
    try {
      await resetAiPrompt(key);
      const refreshed = await getAiPrompts();
      setPrompts(refreshed);
      const restored = refreshed.find((p) => p.key === key);
      setPromptDrafts((prev) => ({ ...prev, [key]: restored?.value ?? "" }));
      setPromptStatus((prev) => ({ ...prev, [key]: { text: "Reset to default.", ok: true } }));
    } catch (e: any) {
      setPromptStatus((prev) => ({ ...prev, [key]: { text: String(e), ok: false } }));
    }
  };

  const handleSaveAi = async () => {
    setSaving(true);
    setMessage(null);
    try {
      const normalizedStandardsMaxChars = normalizeStandardsMaxChars(standardsMaxChars);
      setStandardsMaxChars(String(normalizedStandardsMaxChars));
      await saveAiSettings(provider, endpoint, model, apiKey, connectTimeoutSecs, readTimeoutSecs, hunkConcurrency, normalizedStandardsMaxChars, retryCount);
      setMessage({ text: "AI settings saved.", ok: true });
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    setMessage(null);
    setModelsError(null);
    try {
      const normalizedStandardsMaxChars = normalizeStandardsMaxChars(standardsMaxChars);
      setStandardsMaxChars(String(normalizedStandardsMaxChars));
      await saveAiSettings(provider, endpoint, model, apiKey, connectTimeoutSecs, readTimeoutSecs, hunkConcurrency, normalizedStandardsMaxChars, retryCount);
      const models = await listAiModels(true);
      setAvailableModels(models);

      const selectedModel = models.includes(model) ? model : models[0] ?? "";
      if (selectedModel !== model) {
        setModel(selectedModel);
        if (selectedModel) {
          await saveAiSettings(provider, endpoint, selectedModel, "", connectTimeoutSecs, readTimeoutSecs, hunkConcurrency, normalizedStandardsMaxChars, retryCount);
          setMessage({
            text: `Connected. Model changed to ${selectedModel} because the previous model is not available from this provider.`,
            ok: true,
          });
        } else {
          setMessage({
            text: "Connected, but this provider did not return any models. Select a model after refreshing the model list.",
            ok: true,
          });
        }
      } else {
        setMessage({ text: `Connected. Found ${models.length} model${models.length === 1 ? "" : "s"}.`, ok: true });
      }
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setTesting(false);
    }
  };

  if (!open) return null;

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onMouseDown={(e) => setBackdropMouseDown(e.target === e.currentTarget)}
      onMouseUp={(e) => {
        if (backdropMouseDown && e.target === e.currentTarget) {
          onClose();
        }
        setBackdropMouseDown(false);
      }}
    >
      <div
        class="bg-white dark:bg-gray-900 rounded-xl shadow-xl border border-gray-200 dark:border-gray-700 w-full max-w-2xl mx-4 max-h-[90vh] overflow-y-auto"
      >
        {/* Header */}
        <div class="flex items-center justify-between px-5 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 class="text-base font-semibold">AI Settings</h2>
          <button
            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none"
            onClick={onClose}
          >
            ×
          </button>
        </div>

        {/* Tabs */}
        <div class="flex border-b border-gray-200 dark:border-gray-700 px-5">
          <TabButton label="AI" active={tab === "ai"} onClick={() => setTab("ai")} />
          <TabButton label="Prompts" active={tab === "prompts"} onClick={() => setTab("prompts")} />
        </div>

        <div class="px-5 py-4 space-y-5">
          {tab === "ai" && (
            <section>
              <div class="space-y-3">
                <Field label="Provider">
                  <select
                    value={provider}
                    onChange={(e) => {
                      setProvider(e.currentTarget.value);
                      setModel("");
                      setAvailableModels(null);
                      setModelsError(null);
                      setMessage(null);
                    }}
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  >
                    <option value="openai">OpenAI-compatible</option>
                    <option value="anthropic">Anthropic-compatible</option>
                  </select>
                </Field>

                <Field label="Endpoint URL">
                  <input
                    type="url"
                    value={endpoint}
                    onInput={(e) => {
                      setEndpoint(e.currentTarget.value);
                      setModel("");
                      setAvailableModels(null);
                      setModelsError(null);
                      setMessage(null);
                    }}
                    placeholder={provider === "openai" ? "https://api.openai.com" : "https://api.anthropic.com"}
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                </Field>

                <Field label="API Key">
                  <div class="flex items-center gap-2">
                    <input
                      type="password"
                      value={apiKey}
                      onInput={(e) => {
                        setApiKey(e.currentTarget.value);
                        setAvailableModels(null);
                        setModelsError(null);
                        setMessage(null);
                      }}
                      placeholder="Enter API key"
                      class="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                    />
                    <button
                      type="button"
                      onClick={handleTestConnection}
                      disabled={testing || !endpoint}
                      class="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg text-sm font-medium hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
                    >
                      {testing ? "Testing..." : "Test"}
                    </button>
                  </div>
                </Field>

                <Field label="Model">
                  {(() => {
                    const modelOptions = availableModels ?? [];
                    const showOrphan = !!model && !modelOptions.includes(model);
                    return (
                      <div class="flex items-center gap-2">
                        <select
                          value={model}
                          onChange={(e) => setModel(e.currentTarget.value)}
                          class="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                        >
                          {!model && <option value="">Select a model…</option>}
                          {showOrphan && (
                            <option value={model}>{model} (not in list)</option>
                          )}
                          {modelOptions.map((m) => (
                            <option value={m}>{m}</option>
                          ))}
                        </select>
                        <button
                          type="button"
                          onClick={handleRefreshModels}
                          disabled={modelsRefreshing}
                          title="Re-fetch the available models from your provider"
                          class="text-xs px-2 py-1 rounded-lg border border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
                        >
                          {modelsRefreshing ? "Refreshing…" : "Refresh"}
                        </button>
                      </div>
                    );
                  })()}
                  {modelsError && (
                    <p class="text-xs text-red-600 dark:text-red-400 mt-1">
                      Couldn't load models: {modelsError}
                    </p>
                  )}
                </Field>

                <Field label="Connect timeout (seconds)">
                  <input
                    type="number"
                    min={1}
                    max={3600}
                    value={connectTimeoutSecs}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      setConnectTimeoutSecs(Number.isFinite(n) && n > 0 ? n : 10);
                    }}
                    placeholder="10"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Maximum time for the TCP / TLS handshake. Catches a dead or unreachable server quickly. Does not bound generation time.
                  </p>
                </Field>

                <Field label="Read timeout (seconds)">
                  <input
                    type="number"
                    min={1}
                    max={3600}
                    value={readTimeoutSecs}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      setReadTimeoutSecs(Number.isFinite(n) && n > 0 ? n : 60);
                    }}
                    placeholder="60"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Stalled-stream guard: maximum time between successive bytes from the server. <strong>Does not bound total generation time</strong> — a slow local model that keeps the connection alive will be allowed to finish. Only raise this if your provider returns large bursts with long pauses between them.
                  </p>
                </Field>

                <Field label="Hunk review concurrency">
                  <input
                    type="number"
                    min={1}
                    max={16}
                    value={hunkConcurrency}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      setHunkConcurrency(Number.isFinite(n) && n >= 1 ? Math.min(n, 16) : 1);
                    }}
                    placeholder="1"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Maximum number of hunks a PR review or "Review All" sends to the model at once. Default 1 = sequential. Increase only if your provider can handle parallel requests.
                  </p>
                </Field>

                <Field label="Review context size limit (chars)">
                  <input
                    type="number"
                    min={500}
                    max={65535}
                    value={standardsMaxChars}
                    onInput={(e) => setStandardsMaxChars(e.currentTarget.value)}
                    onBlur={() => {
                      setStandardsMaxChars(String(normalizeStandardsMaxChars(standardsMaxChars)));
                    }}
                    placeholder="8000"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Per-file cap (in characters) for AGENTS.md / STYLE.md content injected into Review prompts. Anything beyond this is truncated with a visible marker.
                  </p>
                </Field>

                <Field label="Retry count">
                  <input
                    type="number"
                    min={0}
                    max={10}
                    value={retryCount}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      // 0 is a deliberate value here ("don't retry"), so don't
                      // collapse it to the default like the other fields do.
                      if (!Number.isFinite(n) || n < 0) {
                        setRetryCount(0);
                      } else {
                        setRetryCount(Math.min(10, n));
                      }
                    }}
                    placeholder="1"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    How many times to retry an LLM call after it fails during a PR review. Set to <strong>0</strong> for slow local providers — a "failure" there is usually just the request timeout firing while the model is still generating, and retrying just doubles the orphaned work.
                  </p>
                </Field>
              </div>

              <button
                onClick={handleSaveAi}
                disabled={saving || !endpoint || !model}
                class="mt-3 px-4 py-2 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50"
              >
                {saving ? "Saving..." : "Save AI Settings"}
              </button>
            </section>
          )}

          {tab === "prompts" && (
            <section>
              <div class="flex items-start justify-between gap-3 mb-3">
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  Customize the system prompts used by AI features. Each prompt can also be pinned to a specific provider model — leave it on <em>Default</em> to use the model from the AI tab.
                </p>
                <button
                  onClick={handleRefreshModels}
                  disabled={modelsRefreshing}
                  title="Re-fetch the available models from your provider"
                  class="shrink-0 px-2.5 py-1 border border-gray-300 dark:border-gray-600 rounded-lg text-[11px] hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
                >
                  {modelsRefreshing ? "Refreshing…" : "Refresh models"}
                </button>
              </div>

              {modelsError && (
                <p class="text-[11px] text-red-600 dark:text-red-400 mb-3">
                  Couldn't load models: {modelsError}
                </p>
              )}

              <div class="space-y-5">
                {prompts.map((p) => {
                  const draft = promptDrafts[p.key] ?? "";
                  const status = promptStatus[p.key];
                  const dirty = draft !== p.value;
                  const selectedModel = p.model ?? "";
                  // The currently-selected model may not appear in the list
                  // (provider /models doesn't include it, or it's a stale
                  // pin). Surface it anyway so the picker stays honest.
                  const modelOptions = availableModels ?? [];
                  const showOrphan =
                    selectedModel && !modelOptions.includes(selectedModel);
                  return (
                    <div key={p.key}>
                      <div class="flex items-center justify-between gap-3 mb-1">
                        <span class="text-xs text-gray-500 dark:text-gray-400">
                          {p.label}
                          {p.isCustomized && (
                            <span class="ml-2 text-[10px] uppercase tracking-wide text-accent">
                              customized
                            </span>
                          )}
                          {selectedModel && (
                            <span class="ml-2 text-[10px] uppercase tracking-wide text-accent">
                              model: {selectedModel}
                            </span>
                          )}
                        </span>
                        <div class="flex items-center gap-1">
                          <label class="text-[11px] text-gray-500 dark:text-gray-400">
                            Model:
                          </label>
                          <select
                            value={selectedModel}
                            onChange={(e) =>
                              handleChangePromptModel(p.key, e.currentTarget.value)
                            }
                            class="text-[11px] px-1.5 py-1 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 max-w-[200px]"
                            title="Override the model used by this prompt. 'Default' uses the model from the AI tab."
                          >
                            <option value="">Default</option>
                            {showOrphan && (
                              <option value={selectedModel}>
                                {selectedModel} (not in list)
                              </option>
                            )}
                            {modelOptions.map((m) => (
                              <option value={m}>{m}</option>
                            ))}
                          </select>
                        </div>
                      </div>
                      <textarea
                        value={draft}
                        onInput={(e) =>
                          setPromptDrafts((prev) => ({
                            ...prev,
                            [p.key]: e.currentTarget.value,
                          }))
                        }
                        rows={8}
                        class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-xs font-mono outline-none focus:ring-2 focus:ring-accent resize-y"
                      />
                      <p class="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                        {p.description}
                      </p>
                      <div class="flex items-center gap-2 mt-2">
                        <button
                          onClick={() => handleSavePrompt(p.key)}
                          disabled={!dirty || draft.trim().length === 0}
                          class="px-3 py-1.5 bg-accent hover:bg-accent-hover text-white rounded-lg text-xs font-medium disabled:opacity-50"
                        >
                          Save
                        </button>
                        <button
                          onClick={() => handleResetPrompt(p.key, p.label)}
                          disabled={!p.isCustomized}
                          title={
                            p.isCustomized
                              ? "Restore the default prompt"
                              : "Already using the default"
                          }
                          class="px-3 py-1.5 border border-gray-300 dark:border-gray-600 rounded-lg text-xs font-medium hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50 disabled:cursor-not-allowed"
                        >
                          Reset to default
                        </button>
                        {status && (
                          <span
                            class={`text-xs ${status.ok ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}`}
                          >
                            {status.text}
                          </span>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </section>
          )}

          {/* Status message (AI tab only) */}
          {tab === "ai" && message && (
            <div class={`text-sm px-3 py-2 rounded-lg ${message.ok ? "bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-300" : "bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300"}`}>
              {message.text}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function TabButton({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      class={`px-3 py-2 text-sm font-medium border-b-2 -mb-px ${
        active
          ? "border-accent text-accent"
          : "border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
      }`}
    >
      {label}
    </button>
  );
}

function Field({ label, children }: { label: string; children: preact.ComponentChildren }) {
  return (
    <label class="block">
      <span class="text-xs text-gray-500 dark:text-gray-400 mb-1 block">{label}</span>
      {children}
    </label>
  );
}
