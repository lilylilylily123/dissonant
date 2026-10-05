import { useEffect } from "react";
import { MenuBar } from "./components/MenuBar";
import { Transport } from "./components/Transport";
import { StatusBar } from "./components/StatusBar";
import { PatternsPanel } from "./components/PatternsPanel";
import { Arrangement } from "./components/Arrangement";
import { BottomPanel } from "./components/BottomPanel";
import { Inspector } from "./components/Inspector";
import { Editor } from "./components/Editor";
import { Dialog } from "./components/Dialog";
import { SettingsWindow } from "./components/Settings";
import { useStore } from "./store";
import { useLiveKeyboard } from "./liveKeyboard";

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable);
}

export default function App() {
  const init = useStore((s) => s.init);
  const snapshot = useStore((s) => s.snapshot);
  const mode = useStore((s) => s.mode);
  const toast = useStore((s) => s.toast);

  useEffect(() => {
    void init();
  }, [init]);
  useLiveKeyboard();

  // Global shortcuts. The piano roll handles its own editing keys when focused.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target)) return;
      const s = useStore.getState();
      const mod = e.metaKey || e.ctrlKey;
      if (s.dialog) return; // the dialog owns the keyboard
      if (mod && e.key === ",") {
        e.preventDefault();
        s.openSettings(!s.settingsOpen);
        return;
      }
      if (s.settingsOpen) return;
      const k = e.key.toLowerCase();
      if (e.code === "Space") {
        e.preventDefault();
        s.togglePlay();
      } else if (e.key === "Enter" && !mod) {
        s.rewind();
      } else if (mod && k === "z") {
        e.preventDefault();
        if (e.shiftKey) void s.redo();
        else void s.undo();
      } else if (mod && k === "y") {
        e.preventDefault();
        void s.redo();
      } else if (mod && k === "s") {
        e.preventDefault();
        void s.saveProject(e.shiftKey);
      } else if (mod && k === "o") {
        e.preventDefault();
        void s.openProject();
      } else if (mod && k === "n") {
        e.preventDefault();
        void s.newProject();
      } else if (mod && k === "e") {
        e.preventDefault();
        void s.exportWav();
      } else if (mod && k === "q") {
        e.preventDefault();
        void s.requestClose();
      } else if (!mod && e.key === "Tab") {
        e.preventDefault();
        s.setMode(s.mode === "pattern" ? "song" : "pattern");
      } else if (!mod && s.liveKeyboard === "off" && k === "l") {
        s.toggleLooping();
      } else if (!mod && s.liveKeyboard === "off" && k === "r") {
        s.toggleRecord();
      } else if (!mod && s.liveKeyboard === "off" && k === "m") {
        s.toggleMetronome();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!snapshot) {
    return (
      <div className="frame" style={{ alignItems: "center", justifyContent: "center", color: "var(--text-5)" }}>
        <span className="mono">loading…</span>
      </div>
    );
  }

  return (
    <div className="frame" onContextMenu={(e) => e.preventDefault()}>
      <MenuBar />
      <Transport />
      <div className="content">
        {mode === "song" ? (
          <>
            <PatternsPanel />
            <div className="center">
              <Arrangement />
              <BottomPanel />
            </div>
          </>
        ) : (
          <>
            <Inspector />
            <Editor />
          </>
        )}
      </div>
      <StatusBar />
      {toast && <div className={`toast${toast.error ? " error" : ""}`}>{toast.text}</div>}
      <SettingsWindow />
      <Dialog />
    </div>
  );
}
