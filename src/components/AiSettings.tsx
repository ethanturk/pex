import { useState, useEffect } from "preact/hooks";
import { showPrChecks } from "@/lib/signals";
import {
  getAiSettings,
  saveAiSettings,
  getAiPrompts,
  saveAiPrompt,
  resetAiPrompt,
  saveAiPromptModel,
  listAiModels,
  getReviewCalibration,
  clearReviewFeedback,
  getDiagnosticsDir,
  type AiPromptInfo,
  type CalibrationStats,
} from "@/lib/api";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Tab = "ai" | "prompts" | "calibration" | "pr-list";

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
  const [confidenceThreshold, setConfidenceThreshold] = useState(80);
  const [blockingConfidence, setBlockingConfidence] = useState(85);
  const [autoVoteOnBlocking, setAutoVoteOnBlocking] = useState(false);
  const [incrementalReview, setIncrementalReview] = useState(false);
  const [autoReview, setAutoReview] = useState(false);
  const [autoPostBlocking, setAutoPostBlocking] = useState(false);
  const [autoPostConfidence, setAutoPostConfidence] = useState(90);
  const [aiDiagnostics, setAiDiagnostics] = useState(false);
  const [diagnosticsDir, setDiagnosticsDir] = useState("");
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

  // ---- Calibration tab ----
  const [calibration, setCalibration] = useState<CalibrationStats | null>(null);
  const [calibrationLoading, setCalibrationLoading] = useState(false);

  useEffect(() => {
    if (open) {
      loadSettings();
    }
  }, [open]);

  const loadCalibration = async () => {
    setCalibrationLoading(true);
    try {
      setCalibration(await getReviewCalibration());
    } catch {
      setCalibration(null);
    } finally {
      setCalibrationLoading(false);
    }
  };

  useEffect(() => {
    if (open && tab === "calibration") {
      loadCalibration();
    }
  }, [open, tab]);

  const handleClearCalibration = async () => {
    if (
      !window.confirm(
        "Clear all recorded review feedback? This resets calibration metrics and forgets every dismissed finding (they may be suggested again).",
      )
    ) {
      return;
    }
    try {
      await clearReviewFeedback();
      await loadCalibration();
    } catch {
      // ignore — the next load will reflect reality
    }
  };

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
      // 0 is a valid threshold ("surface everything"), so keep it as-is.
      setConfidenceThreshold(
        Number.isFinite(settings.confidenceThreshold) ? settings.confidenceThreshold : 80,
      );
      // 0 is a valid critical line ("every critical blocks"), so keep it as-is.
      setBlockingConfidence(
        Number.isFinite(settings.blockingConfidence) ? settings.blockingConfidence : 85,
      );
      setAutoVoteOnBlocking(!!settings.autoVoteOnBlocking);
      setIncrementalReview(!!settings.incrementalReview);
      setAutoReview(!!settings.autoReview);
      setAutoPostBlocking(!!settings.autoPostBlocking);
      setAutoPostConfidence(
        Number.isFinite(settings.autoPostConfidence) ? settings.autoPostConfidence : 90,
      );
      setAiDiagnostics(!!settings.aiDiagnostics);
      getDiagnosticsDir().then(setDiagnosticsDir).catch(() => {});
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
      await saveAiSettings(provider, endpoint, model, apiKey, connectTimeoutSecs, readTimeoutSecs, hunkConcurrency, normalizedStandardsMaxChars, retryCount, confidenceThreshold, blockingConfidence, autoVoteOnBlocking, incrementalReview, autoReview, autoPostBlocking, autoPostConfidence, aiDiagnostics);
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
      await saveAiSettings(provider, endpoint, model, apiKey, connectTimeoutSecs, readTimeoutSecs, hunkConcurrency, normalizedStandardsMaxChars, retryCount, confidenceThreshold, blockingConfidence, autoVoteOnBlocking, incrementalReview, autoReview, autoPostBlocking, autoPostConfidence, aiDiagnostics);
      const models = await listAiModels(true);
      setAvailableModels(models);

      const selectedModel = models.includes(model) ? model : models[0] ?? "";
      if (selectedModel !== model) {
        setModel(selectedModel);
        if (selectedModel) {
          await saveAiSettings(provider, endpoint, selectedModel, "", connectTimeoutSecs, readTimeoutSecs, hunkConcurrency, normalizedStandardsMaxChars, retryCount, confidenceThreshold, blockingConfidence, autoVoteOnBlocking, incrementalReview, autoReview, autoPostBlocking, autoPostConfidence, aiDiagnostics);
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
          <h2 class="text-base font-semibold">Settings</h2>
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
          <TabButton label="Calibration" active={tab === "calibration"} onClick={() => setTab("calibration")} />
          <TabButton label="PR List" active={tab === "pr-list"} onClick={() => setTab("pr-list")} />
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

                <Field label="Review concurrency">
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
                    Maximum number of hunks a PR review sends to the model at once. Default 1 = sequential. Increase only if your provider can handle parallel requests.
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

                <Field label="Confidence threshold (0–100)">
                  <input
                    type="number"
                    min={0}
                    max={100}
                    value={confidenceThreshold}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      // 0 is a deliberate value ("surface everything"); clamp to 0–100.
                      if (!Number.isFinite(n) || n < 0) {
                        setConfidenceThreshold(0);
                      } else {
                        setConfidenceThreshold(Math.min(100, n));
                      }
                    }}
                    placeholder="80"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Minimum confidence a PR review finding must reach to be surfaced. The default <strong>80</strong> filters out likely false positives and low-impact nits. Lower it to see more (noisier) findings; raise it for only the highest-confidence issues. Set to <strong>0</strong> to surface everything.
                  </p>
                </Field>

                <Field label="Critical line — blocking confidence (0–100)">
                  <input
                    type="number"
                    min={0}
                    max={100}
                    value={blockingConfidence}
                    onInput={(e) => {
                      const n = parseInt(e.currentTarget.value, 10);
                      // 0 is deliberate ("every critical blocks"); clamp to 0–100.
                      if (!Number.isFinite(n) || n < 0) {
                        setBlockingConfidence(0);
                      } else {
                        setBlockingConfidence(Math.min(100, n));
                      }
                    }}
                    placeholder="85"
                    class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                  />
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    The confidence a <strong>critical</strong> finding must reach to be tiered <strong>Blocking</strong> (pulled to the top and posted as its own comment). Critical findings below this line are still surfaced as <strong>Should fix</strong>. Default <strong>85</strong>. Set to <strong>0</strong> to treat every critical finding as blocking.
                  </p>
                </Field>

                <label class="flex items-start gap-3">
                  <input
                    type="checkbox"
                    checked={autoVoteOnBlocking}
                    onChange={(e) => setAutoVoteOnBlocking(e.currentTarget.checked)}
                    class="mt-1 accent-accent"
                  />
                  <span>
                    <span class="block text-sm font-medium text-gray-900 dark:text-gray-100">
                      Auto-vote "wait for author" on blocking findings
                    </span>
                    <span class="block text-xs text-gray-500 dark:text-gray-400 mt-1">
                      When you <strong>post a review to ADO</strong> and it contains at least one <strong>blocking</strong> finding, also cast a "wait for author" reviewer vote so the PR can't be approved with an unaddressed blocker. Off by default — this casts a vote on your behalf.
                    </span>
                  </span>
                </label>

                <label class="flex items-start gap-3">
                  <input
                    type="checkbox"
                    checked={incrementalReview}
                    onChange={(e) => setIncrementalReview(e.currentTarget.checked)}
                    class="mt-1 accent-accent"
                  />
                  <span>
                    <span class="block text-sm font-medium text-gray-900 dark:text-gray-100">
                      Incremental review
                    </span>
                    <span class="block text-xs text-gray-500 dark:text-gray-400 mt-1">
                      On a re-review, only review files <strong>changed since the last reviewed iteration</strong> of the PR, instead of the whole PR again. The first review of a PR is always full. Off by default.
                    </span>
                  </span>
                </label>

                <div class="pt-2 mt-1 border-t border-gray-200 dark:border-gray-700">
                  <div class="text-xs font-semibold text-gray-600 dark:text-gray-300 mb-2">
                    Automation
                  </div>

                  <label class="flex items-start gap-3">
                    <input
                      type="checkbox"
                      checked={autoReview}
                      onChange={(e) => setAutoReview(e.currentTarget.checked)}
                      class="mt-1 accent-accent"
                    />
                    <span>
                      <span class="block text-sm font-medium text-gray-900 dark:text-gray-100">
                        Auto-review new iterations
                      </span>
                      <span class="block text-xs text-gray-500 dark:text-gray-400 mt-1">
                        When the PR list loads, automatically run a (Fast) review in the background for any active PR that has a <strong>new iteration</strong> since it was last reviewed. Reviews run one at a time. Off by default — this uses provider quota.
                      </span>
                    </span>
                  </label>

                  <label class="flex items-start gap-3 mt-3">
                    <input
                      type="checkbox"
                      checked={autoPostBlocking}
                      onChange={(e) => setAutoPostBlocking(e.currentTarget.checked)}
                      class="mt-1 accent-accent"
                    />
                    <span>
                      <span class="block text-sm font-medium text-gray-900 dark:text-gray-100">
                        Auto-post high-confidence blocking findings
                      </span>
                      <span class="block text-xs text-gray-500 dark:text-gray-400 mt-1">
                        After an auto-review, automatically post <strong>Blocking</strong> findings at or above the confidence floor below. Everything else waits in the sidebar for you. Off by default — this posts comments unattended.
                      </span>
                    </span>
                  </label>

                  <div class="mt-3">
                    <Field label="Auto-post confidence floor (0–100)">
                      <input
                        type="number"
                        min={0}
                        max={100}
                        value={autoPostConfidence}
                        disabled={!autoPostBlocking}
                        onInput={(e) => {
                          const n = parseInt(e.currentTarget.value, 10);
                          if (!Number.isFinite(n) || n < 0) {
                            setAutoPostConfidence(0);
                          } else {
                            setAutoPostConfidence(Math.min(100, n));
                          }
                        }}
                        placeholder="90"
                        class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent disabled:opacity-50"
                      />
                      <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                        Only blocking findings this confident are auto-posted. Default <strong>90</strong> — autonomy is earned, so keep this high.
                      </p>
                    </Field>
                  </div>

                  <label class="flex items-start gap-3 mt-3">
                    <input
                      type="checkbox"
                      checked={aiDiagnostics}
                      onChange={(e) => setAiDiagnostics(e.currentTarget.checked)}
                      class="mt-1 accent-accent"
                    />
                    <span>
                      <span class="block text-sm font-medium text-gray-900 dark:text-gray-100">
                        Write diagnostic traces
                      </span>
                      <span class="block text-xs text-gray-500 dark:text-gray-400 mt-1">
                        Record a JSONL trace per review run — the exact prompts and model responses, plus every deterministic decision (confidence/anchor guard drops, tiering, suppression, final findings) — for evaluation and tuning. Findings carry the same fingerprint as recorded verdicts, so traces can be joined to your accept/dismiss history. Off by default; traces contain source content and full prompts.
                        {diagnosticsDir && (
                          <>
                            {" "}Written to <code class="font-mono">{diagnosticsDir}</code>.
                          </>
                        )}
                      </span>
                    </span>
                  </label>
                </div>
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

          {tab === "calibration" && (
            <CalibrationPanel
              stats={calibration}
              loading={calibrationLoading}
              onRefresh={loadCalibration}
              onClear={handleClearCalibration}
            />
          )}

          {tab === "pr-list" && (
            <section class="space-y-4">
              <label class="flex items-start gap-3">
                <input
                  type="checkbox"
                  checked={showPrChecks.value}
                  onChange={(e) => (showPrChecks.value = e.currentTarget.checked)}
                  class="mt-1"
                />
                <span>
                  <span class="block text-sm font-medium text-gray-900 dark:text-gray-100">
                    Show PR build checks
                  </span>
                  <span class="block text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Fetch Azure DevOps policy evaluations for each PR and show required/optional check status in the PR list. This adds extra API calls when the PR list loads or refreshes.
                  </span>
                </span>
              </label>
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

function formatRate(rate: number | null): string {
  return rate == null ? "—" : `${rate.toFixed(0)}%`;
}

function CalibrationPanel({
  stats,
  loading,
  onRefresh,
  onClear,
}: {
  stats: CalibrationStats | null;
  loading: boolean;
  onRefresh: () => void;
  onClear: () => void;
}) {
  return (
    <section class="space-y-4">
      <div class="flex items-start justify-between gap-3">
        <p class="text-xs text-gray-500 dark:text-gray-400">
          How review findings have been acted on, aggregated across all PRs. Use the accept rates to tune the confidence threshold and the critical line: a bucket that's mostly dismissed is noise you can raise the floor on.
        </p>
        <div class="flex items-center gap-2 shrink-0">
          <button
            onClick={onRefresh}
            disabled={loading}
            class="px-2.5 py-1 border border-gray-300 dark:border-gray-600 rounded-lg text-[11px] hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
          >
            {loading ? "Loading…" : "Refresh"}
          </button>
          <button
            onClick={onClear}
            disabled={loading || !stats || stats.total === 0}
            class="px-2.5 py-1 border border-red-300 dark:border-red-700 text-red-600 dark:text-red-400 rounded-lg text-[11px] hover:bg-red-50 dark:hover:bg-red-900/30 disabled:opacity-50"
          >
            Reset
          </button>
        </div>
      </div>

      {!stats || stats.total === 0 ? (
        <p class="text-sm text-gray-500 dark:text-gray-400">
          No feedback recorded yet. As you post or dismiss AI review findings, accept rates appear here.
        </p>
      ) : (
        <>
          <div class="grid grid-cols-4 gap-2 text-center">
            <Stat label="Acted on" value={String(stats.total)} />
            <Stat label="Accepted" value={String(stats.accepted)} />
            <Stat label="Edited" value={String(stats.edited)} />
            <Stat label="Dismissed" value={String(stats.dismissed)} />
          </div>
          <div class="text-sm">
            Overall accept rate:{" "}
            <strong>{formatRate(stats.acceptRate)}</strong>
            <span class="text-xs text-gray-500 dark:text-gray-400"> (accepted + edited)</span>
          </div>

          <CalibrationTable title="By severity" buckets={stats.bySeverity} />
          <CalibrationTable title="By tier" buckets={stats.byTier} />
          <CalibrationTable
            title="By specialist (Thorough — a finding may credit several)"
            buckets={stats.bySpecialist}
          />
        </>
      )}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div class="rounded-lg border border-gray-200 dark:border-gray-700 py-2">
      <div class="text-lg font-semibold tabular-nums">{value}</div>
      <div class="text-[10px] uppercase tracking-wide text-gray-500">{label}</div>
    </div>
  );
}

function CalibrationTable({
  title,
  buckets,
}: {
  title: string;
  buckets: CalibrationStats["bySeverity"];
}) {
  if (buckets.length === 0) return null;
  return (
    <div>
      <div class="text-[10px] uppercase tracking-wide text-gray-400 mb-1">{title}</div>
      <table class="w-full text-xs">
        <thead>
          <tr class="text-left text-gray-500">
            <th class="font-medium py-1">Bucket</th>
            <th class="font-medium py-1 text-right">Accepted</th>
            <th class="font-medium py-1 text-right">Edited</th>
            <th class="font-medium py-1 text-right">Dismissed</th>
            <th class="font-medium py-1 text-right">Accept rate</th>
          </tr>
        </thead>
        <tbody>
          {buckets.map((b) => (
            <tr key={b.label} class="border-t border-gray-100 dark:border-gray-800">
              <td class="py-1 capitalize">{b.label || "—"}</td>
              <td class="py-1 text-right tabular-nums">{b.accepted}</td>
              <td class="py-1 text-right tabular-nums">{b.edited}</td>
              <td class="py-1 text-right tabular-nums">{b.dismissed}</td>
              <td class="py-1 text-right tabular-nums">{formatRate(b.acceptRate)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
