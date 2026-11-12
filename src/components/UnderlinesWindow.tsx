import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { LocalLintResult } from "../hooks/useWritingEngine";

export function UnderlinesWindow() {
  const [lints, setLints] = useState<LocalLintResult[]>([]);

  useEffect(() => {
    let unlistenUpdated: (() => void) | undefined;
    let unlistenCleared: (() => void) | undefined;

    // Listen for live screen bounding boxes for misspelled or flagged words
    listen<LocalLintResult[]>("os-underlines-updated", (event) => {
      const active = (event.payload || []).filter((l) => !!l.screen_bounds);
      setLints(active);
    }).then((fn) => {
      unlistenUpdated = fn;
    });

    // Listen for clear events (scrolling, window switch, clearing text, or blacklisted app)
    listen("os-underlines-cleared", () => {
      setLints([]);
    }).then((fn) => {
      unlistenCleared = fn;
    });

    const handleClear = () => setLints([]);
    window.addEventListener("scroll", handleClear, true);

    return () => {
      if (unlistenUpdated) unlistenUpdated();
      if (unlistenCleared) unlistenCleared();
      window.removeEventListener("scroll", handleClear, true);
    };
  }, []);

  if (lints.length === 0) {
    return null;
  }

  return (
    <div className="fixed inset-0 w-screen h-screen pointer-events-none overflow-hidden select-none z-[999999]">
      {lints.map((lint) => {
        if (!lint.screen_bounds) return null;
        const { x, y, width, height } = lint.screen_bounds;
        const isCorrectness = lint.category === "Correctness";

        return (
          <div
            key={lint.id}
            className="absolute pointer-events-none transition-all duration-150 ease-out"
            style={{
              left: `${x}px`,
              top: `${y}px`,
              width: `${width}px`,
              height: `${height}px`,
            }}
          >
            {/* Subtle highlight tint over the flagged word */}
            <div
              className={`w-full h-full rounded-xs transition-opacity duration-200 ${
                isCorrectness ? "bg-red-500/10" : "bg-blue-500/10"
              }`}
            />
            {/* Crisp 2px solid underline directly under the word */}
            <div
              className={`w-full h-[2px] -mt-[2px] rounded-full shadow-sm transition-transform duration-200 ${
                isCorrectness
                  ? "bg-[#ef4444] shadow-red-500/40"
                  : "bg-[#3b82f6] shadow-blue-500/40"
              }`}
            />
          </div>
        );
      })}
    </div>
  );
}
