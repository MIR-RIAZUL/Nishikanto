import { useTauriState } from "./hooks/useTauriState";
import { MainWindow } from "./components/MainWindow";
import { WidgetWindow } from "./components/WidgetWindow";

export default function App() {
  const { windowLabel, isTauri } = useTauriState();

  // Route based on Tauri window label or query param
  const isWidget = windowLabel === "widget";

  return (
    <div className={`w-full h-full ${isWidget ? "bg-transparent" : "bg-slate-950"}`}>
      {isWidget ? <WidgetWindow /> : <MainWindow />}

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
              url.searchParams.set("window", isWidget ? "main" : "widget");
              window.location.href = url.toString();
            }}
            className="px-2 py-0.5 rounded bg-indigo-600 hover:bg-indigo-500 text-white font-medium cursor-pointer transition-colors"
          >
            Switch to {isWidget ? "Main Window (1100x750)" : "Widget (340x440)"}
          </button>
        </aside>
      )}
    </div>
  );
}
