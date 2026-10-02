# dissonant

A cross-platform DAW that shows you which notes sound good. The piano roll re-tiers every
note in real time against the chord that's playing — **solid** chord tones, **spicy** tensions,
**flagged** dissonance — so a player without theory can write, arrange and finish a track.
Dissonance is always labeled, never blocked.

Rust audio engine + TypeScript UI, packaged with Tauri for macOS, Windows and Linux.

```
crates/dissonant-core     model · theory · chord track · document + undo        (pure Rust, tested)
crates/dissonant-engine   sample-accurate scheduler · synth · drums · mixer · render (Rust, tested)
src-tauri                 desktop shell: commands, playhead events, file I/O, export
ui                        Vite + React + TypeScript: piano roll, chord lane, drums, song, mixer
docs                      brainstorms, plans, decisions
legacy/swift              the original macOS prototype (frozen)
```

## Run it

Prerequisites: [Rust](https://rustup.rs) (stable), [Node 20+](https://nodejs.org) with
`pnpm` (`corepack enable`), and the Tauri system deps for your OS:

- **macOS**: Xcode command-line tools.
- **Windows**: Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11).
- **Linux (Debian/Ubuntu)**:
  `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev libasound2-dev patchelf pkg-config`

```sh
pnpm install            # root: tauri cli
pnpm --dir ui install   # ui deps
pnpm dev                # runs the Vite dev server + the Tauri app with hot reload
pnpm build              # production bundle + installer in src-tauri/target/release/bundle
```

UI-only, in a browser (no audio, in-memory mock of the backend — handy for design work):

```sh
pnpm ui                 # http://localhost:5173
```

## Test

```sh
cargo test --workspace --exclude dissonant-app   # 65 Rust tests: theory, document, engine timing, render
pnpm --dir ui test                               # 17 UI tests: theory mirror, note-editing math
pnpm --dir ui typecheck
cargo clippy --workspace --all-targets
```

## Using it

Two screens, switched with the **PAT | SONG** control (or Tab):

| Where | How |
|---|---|
| **PAT** · piano roll | click places a note (keep dragging to move it) · drag a note to move, its right edge to resize · ⌥-drag to paint or duplicate · ⇧-drag marquee · right-click erases · ⌫ ⌘A ⌘C ⌘X ⌘V ⌘D · arrows nudge (⇧ = octave / bar) · `[` `]` velocity · velocity lane: drag the stems · ⌘± zoom · click the ruler to seek · **tiers** tints rows by fit, **map** paints the whole progression |
| Chord lane | starters live under HARMONY in the inspector (always in the project key) · drag a chord to move, its right edge to resize · click to free-build · right-click to delete · **chords** to hear the bed |
| Inspector | tracks (select, rename, M/S, reorder, delete) · pattern (name, color, length) · selection ranges · harmony (key, live tier strip) · instrument (voice, bus knobs: drag, ⇧ fine, double-click resets) · song overview |
| Key | place notes and the KEY cell offers "X? lock" · × unlocks · or pick root + scale under HARMONY |
| Drums | step grid: left-click/drag adds hits, right-click removes · step follows snap (1/8, 1/16, 1/32) |
| **SONG** · arrangement | ＋ on a pattern (left) appends it · click a clip to select its pattern, double-click to open it, drag to reorder, right-click to remove · track headers: M/S, volume slider, meters · click the ruler to seek |
| Bottom panel | **DEVICES**: instrument, track bus (vol / pan / tone / reverb), master (gain, cuts, 3-band EQ, reverb) · **MIXER**: faders, pan, meters, M/S |
| Transport | space play/stop · Enter return to start · tap tempo · ⌘Z/⇧⌘Z undo/redo · ⌘S save · ⌘O open · ⌘E export WAV (offline render, faster than real time) |

The look follows the UI handoff in `docs/design/2026-10-02-ui-handoff-violet.md` (violet accent, IBM Plex
Sans Condensed + JetBrains Mono, bundled). Controls only exist for features that are built; the
record / loop / metronome / tools / ghost notes / CC lane / browser / plugin devices from the mock
arrive with their features.

Projects are JSON (`.dissonant`). The format is documented by `crates/dissonant-core/src/model.rs`.

## Roadmap

See `docs/brainstorms/2026-10-02-full-daw-brainstorm.md` for the full plan (playlist of clips,
session view, device chains, CLAP plugins, MIDI input, audio recording, and the harmony-native
features nobody else has) and `docs/decisions/` for why the stack changed.
