import { useEffect } from "react";
import { useTauriState } from "./hooks/useTauriState";
import { MainWindow } from "./components/MainWindow";
import { WidgetWindow } from "./components/WidgetWindow";
import { UnderlinesWindow } from "./components/UnderlinesWindow";
import { getDb } from "./services/db";

export default function App() {
  const { windowLabel, isTauri } = useTauriState();

  // Initialize SQLite database and sync with Rust engine on startup
  useEffect(() => {
    if (windowLabel !== "underlines") {
      getDb().catch((e) => {
        console.warn("Database initialization:", e);
      });
    }
  }, [windowLabel]);

  // Route based on Tauri window label or query param
  const isWidget = windowLabel === "widget";
  const isUnderlines = windowLabel === "underlines";
  const isTransparent = isWidget || isUnderlines;

  return (
    <div className={`w-full h-full ${isTransparent ? "bg-transparent" : "bg-slate-950"}`}>
      {isUnderlines ? (
        <UnderlinesWindow />
      ) : isWidget ? (
        <WidgetWindow />
      ) : (
        <MainWindow />
      )}

      {/* Browser dev switcher (only visible when testing directly in Vite browser outside Tauri) */}
      {!isTauri && (
        <aside
          aria-label="Dev Window Switcher"
          className="fixed bottom-3 right-3 z-50 bg-slate-900/90 text-white text-[11px] p-2 rounded-xl border border-white/20 shadow-xl flex items-center gap-2"
        >
          <span className="text-slate-400 font-mono">Dev Mode:</span>
          <button
            onClick={() => {
              const url = new URL(window.location.href);
              const next = isUnderlines ? "main" : isWidget ? "underlines" : "widget";
              url.searchParams.set("window", next);
              window.location.href = url.toString();
            }}
            className="px-2 py-0.5 rounded bg-indigo-600 hover:bg-indigo-500 text-white font-medium cursor-pointer transition-colors"
          >
            Switch to {isUnderlines ? "Main Window" : isWidget ? "Underlines" : "Widget"}
          </button>
        </aside>
      )}
    </div>
  );
}
