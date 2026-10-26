import { useEffect } from "react";
import { MenuBar } from "./components/MenuBar";
import { Transport } from "./components/Transport";
import { StatusBar } from "./components/StatusBar";
import { TrackTree } from "./components/TrackTree";
import { Arrangement } from "./components/Arrangement";
import { Setup } from "./components/Setup";
import { BottomPanel } from "./components/BottomPanel";
import { Inspector } from "./components/Inspector";
import { Editor } from "./components/Editor";
import { Dialog } from "./components/Dialog";
import { SettingsWindow } from "./components/Settings";
import { PluginBrowserWindow } from "./components/PluginBrowser";
import { ExportDialog, ExportProgressBar } from "./components/ExportDialog";
import { NewProjectScreen } from "./components/NewProjectScreen";
import { GuitarTab } from "./components/GuitarTab";
import { selectedPattern, useStore } from "./store";
import { useLiveKeyboard } from "./liveKeyboard";
import { splitNotes } from "./noteEditing";

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable);
}

export default function App() {
  const init = useStore((s) => s.init);
  const snapshot = useStore((s) => s.snapshot);
  const screen = useStore((s) => s.screen);
  const toast = useStore((s) => s.toast);

  useEffect(() => {
    void init();
  }, [init]);
  useLiveKeyboard();

  // Global shortcuts. The piano roll handles its own editing keys when focused.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target)) return;
      // Alt belongs to the window manager. Tiling WMs (AeroSpace, yabai, i3-likes) bind
      // alt-<key> for workspaces and focus, and nothing here is an alt chord — without this,
      // `!mod` reads "no modifier" as true while alt is held and the app eats the user's keys.
      // ⌥ still means "fine" inside the roll, which handles its own keys.
      if (e.altKey) return;
      const s = useStore.getState();
      const mod = e.metaKey || e.ctrlKey;
      if (s.dialog) return; // the dialog owns the keyboard
      if (mod && e.key === ",") {
        e.preventDefault();
        s.openSettings(!s.settingsOpen);
        return;
      }
      if (s.settingsOpen || s.newProjectOpen) return;
      if (s.exportOpen) {
        if (e.key === "Escape") s.openExport(false);
        return;
      }
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
      } else if (mod && k === "e" && e.shiftKey) {
        // Split at the playhead; the roll handles this itself when it has focus.
        e.preventDefault();
        if (s.screen === "pattern" && s.selectedNoteIds.length) s.transformSelection((ns, ids) => splitNotes(ns, ids, s.playhead), "split notes");
      } else if (mod && k === "e") {
        e.preventDefault();
        void s.exportWav();
      } else if (mod && k === "q") {
        e.preventDefault();
        void s.requestClose();
      } else if (!mod && e.key === "Tab") {
        e.preventDefault();
        // Tab cycles the working screens in the order the transport lists them; from SETUP it
        // goes to the pattern editor.
        s.setScreen(s.screen === "pattern" ? "song" : s.screen === "song" ? "guitar" : "pattern");
      } else if (!mod && s.liveKeyboard === "off" && k === "l") {
        s.toggleLooping();
      } else if (!mod && s.liveKeyboard === "off" && k === "r") {
        // Recording lands in the selected pattern, so there has to be one to arm.
        if (!s.armed && !selectedPattern(s)) s.showToast("pick a pattern to record into first");
        else s.toggleRecord();
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
        {screen === "setup" ? (
          <Setup />
        ) : screen === "guitar" ? (
          <GuitarTab />
        ) : screen === "song" ? (
          <>
            <TrackTree />
            <div className="center">
              <Arrangement />
              <BottomPanel />
            </div>
          </>
        ) : (
          // The tree is the left panel; the inspector sits on the right and folds to a rail,
          // so the roll keeps most of the width.
          <>
            <TrackTree />
            <Editor />
            <Inspector />
          </>
        )}
      </div>
      <StatusBar />
      {toast && <div className={`toast${toast.error ? " error" : ""}`}>{toast.text}</div>}
      <SettingsWindow />
      <PluginBrowserWindow />
      <ExportDialog />
      <NewProjectScreen />
      <ExportProgressBar />
      <Dialog />
    </div>
  );
}
