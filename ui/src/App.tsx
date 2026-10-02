import { useEffect } from "react";
import { Header } from "./components/Header";
import { TrackSidebar } from "./components/TrackSidebar";
import { PatternBar } from "./components/PatternBar";
import { ChordLane } from "./components/ChordLane";
import { PianoRoll } from "./components/PianoRoll";
import { PlayableNow } from "./components/PlayableNow";
import { DrumGrid } from "./components/DrumGrid";
import { SongView } from "./components/SongView";
import { FxBar } from "./components/FxBar";
import { VoicePicker } from "./components/VoicePicker";
import { selectedTrack, useStore } from "./store";

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
}

export default function App() {
  const init = useStore((s) => s.init);
  const snapshot = useStore((s) => s.snapshot);
  const mode = useStore((s) => s.mode);
  const toast = useStore((s) => s.toast);
  const track = useStore(selectedTrack);

  useEffect(() => {
    void init();
  }, [init]);

  // Global shortcuts. The piano roll handles its own editing keys when focused.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target)) return;
      const s = useStore.getState();
      const mod = e.metaKey || e.ctrlKey;
      if (e.code === "Space") {
        e.preventDefault();
        s.togglePlay();
      } else if (mod && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) void s.redo();
        else void s.undo();
      } else if (mod && e.key.toLowerCase() === "y") {
        e.preventDefault();
        void s.redo();
      } else if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void s.saveProject(e.shiftKey);
      } else if (mod && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void s.openProject();
      } else if (mod && e.key.toLowerCase() === "n") {
        e.preventDefault();
        void s.newProject();
      } else if (mod && e.key.toLowerCase() === "e") {
        e.preventDefault();
        void s.exportWav();
      } else if (!mod && e.key.toLowerCase() === "r") {
        s.rewind();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!snapshot) {
    return <div className="main">loading…</div>;
  }

  return (
    <div className="app" onContextMenu={(e) => e.preventDefault()}>
      <TrackSidebar />
      <div className="main">
        <Header />
        <PatternBar />
        <div className="editor">
          {mode === "pattern" ? (
            <>
              <ChordLane />
              {track?.isDrum ? (
                <DrumGrid />
              ) : (
                <>
                  <VoicePicker />
                  <PianoRoll />
                  <PlayableNow />
                </>
              )}
            </>
          ) : (
            <SongView />
          )}
        </div>
        <FxBar />
      </div>
      {toast && <div className={`toast${toast.error ? " error" : ""}`}>{toast.text}</div>}
    </div>
  );
}
