import { useState, useEffect } from "preact/hooks";
import {
  getAiSettings,
  saveAiSettings,
  testAiConnection,
  checkPurist,
  getPuristPath,
  savePuristPath,
} from "@/lib/api";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function AiSettings({ open, onClose }: Props) {
  const [provider, setProvider] = useState("openai");
  const [endpoint, setEndpoint] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [puristPath, setPuristPath] = useState("");
  const [puristCheck, setPuristCheck] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (open) {
      loadSettings();
    }
  }, [open]);

  const loadSettings = async () => {
    try {
      const [settings, pp] = await Promise.all([getAiSettings(), getPuristPath()]);
      setProvider(settings.provider);
      setEndpoint(settings.endpoint);
      setModel(settings.model);
      setApiKey("");
      if (pp) setPuristPath(pp);
    } catch {
      // defaults are fine
    }
  };

  const handleSaveAi = async () => {
    setSaving(true);
    setMessage(null);
    try {
      await saveAiSettings(provider, endpoint, model, apiKey);
      setMessage({ text: "AI settings saved.", ok: true });
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setSaving(false);
    }
  };

  const handleSavePurist = async () => {
    setSaving(true);
    setMessage(null);
    try {
      await savePuristPath(puristPath);
      setMessage({ text: "Purist path saved.", ok: true });
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setSaving(false);
    }
  };

  const handleCheckPurist = async () => {
    setPuristCheck(null);
    try {
      const r = await checkPurist(puristPath);
      setPuristCheck(r.message);
      if (r.ok) {
        await savePuristPath(puristPath);
      }
    } catch (e: any) {
      setPuristCheck(String(e));
    }
  };

  const handleTestConnection = async () => {
    // Save settings first so the backend can configure the provider
    setTesting(true);
    setMessage(null);
    try {
      await saveAiSettings(provider, endpoint, model, apiKey);
      const result = await testAiConnection();
      setMessage({ text: result, ok: true });
    } catch (e: any) {
      setMessage({ text: String(e), ok: false });
    } finally {
      setTesting(false);
    }
  };

  if (!open) return null;

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div
        class="bg-white dark:bg-gray-900 rounded-xl shadow-xl border border-gray-200 dark:border-gray-700 w-full max-w-md mx-4 max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
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

        <div class="px-5 py-4 space-y-5">
          {/* LLM Provider */}
          <section>
            <h3 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">LLM Provider</h3>
            <div class="space-y-3">
              <Field label="Provider">
                <select
                  value={provider}
                  onChange={(e) => setProvider(e.currentTarget.value)}
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
                  onInput={(e) => setEndpoint(e.currentTarget.value)}
                  placeholder={provider === "openai" ? "https://api.openai.com" : "https://api.anthropic.com"}
                  class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                />
              </Field>

              <Field label="Model">
                <input
                  type="text"
                  value={model}
                  onInput={(e) => setModel(e.currentTarget.value)}
                  placeholder={provider === "openai" ? "gpt-4.1" : "claude-sonnet-4-20250514"}
                  class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                />
              </Field>

              <Field label="API Key">
                <input
                  type="password"
                  value={apiKey}
                  onInput={(e) => setApiKey(e.currentTarget.value)}
                  placeholder="Enter API key"
                  class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                />
              </Field>
            </div>

            <button
              onClick={handleSaveAi}
              disabled={saving || !endpoint || !model}
              class="mt-3 px-4 py-2 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50"
            >
              {saving ? "Saving..." : "Save AI Settings"}
            </button>
            <button
              onClick={handleTestConnection}
              disabled={testing || !endpoint || !model || !apiKey}
              class="mt-3 ml-2 px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg text-sm font-medium hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
            >
              {testing ? "Testing..." : "Test Connection"}
            </button>
          </section>

          <hr class="border-gray-200 dark:border-gray-700" />

          {/* Purist */}
          <section>
            <h3 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">Purist (Full PR Review)</h3>

            <Field label="Purist path">
              <div class="flex gap-2">
                <input
                  type="text"
                  value={puristPath}
                  onInput={(e) => setPuristPath(e.currentTarget.value)}
                  placeholder="~/repos/purist"
                  class="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none focus:ring-2 focus:ring-accent"
                />
                <button
                  onClick={handleCheckPurist}
                  class="px-3 py-2 text-xs border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-800"
                >
                  Check
                </button>
              </div>
            </Field>

            {puristCheck && (
              <p class={`text-xs mt-1.5 ${puristCheck.startsWith("Purist found") ? "text-green-600 dark:text-green-400" : "text-orange-600 dark:text-orange-400"}`}>
                {puristCheck}
              </p>
            )}

            <button
              onClick={handleSavePurist}
              disabled={saving || !puristPath}
              class="mt-3 px-4 py-2 bg-accent hover:bg-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50"
            >
              Save Purist Path
            </button>
          </section>

          {/* Status message */}
          {message && (
            <div class={`text-sm px-3 py-2 rounded-lg ${message.ok ? "bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-300" : "bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300"}`}>
              {message.text}
            </div>
          )}
        </div>
      </div>
    </div>
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
