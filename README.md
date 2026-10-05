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
cargo test --workspace --exclude dissonant-app   # 73 Rust tests: theory, document, engine timing, render
pnpm --dir ui test                               # 27 UI tests: theory mirror, note tools
pnpm --dir ui typecheck
cargo clippy --workspace --all-targets
```

## Using it

Two screens, switched with the **PAT | SONG** control (or Tab):

| Where | How |
|---|---|
| **PAT** · piano roll | click places a note (keep dragging to move it) · drag a note to move, its right edge to resize · ⌥-drag to paint or duplicate · ⇧-drag marquee · right-click erases · ⌫ ⌘A ⌘C ⌘X ⌘V ⌘D · arrows nudge (⇧ = octave / bar) · `[` `]` velocity · velocity lane: drag the stems · ⌘± zoom · click the ruler to seek · drag the ruler's top strip for a loop region (right-click clears) · hover a cell for a plain-words explanation · **tiers** tints rows by fit, **map** paints the whole progression |
| Transform | inspector → TRANSFORM: quantize, humanize (chord tones stay tighter), legato, arp ↑/↓/⇅/random, strum, chop, ×2 slower / ½ faster · **magnet** pulls placed/dragged notes to the nearest chord tone · **stamp chord** places the chord under the cursor |
| Resolve / keep | select one dissonant note: the inspector says why, offers ↓/↑ resolve targets, or **keep** marks it deliberate (hatch stays, the "!" goes) |
| Chord lane | starters live under HARMONY in the inspector (always in the key) · drag a chord to move, its right edge to resize · click to free-build · right-click to delete · **chords** to hear the bed |
| Play live | **KEYS** tier: home row = chord tones, top row = tensions of the chord under the playhead, remapped as it moves · chrom: Z/Q rows chromatic · −/+ octave · **MIDI IN** picks a controller · **REC** (R) while playing writes what you play into the pattern, quantized to the grid |
| Key | place notes and the KEY cell offers "X? lock" · × unlocks · or pick root + scale under HARMONY · a section with its own key modulates the patterns inside it |
| Drums | step grid: left-click/drag adds hits, right-click removes · step follows snap (1/8, 1/16, 1/32) |
| **SONG** · arrangement | ＋ on a pattern (left) appends a clip · drag a clip to move (snaps to bars, ⇧ for beats), its right edge to trim or loop-extend, ⌥-drag to copy, right-click to remove, double-click to edit · double-click the ruler's top strip to add a section; click it to name it, give it a key or color; drag to move; right-click to remove · track headers: M/S, volume slider, meters · click the ruler to seek |
| Bottom panel | **DEVICES**: instrument, track bus (vol / pan / tone / reverb), master (gain, cuts, 3-band EQ, reverb) · **MIXER**: faders, pan, meters, M/S |
| Transport | space play/stop · Enter return to start · L loop · R record · M metronome (count-in and pre-roll when record is armed; volume in Settings) · BPM: click to type (↑↓ ±1, ⇧ ±10), drag up/down, scroll, double-click resets, ÷2 ×2 · tap tempo (last 8 taps) · SWING 50–75 on 8ths or 16ths (the roll shows where swung notes land) · SIG time signature · ⌘Z/⇧⌘Z undo/redo (the Edit menu names the step) · ⌘S save · ⌘O open · ⌘E export WAV (offline render, faster than real time) · ⌘, settings · ⌘Q quit |
| Files | unsaved changes are prompted on close, new and open · autosave every 30 s while dirty (`<project>.autosave.json`, restore offered on launch) · File → recent files · drop a `.dissonant` on the window to open it · the engine restarts by itself when the output device changes or disappears |
| Settings (⌘,) | **audio**: output device, sample rate, buffer size with latency, test tone, engine load / xruns, metronome volume, count-in, pre-roll · **midi**: default input with auto-reconnect, velocity curve, channel, octave · **editing**: default grid / velocity, audition on place, ⌥ = no-snap or paint, confirmations, new-project defaults · **export**: rate, 16/24/32-bit, dither, normalize, tail · **appearance**: UI scale, row height, reduced motion, accent and tier colors (color-blind-safe preset) |

The look follows the UI handoff in `docs/design/2026-10-02-ui-handoff-violet.md` (violet accent, IBM Plex
Sans Condensed + JetBrains Mono, bundled). Controls only exist for features that are built; the
metronome, count-in, tools, ghost notes, CC lane, browser and plugin devices from the mock arrive
with their features.

Projects are JSON (`.dissonant`). The format is documented by `crates/dissonant-core/src/model.rs`.

## Roadmap

The plan lives in `docs/brainstorms/2026-10-02-full-daw-brainstorm.md`; `docs/decisions/` explains
the stack change. Status against it:

| Phase | Done | Open |
|---|---|---|
| 0 Foundations | audio-clock scheduler, render-block synth, command bus + undo, offline render, pan, loop region, time signature, legacy import | integer ticks, tempo map (tempo changes) |
| 1 Roll | full editing, velocity lane, zoom, transform tools, magnet, stamp, resolve/keep, explain-this-note, live tier keyboard, MIDI in + record | ghost notes, per-note probability / slide |
| 2 Arrangement | clips with trim / loop-extend / offset, sections with per-section key | tempo lane, ripple edits, variation generator |
| 3 Mix & devices | track bus, master chain, mixer, meters | device chains, native effects, CLAP hosting, automation, sidechain |
| 4 Instruments | waveform synth, procedural kit | real subtractive synth, sampler, drum machine upgrade, browser |
| 5 Audio | — | recording, clips, warp, audio → notes / chords |
| 6 Harmony | tiers, key detection, starters, free-build chords | reharmonize from melody, function-preserving transpose, voicing engine, next-chord suggestions |
| 7 Jam & polish | — | session view, modulators, command palette, layouts |
