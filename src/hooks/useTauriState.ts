import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

export function useTauriState() {
  const [isPaused, setIsPaused] = useState<boolean>(false);
  const [windowLabel, setWindowLabel] = useState<string>("main");
  const [isTauri, setIsTauri] = useState<boolean>(false);

  useEffect(() => {
    // Detect window label and Tauri environment
    try {
      const current = getCurrentWebviewWindow();
      if (current && current.label) {
        setWindowLabel(current.label);
        setIsTauri(true);
      }
    } catch {
      // In browser mode fallback
      const urlParams = new URLSearchParams(window.location.search);
      const queryWindow = urlParams.get("window");
      if (queryWindow) {
        setWindowLabel(queryWindow);
      }
    }

    // Fetch initial pause state from Rust backend
    invoke<boolean>("get_pause_state")
      .then((state) => setIsPaused(state))
      .catch(() => {});

    // Listen for tray or backend pause toggle events
    let unlisten: (() => void) | undefined;
    listen<boolean>("pause-status-changed", (event) => {
      setIsPaused(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const togglePause = useCallback(async () => {
    try {
      const next = await invoke<boolean>("toggle_pause_state");
      setIsPaused(next);
      return next;
    } catch {
      setIsPaused((prev) => !prev);
      return !isPaused;
    }
  }, [isPaused]);

  const openMainEditor = useCallback(async () => {
    try {
      await invoke("show_main_window");
    } catch (e) {
      console.warn("Could not invoke show_main_window:", e);
    }
  }, []);

  const hideMainEditor = useCallback(async () => {
    try {
      await invoke("hide_main_window");
    } catch (e) {
      console.warn("Could not invoke hide_main_window:", e);
    }
  }, []);

  const setWidgetHeight = useCallback(async (height: number) => {
    try {
      await invoke("set_widget_height", { height });
    } catch {
      // fallback in browser
    }
  }, []);

  return {
    isPaused,
    windowLabel,
    isTauri,
    togglePause,
    openMainEditor,
    hideMainEditor,
    setWidgetHeight,
  };
}
