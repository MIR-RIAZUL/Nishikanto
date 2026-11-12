import { useState, useRef, useMemo } from "react";
import {
  Sparkles,
  Settings,
  Minimize2,
  Pause,
  Play,
  Wand2,
  Check,
  Copy,
  Trash2,
  RotateCcw,
  CheckCircle2,
  AlertCircle,
  HelpCircle,
  ShieldCheck,
  Loader2,
  BookOpen,
} from "lucide-react";
import { useTauriState } from "../hooks/useTauriState";
import { useWritingEngine } from "../hooks/useWritingEngine";

export function MainWindow() {
  const { isPaused, togglePause, hideMainEditor } = useTauriState();
  const [activeTab, setActiveTab] = useState<"editor" | "settings">("editor");
  const [docTitle, setDocTitle] = useState("Q4 Strategy Memo.md");
  const [selectedCategory, setSelectedCategory] = useState<string>("All");
  const [copied, setCopied] = useState(false);

  const {
    text,
    setText,
    lints,
    writingScore,
    toneLabel,
    isAiLoading,
    aiMessage,
    aiStatus,
    hoveredLintId,
    setHoveredLintId,
    acceptSuggestion,
    ignoreSuggestion,
    runGeminiAnalysis,
    loadSampleText,
  } = useWritingEngine();

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);

  // Sync scroll between textarea and underline backdrop
  const handleScroll = () => {
    if (textareaRef.current && backdropRef.current) {
      backdropRef.current.scrollTop = textareaRef.current.scrollTop;
      backdropRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  };

  // Group lints count by category
  const counts = useMemo(() => {
    let correctness = 0;
    let clarity = 0;
    let style = 0;

    for (const l of lints) {
      if (l.category === "Correctness") correctness++;
      else if (l.category === "Clarity") clarity++;
      else if (l.category === "Style") style++;
    }

    return { total: lints.length, correctness, clarity, style };
  }, [lints]);

  // Filtered lints based on tab filter
  const filteredLints = useMemo(() => {
    if (selectedCategory === "All") return lints;
    return lints.filter((l) => l.category === selectedCategory);
  }, [lints, selectedCategory]);

  const stats = useMemo(() => {
    const trimmed = text.trim();
    const words = trimmed ? trimmed.split(/\s+/).length : 0;
    const chars = trimmed.length;
    const readingTime = Math.ceil(words / 200);
    return { words, chars, readingTime };
  }, [text]);

  const handleCopy = () => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  // Segment text for color-coded underlines in the backdrop
  const highlightedSegments = useMemo(() => {
    if (!text) return null;

    const chars = Array.from(text);
    const segments: React.ReactNode[] = [];
    let currentIdx = 0;

    // Filter lints to non-overlapping and sorted by start
    const sorted = [...lints].sort((a, b) => a.start - b.start);

    sorted.forEach((lint) => {
      if (lint.start > currentIdx) {
        // Plain text segment before the lint
        const slice = chars.slice(currentIdx, lint.start).join("");
        segments.push(
          <span key={`text-${currentIdx}`} className="text-transparent">
            {slice}
          </span>
        );
      }

      const lintSlice = chars.slice(lint.start, lint.end).join("");

      // Underline color mapping based on lint category:
      // Red: Correctness, Blue: Clarity, Purple: Style
      let underlineClass = "";
      if (lint.category === "Correctness") {
        underlineClass =
          "border-b-2 border-red-500 underline decoration-red-500 decoration-wavy decoration-2";
      } else if (lint.category === "Clarity") {
        underlineClass =
          "border-b-2 border-sky-400 underline decoration-sky-400 decoration-wavy decoration-2";
      } else {
        underlineClass =
          "border-b-2 border-purple-400 underline decoration-purple-400 decoration-wavy decoration-2";
      }

      const isHovered = hoveredLintId === lint.id;

      segments.push(
        <mark
          key={lint.id}
          className={`bg-transparent text-transparent ${underlineClass} ${
            isHovered ? "bg-white/10 rounded-sm" : ""
          } transition-colors`}
        >
          {lintSlice}
        </mark>
      );

      currentIdx = Math.max(currentIdx, lint.end);
    });

    if (currentIdx < chars.length) {
      const remaining = chars.slice(currentIdx).join("");
      segments.push(
        <span key={`text-${currentIdx}`} className="text-transparent">
          {remaining}
        </span>
      );
    }

    return segments;
  }, [text, lints, hoveredLintId]);

  return (
    <div className="w-screen h-screen flex flex-col bg-slate-950 text-slate-100 font-sans antialiased overflow-hidden select-none">
      {/* Top Application Header */}
      <header
        data-tauri-drag-region
        className="h-12 border-b border-white/10 bg-slate-900/60 backdrop-blur-xl flex items-center justify-between px-4 select-none shrink-0 z-30"
      >
        {/* Brand & Document Name */}
        <div data-tauri-drag-region className="flex items-center gap-3">
          <div className="flex items-center justify-center w-7 h-7 rounded-lg bg-gradient-to-tr from-indigo-500 to-violet-600 shadow-md shadow-indigo-500/20">
            <Sparkles className="w-4 h-4 text-white" />
          </div>
          <span className="font-semibold text-sm tracking-tight text-white">Nishikanto</span>
          <span className="text-slate-600">/</span>
          <input
            type="text"
            value={docTitle}
            onChange={(e) => setDocTitle(e.target.value)}
            className="text-xs text-slate-300 hover:text-white bg-transparent border-none outline-none focus:ring-1 focus:ring-indigo-500/50 rounded px-1.5 py-0.5 font-medium transition-colors"
          />
        </div>

        {/* Header Right Controls */}
        <div className="flex items-center gap-2.5">
          {/* Load Sample Test Paragraph */}
          <button
            onClick={loadSampleText}
            title="Load Sample with Intentional Spelling, Passive Voice & Tone Issues"
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium bg-indigo-500/10 hover:bg-indigo-500/20 text-indigo-300 border border-indigo-500/30 transition-colors cursor-pointer"
          >
            <RotateCcw className="w-3.5 h-3.5" />
            <span>Load Test Sample</span>
          </button>

          {/* Active / Paused Toggle */}
          <button
            onClick={togglePause}
            className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium border transition-all cursor-pointer ${
              isPaused
                ? "bg-amber-500/10 border-amber-500/30 text-amber-300 hover:bg-amber-500/20"
                : "bg-emerald-500/10 border-emerald-500/30 text-emerald-300 hover:bg-emerald-500/20"
            }`}
          >
            {isPaused ? <Play className="w-3 h-3 text-amber-400" /> : <Pause className="w-3 h-3 text-emerald-400" />}
            <span>{isPaused ? "Paused" : "Engine Active"}</span>
          </button>

          {/* Minimize to Tray */}
          <button
            onClick={hideMainEditor}
            title="Minimize to System Tray"
            className="flex items-center gap-1.5 px-2 py-1 rounded-lg text-xs font-medium text-slate-400 hover:text-white hover:bg-white/10 transition-colors cursor-pointer"
          >
            <Minimize2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </header>

      {/* Main Two-Column Workspace */}
      <div className="flex-1 flex overflow-hidden">
        {/* Leftmost Icon Rail */}
        <aside className="w-14 border-r border-white/10 bg-slate-900/40 flex flex-col items-center py-3 gap-3 shrink-0">
          <button
            onClick={() => setActiveTab("editor")}
            title="Writing Editor"
            className={`p-2 rounded-xl transition-all cursor-pointer ${
              activeTab === "editor"
                ? "bg-indigo-600 text-white shadow-md shadow-indigo-600/30"
                : "text-slate-400 hover:text-white hover:bg-white/5"
            }`}
          >
            <BookOpen className="w-4 h-4" />
          </button>

          <button
            onClick={() => setActiveTab("settings")}
            title="Engine Settings & Architecture"
            className={`p-2 rounded-xl transition-all cursor-pointer ${
              activeTab === "settings"
                ? "bg-indigo-600 text-white shadow-md shadow-indigo-600/30"
                : "text-slate-400 hover:text-white hover:bg-white/5"
            }`}
          >
            <Settings className="w-4 h-4" />
          </button>
        </aside>

        {activeTab === "editor" ? (
          <div className="flex-1 flex overflow-hidden">
            {/* COLUMN 1: Writing Canvas with Underline Highlights */}
            <main className="flex-1 flex flex-col bg-slate-950 overflow-hidden relative">
              {/* Category Filter Toolbar */}
              <div className="h-10 border-b border-white/5 bg-slate-900/20 px-6 flex items-center justify-between text-xs text-slate-400 shrink-0">
                <div className="flex items-center gap-2">
                  <span className="text-[11px] font-medium text-slate-500 uppercase tracking-wider mr-1">
                    Issues:
                  </span>
                  <button
                    onClick={() => setSelectedCategory("All")}
                    className={`px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors cursor-pointer ${
                      selectedCategory === "All"
                        ? "bg-slate-800 text-white border border-white/20"
                        : "text-slate-400 hover:text-white"
                    }`}
                  >
                    All ({counts.total})
                  </button>
                  <button
                    onClick={() => setSelectedCategory("Correctness")}
                    className={`flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors cursor-pointer ${
                      selectedCategory === "Correctness"
                        ? "bg-red-500/20 text-red-300 border border-red-500/40"
                        : "text-slate-400 hover:text-red-300"
                    }`}
                  >
                    <span className="w-2 h-2 rounded-full bg-red-500 inline-block" />
                    Correctness ({counts.correctness})
                  </button>
                  <button
                    onClick={() => setSelectedCategory("Clarity")}
                    className={`flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors cursor-pointer ${
                      selectedCategory === "Clarity"
                        ? "bg-sky-500/20 text-sky-300 border border-sky-500/40"
                        : "text-slate-400 hover:text-sky-300"
                    }`}
                  >
                    <span className="w-2 h-2 rounded-full bg-sky-400 inline-block" />
                    Clarity ({counts.clarity})
                  </button>
                  <button
                    onClick={() => setSelectedCategory("Style")}
                    className={`flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors cursor-pointer ${
                      selectedCategory === "Style"
                        ? "bg-purple-500/20 text-purple-300 border border-purple-500/40"
                        : "text-slate-400 hover:text-purple-300"
                    }`}
                  >
                    <span className="w-2 h-2 rounded-full bg-purple-400 inline-block" />
                    Style ({counts.style})
                  </button>
                </div>

                <div className="flex items-center gap-2">
                  <button
                    onClick={handleCopy}
                    className="p-1 rounded hover:bg-white/10 text-slate-400 hover:text-white transition-colors cursor-pointer"
                    title="Copy Text"
                  >
                    {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
                  </button>
                  <button
                    onClick={() => setText("")}
                    className="p-1 rounded hover:bg-white/10 text-slate-400 hover:text-rose-400 transition-colors cursor-pointer"
                    title="Clear Text"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>

              {/* Editor Workspace Container */}
              <div className="flex-1 relative overflow-hidden">
                {/* Backdrop with Color-Coded Underlines */}
                <div
                  ref={backdropRef}
                  className="absolute inset-0 pointer-events-none p-8 whitespace-pre-wrap break-words overflow-y-auto font-mono text-base leading-relaxed select-none"
                  aria-hidden="true"
                >
                  {highlightedSegments}
                </div>

                {/* Transparent Interactive Textarea */}
                <textarea
                  ref={textareaRef}
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                  onScroll={handleScroll}
                  placeholder="Type or paste your text here..."
                  className="relative w-full h-full bg-transparent text-slate-100 p-8 whitespace-pre-wrap break-words outline-none resize-none font-mono text-base leading-relaxed caret-indigo-400 selection:bg-indigo-500/30 overflow-y-auto"
                  spellCheck={false}
                />
              </div>

              {/* Bottom Canvas Status Bar */}
              <footer className="h-8 border-t border-white/5 bg-slate-900/40 px-6 flex items-center justify-between text-[11px] text-slate-400 shrink-0">
                <div className="flex items-center gap-4">
                  <span>{stats.words} words</span>
                  <span className="text-slate-700">•</span>
                  <span>{stats.chars} characters</span>
                  <span className="text-slate-700">•</span>
                  <span>~{stats.readingTime} min read</span>
                </div>

                <div className="flex items-center gap-3 font-mono text-[10px] text-slate-500">
                  <span className="text-emerald-400">⚡ Harper Offline (&lt;5ms)</span>
                  <span className="text-slate-700">|</span>
                  <span>Free-Tier Gemini AI Layer</span>
                </div>
              </footer>
            </main>

            {/* COLUMN 2: Interactive Assistant Sidebar */}
            <aside className="w-96 border-l border-white/10 bg-slate-900/60 backdrop-blur-xl flex flex-col overflow-hidden shrink-0">
              {/* Sidebar Header: Writing Score & Tone */}
              <div className="p-4 border-b border-white/10 flex flex-col gap-3.5 bg-slate-900/30">
                <div className="flex items-center justify-between">
                  <div>
                    <span className="text-[10px] font-semibold tracking-wider text-slate-400 uppercase">
                      Writing Score
                    </span>
                    <div className="flex items-baseline gap-2 mt-0.5">
                      <span
                        className={`text-3xl font-black tracking-tight ${
                          writingScore >= 85
                            ? "text-emerald-400"
                            : writingScore >= 70
                            ? "text-sky-400"
                            : "text-amber-400"
                        }`}
                      >
                        {writingScore}
                      </span>
                      <span className="text-xs text-slate-500 font-medium">/ 100</span>
                    </div>
                  </div>

                  {/* Circular Score Visual Indicator */}
                  <div className="relative w-11 h-11 flex items-center justify-center">
                    <svg className="w-11 h-11 transform -rotate-90">
                      <circle
                        cx="22"
                        cy="22"
                        r="18"
                        stroke="currentColor"
                        strokeWidth="3.5"
                        className="text-white/10"
                        fill="transparent"
                      />
                      <circle
                        cx="22"
                        cy="22"
                        r="18"
                        stroke="currentColor"
                        strokeWidth="3.5"
                        strokeDasharray={113}
                        strokeDashoffset={113 - (113 * writingScore) / 100}
                        strokeLinecap="round"
                        className={`${
                          writingScore >= 85
                            ? "text-emerald-400"
                            : writingScore >= 70
                            ? "text-sky-400"
                            : "text-amber-400"
                        } transition-all duration-500`}
                        fill="transparent"
                      />
                    </svg>
                    <span className="absolute text-[10px] font-bold text-slate-300">
                      {counts.total === 0 ? "100%" : `${writingScore}`}
                    </span>
                  </div>
                </div>

                {/* Tone Badge */}
                <div className="flex items-center justify-between bg-white/5 border border-white/5 rounded-xl px-3 py-2">
                  <div className="flex flex-col">
                    <span className="text-[10px] text-slate-400 font-medium">Detected Tone</span>
                    <span className="text-xs font-semibold text-white tracking-wide mt-0.5">
                      {toneLabel}
                    </span>
                  </div>
                  {isAiLoading && (
                    <div className="flex items-center gap-1.5 text-[11px] text-indigo-400">
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                      <span>Analyzing...</span>
                    </div>
                  )}
                </div>

                {/* One-Click AI Rewrite Buttons */}
                <div className="flex flex-col gap-1.5 pt-1">
                  <span className="text-[10px] font-semibold text-slate-400 uppercase tracking-wider">
                    One-Click AI Rewrite:
                  </span>
                  <div className="grid grid-cols-2 gap-1.5">
                    <button
                      disabled={isAiLoading || isPaused}
                      onClick={() => runGeminiAnalysis(text, "fix_all")}
                      className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-indigo-600/20 hover:bg-indigo-600/30 border border-indigo-500/40 text-xs font-medium text-indigo-200 transition-all disabled:opacity-40 cursor-pointer"
                    >
                      <Wand2 className="w-3.5 h-3.5 text-indigo-400" />
                      <span>✨ Fix All</span>
                    </button>

                    <button
                      disabled={isAiLoading || isPaused}
                      onClick={() => runGeminiAnalysis(text, "make_formal")}
                      className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-white/5 hover:bg-white/10 border border-white/10 text-xs font-medium text-slate-200 transition-all disabled:opacity-40 cursor-pointer"
                    >
                      <span>🏛️ Make Formal</span>
                    </button>

                    <button
                      disabled={isAiLoading || isPaused}
                      onClick={() => runGeminiAnalysis(text, "make_casual")}
                      className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-white/5 hover:bg-white/10 border border-white/10 text-xs font-medium text-slate-200 transition-all disabled:opacity-40 cursor-pointer"
                    >
                      <span>☕ Make Casual</span>
                    </button>

                    <button
                      disabled={isAiLoading || isPaused}
                      onClick={() => runGeminiAnalysis(text, "make_concise")}
                      className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-white/5 hover:bg-white/10 border border-white/10 text-xs font-medium text-slate-200 transition-all disabled:opacity-40 cursor-pointer"
                    >
                      <span>⚡ Make Concise</span>
                    </button>
                  </div>
                </div>

                {/* Status / Rate Limit Banner if applicable */}
                {aiStatus === "rate_limited" && (
                  <div className="bg-amber-500/10 border border-amber-500/30 rounded-lg p-2 text-[11px] text-amber-200 flex items-start gap-1.5">
                    <AlertCircle className="w-3.5 h-3.5 text-amber-400 shrink-0 mt-0.5" />
                    <span>Rate limit reached (429). Harper offline engine is actively verifying your writing.</span>
                  </div>
                )}

                {aiStatus === "missing_api_key" && (
                  <div className="bg-indigo-500/10 border border-indigo-500/20 rounded-lg p-2 text-[11px] text-indigo-300 flex items-start gap-1.5">
                    <HelpCircle className="w-3.5 h-3.5 text-indigo-400 shrink-0 mt-0.5" />
                    <span>Set <code className="text-white">GEMINI_API_KEY</code> in <code className="text-white">.env</code> to activate cloud AI rewrites.</span>
                  </div>
                )}

                {aiStatus === "error" && aiMessage && (
                  <div className="bg-rose-500/10 border border-rose-500/30 rounded-lg p-2 text-[11px] text-rose-200 flex items-start gap-1.5">
                    <AlertCircle className="w-3.5 h-3.5 text-rose-400 shrink-0 mt-0.5" />
                    <span>{aiMessage}</span>
                  </div>
                )}
              </div>

              {/* Suggestion Cards Scrollable List */}
              <div className="flex-1 overflow-y-auto p-4 flex flex-col gap-3">
                <div className="flex items-center justify-between text-xs text-slate-400 px-1">
                  <span className="font-semibold text-slate-300">
                    Suggestions ({filteredLints.length})
                  </span>
                  <span className="text-[10px] text-slate-500">Harper Core 3.0</span>
                </div>

                {filteredLints.length === 0 ? (
                  <div className="flex flex-col items-center justify-center p-8 text-center my-auto">
                    <div className="w-12 h-12 rounded-full bg-emerald-500/10 border border-emerald-500/20 flex items-center justify-center text-emerald-400 mb-3">
                      <CheckCircle2 className="w-6 h-6" />
                    </div>
                    <span className="text-sm font-semibold text-white">All Clear!</span>
                    <p className="text-xs text-slate-400 mt-1 max-w-xs">
                      No errors detected in this category. Your text is ready to share.
                    </p>
                  </div>
                ) : (
                  filteredLints.map((lint) => {
                    const isHovered = hoveredLintId === lint.id;
                    const primarySuggestion = lint.suggestions[0] || "";

                    return (
                      <div
                        key={lint.id}
                        onMouseEnter={() => setHoveredLintId(lint.id)}
                        onMouseLeave={() => setHoveredLintId(null)}
                        className={`p-3.5 rounded-xl border transition-all flex flex-col gap-2.5 ${
                          isHovered
                            ? "bg-slate-800/80 border-indigo-500/50 shadow-lg shadow-indigo-500/10"
                            : "bg-slate-900/50 border-white/5 hover:border-white/10"
                        }`}
                      >
                        {/* Card Header: Category & Subcategory */}
                        <div className="flex items-center justify-between">
                          <span
                            className={`px-2 py-0.5 rounded-md text-[10px] font-semibold tracking-wide ${
                              lint.category === "Correctness"
                                ? "bg-red-500/20 text-red-300 border border-red-500/30"
                                : lint.category === "Clarity"
                                ? "bg-sky-500/20 text-sky-300 border border-sky-500/30"
                                : "bg-purple-500/20 text-purple-300 border border-purple-500/30"
                            }`}
                          >
                            {lint.category} • {lint.subcategory}
                          </span>
                        </div>

                        {/* Flagged Word & Proposed Replacement */}
                        <div className="flex items-center gap-2 text-xs">
                          <span className="line-through text-slate-400 font-mono">
                            {lint.flagged_text}
                          </span>
                          <span className="text-slate-500">→</span>
                          <span className="font-bold text-emerald-400 font-mono">
                            {primarySuggestion || "(remove)"}
                          </span>
                        </div>

                        {/* Explanation Message */}
                        <p className="text-xs text-slate-300 leading-relaxed">{lint.message}</p>

                        {/* Card Actions: Accept & Ignore */}
                        <div className="flex items-center gap-2 pt-1 border-t border-white/5">
                          <button
                            onClick={() => acceptSuggestion(lint, primarySuggestion)}
                            className="flex-1 py-1.5 px-3 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold shadow-sm shadow-indigo-600/30 transition-all cursor-pointer flex items-center justify-center gap-1.5"
                          >
                            <Check className="w-3.5 h-3.5" />
                            <span>Accept</span>
                          </button>

                          <button
                            onClick={() => ignoreSuggestion(lint.id)}
                            className="py-1.5 px-3 rounded-lg bg-white/5 hover:bg-white/10 text-slate-400 hover:text-white text-xs font-medium transition-all cursor-pointer"
                          >
                            Ignore
                          </button>
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </aside>
          </div>
        ) : (
          /* Architecture Settings Tab */
          <main className="flex-1 p-8 overflow-y-auto bg-slate-950">
            <div className="max-w-2xl mx-auto flex flex-col gap-6">
              <div>
                <h2 className="text-xl font-bold text-white tracking-tight">
                  Two-Tier Engine Architecture
                </h2>
                <p className="text-xs text-slate-400 mt-1">
                  Harper Core local-first engine and Google Gemini cloud AI layer.
                </p>
              </div>

              {/* Tier 1 Info Card */}
              <div className="bg-slate-900/60 border border-white/10 rounded-2xl p-5 flex flex-col gap-3">
                <div className="flex items-center gap-2 text-sm font-semibold text-white">
                  <span className="w-2.5 h-2.5 rounded-full bg-emerald-400" />
                  <span>Tier 1: Instant Offline Engine (Harper Core)</span>
                </div>
                <p className="text-xs text-slate-400 leading-relaxed">
                  Embedded Rust language checker compiled natively into the binary. Analyzes text in under 5ms without network calls. Provides exact character start/end offsets, flagged substrings, grammar/spelling classifications, and replacement suggestions.
                </p>
                <div className="flex items-center gap-2 text-xs font-mono text-emerald-400 bg-emerald-500/10 p-2.5 rounded-xl border border-emerald-500/20">
                  <span>Latency: ~2-5ms • Memory: &lt;10MB • Offline Privacy: 100%</span>
                </div>
              </div>

              {/* Tier 2 Info Card */}
              <div className="bg-slate-900/60 border border-white/10 rounded-2xl p-5 flex flex-col gap-3">
                <div className="flex items-center gap-2 text-sm font-semibold text-white">
                  <span className="w-2.5 h-2.5 rounded-full bg-indigo-400" />
                  <span>Tier 2: Free-Tier Gemini AI Layer</span>
                </div>
                <p className="text-xs text-slate-400 leading-relaxed">
                  Calls the Gemini REST API using model <code className="text-indigo-300">gemini-3.5-flash-lite</code> (configured in <code className="text-indigo-300">.env</code>). Computes semantic writing scores (0-100), detects nuanced tone with emojis, and provides 1-click full document rewrites (Formal, Casual, Concise, Fix All).
                </p>
                <div className="p-3 bg-white/5 rounded-xl border border-white/10 text-xs flex flex-col gap-1.5">
                  <span className="text-slate-300 font-medium">Environment Setup:</span>
                  <code className="text-indigo-300 font-mono text-[11px]">
                    GEMINI_API_KEY="your-gemini-api-key" in .env
                  </code>
                </div>
              </div>

              {/* Floating Non-Focusable Widget Info */}
              <div className="bg-slate-900/60 border border-white/10 rounded-2xl p-5 flex flex-col gap-3">
                <div className="flex items-center gap-2 text-sm font-semibold text-white">
                  <ShieldCheck className="w-4 h-4 text-purple-400" />
                  <span>Floating Overlay Widget (340x440)</span>
                </div>
                <p className="text-xs text-slate-400 leading-relaxed">
                  The companion widget is configured with <code className="text-purple-300">focusable: false</code>, <code className="text-purple-300">transparent: true</code>, and <code className="text-purple-300">alwaysOnTop: true</code>. It never steals keyboard focus from Microsoft Word, VS Code, Google Docs, or Slack.
                </p>
              </div>
            </div>
          </main>
        )}
      </div>
    </div>
  );
}
