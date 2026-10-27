# dissonant

A cross-platform DAW that shows you which notes sound good. The piano roll re-tiers every
note in real time against the chord that's playing — **solid** chord tones, **spicy** tensions,
**flagged** dissonance — so a player without theory can write, arrange and finish a track.
Dissonance is always labeled, never blocked.

Rust audio engine + TypeScript UI, packaged with Tauri for macOS, Windows and Linux.

```
crates/dissonant-core     model · theory · chord track · document + undo        (pure Rust, tested)
crates/dissonant-engine   sample-accurate scheduler · synth · drums · mixer · render (Rust, tested)
crates/dissonant-plugins  CLAP: find installed plugins, host them                (Rust, tested)
crates/dissonant-catalog  curated free-plugin catalog · checksum-pinned installer (Rust, tested)
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
cargo test --workspace                           # 443 Rust tests: theory, document, schema migration, sound library, plugin hosting, insert chains, engine timing, render
pnpm --dir ui test                               # 275 UI tests: bridge parity, theory mirror, note tools, step tools, clip editing, song setup, sound design
pnpm --dir ui typecheck
cargo clippy --workspace --all-targets
```

## Using it

dissonant opens on a **new-project screen**: start from a **vibe** (amen jungle, dubstep, blown
out, rage, jersey club, hyperpop, pluggnb — each one sets tempo, key, swing and chords, then
builds a bass line, a chord part and a drum pattern on its own kit, leaving the lead track empty
for you), the **starter** (I–IV–V–vi in C with a melody and drums), an empty
project, one of your templates, or a recent file. The chord bed starts on so space plays music
straight away. Turn the screen off in Settings → editing; File → New from a vibe or template…
brings it back, and ⌘N makes a starter or empty project per the same settings tab.

Five screens, switched with the **SETUP | PAT | SONG | GTR | SOUND** control (Tab cycles PAT and
SONG).
Shortcuts are written for macOS; on Windows and Linux read Ctrl for ⌘, Alt for ⌥ and Shift for ⇧
(the app's own labels do this for you):

| Where | How |
|---|---|
| **SETUP** · the song | key, tempo and swing, the song's chord progression and its length in bars, and the sections that divide it — the page to shape a song on before writing a note |
| Tracks and patterns | every pattern belongs to one track: the track → pattern tree on the left adds, renames, duplicates and deletes patterns, and drags them onto the song on their own track's lane |
| **PAT** · piano roll | click places a note (keep dragging to move it) · drag a note to move (press ⇧ mid-drag to lock an axis), its right edge to resize · **⇧-drag a note steps its length through the note values** (1/16 → 1/8 → 1/4 …; ⇧-click still toggles the selection) · ⌥-drag a note duplicates · ⌥ on empty space places off the grid (or paints, per Settings) · **brush** paints · ⇧/⌘-drag marquee · double-click a key selects that pitch (⌘: pitch class) · Tab/⇧Tab next/previous note · right-click erases · ⌫ ⌘A ⌘C ⌘X ⌘V ⌘D · ⇧⌘E split at playhead · ⌘J glue · ⌘B duplicate one loop later · 0 mute (hollow, not played) · arrows nudge (⇧ = octave / bar, ⌥ = fine) · `[` `]` velocity · ⌥-wheel transposes (⌥⇧ octaves) · ⌘-wheel zooms at the cursor · ⇧-wheel scrolls · velocity lane: drag the stems, ⌥-drag draws a ramp · click the ruler to seek · drag the ruler's top strip for a loop region (right-click clears) · **follow** keeps the playhead in view · **fit** zooms to the pattern · **fold** shows only rows with notes (plus nearby chord tones) · **ghost** plays and draws another pattern behind yours, dashed, with its tiers and its own volume · hover a cell for a plain-words explanation · **tiers** tints rows by fit, **map** paints the whole progression · HARMONY → select by tier picks every chord tone / tension / dissonant note · NOTES → length chips and mute for the selection |
| Note properties | every note carries the performance set the piano roll's lane edits: **velocity · panning · fine pitch · release · mod X · mod Y · shift · repeats** (plus a colour group) · pick one in the lane under the roll, drag a stem to set it, drag across stems to paint, ⌥-drag to ramp a line through them, `[` `]` to nudge · **shift** moves only where the note sounds, so it keeps its grid cell and a tick marks where it belongs · **repeats** retriggers the note inside its own length, drawn as divisions in the note body · mod X is the note's filter cutoff and mod Y its resonance, both offsets from the track's own tone |
| Transform | inspector → TRANSFORM: quantize, humanize (chord tones stay tighter), legato, arp ↑/↓/⇅/random, strum, chop, ×2 slower / ½ faster, reverse, invert · Edit menu: transpose by semitone / octave, split, glue, duplicate-loop, mute · **magnet** pulls placed/dragged notes to the nearest chord tone · **stamp chord** places the chord under the cursor |
| Resolve / keep | select one dissonant note: the inspector says why, offers ↓/↑ resolve targets, or **keep** marks it deliberate (hatch stays, the "!" goes) |
| Chord lane | progressions live under HARMONY in the inspector, named by mood ("sad but hopeful") with the chords they write in your key; the list follows the key's scale · the progression is the song's, so the lane edits it wherever you are · drag a chord to move, its right edge to resize · click to free-build · right-click to delete · **chords** to hear the bed |
| Play live | **KEYS** tier: home row = chord tones, top row = tensions of the chord under the playhead, remapped as it moves · chrom: Z/Q rows chromatic · −/+ octave · **MIDI IN** picks a controller · **REC** (R) while playing writes what you play into the pattern, quantized to the grid |
| Key | place notes and the KEY cell offers "X? lock" · × unlocks · or pick root + scale under HARMONY · a section with its own key modulates the patterns inside it |
| Drums | the grid's rows are the track's own **kit**: each pad names a sound, ▾ swaps it, **+ add sound** adds a pad at the GM pitch that suits the sound (a snare on 38, an open hat on 46 sharing the closed hat's choke group) · left-click/drag adds hits, right-click removes · step follows snap (1/8, 1/16, 1/32) · the grid is one turn of the pattern · a step is a note, so its brightness is its velocity and it leans when shifted · click a row's name to aim the **graph editor** below the grid: it edits the same properties as the roll's lane (drag to set, ⌥-drag to ramp) · the tool row fills every 2/4/8 steps, clears, rotates ◀ ▶, randomises, or writes a **euclidean** rhythm from an onset count and rotation |
| **SONG** · arrangement | **tempo lane** under the ruler: double-click adds a tempo change at that beat, drag a point up/down for bpm (⇧ fine) and sideways to move it, ⌥-click makes it a ramp (a glide from the previous tempo), right-click removes; the transport clock and the engine follow the map, pattern mode plays at the base tempo · each track has its own lane and plays its own patterns · drag a pattern from the tree onto a lane, or ＋ on a pattern to append a clip · drag a clip to move (snaps to bars, ⇧ for beats), its right edge to trim or loop-extend, ⌥-drag to copy, right-click to remove, double-click to edit · select clips and move or delete them as a group, **unique** forks a clip's pattern · the song has a hard end you set in SETUP, and clips are held inside it · double-click the ruler's top strip to add a section; click it to name it, give it a key or color; drag to move; right-click to remove · track headers: M/S, volume slider, meters · click the ruler to seek |
| **GTR** · guitar rig | the rack runs input strip → pedals → amp → cab → rack, and the amp is four separate choices rather than one preset: **model** across the top sets all of them at once (tweed combo, blackface, british blues, vintage stack, modern stack, boutique, high octane, pawnshop fuzz) and each click is one undo step · **amp** picks the preamp voicing, **eq** picks which passive tone stack the bass/mid/treble knobs drive (us classic, british, british bright, boutique) — the knobs load each other the way the circuit does, so closing the mid also moves the treble · **cab** picks the speaker and **mic** the microphone (dynamic, condenser, ribbon) · **pos x** walks the mic from the cone's edge to the dust cap, dark to bright, and **pos z** from against the grille (proximity bass, tight attack) to backed off · a **direct** cab is a DI sound and dims the mic controls, because there is no speaker to point one at |
| **SOUND** · sound design | a browser over 60 factory synth sounds and 55 drum sounds plus everything you have saved, grouped by character (basic, pad, pluck, bass, lead, keys …) · the **synth editor** is the patch the engine plays: two oscillators with detune and shape, a filter with its own envelope, two LFOs you can aim at pitch, cutoff or level, chorus and echo, drawn as an envelope view and a live scope · the **drum editor** shapes body, noise, metal and click per sound · play the audition keys to hear it, **randomize** or **mutate** to find one, then save it and use it on the track · the library is global (it follows you between projects); a sound a project uses that is not in your library travels inside the project file |
| Sounds on a track | inspector → INSTRUMENT: a synth track picks a sound (or a CLAP plugin) and **edit ↗** opens it on the SOUND page; a drum track shows its kit |
| Bottom panel | **DEVICES**: instrument, **inserts** (the track's effect chain), track bus (vol / pan / tone / reverb), **master inserts**, master (gain, cuts, 3-band EQ, reverb) · **MIXER**: faders, pan, meters, M/S · **PLUGINS**: the free-plugin browser |
| Plugins | a track's instrument picker lists the built-in voices, then every CLAP instrument found on this computer; picking one replaces the voice and its parameters appear below (drag to hear, release for one undo step) · the **inserts** card holds up to 8 CLAP effects per track (and on the master), each with bypass, ▲▼ reorder and its own parameter knobs — a track runs instrument → tone → inserts → gain → pan → reverb send, so inserts are pre-fader, and the master's run before its own strip · a project whose plugin is not installed keeps the name and says so instead of silently swapping it · CLAP only, because CLAP is MIT like the rest of this project and every Rust VST3 binding is GPL-3.0 — the host is format-agnostic inside, so another format is additive |
| Plugin browser | **PLUGINS** in the bottom panel: a curated list of genuinely free plugins, searchable by what they sound like ("pad", "lo-fi", "drums") rather than by jargon · **install** downloads, checks the pinned SHA-256, unpacks and drops the bundle in your plugin folder, then rescans — a checksum that does not match installs nothing · plugins that only ship a `.dmg`/`.pkg` cannot be auto-installed and offer **open page** instead |
| Transport | space play/stop · Enter return to start · L loop · R record · M metronome (count-in and pre-roll when record is armed; volume in Settings) · BPM: click to type (↑↓ ±1, ⇧ ±10), drag up/down, scroll, double-click resets, ÷2 ×2 · tap tempo (last 8 taps) · SWING 50–75 on 8ths or 16ths (the roll shows where swung notes land) · SIG time signature · ⌘Z/⇧⌘Z undo/redo (the Edit menu names the step) · ⌘S save · ⌘O open · ⌘E export WAV (offline render, faster than real time) · ⌘, settings · ⌘Q quit |
| Files | **⌘E** export audio: song or pattern, loop count, chord bed, **stems** (one WAV per track), rate / depth / dither / normalize / tail, with a progress strip · File → Export MIDI (one MIDI track per track, chord names as markers) and Import MIDI into the selected pattern (new tracks, one undo step) · File → New from a vibe or template / Save as template (templates live in the app data folder) · unsaved changes are prompted on close, new and open · autosave every 30 s while dirty (`<project>.autosave.json`, restore offered on launch) · File → recent files · drop a `.dissonant` on the window to open it · the engine restarts by itself when the output device changes or disappears |
| Settings (⌘,) | **audio**: output device, sample rate, buffer size with latency, test tone, engine load / xruns, metronome volume, count-in, pre-roll · **midi**: default input with auto-reconnect, velocity curve, channel, octave · **editing**: default grid / velocity, audition on place, ⌥ = no-snap or paint, confirmations, new-project defaults · **export**: rate, 16/24/32-bit, dither, normalize, tail · **appearance**: UI scale, row height, reduced motion, accent and tier colors (color-blind-safe preset) |

The look follows the UI handoff in `docs/design/2026-10-02-ui-handoff-violet.md` (violet accent, IBM Plex
Sans Condensed + JetBrains Mono, bundled). Controls only exist for features that are built; the
metronome, count-in, tools, ghost notes, CC lane, browser and plugin devices from the mock arrive
with their features.

Projects are JSON (`.dissonant`). The format is documented by `crates/dissonant-core/src/model.rs`.
Musical time is **integer ticks** at 960 per quarter note (`crates/dissonant-core/src/ticks.rs`), so
quantizing, loop tiling and note repeats are exact; audio keeps its own seconds. A file written
before schema 9 carried beats and is converted on open.

## Roadmap

The plan lives in `docs/brainstorms/2026-10-02-full-daw-brainstorm.md`; `docs/decisions/` explains
the stack change. Status against it:

| Phase | Done | Open |
|---|---|---|
| 0 Foundations | audio-clock scheduler, render-block synth, command bus + undo (labelled), offline render, pan, loop region, time signature, legacy import, tempo map with ramps, swing, metronome + count-in, settings, autosave, device-loss recovery, integer ticks (960 PPQ) | — |
| 1 Roll | full editing, note property lanes (velocity, panning, fine pitch, release, mod X, mod Y, shift, repeats), zoom, transform tools, magnet, stamp, resolve/keep, explain-this-note, live tier keyboard, MIDI in + record, ghost notes, fold, per-note mute, select by tier, property ramp, split / glue / duplicate-loop | per-note probability / slide |
| 2 Arrangement | track-owned patterns with a track → pattern tree and drag-to-song, clips with trim / loop-extend / offset, group moves, unique-ify, a hard song end, sections with per-section key, tempo lane, MIDI file export / import, templates, vibes, stems | ripple edits, variation generator |
| 3 Mix & devices | track bus, master chain, mixer, meters, CLAP instrument hosting (scan, params, state, project round-trip), effect insert chains per track and master (CLAP effects, bypass, reorder, undo, project round-trip) | plugin GUIs, native effects, automation, sidechain, inserts in offline export |
| 4 Instruments | patch-driven synth (oscillators, filter, LFOs, chorus, echo) and drum engine, a 115-sound factory library with a global user library, the SOUND page (browser, synth and drum editors, audition, randomize / mutate), editable per-track drum kits with choke groups, step graph editor, euclidean + fill / rotate / randomise step tools, free-plugin browser with checksum-pinned install | sampler, modulation matrix |
| 5 Audio | audio tracks, import (wav/aiff/flac/mp3/m4a), DI recording with monitoring, native rig: tabulated-triode preamp with grid conduction, passive tone stacks, convolved cabinet with a morphed mic position | warp, comping, audio → notes / chords |
| 6 Harmony | tiers, key detection, starters, free-build chords | reharmonize from melody, function-preserving transpose, voicing engine, next-chord suggestions |
| 7 Jam & polish | — | session view, modulators, command palette, layouts |
