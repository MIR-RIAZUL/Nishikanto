import { useState, useEffect } from "react";
import {
  Sparkles,
  Pause,
  Play,
  Maximize2,
  ChevronDown,
  ChevronUp,
  Wand2,
  CheckCircle2,
  GripHorizontal,
  CornerDownLeft,
  ScanText,
  Loader2,
  Check,
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
  const { isPaused, togglePause, openMainEditor, setWidgetHeight, replaceActiveText, captureActiveText } =
    useTauriState();
  const [isExpanded, setIsExpanded] = useState(true);
  const [lastAction, setLastAction] = useState<string | null>(null);

  // OS capture state
  const [capturedText, setCapturedText] = useState<string>("");
  const [sourceApp, setSourceApp] = useState<string>("External Desktop App");
  const [capturedLints, setCapturedLints] = useState<LocalLintResult[]>([]);
  const [isAiProcessing, setIsAiProcessing] = useState<boolean>(false);
  const [appliedSuccess, setAppliedSuccess] = useState<boolean>(false);

  // Listen to OS hotkey capture events
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<OsCapturePayload>("os-text-captured", (event) => {
      const payload = event.payload;
      setCapturedText(payload.text);
      setSourceApp(payload.source_app || "Active App");
      setCapturedLints(payload.lints || []);
      setIsExpanded(true);
      setLastAction("Captured via Alt+G");
      setTimeout(() => setLastAction(null), 3000);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Dynamic height handling (collapsed: ~90px, expanded: ~390px - 440px)
  useEffect(() => {
    const targetHeight = isExpanded ? (capturedText ? 430 : 360) : 85;
    setWidgetHeight(targetHeight);
  }, [isExpanded, capturedText, setWidgetHeight]);

  const handleApplyReplacement = async (newText: string) => {
    setAppliedSuccess(true);
    await replaceActiveText(newText);
    setTimeout(() => {
      setAppliedSuccess(false);
      setCapturedText("");
      setCapturedLints([]);
    }, 1500);
  };

  const handleAiRewrite = async (mode: string) => {
    if (!capturedText.trim()) {
      // Trigger capture if no text is currently held
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
        setLastAction(`Rewritten: ${mode}`);
        setTimeout(() => setLastAction(null), 2500);
      }
    } catch (e) {
      console.warn("AI rewrite error:", e);
    } finally {
      setIsAiProcessing(false);
    }
  };

  const applySingleLintFix = (lint: LocalLintResult, replacement: string) => {
    const chars = Array.from(capturedText);
    const before = chars.slice(0, lint.start).join("");
    const after = chars.slice(lint.end).join("");
    const updated = before + replacement + after;

    setCapturedText(updated);
    setCapturedLints((prev) => prev.filter((l) => l.id !== lint.id));
  };

  return (
    <div className="w-full h-full p-2 flex flex-col justify-start select-none font-sans antialiased text-slate-100">
      {/* Floating Glassmorphic Overlay Card */}
      <div className="w-full bg-slate-950/90 backdrop-blur-2xl border border-white/10 shadow-[0_16px_40px_rgba(0,0,0,0.65)] rounded-2xl p-3 flex flex-col gap-2.5 transition-all duration-200">
        
        {/* Header / Drag Handle */}
        <div
          data-tauri-drag-region
          className="flex items-center justify-between cursor-move pb-1 border-b border-white/5"
        >
          <div data-tauri-drag-region className="flex items-center gap-2">
            <div className="relative flex items-center justify-center w-7 h-7 rounded-xl bg-gradient-to-tr from-indigo-500 to-purple-600 shadow-md shadow-indigo-500/20">
              <Sparkles className="w-3.5 h-3.5 text-white" />
              <span
                className={`absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full border border-slate-950 ${
                  isPaused ? "bg-amber-400" : "bg-emerald-400 animate-pulse"
                }`}
              />
            </div>
            <div data-tauri-drag-region className="flex flex-col">
              <span className="text-xs font-semibold tracking-wide text-white leading-none">
                Nishikanto
              </span>
              <span className="text-[10px] text-slate-400 leading-tight truncate max-w-[140px]">
                {sourceApp}
              </span>
            </div>
          </div>

          {/* Quick controls */}
          <div className="flex items-center gap-1">
            <button
              onClick={() => captureActiveText()}
              title="Capture from active application (Alt+G)"
              className="p-1.5 rounded-lg hover:bg-white/10 text-indigo-400 hover:text-indigo-300 transition-colors cursor-pointer"
            >
              <ScanText className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={togglePause}
              title={isPaused ? "Resume Nishikanto" : "Pause Nishikanto"}
              className="p-1.5 rounded-lg hover:bg-white/10 text-slate-300 hover:text-white transition-colors cursor-pointer"
            >
              {isPaused ? <Play className="w-3.5 h-3.5 text-amber-400" /> : <Pause className="w-3.5 h-3.5" />}
            </button>
            <button
              onClick={() => setIsExpanded(!isExpanded)}
              title={isExpanded ? "Collapse" : "Expand"}
              className="p-1.5 rounded-lg hover:bg-white/10 text-slate-300 hover:text-white transition-colors cursor-pointer"
            >
              {isExpanded ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
            </button>
            <button
              onClick={openMainEditor}
              title="Open Nishikanto Full Editor"
              className="p-1.5 rounded-lg hover:bg-white/10 text-slate-300 hover:text-indigo-400 transition-colors cursor-pointer"
            >
              <Maximize2 className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Expanded View */}
        {isExpanded && (
          <div className="flex flex-col gap-2 pt-1 animate-fadeIn">
            {/* Captured Text Preview Bar (if active) */}
            {capturedText ? (
              <div className="flex flex-col gap-1.5 bg-white/5 border border-white/5 rounded-xl p-2.5">
                <div className="flex items-center justify-between text-[10px] text-slate-400">
                  <span className="font-semibold text-slate-300 flex items-center gap-1">
                    <ScanText className="w-3 h-3 text-indigo-400" /> Captured Active Text:
                  </span>
                  <span className="font-mono text-emerald-400">
                    {capturedLints.length} issue{capturedLints.length === 1 ? "" : "s"}
                  </span>
                </div>

                <p className="text-[11px] font-mono text-slate-200 line-clamp-3 leading-relaxed break-words">
                  "{capturedText}"
                </p>

                {/* Detected issue quick-fix pills */}
                {capturedLints.length > 0 && (
                  <div className="flex flex-wrap gap-1 mt-1 max-h-20 overflow-y-auto">
                    {capturedLints.slice(0, 3).map((lint) => (
                      <button
                        key={lint.id}
                        onClick={() => applySingleLintFix(lint, lint.suggestions[0] || "")}
                        title={lint.message}
                        className="px-2 py-0.5 rounded-md bg-red-500/15 hover:bg-red-500/25 border border-red-500/30 text-[10px] text-red-200 font-medium transition-colors cursor-pointer flex items-center gap-1"
                      >
                        <span className="line-through opacity-75">{lint.flagged_text}</span>
                        <span>→</span>
                        <span className="font-bold text-emerald-400">{lint.suggestions[0] || "fix"}</span>
                      </button>
                    ))}
                  </div>
                )}

                {/* 1-Click Replace in App Action Button */}
                <button
                  onClick={() => handleApplyReplacement(capturedText)}
                  className={`mt-1 w-full py-1.5 px-3 rounded-lg text-xs font-semibold shadow-md transition-all flex items-center justify-center gap-1.5 cursor-pointer ${
                    appliedSuccess
                      ? "bg-emerald-600 text-white shadow-emerald-600/30"
                      : "bg-gradient-to-r from-emerald-600 to-teal-600 hover:from-emerald-500 hover:to-teal-500 text-white shadow-emerald-600/20"
                  }`}
                >
                  {appliedSuccess ? <Check className="w-3.5 h-3.5" /> : <CornerDownLeft className="w-3.5 h-3.5" />}
                  <span>{appliedSuccess ? "Replaced in App!" : "Replace into Focused App"}</span>
                </button>
              </div>
            ) : (
              /* Hotkey Prompt when no text captured */
              <div className="bg-white/5 border border-white/5 rounded-xl p-2.5 flex items-center justify-between text-[11px]">
                <div className="flex items-center gap-2 text-slate-300">
                  <kbd className="px-1.5 py-0.5 rounded bg-white/10 font-mono text-[10px] text-white border border-white/10">
                    Alt+G
                  </kbd>
                  <span className="text-[11px] leading-tight">Focus any app &amp; press hotkey</span>
                </div>
                <button
                  onClick={() => captureActiveText()}
                  className="px-2 py-1 rounded bg-indigo-600 hover:bg-indigo-500 text-white text-[10px] font-semibold transition-colors cursor-pointer"
                >
                  Capture
                </button>
              </div>
            )}

            {/* AI Rewrite Options */}
            <div className="grid grid-cols-2 gap-1.5 pt-0.5">
              <button
                disabled={isAiProcessing || isPaused}
                onClick={() => handleAiRewrite("fix_all")}
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl bg-indigo-600/20 hover:bg-indigo-600/30 border border-indigo-500/40 text-left text-xs font-medium text-indigo-200 transition-all disabled:opacity-40 cursor-pointer"
              >
                {isAiProcessing ? <Loader2 className="w-3 h-3 animate-spin" /> : <Wand2 className="w-3 h-3 text-indigo-400" />}
                <span className="truncate">Fix All</span>
              </button>

              <button
                disabled={isAiProcessing || isPaused}
                onClick={() => handleAiRewrite("make_formal")}
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl bg-white/5 hover:bg-white/10 border border-white/5 text-left text-xs font-medium text-slate-200 transition-all disabled:opacity-40 cursor-pointer"
              >
                <span>🏛️ Formal</span>
              </button>

              <button
                disabled={isAiProcessing || isPaused}
                onClick={() => handleAiRewrite("make_casual")}
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl bg-white/5 hover:bg-white/10 border border-white/5 text-left text-xs font-medium text-slate-200 transition-all disabled:opacity-40 cursor-pointer"
              >
                <span>☕ Casual</span>
              </button>

              <button
                disabled={isAiProcessing || isPaused}
                onClick={() => handleAiRewrite("make_concise")}
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl bg-white/5 hover:bg-white/10 border border-white/5 text-left text-xs font-medium text-slate-200 transition-all disabled:opacity-40 cursor-pointer"
              >
                <span>⚡ Concise</span>
              </button>
            </div>

            {/* Status Footer */}
            <div className="flex items-center justify-between text-[10px] text-slate-400 pt-0.5 px-1">
              <div className="flex items-center gap-1">
                <CheckCircle2 className="w-3 h-3 text-emerald-400" />
                <span>{lastAction || "Non-focusable overlay active"}</span>
              </div>
              <button
                onClick={openMainEditor}
                className="text-indigo-400 hover:text-indigo-300 font-medium cursor-pointer"
              >
                Full Editor →
              </button>
            </div>
          </div>
        )}

        {/* Drag handle subtle indicator */}
        <div data-tauri-drag-region className="w-full flex justify-center py-0.5 cursor-move opacity-30 hover:opacity-75">
          <GripHorizontal className="w-4 h-3 text-slate-400" />
        </div>
      </div>
    </div>
  );
}
