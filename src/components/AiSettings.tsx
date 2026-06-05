import { useState, useEffect, useRef } from "preact/hooks";
import {
  showPrChecks,
  theme,
  applyTheme,
  diffView,
  appFont,
  appTextSize,
  diffTextSize,
  syncStatus,
  reconcilePersistedReviews,
  type Theme,
  type DiffView,
  type TextSize,
} from "@/lib/signals";
import {
  getAiSettings,
  saveAiProviderConfig,
  removeAiProvider,
  saveAiPreferences,
  testAiDefaults,
  getAiPrompts,
  saveAiPrompt,
  resetAiPrompt,
  saveAiPromptModel,
  listAiModels,
  listAiProviderModels,
  getReviewCalibration,
  clearReviewFeedback,
  getDiagnosticsDir,
  getSyncStatus,
  enableSync,
  disableSync,
  syncNow,
  listCompletedReviews,
  type AiProviderConfig,
  type AiPromptInfo,
  type CalibrationStats,
} from "@/lib/api";

interface Props {
  open: boolean;
  onClose: () => void;
  /** Render as static content without modal backdrop.
   *  Used in the mobile Settings tab where the tab shell provides containment. */
  standalone?: boolean;
}

type Tab = "general" | "ai-defaults" | "review" | "prompts" | "calibration" | "sync" | "pr-list";

// Common system font stacks for the appearance dropdown. "" = app default.
const FONT_OPTIONS: { label: string; value: string }[] = [
  { label: "System default", value: "" },
  { label: "Sans-serif", value: "ui-sans-serif, system-ui, sans-serif" },
  { label: "Serif", value: "ui-serif, Georgia, Cambria, 'Times New Roman', serif" },
  { label: "Arial", value: "Arial, Helvetica, sans-serif" },
  { label: "Helvetica Neue", value: "'Helvetica Neue', Helvetica, Arial, sans-serif" },
  { label: "Segoe UI", value: "'Segoe UI', Tahoma, Geneva, sans-serif" },
  { label: "Roboto", value: "Roboto, system-ui, sans-serif" },
  { label: "Georgia", value: "Georgia, 'Times New Roman', serif" },
  { label: "Times New Roman", value: "'Times New Roman', Times, serif" },
  { label: "Verdana", value: "Verdana, Geneva, sans-serif" },
  { label: "Tahoma", value: "Tahoma, Geneva, sans-serif" },
  { label: "Courier New", value: "'Courier New', Courier, monospace" },
];

const TEXT_SIZE_OPTIONS: { label: string; value: TextSize }[] = [
  { label: "Small", value: "small" },
  { label: "Medium", value: "medium" },
  { label: "Large", value: "large" },
  { label: "XL", value: "xl" },
];

const THEME_OPTIONS: { label: string; value: Theme }[] = [
  { label: "System", value: "system" },
  { label: "Light", value: "light" },
  { label: "Dark", value: "dark" },
];

const DIFF_VIEW_OPTIONS: { label: string; value: DiffView }[] = [
  { label: "Inline", value: "inline" },
  { label: "Side-by-side", value: "split" },
];

const DEFAULT_STANDARDS_MAX_CHARS = 8000;
const MIN_STANDARDS_MAX_CHARS = 500;
const MAX_STANDARDS_MAX_CHARS = 65535;

// Placeholder shown in the API Key field when a key is already stored. Typing
// replaces it; leaving it blank keeps the stored key.
const API_KEY_MASK = "••••••••••••";

const DEFAULT_OPENAI_ENDPOINT = "https://api.openai.com";
const DEFAULT_ANTHROPIC_ENDPOINT = "https://api.anthropic.com";

function defaultEndpoint(provider: string): string {
  return provider === "anthropic" ? DEFAULT_ANTHROPIC_ENDPOINT : DEFAULT_OPENAI_ENDPOINT;
}

function providerKindLabel(provider: string): string {
  return provider === "anthropic" ? "Anthropic-compatible" : "OpenAI-compatible";
}

function makeProviderId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `provider-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function normalizeStandardsMaxChars(value: string | number): number {
  const n = typeof value === "number" ? value : parseInt(value, 10);
  if (!Number.isFinite(n)) return DEFAULT_STANDARDS_MAX_CHARS;
  return Math.min(MAX_STANDARDS_MAX_CHARS, Math.max(MIN_STANDARDS_MAX_CHARS, n));
}

const INPUT_CLASS =
  "w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent";

export function AiSettings({ open, onClose, standalone }: Props) {
  const [tab, setTab] = useState<Tab>("ai-defaults");

  // ---- AI providers tab (provider creds + defaults — autosaved) ----
  const [providers, setProviders] = useState<AiProviderConfig[]>([]);
  const [defaultProviderId, setDefaultProviderId] = useState("default");
  const [selectedProviderId, setSelectedProviderId] = useState("default");
  const [apiKeyDrafts, setApiKeyDrafts] = useState<Record<string, string>>({});
  const [providerModels, setProviderModels] = useState<Record<string, string[]>>({});
  const [providerModelsError, setProviderModelsError] = useState<string | null>(null);
  const [providerSaveStatus, setProviderSaveStatus] = useState<{ text: string; ok: boolean } | null>(null);
  const [providerSaving, setProviderSaving] = useState(false);
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);
  const [testing, setTesting] = useState(false);

  // ---- Review preferences tab (autosaved) ----
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
  const [prefsSaved, setPrefsSaved] = useState(false);

  const [backdropMouseDown, setBackdropMouseDown] = useState(false);

  // ---- Prompts tab ----
  const [prompts, setPrompts] = useState<AiPromptInfo[]>([]);
  const [promptDrafts, setPromptDrafts] = useState<Record<string, string>>({});
  const [promptStatus, setPromptStatus] = useState<Record<string, { text: string; ok: boolean } | null>>({});
  const [openPromptModelPicker, setOpenPromptModelPicker] = useState<string | null>(null);
  const [promptModelDrafts, setPromptModelDrafts] = useState<Record<string, string>>({});
  // Available models from the configured provider's /models endpoint.
  // `null` distinguishes "not yet attempted" from "fetched but empty".
  const [availableModels, setAvailableModels] = useState<string[] | null>(null);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [modelsRefreshing, setModelsRefreshing] = useState(false);

  // ---- Calibration tab ----
  const [calibration, setCalibration] = useState<CalibrationStats | null>(null);
  const [calibrationLoading, setCalibrationLoading] = useState(false);

  // Guards autosave: don't persist while loadSettings is populating state.
  const hydrating = useRef(false);
  const selectedProvider = providers.find((p) => p.id === selectedProviderId) ?? providers[0] ?? null;
  const selectedProviderModels = selectedProvider ? (providerModels[selectedProvider.id] ?? []) : [];
  const selectedApiKeyDraft = selectedProvider ? (apiKeyDrafts[selectedProvider.id] ?? "") : "";

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

  useEffect(() => {
    if (!openPromptModelPicker) return;
    const closeOnOutsideClick = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      if (target.closest("[data-prompt-model-picker]")) return;
      setOpenPromptModelPicker(null);
    };
    document.addEventListener("mousedown", closeOnOutsideClick);
    return () => document.removeEventListener("mousedown", closeOnOutsideClick);
  }, [openPromptModelPicker]);

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
    hydrating.current = true;
    try {
      const [settings, ps] = await Promise.all([
        getAiSettings(),
        getAiPrompts(),
      ]);
      const loadedProviders = settings.providers?.length
        ? settings.providers
        : [{
            id: "default",
            name: "Default",
            provider: settings.provider || "openai",
            endpoint: settings.endpoint || DEFAULT_OPENAI_ENDPOINT,
            model: settings.model || "gpt-4.1",
            hasApiKey: !!settings.hasApiKey,
            connectTimeoutSecs: settings.connectTimeoutSecs || 10,
            readTimeoutSecs: settings.readTimeoutSecs || 60,
          }];
      setProviders(loadedProviders);
      const defaultId = loadedProviders.some((p) => p.id === settings.defaultProviderId)
        ? settings.defaultProviderId
        : loadedProviders[0].id;
      setDefaultProviderId(defaultId);
      setSelectedProviderId(defaultId);
      setApiKeyDrafts({});
      setProviderModels({});
      setProviderModelsError(null);
      setProviderSaveStatus(null);
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
      setMessage(null);
      getDiagnosticsDir().then(setDiagnosticsDir).catch(() => {});
      setPrompts(ps);
      setPromptDrafts(Object.fromEntries(ps.map((p) => [p.key, p.value])));
      setPromptStatus({});
      setPromptModelDrafts({});
      setOpenPromptModelPicker(null);
      // Fire-and-forget: populate the model dropdown from the cached list if
      // there is one, so the picker shows real options without blocking the
      // dialog. A refresh button gives the user explicit control over hitting
      // the live /models endpoint.
      listAiModels(false)
        .then((m) => {
          setAvailableModels(m);
          setProviderModels((prev) => ({ ...prev, [defaultId]: m }));
          setModelsError(null);
        })
        .catch((e: unknown) => {
          setAvailableModels([]);
          setModelsError(String(e));
        });
    } catch {
      // defaults are fine
    } finally {
      hydrating.current = false;
    }
  };

  // Autosave the review/automation preferences whenever any of them change
  // (but not during initial hydration). Defaults live on their own tab and are
  // intentionally excluded.
  useEffect(() => {
    if (!open || hydrating.current) return;
    const normalized = normalizeStandardsMaxChars(standardsMaxChars);
    let cancelled = false;
    saveAiPreferences({
      hunkConcurrency,
      standardsMaxChars: normalized,
      retryCount,
      confidenceThreshold,
      blockingConfidence,
      autoVoteOnBlocking,
      incrementalReview,
      autoReview,
      autoPostBlocking,
      autoPostConfidence,
      aiDiagnostics,
    })
      .then(() => {
        if (cancelled) return;
        setPrefsSaved(true);
        window.setTimeout(() => !cancelled && setPrefsSaved(false), 1500);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [
    hunkConcurrency,
    standardsMaxChars,
    retryCount,
    confidenceThreshold,
    blockingConfidence,
    autoVoteOnBlocking,
    incrementalReview,
    autoReview,
    autoPostBlocking,
    autoPostConfidence,
    aiDiagnostics,
  ]);

  useEffect(() => {
    if (!open || hydrating.current || providers.length === 0) return;
    let cancelled = false;
    const timer = window.setTimeout(async () => {
      setProviderSaving(true);
      try {
        for (const p of providers) {
          await saveAiProviderConfig(
            p,
            apiKeyDrafts[p.id] ?? "",
            p.id === defaultProviderId,
          );
        }
        if (cancelled) return;
        const savedKeyIds = new Set(
          Object.entries(apiKeyDrafts)
            .filter(([, value]) => value.trim().length > 0)
            .map(([id]) => id),
        );
        if (savedKeyIds.size > 0) {
          setProviders((prev) =>
            prev.map((p) =>
              savedKeyIds.has(p.id) ? { ...p, hasApiKey: true } : p,
            ),
          );
          setApiKeyDrafts((prev) => {
            const next = { ...prev };
            savedKeyIds.forEach((id) => delete next[id]);
            return next;
          });
        }
        setProviderSaveStatus({ text: "Saved.", ok: true });
      } catch (e: any) {
        if (!cancelled) setProviderSaveStatus({ text: String(e), ok: false });
      } finally {
        if (!cancelled) setProviderSaving(false);
      }
    }, 600);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [open, providers, defaultProviderId, apiKeyDrafts]);

  const handleRefreshModels = async () => {
    setModelsRefreshing(true);
    setModelsError(null);
    try {
      const m = await listAiModels(true);
      setAvailableModels(m);
      setProviderModels((prev) => ({ ...prev, [defaultProviderId]: m }));
    } catch (e: unknown) {
      setModelsError(String(e));
    } finally {
      setModelsRefreshing(false);
    }
  };

  const handleRefreshProviderModels = async (providerId: string) => {
    setModelsRefreshing(true);
    setModelsError(null);
    try {
      const m = providerId === defaultProviderId
        ? await listAiModels(true)
        : await listAiProviderModels(providerId);
      setProviderModels((prev) => ({ ...prev, [providerId]: m }));
      if (providerId === defaultProviderId) {
        setAvailableModels(m);
      }
    } catch (e: unknown) {
      setModelsError(String(e));
    } finally {
      setModelsRefreshing(false);
    }
  };

  const handleChangePromptModel = async (key: string, model: string, providerId?: string) => {
    try {
      await saveAiPromptModel(key, model, providerId);
      const refreshed = await getAiPrompts();
      setPrompts(refreshed);
      const providerName = providerId
        ? providers.find((p) => p.id === providerId)?.name ?? "provider"
        : "default provider";
      setPromptStatus((prev) => ({
        ...prev,
        [key]: {
          text: model ? `Model set to ${model} on ${providerName}.` : "Model set to default.",
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

  const updateProvider = (id: string, patch: Partial<AiProviderConfig>) => {
    setProviders((prev) =>
      prev.map((p) => (p.id === id ? { ...p, ...patch } : p)),
    );
    setProviderSaveStatus(null);
    setProviderModelsError(null);
    setMessage(null);
  };

  const handleAddProvider = () => {
    const id = makeProviderId();
    const next: AiProviderConfig = {
      id,
      name: `Provider ${providers.length + 1}`,
      provider: "openai",
      endpoint: DEFAULT_OPENAI_ENDPOINT,
      model: "",
      hasApiKey: false,
      connectTimeoutSecs: 10,
      readTimeoutSecs: 60,
    };
    setProviders((prev) => [...prev, next]);
    setSelectedProviderId(id);
    setProviderSaveStatus(null);
  };

  const handleRemoveProvider = async (id: string) => {
    if (providers.length <= 1) return;
    const providerToRemove = providers.find((p) => p.id === id);
    if (!providerToRemove) return;
    const confirmed = window.confirm(`Remove "${providerToRemove.name}" from AI providers?`);
    if (!confirmed) return;
    setProviderSaving(true);
    setProviderSaveStatus(null);
    try {
      await removeAiProvider(id);
      const remaining = providers.filter((p) => p.id !== id);
      const nextDefault = defaultProviderId === id ? remaining[0].id : defaultProviderId;
      setProviders(remaining);
      setDefaultProviderId(nextDefault);
      setSelectedProviderId((current) => (current === id ? nextDefault : current));
      setApiKeyDrafts((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setProviderSaveStatus({ text: "Provider removed.", ok: true });
    } catch (e: any) {
      setProviderSaveStatus({ text: String(e), ok: false });
    } finally {
      setProviderSaving(false);
    }
  };

  const handleMakeDefault = (id: string) => {
    setDefaultProviderId(id);
    setProviderSaveStatus(null);
  };

  const handleTestDefaults = async () => {
    if (!selectedProvider) return;
    setTesting(true);
    setMessage(null);
    setProviderModelsError(null);
    try {
      const models = await testAiDefaults(
        selectedProvider.provider,
        selectedProvider.endpoint,
        selectedApiKeyDraft,
        selectedProvider.id,
      );
      setProviderModels((prev) => ({ ...prev, [selectedProvider.id]: models }));
      if (selectedProvider.id === defaultProviderId) {
        setAvailableModels(models);
        setModelsError(null);
      }
      // Validate / settle the selected model against what the provider offers.
      const selected = models.includes(selectedProvider.model)
        ? selectedProvider.model
        : models[0] ?? "";
      if (selected !== selectedProvider.model) {
        updateProvider(selectedProvider.id, { model: selected });
      }
      if (models.length === 0) {
        setMessage({
          text: "Connected, but the provider returned no models. You can still type a model.",
          ok: true,
        });
      } else if (selected !== selectedProvider.model) {
        setMessage({
          text: `Connected. Selected ${selected} (the previous model isn't offered by this provider).`,
          ok: true,
        });
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

  // On mobile (standalone): render just the card content without backdrop.
  // On desktop: render as a centered modal with backdrop (see final return).
  const card = (
    <div
      class={`bg-white dark:bg-gray-900 w-full flex flex-col overflow-hidden ${
        standalone
          ? "h-full max-w-none rounded-none shadow-none border-0"
          : "max-w-2xl mx-4 h-[85vh] max-h-[720px] rounded-xl shadow-xl border border-gray-200 dark:border-gray-700"
      }`}
    >
      {!standalone && (
        <div class="shrink-0 flex items-center justify-between px-5 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 class="text-base font-semibold">Settings</h2>
          <button
            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none"
            onClick={onClose}
          >
            ×
          </button>
        </div>
      )}

        {/* Tabs */}
        <div class="shrink-0 flex overflow-x-auto border-b border-gray-200 dark:border-gray-700 px-5">
          <TabButton label="General" active={tab === "general"} onClick={() => setTab("general")} />
          <TabButton label="AI" active={tab === "ai-defaults"} onClick={() => setTab("ai-defaults")} />
          <TabButton label="Review" active={tab === "review"} onClick={() => setTab("review")} />
          <TabButton label="Prompts" active={tab === "prompts"} onClick={() => setTab("prompts")} />
          <TabButton label="Calibration" active={tab === "calibration"} onClick={() => setTab("calibration")} />
          <TabButton label="Sync" active={tab === "sync"} onClick={() => setTab("sync")} />
          <TabButton label="PR List" active={tab === "pr-list"} onClick={() => setTab("pr-list")} />
        </div>

        <div class="flex-1 min-h-0 px-5 py-4 space-y-5 overflow-y-auto overflow-x-hidden">
          {tab === "general" && (
            <section class="space-y-5">
              <p class="text-xs text-gray-500 dark:text-gray-400">
                Appearance preferences. These apply instantly and are saved on this device.
              </p>

              <Field label="Color scheme">
                <select
                  value={theme.value}
                  onChange={(e) => {
                    const next = e.currentTarget.value as Theme;
                    theme.value = next;
                    applyTheme(next);
                  }}
                  class={INPUT_CLASS}
                >
                  {THEME_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>{o.label}</option>
                  ))}
                </select>
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Controls whether Pex uses light mode, dark mode, or your system setting.
                </p>
              </Field>

              <Field label="Diff display">
                <select
                  value={diffView.value}
                  onChange={(e) => (diffView.value = e.currentTarget.value as DiffView)}
                  class={INPUT_CLASS}
                >
                  {DIFF_VIEW_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>{o.label}</option>
                  ))}
                </select>
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Chooses inline or side-by-side layout for file diffs.
                </p>
              </Field>

              <Field label="Font">
                <select
                  value={appFont.value}
                  onChange={(e) => (appFont.value = e.currentTarget.value)}
                  class={INPUT_CLASS}
                  style={{ fontFamily: appFont.value || undefined }}
                >
                  {FONT_OPTIONS.map((f) => (
                    <option key={f.label} value={f.value} style={{ fontFamily: f.value || undefined }}>
                      {f.label}
                    </option>
                  ))}
                </select>
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Applies to app text. Code and diffs keep their monospace font.
                </p>
              </Field>

              <Field label="Text size">
                <select
                  value={appTextSize.value}
                  onChange={(e) => (appTextSize.value = e.currentTarget.value as TextSize)}
                  class={INPUT_CLASS}
                >
                  {TEXT_SIZE_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>{o.label}</option>
                  ))}
                </select>
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Scales the overall app interface text.
                </p>
              </Field>

              <Field label="Diff viewer text size">
                <select
                  value={diffTextSize.value}
                  onChange={(e) => (diffTextSize.value = e.currentTarget.value as TextSize)}
                  class={INPUT_CLASS}
                >
                  {TEXT_SIZE_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>{o.label}</option>
                  ))}
                </select>
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Sizes the code in the diff viewer, independently of the app text size.
                </p>
              </Field>
            </section>
          )}

          {tab === "ai-defaults" && (
            <section class="space-y-3">
              <div class="flex items-center justify-between gap-3">
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  AI provider settings. The default provider is used for reviews; changes save automatically.
                </p>
                <div class="text-xs min-w-[56px] text-right">
                  {providerSaving && (
                    <span class="text-gray-500 dark:text-gray-400">Saving...</span>
                  )}
                  {!providerSaving && providerSaveStatus && (
                    <span class={providerSaveStatus.ok ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}>
                      {providerSaveStatus.text}
                    </span>
                  )}
                </div>
              </div>

              <div class="grid gap-3 md:grid-cols-[190px_1fr]">
                <div class="space-y-2">
                  <div class="flex items-center justify-between">
                    <span class="text-xs font-medium text-gray-600 dark:text-gray-300">
                      Providers
                    </span>
                    <button
                      type="button"
                      onClick={handleAddProvider}
                      class="text-xs px-2 py-1 rounded-lg border border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-800"
                    >
                      Add
                    </button>
                  </div>

                  <div class="space-y-1">
                    {providers.map((p) => (
                      <button
                        type="button"
                        key={p.id}
                        onClick={() => {
                          setSelectedProviderId(p.id);
                          setMessage(null);
                          setProviderModelsError(null);
                        }}
                        class={`w-full text-left px-3 py-2 rounded-lg border text-sm ${
                          selectedProvider?.id === p.id
                            ? "border-accent bg-accent/10"
                            : "border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
                        }`}
                      >
                        <span class="flex items-center justify-between gap-2">
                          <span class="min-w-0 truncate">{p.name || "Provider"}</span>
                          {p.id === defaultProviderId && (
                            <span class="shrink-0 text-[10px] uppercase tracking-wide text-accent">
                              Default
                            </span>
                          )}
                        </span>
                        <span class="block text-[11px] text-gray-500 dark:text-gray-400 truncate">
                          {providerKindLabel(p.provider)}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>

                {selectedProvider && (
                  <div class="space-y-3 min-w-0">
                    <div class="flex items-center justify-between gap-2">
                      <div class="min-w-0">
                        <div class="text-sm font-medium truncate">
                          {selectedProvider.name || "Provider"}
                        </div>
                        <div class="text-[11px] text-gray-500 dark:text-gray-400">
                          {selectedProvider.id === defaultProviderId
                            ? "Default review provider"
                            : "Available provider"}
                        </div>
                      </div>
                      <div class="flex items-center gap-2 shrink-0">
                        {selectedProvider.id !== defaultProviderId && (
                          <button
                            type="button"
                            onClick={() => handleMakeDefault(selectedProvider.id)}
                            class="text-xs px-2 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-800"
                          >
                            Make default
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() => handleRemoveProvider(selectedProvider.id)}
                          disabled={providers.length <= 1}
                          class="text-xs px-2 py-1.5 rounded-lg border border-red-200 dark:border-red-900/60 text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/20 disabled:opacity-50"
                        >
                          Remove
                        </button>
                      </div>
                    </div>

                    <Field label="Name">
                      <input
                        value={selectedProvider.name}
                        onInput={(e) => updateProvider(selectedProvider.id, { name: e.currentTarget.value })}
                        placeholder="Provider name"
                        class={INPUT_CLASS}
                      />
                    </Field>

                    <Field label="Provider">
                      <select
                        value={selectedProvider.provider}
                        onChange={(e) => {
                          const nextProvider = e.currentTarget.value;
                          updateProvider(selectedProvider.id, {
                            provider: nextProvider,
                            endpoint: defaultEndpoint(nextProvider),
                            model: "",
                          });
                          setProviderModels((prev) => ({ ...prev, [selectedProvider.id]: [] }));
                        }}
                        class={INPUT_CLASS}
                      >
                        <option value="openai">OpenAI-compatible</option>
                        <option value="anthropic">Anthropic-compatible</option>
                      </select>
                    </Field>

                    <Field label="Endpoint URL">
                      <input
                        type="url"
                        value={selectedProvider.endpoint}
                        onInput={(e) => {
                          updateProvider(selectedProvider.id, {
                            endpoint: e.currentTarget.value,
                            model: "",
                          });
                          setProviderModels((prev) => ({ ...prev, [selectedProvider.id]: [] }));
                        }}
                        placeholder={defaultEndpoint(selectedProvider.provider)}
                        class={INPUT_CLASS}
                      />
                    </Field>

                    <Field label="API Key">
                      <input
                        type="password"
                        value={selectedApiKeyDraft}
                        onInput={(e) => {
                          const value = e.currentTarget.value;
                          setApiKeyDrafts((prev) => ({ ...prev, [selectedProvider.id]: value }));
                          setProviderSaveStatus(null);
                        }}
                        placeholder={selectedProvider.hasApiKey ? API_KEY_MASK : "Enter API key"}
                        class={INPUT_CLASS}
                      />
                      {selectedProvider.hasApiKey && !selectedApiKeyDraft && (
                        <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                          A key is saved. Leave blank to keep it, or type a new one to replace it.
                        </p>
                      )}
                    </Field>

                    <Field label="Model">
                      {(() => {
                        const modelOptions = selectedProviderModels;
                        const modelListId = `ai-provider-models-${selectedProvider.id}`;
                        return (
                          <div class="flex items-center gap-2">
                            <input
                              list={modelListId}
                              value={selectedProvider.model}
                              onInput={(e) => updateProvider(selectedProvider.id, { model: e.currentTarget.value })}
                              placeholder="Model name"
                              class={`flex-1 min-w-0 ${INPUT_CLASS}`}
                            />
                            <datalist id={modelListId}>
                              {modelOptions.map((m) => (
                                <option value={m}>{m}</option>
                              ))}
                            </datalist>
                            <button
                              type="button"
                              onClick={handleTestDefaults}
                              disabled={testing || !selectedProvider.endpoint}
                              title="Test connection and fetch models"
                              class="text-xs px-2 py-2 rounded-lg border border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
                            >
                              {testing ? "Testing..." : "Test"}
                            </button>
                          </div>
                        );
                      })()}
                      {providerModelsError && (
                        <p class="text-xs text-red-600 dark:text-red-400 mt-1">
                          Couldn't load models: {providerModelsError}
                        </p>
                      )}
                    </Field>

                    <Field label="Connect timeout (seconds)">
                      <input
                        type="number"
                        min={1}
                        max={3600}
                        value={selectedProvider.connectTimeoutSecs}
                        onInput={(e) => {
                          const n = parseInt(e.currentTarget.value, 10);
                          updateProvider(selectedProvider.id, {
                            connectTimeoutSecs: Number.isFinite(n) && n > 0 ? n : 10,
                          });
                        }}
                        placeholder="10"
                        class={INPUT_CLASS}
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
                        value={selectedProvider.readTimeoutSecs}
                        onInput={(e) => {
                          const n = parseInt(e.currentTarget.value, 10);
                          updateProvider(selectedProvider.id, {
                            readTimeoutSecs: Number.isFinite(n) && n > 0 ? n : 60,
                          });
                        }}
                        placeholder="60"
                        class={INPUT_CLASS}
                      />
                      <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                        Stalled-stream guard: maximum time between successive bytes from the server. <strong>Does not bound total generation time</strong> — a slow local model that keeps the connection alive will be allowed to finish.
                      </p>
                    </Field>

                    {message && (
                      <div class={`text-sm px-3 py-2 rounded-lg ${message.ok ? "bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-300" : "bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300"}`}>
                        {message.text}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </section>
          )}

          {tab === "review" && (
            <section>
              <div class="flex items-center justify-between mb-1">
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  How PR review behaves. Changes save automatically.
                </p>
                {prefsSaved && (
                  <span class="text-xs text-green-600 dark:text-green-400">Saved ✓</span>
                )}
              </div>
              <div class="space-y-3">
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
                    class={INPUT_CLASS}
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
                    class={INPUT_CLASS}
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
                      if (!Number.isFinite(n) || n < 0) {
                        setRetryCount(0);
                      } else {
                        setRetryCount(Math.min(10, n));
                      }
                    }}
                    placeholder="1"
                    class={INPUT_CLASS}
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
                      if (!Number.isFinite(n) || n < 0) {
                        setConfidenceThreshold(0);
                      } else {
                        setConfidenceThreshold(Math.min(100, n));
                      }
                    }}
                    placeholder="80"
                    class={INPUT_CLASS}
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
                      if (!Number.isFinite(n) || n < 0) {
                        setBlockingConfidence(0);
                      } else {
                        setBlockingConfidence(Math.min(100, n));
                      }
                    }}
                    placeholder="85"
                    class={INPUT_CLASS}
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
                        class={`${INPUT_CLASS} disabled:opacity-50`}
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
            </section>
          )}

          {tab === "prompts" && (
            <section>
              <div class="flex items-start justify-between gap-3 mb-3">
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  Customize the system prompts used by AI features. Edits save automatically when you click out of a prompt. Each prompt can also be pinned to a specific provider model — leave it on <em>Default</em> to use the model from the AI Defaults tab.
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
                  const modelInputValue = promptModelDrafts[p.key] ?? selectedModel;
                  const selectedPromptProviderId = p.providerId || defaultProviderId;
                  const promptProvider = providers.find((provider) => provider.id === selectedPromptProviderId)
                    ?? providers.find((provider) => provider.id === defaultProviderId)
                    ?? providers[0];
                  const modelOptions = promptProvider
                    ? (providerModels[promptProvider.id] ?? (promptProvider.id === defaultProviderId ? availableModels ?? [] : []))
                    : [];
                  const modelFilter = modelInputValue.trim().toLowerCase();
                  const filteredModelOptions = modelFilter
                    ? modelOptions.filter((m) => m.toLowerCase().includes(modelFilter))
                    : modelOptions;
                  const modelPickerOpen = openPromptModelPicker === p.key;
                  return (
                    <div key={p.key}>
                      <div class="flex items-center gap-2 mb-1">
                        <span class="text-xs text-gray-500 dark:text-gray-400 min-w-0 truncate">
                          {p.label}
                        </span>
                        {p.isCustomized && (
                          <span class="text-[10px] uppercase tracking-wide text-accent shrink-0">
                            customized
                          </span>
                        )}
                      </div>
                      <div class="grid grid-cols-1 sm:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)_auto] gap-2 mb-1">
                        <label class="min-w-0">
                          <span class="block text-[11px] text-gray-500 dark:text-gray-400 mb-1">
                            Provider
                          </span>
                          <select
                            value={promptProvider?.id ?? ""}
                            onChange={(e) => {
                              const providerId = e.currentTarget.value;
                              setPrompts((prev) =>
                                prev.map((prompt) =>
                                  prompt.key === p.key ? { ...prompt, providerId } : prompt,
                                ),
                              );
                              setPromptModelDrafts((prev) => {
                                const next = { ...prev };
                                delete next[p.key];
                                return next;
                              });
                              setOpenPromptModelPicker(null);
                              if (selectedModel) {
                                handleChangePromptModel(p.key, selectedModel, providerId);
                              }
                            }}
                            class="w-full min-w-0 h-7 text-[11px] px-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800"
                            title="Provider used by this prompt. Default follows the AI tab default provider."
                          >
                            {providers.map((provider) => (
                              <option value={provider.id}>
                                {provider.id === defaultProviderId ? `${provider.name} (Default)` : provider.name}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label class="min-w-0">
                          <span class="block text-[11px] text-gray-500 dark:text-gray-400 mb-1">
                            Model
                          </span>
                          <div class="relative" data-prompt-model-picker>
                            <div class="flex min-w-0">
                              <input
                                value={modelInputValue}
                                onFocus={() => setOpenPromptModelPicker(p.key)}
                                onClick={() => setOpenPromptModelPicker(p.key)}
                                onInput={(e) => {
                                  const model = e.currentTarget.value;
                                  setOpenPromptModelPicker(p.key);
                                  setPromptModelDrafts((prev) => ({ ...prev, [p.key]: model }));
                                }}
                                onKeyDown={(e) => {
                                  if (e.key !== "Enter") return;
                                  e.preventDefault();
                                  const draftModel = promptModelDrafts[p.key];
                                  if (draftModel !== undefined && draftModel.trim() === "") {
                                    setOpenPromptModelPicker(null);
                                    setPromptModelDrafts((prev) => {
                                      const next = { ...prev };
                                      delete next[p.key];
                                      return next;
                                    });
                                    setPrompts((prev) =>
                                      prev.map((prompt) =>
                                        prompt.key === p.key ? { ...prompt, model: "" } : prompt,
                                      ),
                                    );
                                    handleChangePromptModel(p.key, "", promptProvider?.id);
                                  }
                                  e.currentTarget.blur();
                                }}
                                onBlur={() => {
                                  window.setTimeout(() => {
                                    const active = document.activeElement;
                                    if (active instanceof Element && active.closest("[data-prompt-model-picker]")) {
                                      return;
                                    }
                                    const draftModel = promptModelDrafts[p.key];
                                    if (draftModel !== undefined && draftModel.trim() === "") {
                                      setPrompts((prev) =>
                                        prev.map((prompt) =>
                                          prompt.key === p.key ? { ...prompt, model: "" } : prompt,
                                        ),
                                      );
                                      handleChangePromptModel(p.key, "", promptProvider?.id);
                                    }
                                    setOpenPromptModelPicker((current) => (current === p.key ? null : current));
                                    setPromptModelDrafts((prev) => {
                                      const next = { ...prev };
                                      delete next[p.key];
                                      return next;
                                    });
                                  }, 0);
                                }}
                                placeholder="Default model"
                                class="w-full min-w-0 h-7 text-[11px] px-1.5 rounded-l-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800"
                                title="Override the model used by this prompt. Empty uses the selected provider's configured model."
                              />
                              <button
                                type="button"
                                onClick={() =>
                                  setOpenPromptModelPicker((current) =>
                                    current === p.key ? null : p.key,
                                  )
                                }
                                disabled={modelOptions.length === 0}
                                class="shrink-0 h-7 px-2 rounded-r-lg border border-l-0 border-gray-300 dark:border-gray-600 text-[11px] hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
                                title="Show matching fetched models"
                                aria-label="Show matching fetched models"
                              >
                                ▾
                              </button>
                            </div>
                            {modelPickerOpen && (
                              <div class="absolute z-20 mt-1 max-h-44 w-full overflow-y-auto rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 shadow-lg">
                                {modelOptions.length === 0 ? (
                                  <div class="px-2 py-1.5 text-[11px] text-gray-500 dark:text-gray-400">
                                    No models loaded
                                  </div>
                                ) : filteredModelOptions.length === 0 ? (
                                  <div class="px-2 py-1.5 text-[11px] text-gray-500 dark:text-gray-400">
                                    No matching models
                                  </div>
                                ) : (
                                  filteredModelOptions.map((m) => (
                                    <button
                                      type="button"
                                      key={m}
                                      onMouseDown={(e) => e.preventDefault()}
                                      onClick={() => {
                                        setOpenPromptModelPicker(null);
                                        setPromptModelDrafts((prev) => {
                                          const next = { ...prev };
                                          delete next[p.key];
                                          return next;
                                        });
                                        setPrompts((prev) =>
                                          prev.map((prompt) =>
                                            prompt.key === p.key ? { ...prompt, model: m } : prompt,
                                          ),
                                        );
                                        handleChangePromptModel(p.key, m, promptProvider?.id);
                                      }}
                                      class={`block w-full px-2 py-1.5 text-left text-[11px] hover:bg-gray-50 dark:hover:bg-gray-800 ${
                                        m === selectedModel ? "text-accent" : "text-gray-700 dark:text-gray-200"
                                      }`}
                                    >
                                      {m}
                                    </button>
                                  ))
                                )}
                              </div>
                            )}
                          </div>
                        </label>
                        <button
                          type="button"
                          onClick={() => promptProvider && handleRefreshProviderModels(promptProvider.id)}
                          disabled={modelsRefreshing || !promptProvider}
                          title="Fetch models for this provider"
                          class="self-end px-2 py-1 rounded-lg border border-gray-300 dark:border-gray-600 text-[11px] hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
                        >
                          {modelsRefreshing ? "Refreshing..." : "Refresh"}
                        </button>
                      </div>
                      <textarea
                        value={draft}
                        onInput={(e) =>
                          setPromptDrafts((prev) => ({
                            ...prev,
                            [p.key]: e.currentTarget.value,
                          }))
                        }
                        onBlur={() => {
                          // Autosave on blur: only when the text actually changed
                          // and isn't empty (use Reset to restore the default).
                          if (dirty && draft.trim().length > 0) {
                            handleSavePrompt(p.key);
                          }
                        }}
                        rows={8}
                        class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-xs font-mono outline-none focus:ring-2 focus:ring-accent resize-y"
                      />
                      <p class="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                        {p.description}
                      </p>
                      <div class="flex items-center gap-2 mt-2">
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

          {tab === "sync" && <SyncPanel active={open && tab === "sync"} />}

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
        </div>
      </div>
    );

  return standalone ? card : (
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
      {card}
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
      class={`px-3 py-2 text-sm font-medium border-b-2 -mb-px whitespace-nowrap ${
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

function SyncPanel({ active }: { active: boolean }) {
  const status = syncStatus.value;
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load current status when the panel becomes visible, and seed the URL field
  // from whatever's already configured. The token is never echoed back.
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    getSyncStatus()
      .then((s) => {
        if (cancelled) return;
        syncStatus.value = s;
        setUrl(s.url);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [active]);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleEnable = () =>
    run(async () => {
      syncStatus.value = await enableSync(url.trim(), token);
      setToken("");
    });
  const handleDisable = () =>
    run(async () => {
      syncStatus.value = await disableSync();
    });
  const handleSyncNow = () =>
    run(async () => {
      syncStatus.value = await syncNow();
      // Reflect any rows the pull brought in (e.g. a review another device
      // marked completed) immediately, even if this sync reported 0 new frames
      // because a background sync already pulled them.
      reconcilePersistedReviews(await listCompletedReviews());
    });

  const enabled = status?.enabled ?? false;
  const lastSync = status?.lastSync
    ? new Date(status.lastSync).toLocaleString()
    : "Never";

  return (
    <section class="space-y-4">
      <p class="text-xs text-gray-500 dark:text-gray-400">
        Optionally sync your reviewer state — viewed files, saved connections,
        AI settings, prompts, and finding verdicts — across your own devices via
        a private libsql/Turso database. Your local data keeps working unchanged
        when sync is off. Secrets (PATs, API keys, the sync token) never sync;
        they stay on each device, so you'll re-enter them once per device.
      </p>

      <div class="rounded-lg border border-gray-200 dark:border-gray-700 p-3 text-xs space-y-1">
        <div class="flex items-center justify-between">
          <span class="text-gray-500 dark:text-gray-400">Status</span>
          <span class={`font-medium ${enabled ? "text-green-600 dark:text-green-400" : "text-gray-500 dark:text-gray-400"}`}>
            {status?.syncing ? "Syncing…" : enabled ? "Enabled" : "Disabled"}
          </span>
        </div>
        <div class="flex items-center justify-between">
          <span class="text-gray-500 dark:text-gray-400">Last sync</span>
          <span class="text-gray-700 dark:text-gray-200">{lastSync}</span>
        </div>
        {status?.lastError && (
          <div class="text-red-600 dark:text-red-400 pt-1 break-words">
            Last error: {status.lastError}
          </div>
        )}
      </div>

      <Field label="Remote database URL">
        <input
          type="text"
          value={url}
          onInput={(e) => setUrl(e.currentTarget.value)}
          placeholder="libsql://your-db.turso.io"
          spellcheck={false}
          autocapitalize="off"
          class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-900 text-sm"
        />
      </Field>

      <Field label={enabled ? "Auth token (leave blank to keep current)" : "Auth token"}>
        <input
          type="password"
          value={token}
          onInput={(e) => setToken(e.currentTarget.value)}
          placeholder={status?.configured ? "••••••••" : "Paste your auth token"}
          spellcheck={false}
          autocapitalize="off"
          class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-900 text-sm"
        />
      </Field>

      {error && (
        <p class="text-xs text-red-600 dark:text-red-400 break-words">{error}</p>
      )}

      <div class="flex items-center gap-2">
        <button
          onClick={handleEnable}
          disabled={busy || !url.trim()}
          class="px-3 py-1.5 rounded-lg bg-accent text-white text-sm font-medium disabled:opacity-50"
        >
          {enabled ? "Update & sync" : "Enable sync"}
        </button>
        {enabled && (
          <button
            onClick={handleSyncNow}
            disabled={busy}
            class="px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 text-sm hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
          >
            Sync now
          </button>
        )}
        {enabled && (
          <button
            onClick={handleDisable}
            disabled={busy}
            class="px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 text-sm text-red-600 dark:text-red-400 hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
          >
            Disable
          </button>
        )}
      </div>
    </section>
  );
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
