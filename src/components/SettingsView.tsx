import { useState, useEffect } from "react";
import {
  Key,
  BookMarked,
  ShieldAlert,
  Target,
  CheckCircle2,
  AlertCircle,
  Loader2,
  Eye,
  EyeOff,
  Plus,
  Trash2,
  RefreshCw,
  Zap,
} from "lucide-react";
import {
  getPersonalDictionary,
  addWordToDictionary,
  removeWordFromDictionary,
  getAppExclusions,
  addAppExclusion,
  removeAppExclusion,
  getSetting,
  saveSetting,
  AppExclusionItem,
} from "../services/db";
import { invoke } from "@tauri-apps/api/core";

export function SettingsView() {
  // API Key state
  const [apiKey, setApiKey] = useState<string>("");
  const [showKey, setShowKey] = useState<boolean>(false);
  const [isTestingKey, setIsTestingKey] = useState<boolean>(false);
  const [keyStatus, setKeyStatus] = useState<{
    tested: boolean;
    success?: boolean;
    latency?: number;
    message?: string;
  }>({ tested: false });

  // Writing Goals state
  const [audience, setAudience] = useState<string>("general");
  const [formality, setFormality] = useState<string>("neutral");
  const [primaryTone, setPrimaryTone] = useState<string>("confident");
  const [goalsSaved, setGoalsSaved] = useState<boolean>(false);

  // Personal Dictionary state
  const [dictionary, setDictionary] = useState<string[]>([]);
  const [newWord, setNewWord] = useState<string>("");
  const [dictSearch, setDictSearch] = useState<string>("");

  // App Exclusions state
  const [exclusions, setExclusions] = useState<AppExclusionItem[]>([]);
  const [newAppName, setNewAppName] = useState<string>("");
  const [newAppDisplay, setNewAppDisplay] = useState<string>("");
  const [activeProcess, setActiveProcess] = useState<string>("");

  // Load initial settings from SQLite
  useEffect(() => {
    async function loadData() {
      // 1. Load API key
      const savedKey = await getSetting("gemini_api_key");
      setApiKey(savedKey);

      // 2. Load Writing Goals
      const savedAudience = await getSetting("writing_audience", "general");
      const savedFormality = await getSetting("writing_formality", "neutral");
      const savedTone = await getSetting("writing_tone", "confident");
      setAudience(savedAudience);
      setFormality(savedFormality);
      setPrimaryTone(savedTone);

      // 3. Load Personal Dictionary
      const words = await getPersonalDictionary();
      setDictionary(words);

      // 4. Load App Exclusions
      const ex = await getAppExclusions();
      setExclusions(ex);

      // Query current active foreground process
      try {
        const proc = await invoke<string>("get_active_process");
        if (proc) setActiveProcess(proc);
      } catch {
        // ignore
      }
    }
    loadData();
  }, []);

  // Save API Key
  const handleSaveApiKey = async () => {
    await saveSetting("gemini_api_key", apiKey.trim());
    setKeyStatus({ tested: false });
  };

  // Test API Key
  const handleTestApiKey = async () => {
    setIsTestingKey(true);
    setKeyStatus({ tested: false });
    try {
      const res = await invoke<{
        success: boolean;
        status: string;
        latency_ms?: number;
        message?: string;
      }>("test_gemini_connection", { apiKey: apiKey.trim() || null });

      setKeyStatus({
        tested: true,
        success: res.success,
        latency: res.latency_ms,
        message: res.message || "Connection verified successfully!",
      });

      if (res.success && apiKey.trim()) {
        await saveSetting("gemini_api_key", apiKey.trim());
      }
    } catch (e: any) {
      setKeyStatus({
        tested: true,
        success: false,
        message: String(e || "Connection failed"),
      });
    } finally {
      setIsTestingKey(false);
    }
  };

  // Save Writing Goals
  const handleGoalChange = async (
    type: "audience" | "formality" | "tone",
    value: string
  ) => {
    if (type === "audience") {
      setAudience(value);
      await saveSetting("writing_audience", value);
    } else if (type === "formality") {
      setFormality(value);
      await saveSetting("writing_formality", value);
    } else if (type === "tone") {
      setPrimaryTone(value);
      await saveSetting("writing_tone", value);
    }
    setGoalsSaved(true);
    setTimeout(() => setGoalsSaved(false), 2000);
  };

  // Personal Dictionary Handlers
  const handleAddWord = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!newWord.trim()) return;
    const updated = await addWordToDictionary(newWord.trim());
    setDictionary(updated);
    setNewWord("");
  };

  const handleRemoveWord = async (word: string) => {
    const updated = await removeWordFromDictionary(word);
    setDictionary(updated);
  };

  // App Exclusion Handlers
  const handleAddExclusion = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!newAppName.trim()) return;
    const updated = await addAppExclusion(
      newAppName.trim(),
      newAppDisplay.trim() || newAppName.trim()
    );
    setExclusions(updated);
    setNewAppName("");
    setNewAppDisplay("");
  };

  const handleRemoveExclusion = async (appName: string) => {
    const updated = await removeAppExclusion(appName);
    setExclusions(updated);
  };

  const filteredDictionary = dictionary.filter((w) =>
    w.toLowerCase().includes(dictSearch.toLowerCase())
  );

  return (
    <div className="flex-1 overflow-y-auto px-8 py-6 space-y-8 max-w-5xl mx-auto w-full text-slate-200">
      {/* View Header */}
      <div>
        <h2 className="text-xl font-bold text-white flex items-center gap-2">
          <span>Settings &amp; Preferences</span>
          {goalsSaved && (
            <span className="text-xs font-semibold text-emerald-400 bg-emerald-500/10 border border-emerald-500/30 px-2 py-0.5 rounded-full flex items-center gap-1 animate-fadeIn">
              <CheckCircle2 className="w-3 h-3" /> Saved to SQLite
            </span>
          )}
        </h2>
        <p className="text-xs text-slate-400 mt-1">
          Manage your personal dictionary, blacklisted apps, AI writing goals, and Gemini credentials.
        </p>
      </div>

      {/* SECTION 1: Gemini AI Key & Writing Goals */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Gemini API Key Card */}
        <div className="rounded-2xl border border-white/10 bg-slate-900/50 p-5 backdrop-blur-sm flex flex-col justify-between space-y-4">
          <div>
            <div className="flex items-center gap-2 text-sm font-semibold text-white">
              <div className="p-1.5 rounded-lg bg-indigo-500/10 border border-indigo-500/20 text-indigo-400">
                <Key className="w-4 h-4" />
              </div>
              <span>Gemini AI Key (Cloud Tier)</span>
            </div>
            <p className="text-xs text-slate-400 mt-1 leading-relaxed">
              Powers tone scoring and context rewrites with <span className="text-indigo-300 font-mono">gemini-2.5-flash-lite</span>. If omitted, the offline Harper engine operates standalone.
            </p>

            <div className="mt-3 relative">
              <input
                type={showKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="AIzaSy..."
                className="w-full px-3 py-2 text-xs font-mono rounded-xl bg-slate-950 border border-white/10 focus:border-indigo-500 focus:outline-none pr-9 text-slate-100"
              />
              <button
                type="button"
                onClick={() => setShowKey(!showKey)}
                className="absolute right-2.5 top-2.5 text-slate-400 hover:text-white"
              >
                {showKey ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
              </button>
            </div>

            {/* Test result status badge */}
            {keyStatus.tested && (
              <div
                className={`mt-2.5 p-2 rounded-xl text-xs flex items-center gap-2 border ${
                  keyStatus.success
                    ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-300"
                    : "bg-red-500/10 border-red-500/30 text-red-300"
                }`}
              >
                {keyStatus.success ? (
                  <CheckCircle2 className="w-4 h-4 shrink-0 text-emerald-400" />
                ) : (
                  <AlertCircle className="w-4 h-4 shrink-0 text-red-400" />
                )}
                <span className="truncate">
                  {keyStatus.success
                    ? `Connected (${keyStatus.latency}ms latency)`
                    : keyStatus.message}
                </span>
              </div>
            )}
          </div>

          <div className="flex items-center gap-2 pt-2 border-t border-white/5">
            <button
              onClick={handleSaveApiKey}
              className="px-3 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold transition-colors cursor-pointer"
            >
              Save Key
            </button>
            <button
              onClick={handleTestApiKey}
              disabled={isTestingKey}
              className="px-3 py-1.5 rounded-lg bg-white/5 hover:bg-white/10 border border-white/10 text-slate-200 text-xs font-medium flex items-center gap-1.5 transition-colors disabled:opacity-40 cursor-pointer"
            >
              {isTestingKey ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <RefreshCw className="w-3.5 h-3.5" />
              )}
              <span>Test Connection</span>
            </button>
          </div>
        </div>

        {/* Writing Goals Card */}
        <div className="rounded-2xl border border-white/10 bg-slate-900/50 p-5 backdrop-blur-sm space-y-4">
          <div className="flex items-center gap-2 text-sm font-semibold text-white">
            <div className="p-1.5 rounded-lg bg-purple-500/10 border border-purple-500/20 text-purple-400">
              <Target className="w-4 h-4" />
            </div>
            <span>Writing Goals &amp; Personas</span>
          </div>
          <p className="text-xs text-slate-400 leading-relaxed">
            Configure how the AI tunes rewrite suggestions and tone evaluation.
          </p>

          <div className="space-y-3 pt-1">
            {/* Audience */}
            <div>
              <label className="text-[11px] font-semibold text-slate-300 uppercase tracking-wider block mb-1">
                Target Audience
              </label>
              <div className="grid grid-cols-3 gap-1.5">
                {[
                  { id: "general", label: "General" },
                  { id: "technical", label: "Technical" },
                  { id: "academic", label: "Academic" },
                ].map((item) => (
                  <button
                    key={item.id}
                    onClick={() => handleGoalChange("audience", item.id)}
                    className={`py-1.5 text-xs font-medium rounded-lg border transition-all cursor-pointer ${
                      audience === item.id
                        ? "bg-purple-600/30 border-purple-500 text-white"
                        : "bg-white/5 border-white/5 text-slate-400 hover:text-white"
                    }`}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Formality */}
            <div>
              <label className="text-[11px] font-semibold text-slate-300 uppercase tracking-wider block mb-1">
                Formality Level
              </label>
              <div className="grid grid-cols-3 gap-1.5">
                {[
                  { id: "casual", label: "Casual ☕" },
                  { id: "neutral", label: "Neutral ⚖️" },
                  { id: "formal", label: "Formal 🏛️" },
                ].map((item) => (
                  <button
                    key={item.id}
                    onClick={() => handleGoalChange("formality", item.id)}
                    className={`py-1.5 text-xs font-medium rounded-lg border transition-all cursor-pointer ${
                      formality === item.id
                        ? "bg-purple-600/30 border-purple-500 text-white"
                        : "bg-white/5 border-white/5 text-slate-400 hover:text-white"
                    }`}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Primary Tone */}
            <div>
              <label className="text-[11px] font-semibold text-slate-300 uppercase tracking-wider block mb-1">
                Preferred Tone Preset
              </label>
              <div className="grid grid-cols-4 gap-1">
                {[
                  { id: "confident", label: "Confident" },
                  { id: "direct", label: "Direct" },
                  { id: "friendly", label: "Friendly" },
                  { id: "analytical", label: "Analytical" },
                ].map((item) => (
                  <button
                    key={item.id}
                    onClick={() => handleGoalChange("tone", item.id)}
                    className={`py-1 text-[11px] font-medium rounded-lg border transition-all cursor-pointer truncate ${
                      primaryTone === item.id
                        ? "bg-purple-600/30 border-purple-500 text-white"
                        : "bg-white/5 border-white/5 text-slate-400 hover:text-white"
                    }`}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* SECTION 2: Personal Dictionary */}
      <div className="rounded-2xl border border-white/10 bg-slate-900/50 p-5 backdrop-blur-sm space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 text-sm font-semibold text-white">
            <div className="p-1.5 rounded-lg bg-emerald-500/10 border border-emerald-500/20 text-emerald-400">
              <BookMarked className="w-4 h-4" />
            </div>
            <span>Personal Dictionary</span>
            <span className="px-2 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400 text-xs font-mono font-bold">
              {dictionary.length}
            </span>
          </div>
          <span className="text-[11px] text-slate-400">
            Never flagged by Harper in editor or widget
          </span>
        </div>

        {/* Add Word Input */}
        <form onSubmit={handleAddWord} className="flex gap-2">
          <input
            type="text"
            value={newWord}
            onChange={(e) => setNewWord(e.target.value)}
            placeholder="Add new word to personal dictionary (e.g., Nishikanto, backend, Tauri)..."
            className="flex-1 px-3 py-2 text-xs rounded-xl bg-slate-950 border border-white/10 focus:border-emerald-500 focus:outline-none text-slate-100"
          />
          <button
            type="submit"
            className="px-4 py-2 rounded-xl bg-emerald-600 hover:bg-emerald-500 text-white text-xs font-semibold flex items-center gap-1.5 transition-colors cursor-pointer"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Add Word</span>
          </button>
        </form>

        {/* Word Chips */}
        {dictionary.length === 0 ? (
          <div className="py-6 text-center text-xs text-slate-500 border border-dashed border-white/5 rounded-xl">
            No custom words added yet. You can also click "Add to Dictionary" directly on flagged words.
          </div>
        ) : (
          <div className="space-y-2">
            {dictionary.length > 8 && (
              <input
                type="text"
                value={dictSearch}
                onChange={(e) => setDictSearch(e.target.value)}
                placeholder="Search dictionary..."
                className="w-full px-2.5 py-1 text-[11px] rounded-lg bg-slate-950/70 border border-white/5 text-slate-300"
              />
            )}
            <div className="flex flex-wrap gap-1.5 max-h-48 overflow-y-auto p-1">
              {filteredDictionary.map((word) => (
                <span
                  key={word}
                  className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-emerald-500/10 border border-emerald-500/25 text-xs text-emerald-200"
                >
                  <span className="font-mono">{word}</span>
                  <button
                    type="button"
                    onClick={() => handleRemoveWord(word)}
                    title="Remove from dictionary"
                    className="text-emerald-400 hover:text-red-400 transition-colors cursor-pointer"
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* SECTION 3: App Exclusions (Blacklist) */}
      <div className="rounded-2xl border border-white/10 bg-slate-900/50 p-5 backdrop-blur-sm space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 text-sm font-semibold text-white">
            <div className="p-1.5 rounded-lg bg-rose-500/10 border border-rose-500/20 text-rose-400">
              <ShieldAlert className="w-4 h-4" />
            </div>
            <span>App Exclusions (Blacklist)</span>
            <span className="px-2 py-0.5 rounded-full bg-rose-500/15 text-rose-400 text-xs font-mono font-bold">
              {exclusions.length}
            </span>
          </div>
          <span className="text-[11px] text-slate-400">
            Floating widget stays hidden when these apps are active
          </span>
        </div>

        {/* Active process detection banner */}
        {activeProcess && (
          <div className="p-2.5 rounded-xl bg-indigo-500/10 border border-indigo-500/20 text-xs text-indigo-300 flex items-center justify-between">
            <span className="flex items-center gap-1.5">
              <Zap className="w-3.5 h-3.5 text-indigo-400" />
              <span>Current Foreground App: <strong className="font-mono">{activeProcess}</strong></span>
            </span>
            {!exclusions.some((x) => x.app_name.toLowerCase() === activeProcess.toLowerCase()) ? (
              <button
                onClick={() => addAppExclusion(activeProcess, activeProcess)}
                className="px-2 py-0.5 rounded bg-rose-600 hover:bg-rose-500 text-white text-[10px] font-semibold cursor-pointer"
              >
                + Exclude This App
              </button>
            ) : (
              <span className="text-[10px] text-rose-400 font-semibold">Already Blacklisted</span>
            )}
          </div>
        )}

        {/* Add custom exclusion form */}
        <form onSubmit={handleAddExclusion} className="grid grid-cols-1 sm:grid-cols-3 gap-2">
          <input
            type="text"
            value={newAppName}
            onChange={(e) => setNewAppName(e.target.value)}
            placeholder="Process name (e.g. Code.exe, iterm2)..."
            className="sm:col-span-2 px-3 py-2 text-xs rounded-xl bg-slate-950 border border-white/10 focus:border-rose-500 focus:outline-none text-slate-100 font-mono"
          />
          <button
            type="submit"
            className="px-4 py-2 rounded-xl bg-rose-600 hover:bg-rose-500 text-white text-xs font-semibold flex items-center justify-center gap-1.5 transition-colors cursor-pointer"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Blacklist App</span>
          </button>
        </form>

        {/* Quick presets */}
        <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-slate-400">
          <span>Quick presets:</span>
          {[
            { name: "code.exe", display: "VS Code" },
            { name: "windowsterminal.exe", display: "Windows Terminal" },
            { name: "discord.exe", display: "Discord" },
            { name: "slack.exe", display: "Slack" },
          ].map((preset) => {
            const isAdded = exclusions.some((x) => x.app_name.toLowerCase() === preset.name);
            return (
              <button
                key={preset.name}
                disabled={isAdded}
                onClick={async () => {
                  const updated = await addAppExclusion(preset.name, preset.display);
                  setExclusions(updated);
                }}
                className={`px-2 py-0.5 rounded-md border text-[10px] transition-colors ${
                  isAdded
                    ? "opacity-40 border-white/5 cursor-not-allowed text-slate-500"
                    : "bg-white/5 border-white/10 hover:border-rose-400 text-slate-300 hover:text-white cursor-pointer"
                }`}
              >
                + {preset.display}
              </button>
            );
          })}
        </div>

        {/* Exclusions Table / List */}
        <div className="divide-y divide-white/5 rounded-xl border border-white/5 bg-slate-950/60 overflow-hidden">
          {exclusions.map((item) => (
            <div
              key={item.app_name}
              className="flex items-center justify-between px-3.5 py-2.5 text-xs hover:bg-white/[0.02]"
            >
              <div className="flex items-center gap-2">
                <span className="font-semibold text-slate-200">
                  {item.display_name}
                </span>
                <span className="text-[11px] font-mono text-slate-400 bg-white/5 px-1.5 py-0.5 rounded">
                  {item.app_name}
                </span>
              </div>
              <button
                onClick={() => handleRemoveExclusion(item.app_name)}
                title="Remove exclusion"
                className="p-1 rounded text-slate-500 hover:text-rose-400 hover:bg-rose-500/10 transition-colors cursor-pointer"
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
