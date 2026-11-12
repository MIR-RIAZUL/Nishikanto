import { useState, useEffect, useCallback, useMemo, useRef } from "react";
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
  ChevronLeft,
  BookMarked,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { useTauriState } from "../hooks/useTauriState";
import { LocalLintResult, GeminiAnalysisResult } from "../hooks/useWritingEngine";
import { addWordToDictionary, addAppExclusion } from "../services/db";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

interface OsCapturePayload {
  text: string;
  source_app: string;
  process_name?: string;
  method: string;
  bounds: { x: number; y: number; width: number; height: number };
  lints: LocalLintResult[];
}

type WidgetMode = "pill" | "micro" | "full";

function getRuleDescription(lint: LocalLintResult): string {
  const msg = lint.message.toLowerCase();
  if (msg.includes("capitalize")) return "Capitalize the word";
  if (msg.includes("spelling") || msg.includes("did you mean")) return "Spelling mistake";
  if (msg.includes("passive")) return "Passive voice";
  if (msg.includes("redundant") || msg.includes("wordy")) return "Wordiness / Redundancy";
  if (lint.subcategory) {
    return lint.subcategory
      .replace(/_/g, " ")
      .replace(/\b\w/g, (c) => c.toUpperCase());
  }
  return lint.category === "Correctness" ? "Correctness error" : "Clarity suggestion";
}

export function WidgetWindow() {
  const {
    isPaused,
    openMainEditor,
    expandWidget,
    expandPillToMicro,
    collapseWidget,
    replaceActiveText,
    replaceSingleWord,
    showMicroPopover,
    captureActiveText,
  } = useTauriState();

  // Widget display modes: "pill" (sleek 32x32 circular badge), "micro" (Grammarly popover card), "full" (AI tone & rewrite panel)
  const [mode, setMode] = useState<WidgetMode>("pill");
  const [activeMicroLint, setActiveMicroLint] = useState<LocalLintResult | null>(null);
  const [lastAction, setLastAction] = useState<string | null>(null);

  // OS capture state
  const [capturedText, setCapturedText] = useState<string>("");
  const [sourceApp, setSourceApp] = useState<string>("Active App");
  const [processName, setProcessName] = useState<string>("");
  const [capturedLints, setCapturedLints] = useState<LocalLintResult[]>([]);
  const [isAiProcessing, setIsAiProcessing] = useState<boolean>(false);
  const [appliedSuccess, setAppliedSuccess] = useState<boolean>(false);
  const [textboxBounds, setTextboxBounds] = useState<{
    x: number;
    y: number;
    width: number;
    height: number;
  }>({ x: 0, y: 0, width: 0, height: 0 });

  // Floating pill mouse dragging state
  const dragRef = useRef<{
    startX: number;
    startY: number;
    moved: boolean;
  } | null>(null);

  // Hover 250ms delay state to avoid accidental dismissals
  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelHoverCollapse = useCallback(() => {
    if (hoverTimeoutRef.current) {
      clearTimeout(hoverTimeoutRef.current);
      hoverTimeoutRef.current = null;
    }
  }, []);

  const handleCollapseToPill = useCallback(async () => {
    cancelHoverCollapse();
    setMode("pill");
    setActiveMicroLint(null);
    await collapseWidget();
  }, [cancelHoverCollapse, collapseWidget]);

  const scheduleHoverCollapse = useCallback(() => {
    cancelHoverCollapse();
    hoverTimeoutRef.current = setTimeout(() => {
      handleCollapseToPill();
    }, 250);
  }, [cancelHoverCollapse, handleCollapseToPill]);

  useEffect(() => {
    return () => {
      cancelHoverCollapse();
    };
  }, [cancelHoverCollapse]);

  // Priority ranking: sentence rewrites first, then correctness, then other suggestions
  const getTopPriorityLint = useCallback((lints: LocalLintResult[]): LocalLintResult | null => {
    if (!lints || lints.length === 0) return null;
    const sorted = [...lints].sort((a, b) => {
      const aIsSent = Boolean(
        a.is_sentence_rewrite ||
        a.flagged_text.split(/\s+/).length > 2 ||
        (a.suggestions[0] && a.suggestions[0].split(/\s+/).length > 3)
      );
      const bIsSent = Boolean(
        b.is_sentence_rewrite ||
        b.flagged_text.split(/\s+/).length > 2 ||
        (b.suggestions[0] && b.suggestions[0].split(/\s+/).length > 3)
      );
      if (aIsSent && !bIsSent) return -1;
      if (!aIsSent && bIsSent) return 1;
      if (a.category === "Correctness" && b.category !== "Correctness") return -1;
      if (a.category !== "Correctness" && b.category === "Correctness") return 1;
      return 0;
    });
    return sorted[0];
  }, []);

  // Hovering over the floating pill immediately opens the top suggestion
  const handlePillMouseEnter = useCallback(async () => {
    cancelHoverCollapse();
    if (dragRef.current?.moved) return;
    if (capturedLints.length > 0) {
      const topLint = getTopPriorityLint(capturedLints);
      if (topLint) {
        setActiveMicroLint(topLint);
        setMode("micro");
        await expandPillToMicro();
      }
    }
  }, [cancelHoverCollapse, capturedLints, getTopPriorityLint, expandPillToMicro]);

  const handlePillPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return; // Only left mouse click
    dragRef.current = {
      startX: e.screenX,
      startY: e.screenY,
      moved: false,
    };
  };

  const handlePillPointerMove = async (e: React.PointerEvent) => {
    if (!dragRef.current) return;
    const dx = e.screenX - dragRef.current.startX;
    const dy = e.screenY - dragRef.current.startY;
    if (!dragRef.current.moved && Math.hypot(dx, dy) > 4) {
      dragRef.current.moved = true;
      try {
        await invoke("start_pill_drag");
        const currentWin = getCurrentWebviewWindow();
        await currentWin.startDragging();
        // Mouse released, window dragging finished:
        const pos = await currentWin.outerPosition();
        const boxRight = textboxBounds.x + textboxBounds.width;
        const boxBottom = textboxBounds.y + textboxBounds.height;
        await invoke("end_pill_drag", {
          screenX: pos.x,
          screenY: pos.y,
          boxRight,
          boxBottom,
        });
      } catch (err) {
        console.warn("Pill drag error:", err);
        await invoke("end_pill_drag_cancel").catch(() => {});
      }
    }
  };

  const handlePillPointerUp = () => {
    if (dragRef.current && !dragRef.current.moved) {
      // Single click under 4px movement: expand/collapse suggestion
      handlePillClick();
    }
    dragRef.current = null;
  };

  // Derive overall tone like Grammarly
  const overallTone = useMemo(() => {
    if (!capturedText) return "Confident 🎯";
    const lower = capturedText.toLowerCase();
    if (
      lower.includes("hey") ||
      lower.includes("thanks") ||
      lower.includes("cool") ||
      lower.includes("awesome") ||
      lower.includes("!")
    ) {
      return "Friendly ☕";
    }
    if (
      lower.includes("furthermore") ||
      lower.includes("therefore") ||
      lower.includes("in regards to") ||
      lower.includes("respectfully") ||
      lower.includes("sincerely")
    ) {
      return "Formal 🏛️";
    }
    const words = capturedText.trim().split(/\s+/).length;
    if (words < 12 && capturedLints.length === 0) {
      return "Direct ⚡";
    }
    return "Confident 🎯";
  }, [capturedText, capturedLints]);

  // Expansion handler
  const handleExpandToFull = useCallback(async () => {
    cancelHoverCollapse();
    setMode("full");
    await expandWidget();
  }, [cancelHoverCollapse, expandWidget]);

  // Handle pill click (trigger micro-popover on first flagged word or expand)
  const handlePillClick = useCallback(async () => {
    if (capturedLints.length > 0) {
      const target = capturedLints.find((l) => !!l.screen_bounds) || capturedLints[0];
      if (target && target.screen_bounds) {
        setActiveMicroLint(target);
        setMode("micro");
        await showMicroPopover(
          target.screen_bounds.x,
          target.screen_bounds.y + target.screen_bounds.height + 6.0,
          target
        );
        return;
      }
    }
    await handleExpandToFull();
  }, [capturedLints, showMicroPopover, handleExpandToFull]);

  // Listen to OS hotkey capture & hover events
  useEffect(() => {
    let unlistenCapture: (() => void) | undefined;
    let unlistenHover: (() => void) | undefined;
    let unlistenUnhover: (() => void) | undefined;
    let unlistenResolved: (() => void) | undefined;

    listen<OsCapturePayload>("os-text-captured", (event) => {
      const payload = event.payload;
      setCapturedText(payload.text);
      setSourceApp(payload.source_app || "Active App");
      setProcessName(payload.process_name || "");
      setCapturedLints(payload.lints || []);
      if (payload.bounds) {
        setTextboxBounds(payload.bounds);
      }
      setLastAction("Captured via Alt+G");
      setTimeout(() => setLastAction(null), 2500);
    }).then((fn) => {
      unlistenCapture = fn;
    });

    listen<LocalLintResult>("os-lint-hovered", (event) => {
      setActiveMicroLint(event.payload);
      setMode((current) => (current === "full" ? "full" : "micro"));
    }).then((fn) => {
      unlistenHover = fn;
    });

    listen("os-lint-unhovered", () => {
      setMode((current) => (current === "micro" ? "pill" : current));
    }).then((fn) => {
      unlistenUnhover = fn;
    });

    listen<string>("os-lint-resolved", (event) => {
      const resolvedId = event.payload;
      setCapturedLints((prev) => prev.filter((l) => l.id !== resolvedId));
      setActiveMicroLint((current) => (current?.id === resolvedId ? null : current));
    }).then((fn) => {
      unlistenResolved = fn;
    });

    return () => {
      if (unlistenCapture) unlistenCapture();
      if (unlistenHover) unlistenHover();
      if (unlistenUnhover) unlistenUnhover();
      if (unlistenResolved) unlistenResolved();
    };
  }, []);

  // Auto-collapse on Escape key
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (mode === "full" || mode === "micro") {
          handleCollapseToPill();
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [mode, handleCollapseToPill]);

  // Helper for category dot styling
  const getCategoryDotClass = (cat?: string) => {
    switch (cat) {
      case "Correctness":
        return "bg-red-500 shadow-red-500/50";
      case "Clarity":
        return "bg-blue-500 shadow-blue-500/50";
      case "Fluency":
        return "bg-indigo-500 shadow-indigo-500/50";
      default:
        return "bg-amber-500 shadow-amber-500/50";
    }
  };

  // Track active lint index for < 1 of 3 > pagination
  const currentLintIndex = useMemo(() => {
    if (!activeMicroLint) return 0;
    const idx = capturedLints.findIndex((l) => l.id === activeMicroLint.id);
    return idx >= 0 ? idx : 0;
  }, [activeMicroLint, capturedLints]);

  const handleStepLint = async (direction: "prev" | "next") => {
    if (capturedLints.length <= 1) return;
    const total = capturedLints.length;
    let nextIdx = 0;
    if (direction === "prev") {
      nextIdx = currentLintIndex > 0 ? currentLintIndex - 1 : total - 1;
    } else {
      nextIdx = currentLintIndex < total - 1 ? currentLintIndex + 1 : 0;
    }
    const targetLint = capturedLints[nextIdx];
    setActiveMicroLint(targetLint);
    if (targetLint.screen_bounds) {
      await showMicroPopover(
        targetLint.screen_bounds.x,
        targetLint.screen_bounds.y + targetLint.screen_bounds.height + 6.0,
        targetLint
      );
    }
  };

  // Accept single word or sentence suggestion from micro-popover (replaces that exact span in-place)
  const handleAcceptSingleWord = async (lint: LocalLintResult) => {
    const replacement = lint.suggestions[0] || "fix";
    const originalSpan = lint.flagged_text || "";

    const updatedFull = await replaceSingleWord(
      lint.start,
      lint.end,
      replacement,
      capturedText,
      lint.id,
      originalSpan
    );

    // Immediately remove that applied suggestion from the active list
    const remaining = capturedLints.filter((l) => l.id !== lint.id);
    setCapturedLints(remaining);
    if (updatedFull) {
      setCapturedText(updatedFull);
    }

    if (remaining.length > 0) {
      const nextLint = remaining[0];
      setActiveMicroLint(nextLint);
      if (nextLint.screen_bounds) {
        await showMicroPopover(
          nextLint.screen_bounds.x,
          nextLint.screen_bounds.y + nextLint.screen_bounds.height + 6.0,
          nextLint
        );
      }
    } else {
      setActiveMicroLint(null);
      setMode("pill");
      await collapseWidget();
    }
  };

  // Dismiss a lint from current session
  const handleDismissLint = async (lintId: string) => {
    const remaining = capturedLints.filter((l) => l.id !== lintId);
    setCapturedLints(remaining);
    if (activeMicroLint?.id === lintId) {
      if (remaining.length > 0) {
        const nextLint = remaining[0];
        setActiveMicroLint(nextLint);
        if (nextLint.screen_bounds) {
          await showMicroPopover(
            nextLint.screen_bounds.x,
            nextLint.screen_bounds.y + nextLint.screen_bounds.height + 6.0,
            nextLint
          );
        }
      } else {
        setActiveMicroLint(null);
        setMode("pill");
        await collapseWidget();
      }
    }
  };

  // Add word to personal dictionary
  const handleAddToDictionary = async (word: string, lintId: string) => {
    await addWordToDictionary(word);
    const remaining = capturedLints.filter((l) => l.id !== lintId);
    setCapturedLints(remaining);
    setLastAction(`Saved to Dict: ${word}`);
    setTimeout(() => setLastAction(null), 2000);
    if (activeMicroLint?.id === lintId) {
      if (remaining.length > 0) {
        const nextLint = remaining[0];
        setActiveMicroLint(nextLint);
        if (nextLint.screen_bounds) {
          await showMicroPopover(
            nextLint.screen_bounds.x,
            nextLint.screen_bounds.y + nextLint.screen_bounds.height + 6.0,
            nextLint
          );
        }
      } else {
        setActiveMicroLint(null);
        setMode("pill");
        await collapseWidget();
      }
    }
  };

  // Apply replacement for full panel
  const handleApplyReplacement = async (newText: string) => {
    setAppliedSuccess(true);
    await replaceActiveText(newText);
    setTimeout(() => {
      setAppliedSuccess(false);
      setCapturedText("");
      setCapturedLints([]);
      handleCollapseToPill();
    }, 1200);
  };

  const handleBlacklistApp = async () => {
    const target = (processName || sourceApp).trim();
    if (!target) return;
    await addAppExclusion(target, sourceApp || target);
    setLastAction(`Blacklisted: ${sourceApp}`);
    setTimeout(() => {
      handleCollapseToPill();
    }, 1000);
  };

  // AI Rewrite Modes
  const handleAiRewrite = async (modeName: string) => {
    if (!capturedText.trim()) {
      await captureActiveText();
      return;
    }
    setIsAiProcessing(true);
    try {
      const res = await invoke<GeminiAnalysisResult>("analyze_with_gemini", {
        text: capturedText,
        mode: modeName,
      });
      if (res.rewritten_text) {
        setCapturedText(res.rewritten_text);
        const newLints = await invoke<LocalLintResult[]>("check_text_local", {
          text: res.rewritten_text,
        });
        setCapturedLints(newLints);
        setLastAction(`AI: ${modeName}`);
        setTimeout(() => setLastAction(null), 2500);
      }
    } catch (e) {
      console.warn("AI rewrite error:", e);
    } finally {
      setIsAiProcessing(false);
    }
  };

  // -------------------------------------------------------------
  // 1. COMPACT FLOATING PILL STATE (32x32px circular Grammarly badge)
  // -------------------------------------------------------------
  if (mode === "pill") {
    const issueCount = capturedLints.length;
    const isClean = issueCount === 0;

    return (
      <div
        className="w-full h-full flex items-center justify-center relative bg-transparent p-0 m-0 select-none font-sans antialiased overflow-visible"
      >
        <button
          onPointerDown={handlePillPointerDown}
          onPointerMove={handlePillPointerMove}
          onPointerUp={handlePillPointerUp}
          onPointerCancel={() => {
            dragRef.current = null;
          }}
          onMouseEnter={handlePillMouseEnter}
          onContextMenu={(e) => {
            e.preventDefault();
            handleExpandToFull();
          }}
          title={
            isClean
              ? "Nishikanto: All clear! Drag to reposition, click to open assistant."
              : `Nishikanto: ${issueCount} suggestion${issueCount === 1 ? "" : "s"}. Hover to view top suggestion, drag to move.`
          }
          className={`w-8 h-8 rounded-full cursor-grab active:cursor-grabbing relative flex items-center justify-center transition-all duration-150 hover:scale-105 active:scale-95 ${
            isClean
              ? "bg-gradient-to-br from-emerald-500 to-teal-600 shadow-[inset_0_1px_1px_rgba(255,255,255,0.4),0_2px_8px_rgba(16,185,129,0.35)]"
              : "bg-gradient-to-br from-indigo-500 to-indigo-700 shadow-[inset_0_1px_1px_rgba(255,255,255,0.4),0_2px_8px_rgba(99,102,241,0.35)]"
          }`}
        >
          {isAiProcessing ? (
            <Loader2 className="w-4 h-4 text-white animate-spin" />
          ) : isClean ? (
            <Check className="w-4 h-4 text-white stroke-[2.75]" />
          ) : (
            <Sparkles className="w-3.5 h-3.5 text-white" />
          )}

          {/* Clean Red / Blue Pill Counter Badge */}
          {!isClean && (
            <span
              className={`absolute -top-1 -right-1 min-w-[16px] h-4 px-1 rounded-full text-[9px] font-extrabold flex items-center justify-center text-white border border-white/40 shadow-sm leading-none ${
                capturedLints.some((l) => l.category === "Correctness")
                  ? "bg-red-500"
                  : "bg-blue-500"
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
  // 2. GRAMMARLY-STYLE MICRO-POPOVER CARD (290x185px)
  // -------------------------------------------------------------
  if (mode === "micro" && activeMicroLint) {
    const primarySuggestion = activeMicroLint.suggestions[0] || "Fix";
    const reasonHeader = activeMicroLint.reason || getRuleDescription(activeMicroLint);
    const isSentence = Boolean(
      activeMicroLint.is_sentence_rewrite ||
      activeMicroLint.flagged_text.split(/\s+/).length > 2 ||
      primarySuggestion.split(/\s+/).length > 3
    );

    return (
      <div
        onMouseEnter={cancelHoverCollapse}
        onMouseLeave={scheduleHoverCollapse}
        className="fixed inset-0 w-full h-full p-1.5 flex items-center justify-center select-none font-sans antialiased text-slate-900"
      >
        <div className="bg-white text-slate-900 rounded-xl shadow-2xl border border-slate-200/90 w-full h-full p-2.5 flex flex-col justify-between animate-fadeIn">
          {/* Header: Category dot + reason + < 1 of 3 > navigation */}
          <div className="flex items-center justify-between border-b border-slate-100 pb-1.5">
            <div className="flex items-center gap-1.5 min-w-0 pr-1">
              <span className={`w-2 h-2 rounded-full shrink-0 ${getCategoryDotClass(activeMicroLint.category)}`} />
              <span className="text-xs font-semibold text-slate-800 truncate" title={reasonHeader}>
                {reasonHeader}
              </span>
            </div>

            {/* Pagination < 1 of 3 > & Close */}
            <div className="flex items-center gap-1 text-[11px] font-medium text-slate-500 shrink-0">
              <button
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => handleStepLint("prev")}
                disabled={capturedLints.length <= 1}
                title="Previous suggestion"
                className="p-0.5 rounded hover:bg-slate-100 disabled:opacity-30 disabled:hover:bg-transparent cursor-pointer transition-colors"
              >
                <ChevronLeft className="w-3.5 h-3.5" />
              </button>
              <span className="tabular-nums font-mono text-[10px]">
                {currentLintIndex + 1}/{Math.max(capturedLints.length, 1)}
              </span>
              <button
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => handleStepLint("next")}
                disabled={capturedLints.length <= 1}
                title="Next suggestion"
                className="p-0.5 rounded hover:bg-slate-100 disabled:opacity-30 disabled:hover:bg-transparent cursor-pointer transition-colors"
              >
                <ChevronRight className="w-3.5 h-3.5" />
              </button>
              <button
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => handleDismissLint(activeMicroLint.id)}
                title="Dismiss"
                className="text-slate-400 hover:text-slate-600 p-0.5 ml-1 rounded hover:bg-slate-100 transition-colors cursor-pointer"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>

          {/* Suggestion Body */}
          <div className="flex-1 flex flex-col justify-center py-1">
            {!isSentence ? (
              /* Single-word error: crossed-out word + large primary blue replacement button */
              <div className="flex flex-col gap-1.5 py-0.5">
                <div className="flex items-center gap-1.5 text-xs text-slate-500">
                  <span
                    className="text-xs font-mono text-slate-600 bg-red-50 px-2 py-0.5 rounded border border-red-200 line-through decoration-red-500 decoration-2 shrink-0 max-w-[150px] truncate"
                    title={activeMicroLint.flagged_text}
                  >
                    {activeMicroLint.flagged_text}
                  </span>
                  <ChevronRight className="w-3.5 h-3.5 text-slate-400 shrink-0" />
                </div>
                <button
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => handleAcceptSingleWord(activeMicroLint)}
                  title={`Click to apply: "${primarySuggestion}"`}
                  className="w-full text-left font-semibold text-blue-600 bg-blue-50/70 hover:bg-blue-100 px-3 py-2 rounded-lg transition cursor-pointer flex items-center justify-between group shadow-xs border border-blue-200/60"
                >
                  <span className="truncate text-xs font-bold text-blue-700">
                    {primarySuggestion}
                  </span>
                  <CornerDownLeft className="w-3.5 h-3.5 text-blue-500 shrink-0 group-hover:translate-x-0.5 transition-transform" />
                </button>
              </div>
            ) : (
              /* Sentence-level issues: diff styling + clickable bold blue full-sentence replacement box */
              <div className="flex flex-col gap-1 max-h-[86px] overflow-y-auto pr-0.5">
                <div
                  className="text-[11px] leading-snug bg-red-50/70 text-slate-600 border border-red-100 rounded px-2 py-1 line-through decoration-red-400 decoration-1 truncate"
                  title={activeMicroLint.flagged_text}
                >
                  {activeMicroLint.flagged_text}
                </div>
                <button
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => handleAcceptSingleWord(activeMicroLint)}
                  title="Click to replace sentence in-place"
                  className="w-full text-left font-semibold text-blue-600 bg-blue-50/70 hover:bg-blue-100 px-3 py-2 rounded-lg transition cursor-pointer flex items-center justify-between group shadow-xs border border-blue-200/60"
                >
                  <div className="flex-1 min-w-0 pr-2">
                    <div className="flex items-center justify-between text-[9px] text-blue-500 font-bold uppercase tracking-wider mb-0.5">
                      <span>Rewrite in-place</span>
                    </div>
                    <div className="text-blue-900 font-semibold text-xs leading-snug line-clamp-2">
                      {primarySuggestion}
                    </div>
                  </div>
                  <CornerDownLeft className="w-3.5 h-3.5 text-blue-500 shrink-0 group-hover:translate-x-0.5 transition-transform" />
                </button>
              </div>
            )}
          </div>

          {/* Footer Actions */}
          <div className="flex items-center justify-between pt-1 border-t border-slate-100 text-xs">
            <div className="flex items-center gap-2">
              <button
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => handleDismissLint(activeMicroLint.id)}
                className="flex items-center gap-1 text-[11px] text-slate-400 hover:text-slate-700 cursor-pointer transition-colors"
                title="Dismiss this suggestion"
              >
                <Trash2 className="w-3 h-3" />
                <span>Dismiss</span>
              </button>
              {activeMicroLint.category === "Correctness" && !isSentence && (
                <button
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => handleAddToDictionary(activeMicroLint.flagged_text, activeMicroLint.id)}
                  title="Add to personal dictionary"
                  className="text-[11px] text-slate-400 hover:text-emerald-600 flex items-center gap-0.5 cursor-pointer transition-colors"
                >
                  <BookMarked className="w-3 h-3" />
                  <span>+ Dict</span>
                </button>
              )}
            </div>

            <button
              onMouseDown={(e) => e.preventDefault()}
              onClick={handleExpandToFull}
              className="flex items-center gap-1 text-[11px] font-medium text-slate-500 hover:text-indigo-600 transition-colors cursor-pointer"
            >
              <Sparkles className="w-3 h-3 text-indigo-500" />
              <span>Open Assistant</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  // -------------------------------------------------------------
  // 3. FULL AI TONE & REWRITE PANEL (340x420px Glassmorphic Card)
  // -------------------------------------------------------------
  return (
    <div
      onClick={handleCollapseToPill}
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
              onClick={handleBlacklistApp}
              title={`Disable Nishikanto for ${sourceApp} (${processName || "active app"})`}
              className="p-1 rounded-md text-slate-400 hover:text-rose-400 hover:bg-rose-500/10 transition-colors cursor-pointer"
            >
              <ShieldAlert className="w-3.5 h-3.5" />
            </button>
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
              onClick={handleCollapseToPill}
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
                      onClick={() => handleDismissLint(lint.id)}
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

                  {/* Actions: Add to Dict & Accept */}
                  <div className="flex items-center justify-end gap-1.5 pt-1">
                    <button
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => handleAddToDictionary(lint.flagged_text, lint.id)}
                      title="Add to personal dictionary so it is never flagged again"
                      className="px-2 py-0.5 rounded-lg bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 text-[10px] font-medium flex items-center gap-1 transition-all cursor-pointer"
                    >
                      <BookMarked className="w-3 h-3" />
                      <span>+ Dict</span>
                    </button>
                    <button
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => handleAcceptSingleWord(lint)}
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
          {/* AI Rewrite Chips powered by gemini-3.5-flash-lite */}
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
