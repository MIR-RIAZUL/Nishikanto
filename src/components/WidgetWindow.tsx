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
  FileEdit,
  SlidersHorizontal,
} from "lucide-react";
import { useTauriState } from "../hooks/useTauriState";

export function WidgetWindow() {
  const { isPaused, togglePause, openMainEditor, setWidgetHeight } = useTauriState();
  const [isExpanded, setIsExpanded] = useState(true);
  const [activeTone, setActiveTone] = useState("Professional");
  const [lastAction, setLastAction] = useState<string | null>(null);

  // Dynamic height handling (collapsed: ~90px, expanded: ~390px)
  useEffect(() => {
    const targetHeight = isExpanded ? 390 : 90;
    setWidgetHeight(targetHeight);
  }, [isExpanded, setWidgetHeight]);

  const handleActionClick = (actionName: string) => {
    if (isPaused) return;
    setLastAction(actionName);
    setTimeout(() => setLastAction(null), 2500);
  };

  return (
    <div className="w-full h-full p-2 flex flex-col justify-start select-none font-sans antialiased text-slate-100">
      {/* Floating Card Container */}
      <div className="w-full bg-slate-950/85 backdrop-blur-2xl border border-white/10 shadow-[0_12px_40px_rgba(0,0,0,0.6)] rounded-2xl p-3 flex flex-col gap-2.5 transition-all duration-200">
        
        {/* Header / Drag Handle */}
        <div
          data-tauri-drag-region
          className="flex items-center justify-between cursor-move pb-1 border-b border-white/5"
        >
          <div data-tauri-drag-region className="flex items-center gap-2">
            <div className="relative flex items-center justify-center w-7 h-7 rounded-xl bg-gradient-to-tr from-indigo-500 to-purple-600 shadow-md shadow-indigo-500/20">
              <Sparkles className="w-3.5 h-3.5 text-white" />
              {/* Status pulse dot */}
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
              <span className="text-[10px] text-slate-400 leading-tight">
                {isPaused ? "Paused" : "AI Assistant Active"}
              </span>
            </div>
          </div>

          {/* Quick controls */}
          <div className="flex items-center gap-1">
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

        {/* Expanded Content View */}
        {isExpanded && (
          <div className="flex flex-col gap-2.5 pt-1 animate-fadeIn">
            {/* Tone selector pill row */}
            <div className="flex items-center justify-between text-[11px] px-1 text-slate-400">
              <span className="flex items-center gap-1 font-medium">
                <SlidersHorizontal className="w-3 h-3 text-indigo-400" /> Mode:
              </span>
              <div className="flex gap-1">
                {["Professional", "Casual", "Concise"].map((tone) => (
                  <button
                    key={tone}
                    onClick={() => setActiveTone(tone)}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all cursor-pointer ${
                      activeTone === tone
                        ? "bg-indigo-600/80 text-white shadow-sm"
                        : "bg-white/5 hover:bg-white/10 text-slate-400"
                    }`}
                  >
                    {tone}
                  </button>
                ))}
              </div>
            </div>

            {/* Quick Action Buttons (Non-stealing focus) */}
            <div className="grid grid-cols-2 gap-1.5">
              <button
                disabled={isPaused}
                onClick={() => handleActionClick("Grammar & Polish")}
                className="flex items-center gap-1.5 px-2.5 py-2 rounded-xl bg-white/5 hover:bg-indigo-600/20 hover:border-indigo-500/40 border border-white/5 text-left text-xs font-medium text-slate-200 transition-all disabled:opacity-40 disabled:pointer-events-none cursor-pointer group"
              >
                <Wand2 className="w-3.5 h-3.5 text-indigo-400 group-hover:scale-110 transition-transform" />
                <span className="truncate">Fix Grammar</span>
              </button>

              <button
                disabled={isPaused}
                onClick={() => handleActionClick("Rewrite Flow")}
                className="flex items-center gap-1.5 px-2.5 py-2 rounded-xl bg-white/5 hover:bg-purple-600/20 hover:border-purple-500/40 border border-white/5 text-left text-xs font-medium text-slate-200 transition-all disabled:opacity-40 disabled:pointer-events-none cursor-pointer group"
              >
                <Sparkles className="w-3.5 h-3.5 text-purple-400 group-hover:scale-110 transition-transform" />
                <span className="truncate">Improve Flow</span>
              </button>
            </div>

            {/* Feedback / Active State Badge */}
            <div className="bg-white/5 border border-white/5 rounded-xl p-2 flex items-center justify-between text-[11px]">
              <div className="flex items-center gap-1.5 text-slate-300">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
                <span>{lastAction ? `Applied: ${lastAction}` : "Focus in any app & press AI hotkey"}</span>
              </div>
              <span className="text-[10px] font-mono text-slate-500">60 FPS</span>
            </div>

            {/* Footer action to launch main editor */}
            <button
              onClick={openMainEditor}
              className="w-full flex items-center justify-center gap-1.5 py-1.5 px-3 rounded-xl bg-gradient-to-r from-indigo-600 to-violet-600 hover:from-indigo-500 hover:to-violet-500 text-white text-xs font-medium shadow-md shadow-indigo-600/20 transition-all cursor-pointer"
            >
              <FileEdit className="w-3.5 h-3.5" />
              <span>Open Document Editor</span>
            </button>
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
