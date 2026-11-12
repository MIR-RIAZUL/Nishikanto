import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LocalLintResult } from "./useWritingEngine";

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

  const setWidgetSize = useCallback(async (width: number, height: number) => {
    try {
      await invoke("set_widget_size", { width, height });
    } catch {
      // fallback in browser
    }
  }, []);

  const expandWidget = useCallback(async () => {
    try {
      await invoke("expand_widget");
    } catch (e) {
      console.warn("Could not invoke expand_widget:", e);
    }
  }, []);

  const expandPillToMicro = useCallback(async () => {
    try {
      await invoke("expand_pill_to_micro");
    } catch (e) {
      console.warn("Could not invoke expand_pill_to_micro:", e);
    }
  }, []);

  const collapseWidget = useCallback(async () => {
    try {
      await invoke("collapse_widget");
    } catch (e) {
      console.warn("Could not invoke collapse_widget:", e);
    }
  }, []);

  const hideWidget = useCallback(async () => {
    try {
      await invoke("hide_widget");
    } catch (e) {
      console.warn("Could not invoke hide_widget:", e);
    }
  }, []);

  const replaceActiveText = useCallback(async (newText: string) => {
    try {
      await invoke("replace_active_text", { newText });
    } catch (e) {
      console.warn("Could not invoke replace_active_text:", e);
    }
  }, []);

  const replaceSingleWord = useCallback(
    async (
      start: number,
      end: number,
      replacement: string,
      fullTextFallback: string,
      lintId: string,
      originalSpan?: string
    ) => {
      try {
        return await invoke<string>("replace_single_word", {
          start,
          end,
          replacement,
          fullTextFallback,
          lintId,
          originalSpan: originalSpan || null,
        });
      } catch (e) {
        console.warn("Could not invoke replace_single_word:", e);
        return null;
      }
    },
    []
  );

  const showMicroPopover = useCallback(
    async (x: number, y: number, lint: LocalLintResult) => {
      try {
        await invoke("show_micro_popover", { x, y, lint });
      } catch (e) {
        console.warn("Could not invoke show_micro_popover:", e);
      }
    },
    []
  );

  const captureActiveText = useCallback(async () => {
    try {
      return await invoke("capture_active_text");
    } catch (e) {
      console.warn("Could not invoke capture_active_text:", e);
      return null;
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
    setWidgetSize,
    expandWidget,
    expandPillToMicro,
    collapseWidget,
    hideWidget,
    replaceActiveText,
    replaceSingleWord,
    showMicroPopover,
    captureActiveText,
  };
}
