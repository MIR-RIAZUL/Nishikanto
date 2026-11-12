import { useState, useEffect, useCallback, useMemo } from "react";
import {
  Sparkles,
  Check,
  CheckCircle2,
  Minus,
  Maximize2,
  CornerDownLeft,
  Wand2,
  ScanText,
  Loader2,
  X,
  ChevronRight,
} from "lucide-react";
import { useTauriState } from "../hooks/useTauriState";
import { LocalLintResult, GeminiAnalysisResult } from "../hooks/useWritingEngine";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

interface OsCapturePayload {
  text: string;
  source_app: string;
  method: string;
  bounds: { x: number; y: number; width: number; height: number };
  lints: LocalLintResult[];
}

export function WidgetWindow() {
  const {
    isPaused,
    openMainEditor,
    expandWidget,
    collapseWidget,
    replaceActiveText,
    captureActiveText,
  } = useTauriState();

  // State: pill (false) vs expanded popover (true)
  const [isExpanded, setIsExpanded] = useState<boolean>(false);
  const [lastAction, setLastAction] = useState<string | null>(null);

  // OS capture state
  const [capturedText, setCapturedText] = useState<string>("");
  const [sourceApp, setSourceApp] = useState<string>("Active App");
  const [capturedLints, setCapturedLints] = useState<LocalLintResult[]>([]);
  const [isAiProcessing, setIsAiProcessing] = useState<boolean>(false);
  const [appliedSuccess, setAppliedSuccess] = useState<boolean>(false);

  // Derive overall tone like Grammarly
  const overallTone = useMemo(() => {
    if (!capturedText) return "Confident 🎯";
    const lower = capturedText.toLowerCase();
    if (lower.includes("hey") || lower.includes("thanks") || lower.includes("cool") || lower.includes("awesome") || lower.includes("!")) {
      return "Friendly ☕";
    }
    if (lower.includes("furthermore") || lower.includes("therefore") || lower.includes("in regards to") || lower.includes("respectfully") || lower.includes("sincerely")) {
      return "Formal 🏛️";
    }
    const words = capturedText.trim().split(/\s+/).length;
    if (words < 12 && capturedLints.length === 0) {
      return "Direct ⚡";
    }
    return "Confident 🎯";
  }, [capturedText, capturedLints]);

  // Expansion and collapse handlers
  const handleExpand = useCallback(async () => {
    setIsExpanded(true);
    await expandWidget();
  }, [expandWidget]);

  const handleCollapse = useCallback(async () => {
    setIsExpanded(false);
    await collapseWidget();
  }, [collapseWidget]);

  // Listen to OS hotkey capture events
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<OsCapturePayload>("os-text-captured", (event) => {
      const payload = event.payload;
      setCapturedText(payload.text);
      setSourceApp(payload.source_app || "Active App");
      setCapturedLints(payload.lints || []);
      // When Alt+G is pressed, default to compact pill with badge
      setIsExpanded(false);
      collapseWidget();
      setLastAction("Captured via Alt+G");
      setTimeout(() => setLastAction(null), 2500);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, [collapseWidget]);

  // Auto-collapse on Escape key
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isExpanded) {
        handleCollapse();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isExpanded, handleCollapse]);

  // 1-Click Replacement into external app
  const handleApplyReplacement = async (newText: string) => {
    setAppliedSuccess(true);
    await replaceActiveText(newText);
    setTimeout(() => {
      setAppliedSuccess(false);
      setCapturedText("");
      setCapturedLints([]);
      handleCollapse();
    }, 1200);
  };

  // Accept single lint suggestion
  const applySingleLintFix = async (lint: LocalLintResult, replacement: string) => {
    const chars = Array.from(capturedText);
    const before = chars.slice(0, lint.start).join("");
    const after = chars.slice(lint.end).join("");
    const updated = before + replacement + after;

    setCapturedText(updated);
    setCapturedLints((prev) => prev.filter((l) => l.id !== lint.id));

    // Immediately replace into the external app
    await handleApplyReplacement(updated);
  };

  const dismissLint = (lintId: string) => {
    setCapturedLints((prev) => prev.filter((l) => l.id !== lintId));
  };

  // AI Rewrite Modes
  const handleAiRewrite = async (mode: string) => {
    if (!capturedText.trim()) {
      await captureActiveText();
      return;
    }
    setIsAiProcessing(true);
    try {
      const res = await invoke<GeminiAnalysisResult>("analyze_with_gemini", {
        text: capturedText,
        mode,
      });
      if (res.rewritten_text) {
        setCapturedText(res.rewritten_text);
        // re-run local check on newly rewritten text
        const newLints = await invoke<LocalLintResult[]>("check_text_local", {
          text: res.rewritten_text,
        });
        setCapturedLints(newLints);
        setLastAction(`AI: ${mode}`);
        setTimeout(() => setLastAction(null), 2500);
      }
    } catch (e) {
      console.warn("AI rewrite error:", e);
    } finally {
      setIsAiProcessing(false);
    }
  };

  // -------------------------------------------------------------
  // 1. COMPACT FLOATING PILL STATE (40x40px sleek draggable pill)
  // -------------------------------------------------------------
  if (!isExpanded) {
    const issueCount = capturedLints.length;
    const isClean = issueCount === 0;

    return (
      <div
        className="w-[52px] h-[52px] flex items-center justify-center relative p-1 select-none font-sans antialiased"
        data-tauri-drag-region
      >
        <button
          data-tauri-drag-region
          onMouseDown={(e) => e.preventDefault()}
          onClick={handleExpand}
          title={
            isClean
              ? "Nishikanto: All clear! Click to open assistant."
              : `Nishikanto: ${issueCount} suggestion${issueCount === 1 ? "" : "s"}. Click to open.`
          }
          className={`w-10 h-10 rounded-full cursor-pointer relative flex items-center justify-center transition-all duration-300 shadow-[0_4px_20px_rgba(0,0,0,0.65)] hover:scale-105 active:scale-95 ${
            isClean
              ? "bg-slate-900/95 border-2 border-emerald-500/70 hover:border-emerald-400 ring-2 ring-emerald-500/20"
              : "bg-slate-900/95 border-2 border-indigo-500/70 hover:border-indigo-400 ring-2 ring-indigo-500/20"
          }`}
        >
          {isAiProcessing ? (
            <Loader2 className="w-5 h-5 text-indigo-400 animate-spin" />
          ) : isClean ? (
            <Check className="w-5 h-5 text-emerald-400 stroke-[3]" />
          ) : (
            <Sparkles className="w-4 h-4 text-indigo-300" />
          )}

          {/* Issue Count Badge */}
          {!isClean && (
            <span
              className={`absolute -top-1 -right-1 min-w-[18px] h-[18px] px-1 rounded-full text-[10px] font-bold flex items-center justify-center text-white shadow-md animate-pulse ${
                capturedLints.some((l) => l.category === "Correctness")
                  ? "bg-red-500 shadow-red-500/50"
                  : "bg-amber-500 shadow-amber-500/50"
              }`}
            >
              {issueCount}
            </span>
          )}

          {/* Subtle pause dot if paused */}
          {isPaused && (
            <span className="absolute bottom-0 right-0 w-2 h-2 rounded-full bg-amber-400 border border-slate-900" />
          )}
        </button>
      </div>
    );
  }

  // -------------------------------------------------------------
  // 2. SMOOTH POPOVER EXPANSION (340x420px Glassmorphic Card)
  // -------------------------------------------------------------
  return (
    <div
      onClick={handleCollapse}
      className="fixed inset-0 w-full h-full p-2 flex flex-col justify-start select-none font-sans antialiased text-slate-100"
    >
      {/* Crisp Glassmorphic Card */}
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full h-full bg-slate-950/95 backdrop-blur-2xl border border-white/10 shadow-[0_20px_50px_rgba(0,0,0,0.8)] rounded-2xl flex flex-col overflow-hidden animate-fadeIn"
      >
        {/* Header (Drag Region) */}
        <div
          data-tauri-drag-region
          className="flex items-center justify-between px-3 py-2.5 border-b border-white/10 cursor-move bg-slate-900/40"
        >
          {/* Left: App & Source */}
          <div data-tauri-drag-region className="flex items-center gap-2">
            <div className="flex items-center justify-center w-6 h-6 rounded-lg bg-gradient-to-tr from-indigo-500 to-purple-600 shadow-sm">
              <Sparkles className="w-3.5 h-3.5 text-white" />
            </div>
            <div data-tauri-drag-region className="flex flex-col">
              <span className="text-xs font-semibold text-white leading-tight">
                Nishikanto
              </span>
              <span className="text-[10px] text-slate-400 leading-none truncate max-w-[95px]">
                {lastAction || sourceApp}
              </span>
            </div>
          </div>

          {/* Center: Tone Pill */}
          <div className="px-2 py-0.5 rounded-full bg-indigo-500/15 border border-indigo-500/30 text-[10px] font-medium text-indigo-300 flex items-center gap-1 shadow-sm">
            <span>{overallTone}</span>
          </div>

          {/* Right: Actions */}
          <div className="flex items-center gap-1">
            <button
              onMouseDown={(e) => e.preventDefault()}
              onClick={openMainEditor}
              title="Open Nishikanto Full Editor"
              className="p-1 rounded-md text-slate-400 hover:text-white hover:bg-white/10 transition-colors cursor-pointer"
            >
              <Maximize2 className="w-3.5 h-3.5" />
            </button>
            <button
              onMouseDown={(e) => e.preventDefault()}
              onClick={handleCollapse}
              title="Collapse to pill (Esc)"
              className="p-1 rounded-md text-slate-400 hover:text-white hover:bg-white/10 transition-colors cursor-pointer"
            >
              <Minus className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Captured Text Snippet Header */}
        <div className="px-3 py-1.5 bg-white/[0.02] border-b border-white/5 flex items-center justify-between text-[10px] text-slate-400">
          <span className="flex items-center gap-1 text-slate-300 truncate max-w-[200px]">
            <ScanText className="w-3 h-3 text-indigo-400 shrink-0" />
            <span className="truncate italic">
              {capturedText ? `"${capturedText}"` : "No active text captured (Alt+G)"}
            </span>
          </span>
          <span
            className={`font-semibold shrink-0 ${
              capturedLints.length === 0 ? "text-emerald-400" : "text-amber-400"
            }`}
          >
            {capturedLints.length === 0
              ? "All Clear"
              : `${capturedLints.length} issue${capturedLints.length === 1 ? "" : "s"}`}
          </span>
        </div>

        {/* Body: Scrollable Harper Suggestion Cards */}
        <div className="flex-1 overflow-y-auto px-3 py-2 space-y-2">
          {capturedLints.length === 0 ? (
            <div className="h-full min-h-[140px] flex flex-col items-center justify-center text-center p-4">
              <div className="w-10 h-10 rounded-full bg-emerald-500/10 border border-emerald-500/30 flex items-center justify-center mb-2">
                <CheckCircle2 className="w-6 h-6 text-emerald-400" />
              </div>
              <h4 className="text-xs font-semibold text-slate-200">
                Writing is clean and polished!
              </h4>
              <p className="text-[11px] text-slate-400 mt-0.5 max-w-[220px]">
                No grammar, spelling, or clarity issues found in the active text.
              </p>
            </div>
          ) : (
            capturedLints.map((lint) => {
              const isCorrectness = lint.category === "Correctness";
              const isClarity = lint.category === "Clarity";

              const categoryColor = isCorrectness
                ? "text-red-300 border-red-500/30 bg-red-500/10"
                : isClarity
                ? "text-sky-300 border-sky-500/30 bg-sky-500/10"
                : "text-purple-300 border-purple-500/30 bg-purple-500/10";

              const cardBorder = isCorrectness
                ? "border-red-500/25 bg-red-500/[0.03]"
                : isClarity
                ? "border-sky-500/25 bg-sky-500/[0.03]"
                : "border-purple-500/25 bg-purple-500/[0.03]";

              const suggestion = lint.suggestions[0] || "fix";

              return (
                <div
                  key={lint.id}
                  className={`rounded-xl border p-2.5 flex flex-col gap-1.5 transition-all shadow-sm ${cardBorder}`}
                >
                  {/* Category Pill & Dismiss */}
                  <div className="flex items-center justify-between">
                    <span
                      className={`px-1.5 py-0.5 rounded text-[9px] font-bold uppercase tracking-wider border ${categoryColor}`}
                    >
                      {lint.category}
                    </span>
                    <button
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => dismissLint(lint.id)}
                      title="Ignore suggestion"
                      className="text-slate-500 hover:text-slate-300 p-0.5 rounded transition-colors cursor-pointer"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </div>

                  {/* Flagged Text -> Replacement */}
                  <div className="flex items-center gap-1.5 font-mono text-xs">
                    <span className="line-through text-red-300/80 bg-red-500/10 px-1 py-0.5 rounded">
                      {lint.flagged_text}
                    </span>
                    <ChevronRight className="w-3 h-3 text-slate-500" />
                    <span className="font-bold text-emerald-400 bg-emerald-500/10 px-1 py-0.5 rounded">
                      {suggestion}
                    </span>
                  </div>

                  {/* Explanation Message */}
                  <p className="text-[11px] text-slate-300 leading-snug">
                    {lint.message}
                  </p>

                  {/* Accept Button */}
                  <div className="flex justify-end pt-1">
                    <button
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => applySingleLintFix(lint, suggestion)}
                      className="px-2.5 py-1 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white text-[11px] font-semibold flex items-center gap-1 shadow-sm transition-all cursor-pointer"
                    >
                      <Check className="w-3 h-3" />
                      <span>Accept</span>
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>

        {/* Footer: AI Bar & Replace Button */}
        <div className="p-2.5 border-t border-white/10 bg-slate-900/60 flex flex-col gap-2">
          {/* AI Rewrite Chips */}
          <div className="grid grid-cols-4 gap-1">
            <button
              onMouseDown={(e) => e.preventDefault()}
              disabled={isAiProcessing || isPaused}
              onClick={() => handleAiRewrite("fix_all")}
              title="Fix all spelling and grammar"
              className="py-1 px-1.5 rounded-lg bg-indigo-600/20 hover:bg-indigo-600/30 border border-indigo-500/30 text-[10px] font-medium text-indigo-200 flex items-center justify-center gap-1 transition-all disabled:opacity-40 cursor-pointer"
            >
              {isAiProcessing ? (
                <Loader2 className="w-3 h-3 animate-spin" />
              ) : (
                <Wand2 className="w-3 h-3 text-indigo-400" />
              )}
              <span className="truncate">Fix All</span>
            </button>

            <button
              onMouseDown={(e) => e.preventDefault()}
              disabled={isAiProcessing || isPaused}
              onClick={() => handleAiRewrite("make_formal")}
              title="Rewrite in a formal tone"
              className="py-1 px-1.5 rounded-lg bg-white/5 hover:bg-white/10 border border-white/5 text-[10px] font-medium text-slate-200 text-center transition-all disabled:opacity-40 cursor-pointer truncate"
            >
              Formal
            </button>

            <button
              onMouseDown={(e) => e.preventDefault()}
              disabled={isAiProcessing || isPaused}
              onClick={() => handleAiRewrite("make_concise")}
              title="Shorten and make concise"
              className="py-1 px-1.5 rounded-lg bg-white/5 hover:bg-white/10 border border-white/5 text-[10px] font-medium text-slate-200 text-center transition-all disabled:opacity-40 cursor-pointer truncate"
            >
              Shorten
            </button>

            <button
              onMouseDown={(e) => e.preventDefault()}
              disabled={isAiProcessing || isPaused}
              onClick={() => handleAiRewrite("make_casual")}
              title="Rewrite in a casual tone"
              className="py-1 px-1.5 rounded-lg bg-white/5 hover:bg-white/10 border border-white/5 text-[10px] font-medium text-slate-200 text-center transition-all disabled:opacity-40 cursor-pointer truncate"
            >
              Casual
            </button>
          </div>

          {/* Prominent "Replace into Focused App" Button */}
          <button
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => handleApplyReplacement(capturedText)}
            disabled={!capturedText || appliedSuccess}
            className={`w-full py-1.5 px-3 rounded-xl text-xs font-semibold shadow-md transition-all flex items-center justify-center gap-1.5 cursor-pointer disabled:opacity-40 ${
              appliedSuccess
                ? "bg-emerald-600 text-white shadow-emerald-600/30"
                : "bg-gradient-to-r from-emerald-600 to-teal-600 hover:from-emerald-500 hover:to-teal-500 text-white shadow-emerald-600/20"
            }`}
          >
            {appliedSuccess ? (
              <Check className="w-3.5 h-3.5" />
            ) : (
              <CornerDownLeft className="w-3.5 h-3.5" />
            )}
            <span>
              {appliedSuccess
                ? "Replaced in App!"
                : `Replace into ${sourceApp}`}
            </span>
          </button>
        </div>
      </div>
    </div>
  );
}
