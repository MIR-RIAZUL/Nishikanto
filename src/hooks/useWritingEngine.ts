import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LocalLintResult {
  id: string;
  start: number;
  end: number;
  flagged_text: string;
  category: "Correctness" | "Clarity" | "Style" | "Fluency" | string;
  subcategory: string;
  message: string;
  suggestions: string[];
  screen_bounds?: ScreenRect;
  is_sentence_rewrite?: boolean;
  reason?: string;
}

export interface GeminiAnalysisResult {
  success: boolean;
  status: "ok" | "rate_limited" | "missing_api_key" | "error";
  message: string;
  tone: string;
  score: number;
  rewritten_text: string;
}

export const SAMPLE_TEST_PARAGRAPH =
  "Their is definately many reasons why the report was written by our team yesterday. In regards to the basic fundamentals, we should of took further actions ASAP to avoid this bad mistakes. I am very confident that we will fix it in a efficient manner.";

export function useWritingEngine(initialText: string = SAMPLE_TEST_PARAGRAPH) {
  const [text, setText] = useState<string>(initialText);
  const [lints, setLints] = useState<LocalLintResult[]>([]);
  const [ignoredIds, setIgnoredIds] = useState<Set<string>>(new Set());
  const [hoveredLintId, setHoveredLintId] = useState<string | null>(null);

  // Gemini AI state
  const [writingScore, setWritingScore] = useState<number>(68);
  const [toneLabel, setToneLabel] = useState<string>("Casual & Conversational ☕");
  const [isAiLoading, setIsAiLoading] = useState<boolean>(false);
  const [aiMessage, setAiMessage] = useState<string | null>(null);
  const [aiStatus, setAiStatus] = useState<string>("ok");

  // Timer ref for 1.5s debounce AI analysis
  const debounceTimerRef = useRef<number | null>(null);
  const lastAnalyzedTextRef = useRef<string>("");

  // Instant local check via Harper Core (<5ms)
  const runLocalCheck = useCallback(async (content: string) => {
    if (!content.trim()) {
      setLints([]);
      return;
    }
    try {
      const results = await invoke<LocalLintResult[]>("check_text_local", {
        text: content,
      });
      setLints(results);

      // Compute instant local heuristic score (penalizing based on error count per 100 words)
      const words = Math.max(content.trim().split(/\s+/).length, 1);
      const penalty = Math.min(Math.round((results.length / words) * 100 * 8), 55);
      const baseScore = Math.max(100 - penalty, 45);
      setWritingScore((prev) => (prev ? Math.round((prev * 0.4) + (baseScore * 0.6)) : baseScore));
    } catch (err) {
      console.warn("Local harper check error:", err);
    }
  }, []);

  // Tier 2: Gemini AI Analysis
  const runGeminiAnalysis = useCallback(
    async (content: string, mode: string = "tone_and_score") => {
      if (!content.trim()) return;
      setIsAiLoading(true);
      try {
        const res = await invoke<GeminiAnalysisResult>("analyze_with_gemini", {
          text: content,
          mode,
        });

        setAiStatus(res.status);
        setToneLabel(res.tone);
        setWritingScore(res.score);
        setAiMessage(res.message);

        // If a rewrite mode was requested and succeeded, update text
        if (
          mode !== "tone_and_score" &&
          res.success &&
          res.rewritten_text &&
          res.rewritten_text !== content
        ) {
          setText(res.rewritten_text);
          runLocalCheck(res.rewritten_text);
        }

        lastAnalyzedTextRef.current = content;
      } catch (err) {
        setAiStatus("error");
        setAiMessage(`AI connection error: ${String(err)}`);
      } finally {
        setIsAiLoading(false);
      }
    },
    [runLocalCheck]
  );

  // Trigger local check immediately on every keystroke, and debounce Gemini AI by 1.5s
  const updateText = useCallback(
    (newText: string) => {
      setText(newText);
      runLocalCheck(newText);

      // Debounce Gemini AI analysis for 1.5s after typing pause
      if (debounceTimerRef.current) {
        window.clearTimeout(debounceTimerRef.current);
      }

      debounceTimerRef.current = window.setTimeout(() => {
        if (newText.trim().length > 10 && newText !== lastAnalyzedTextRef.current) {
          runGeminiAnalysis(newText, "tone_and_score");
        }
      }, 1500);
    },
    [runLocalCheck, runGeminiAnalysis]
  );

  // Initial check on mount
  useEffect(() => {
    runLocalCheck(text);
    runGeminiAnalysis(text, "tone_and_score");

    return () => {
      if (debounceTimerRef.current) {
        window.clearTimeout(debounceTimerRef.current);
      }
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Accept a suggestion: replace the flagged slice
  const acceptSuggestion = useCallback(
    (lint: LocalLintResult, replacement: string) => {
      const chars = Array.from(text);
      const before = chars.slice(0, lint.start).join("");
      const after = chars.slice(lint.end).join("");
      const updated = before + replacement + after;

      updateText(updated);
    },
    [text, updateText]
  );

  // Ignore a suggestion: hide it from UI
  const ignoreSuggestion = useCallback((lintId: string) => {
    setIgnoredIds((prev) => new Set(prev).add(lintId));
  }, []);

  // Filter out ignored lints
  const activeLints = lints.filter((l) => !ignoredIds.has(l.id));

  return {
    text,
    setText: updateText,
    lints: activeLints,
    rawLintsCount: lints.length,
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
    loadSampleText: () => updateText(SAMPLE_TEST_PARAGRAPH),
  };
}
