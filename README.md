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

dissonant opens on a **new-project screen**: start from a **vibe** (lo-fi bedroom, post-punk,
ambient texture, songwriter demo, club loop: tempo, key, chords, drums and a bass line, with an
empty track for your melody), the **starter** (I–IV–V–vi in C with a melody and drums), an empty
project, one of your templates, or a recent file. The chord bed starts on so space plays music
straight away. Turn the screen off in Settings → editing; File → New from a vibe or template…
brings it back, and ⌘N makes a starter or empty project per the same settings tab.

Two screens, switched with the **PAT | SONG** control (or Tab). Shortcuts are written for macOS;
on Windows and Linux read Ctrl for ⌘, Alt for ⌥ and Shift for ⇧ (the app's own labels do this
for you):

| Where | How |
|---|---|
| **PAT** · piano roll | click places a note (keep dragging to move it) · drag a note to move (⇧ locks to one axis), its right edge to resize · ⌥-drag a note duplicates · ⌥ on empty space places off the grid (or paints, per Settings) · **brush** paints · ⇧/⌘-drag marquee · double-click a key selects that pitch (⌘: pitch class) · Tab/⇧Tab next/previous note · right-click erases · ⌫ ⌘A ⌘C ⌘X ⌘V ⌘D · ⇧⌘E split at playhead · ⌘J glue · ⌘B duplicate one loop later · 0 mute (hollow, not played) · arrows nudge (⇧ = octave / bar, ⌥ = fine) · `[` `]` velocity · ⌥-wheel transposes (⌥⇧ octaves) · ⌘-wheel zooms at the cursor · ⇧-wheel scrolls · velocity lane: drag the stems, ⌥-drag draws a ramp · click the ruler to seek · drag the ruler's top strip for a loop region (right-click clears) · **follow** keeps the playhead in view · **fit** zooms to the pattern · **fold** shows only rows with notes (plus nearby chord tones) · **ghost** draws another track's notes behind yours, dashed, with their tiers · hover a cell for a plain-words explanation · **tiers** tints rows by fit, **map** paints the whole progression · HARMONY → select by tier picks every chord tone / tension / dissonant note · NOTES → length chips and mute for the selection |
| Transform | inspector → TRANSFORM: quantize, humanize (chord tones stay tighter), legato, arp ↑/↓/⇅/random, strum, chop, ×2 slower / ½ faster, reverse, invert · Edit menu: transpose by semitone / octave, split, glue, duplicate-loop, mute · **magnet** pulls placed/dragged notes to the nearest chord tone · **stamp chord** places the chord under the cursor |
| Resolve / keep | select one dissonant note: the inspector says why, offers ↓/↑ resolve targets, or **keep** marks it deliberate (hatch stays, the "!" goes) |
| Chord lane | progressions live under HARMONY in the inspector, named by mood ("sad but hopeful") with the chords they write in your key; the list follows the key's scale · drag a chord to move, its right edge to resize · click to free-build · right-click to delete · **chords** to hear the bed |
| Play live | **KEYS** tier: home row = chord tones, top row = tensions of the chord under the playhead, remapped as it moves · chrom: Z/Q rows chromatic · −/+ octave · **MIDI IN** picks a controller · **REC** (R) while playing writes what you play into the pattern, quantized to the grid |
| Key | place notes and the KEY cell offers "X? lock" · × unlocks · or pick root + scale under HARMONY · a section with its own key modulates the patterns inside it |
| Drums | step grid: left-click/drag adds hits, right-click removes · step follows snap (1/8, 1/16, 1/32) |
| **SONG** · arrangement | **tempo lane** under the ruler: double-click adds a tempo change at that beat, drag a point up/down for bpm (⇧ fine) and sideways to move it, ⌥-click makes it a ramp (a glide from the previous tempo), right-click removes; the transport clock and the engine follow the map, pattern mode plays at the base tempo · ＋ on a pattern (left) appends a clip · drag a clip to move (snaps to bars, ⇧ for beats), its right edge to trim or loop-extend, ⌥-drag to copy, right-click to remove, double-click to edit · double-click the ruler's top strip to add a section; click it to name it, give it a key or color; drag to move; right-click to remove · track headers: M/S, volume slider, meters · click the ruler to seek |
| Sounds | inspector → INSTRUMENT: presets by character (warm pad, soft keys, glass pluck, buzzy bass, round bass, lead with bite) set waveform, tone and reverb in one undo step; the waveform row stays underneath |
| Bottom panel | **DEVICES**: instrument, track bus (vol / pan / tone / reverb), master (gain, cuts, 3-band EQ, reverb) · **MIXER**: faders, pan, meters, M/S |
| Transport | space play/stop · Enter return to start · L loop · R record · M metronome (count-in and pre-roll when record is armed; volume in Settings) · BPM: click to type (↑↓ ±1, ⇧ ±10), drag up/down, scroll, double-click resets, ÷2 ×2 · tap tempo (last 8 taps) · SWING 50–75 on 8ths or 16ths (the roll shows where swung notes land) · SIG time signature · ⌘Z/⇧⌘Z undo/redo (the Edit menu names the step) · ⌘S save · ⌘O open · ⌘E export WAV (offline render, faster than real time) · ⌘, settings · ⌘Q quit |
| Files | **⌘E** export audio: song or pattern, loop count, chord bed, **stems** (one WAV per track), rate / depth / dither / normalize / tail, with a progress strip · File → Export MIDI (one MIDI track per track, chord names as markers) and Import MIDI into the selected pattern (new tracks, one undo step) · File → New from a vibe or template / Save as template (templates live in the app data folder) · unsaved changes are prompted on close, new and open · autosave every 30 s while dirty (`<project>.autosave.json`, restore offered on launch) · File → recent files · drop a `.dissonant` on the window to open it · the engine restarts by itself when the output device changes or disappears |
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
| 0 Foundations | audio-clock scheduler, render-block synth, command bus + undo (labelled), offline render, pan, loop region, time signature, legacy import, tempo map with ramps, swing, metronome + count-in, settings, autosave, device-loss recovery | integer ticks |
| 1 Roll | full editing, velocity lane, zoom, transform tools, magnet, stamp, resolve/keep, explain-this-note, live tier keyboard, MIDI in + record, ghost notes, fold, per-note mute, select by tier, velocity ramp, split / glue / duplicate-loop | per-note probability / slide |
| 2 Arrangement | clips with trim / loop-extend / offset, sections with per-section key, tempo lane, MIDI file export / import, templates, stems | ripple edits, variation generator |
| 3 Mix & devices | track bus, master chain, mixer, meters | device chains, native effects, CLAP hosting, automation, sidechain |
| 4 Instruments | waveform synth, procedural kit | real subtractive synth, sampler, drum machine upgrade, browser |
| 5 Audio | — | recording, clips, warp, audio → notes / chords |
| 6 Harmony | tiers, key detection, starters, free-build chords | reharmonize from melody, function-preserving transpose, voicing engine, next-chord suggestions |
| 7 Jam & polish | — | session view, modulators, command palette, layouts |
