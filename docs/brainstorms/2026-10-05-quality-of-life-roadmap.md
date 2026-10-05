---
name: quality-of-life-roadmap
status: draft
created: 2026-10-05
origin: docs/brainstorms/2026-10-02-full-daw-brainstorm.md
---

# Quality-of-life roadmap: settings, audio, tempo, note editing

The big roadmap (`2026-10-02-full-daw-brainstorm.md`) is about capabilities. This one is about
friction: the hundred small things that decide whether Dissonant feels like a tool someone
reaches for every day or a demo they admire once. Everything here is scoped against the code
as of branch `feat/phase-0-1` (schema v4).

Sizing: **S** = an hour or two, one file or two · **M** = half a day, touches Rust + UI ·
**L** = a day or more, new subsystem. Priority: **now** (next sprint), **next**, **later**.

---

## 0. The gaps that bite first (do these before anything else)

| # | Item | Why it matters | Size |
|---|---|---|---|
| 0.1 | **Unsaved-changes prompt on close** | Tauri closes the window instantly; a dirty project is lost. Hook `WindowEvent::CloseRequested`, ask save / discard / cancel. | S |
| 0.2 | **Window title = file name + •** | Standard desktop expectation; the menu bar shows it but the OS title bar says "dissonant". `core:window:allow-set-title` is already granted. | S |
| 0.3 | **In-app dialogs instead of `confirm()` / `alert()`** | Native `confirm` looks foreign in a dark DAW and blocks the playhead thread's UI. One modal component, styled like the popover. | S |
| 0.4 | **Autosave + crash recovery** | Write `<project>.autosave.json` every N seconds while dirty (default 60 s), offer to restore on launch. | M |
| 0.5 | **Recent files** (File menu + first-run screen) | Tauri app-data JSON list; drag-and-drop a `.dissonant` onto the window to open. | S |
| 0.6 | **Audio device loss** | If the output device disappears (headphones unplugged), restart the engine on the new default and toast it, instead of going silent until the dot is clicked. | M |
| 0.7 | **Undo labels** | "Undo move notes" beats "Undo". Commands already carry enough information; `Document` keeps a label per snapshot, the Edit menu shows it. | S |

---

## 1. Settings

Nothing is persisted outside the project file today. A single `settings.json` in the Tauri
app-data dir, loaded at startup, edited through a **Settings** window (⌘,), with these tabs.

### 1.1 Audio
- **Output device** picker (cpal `host.output_devices()`), with "system default".
- **Sample rate**: list the rates the device supports (`supported_output_configs`), choose
  44.1 / 48 / 88.2 / 96 kHz. Today the engine takes the device default and reports it.
- **Buffer size**: 64 / 128 / 256 / 512 / 1024 frames, with the resulting latency shown in ms.
  cpal `BufferSize::Fixed(n)`; fall back to default if the device refuses.
- **Test tone** button, **restart engine** button, live CPU-ish readout (render time / block
  time) in the status bar.
- **Export defaults**: sample rate (44.1 / 48 / 96), bit depth (16 / 24 / 32-float), dither on
  16-bit, normalize to -1 dBFS or to a LUFS target, tail seconds, "render loop N times".

### 1.2 MIDI
- Default input device, **auto-reconnect** when it reappears (midir port scan every few s).
- Channel filter (all / 1–16), **velocity curve** (linear / soft / hard / fixed 100), octave
  transpose, "MIDI thru to typing-keyboard track".
- **MIDI learn** for transport (play/stop/record) — the first step toward controller mapping.

### 1.3 Editing defaults
- Default grid, default note length, **default velocity** (currently hard-wired to 100),
  default pattern length, default tempo and time signature for new projects.
- "New project opens the starter / an empty project / last template".
- Snap behaviour: snap on/off default, **snap to chord changes** (magnetic to chord
  boundaries in the lane), "⌥ bypasses snap" vs "⌥ paints" (see 4.2).
- Audition on placement on/off; audition velocity; audition length.
- Confirmations on/off (clear notes, delete track, delete pattern).

### 1.4 Keyboard
- **Shortcut presets**: Dissonant / FL / Ableton / Logic, plus a per-action override table.
- Typing keyboard: base octave, velocity, layout (QWERTY / AZERTY / QWERTZ), and whether
  KEYS swallows letters globally or only when the roll has focus.
- A **cheat-sheet overlay** (hold `?`) generated from the live binding table, so the help
  strings in the README and the toolbar's `?` stop drifting from the code.

### 1.5 Appearance
- **UI scale** (90–150 %), row height (14 / 18 / 22 px), beat width default.
- Accent color and the three **tier colors**, with built-in color-blind-safe presets
  (deuteranopia / protanopia / tritanopia). The non-color cues are already there; this makes
  the colors adjustable too.
- Meter ballistics (peak hold time, fall rate), **reduced motion** (static playhead redraw at
  10 Hz instead of 60, no glows), font size.
- Show / hide: velocity lane, chord lane, status bar, inspector sections (collapsible with
  remembered state).

### 1.6 Per-project UI state (saved in the `.dissonant` next to the model)
Zoom, scroll position, selected track / pattern, mode, bottom panel tab, loop region,
inspector section collapse state, window size and position. Reopening a project should look
exactly like leaving it.

---

## 2. Audio & engine

| Pri | Item | Notes | Size |
|---|---|---|---|
| now | Sample rate + buffer size selection | See 1.1. Needs `AudioDevice::start(config)` to take a requested config and report the actual one. | M |
| now | **Metronome** | Click on beats (accent on bar), volume, on/off during play and record, **count-in** (1 / 2 bars) before recording. Procedural click in the engine, scheduled like notes. | M |
| now | **Pre-roll** when recording | Start N bars before the loop/playhead so the first note isn't clipped. | S |
| next | Export options dialog | Format, rate, depth, dither, normalize, tail, loop count, **stems** (one WAV per track, solo-rendered offline). | M |
| next | Render-in-place a pattern to audio preview | For checking a mix on the arrangement without playing; groundwork for freeze. | M |
| next | Engine stats | Render load %, xruns counter (cpal callback timing), shown in the status bar; a warning toast when xruns spike with a "raise buffer size" shortcut. | S |
| later | Per-track output latency compensation | Needed once devices/plugins add latency. | L |
| later | Master limiter on by default (switchable) | Beginners clip the master; a transparent brickwall at -0.3 dBFS avoids ugly exports. | M |

---

## 3. Tempo & time

| Pri | Item | Notes | Size |
|---|---|---|---|
| now | BPM field QOL | Arrow keys ±1 (⇧ ±10) when the BPM field is focused; double-click resets to 120; drag vertically on the LCD like a knob; decimals optional. | S |
| now | **Swing** | Global swing % (50–75) delaying even 8ths or 16ths; applied in the engine scheduler so it's audible, and drawn in the grid as shifted sub-beat lines. | M |
| now | Half / double time buttons | `÷2` `×2` on the BPM LCD; also "tempo-scale the selection" in TRANSFORM (notes ×2 / ½ speed, which the handoff listed). | S |
| next | **Tempo lane** (tempo map) | Tempo changes over the song with ramps. Engine: piecewise beats↔samples conversion; UI: a lane under the ruler in SONG mode. The biggest item in this doc. | L |
| next | Tap tempo polish | Averages the last 8 taps, shows the detected BPM live, "set" button vs auto-apply, resets after 3 s of silence (already) with a visible countdown. | S |
| next | Time signature per section | `Section.time_signature`, bar math follows sections in the arrangement ruler. | M |
| later | Tempo detection from a recorded loop | For the audio phase: estimate BPM from transients. | L |

---

## 4. Note editing QOL (the roll)

### 4.1 Selection
- **Double-click a key** selects every note of that pitch; **⌘-double-click** selects the pitch
  class in every octave.
- **Select by tier**: HARMONY section buttons "select all dissonant / tension / chord tones"
  in the pattern. Pairs with resolve/keep to review a pattern's rough spots one by one.
- Select next / previous note (Tab / ⇧Tab), select to end of pattern (⇧End).
- Marquee from a note with ⌘-drag (the handoff's convention) in addition to ⇧-drag on empty.
- Invert selection, deselect on Esc (exists), "select same length / same velocity".

### 4.2 Modifiers & drag behaviour
- **⌥ temporarily bypasses snap** while dragging (industry standard). Today ⌥ on empty space
  paints. Proposal: paint becomes a toggle in the toolbar (brush), ⌥ becomes "no snap",
  ⌥+drag on a note keeps "duplicate". Settings 1.3 lets people keep the old mapping.
- **⇧ constrains** a drag to one axis (pitch only or time only).
- Auto-scroll when dragging past the viewport edge; the viewport follows the playhead in
  play (page / smooth / off), with a FOLLOW toggle in the toolbar.
- **Wheel transposes** the selection (⌥-wheel ±1 semitone, ⌥⇧-wheel ±octave); ⇧-wheel scrolls
  horizontally; ⌘-wheel zooms around the cursor (zoom today is centered on the left edge).
- **Vertical zoom** (row height) with ⌘⌥±, "zoom to fit pattern", "zoom to selection".

### 4.3 Velocity & per-note properties
- **Velocity ramp tool**: drag across the velocity lane with ⌥ to draw a straight line
  through the stems (crescendo / decrescendo in one gesture).
- ⌥-drag vertically on a note itself to change its velocity without leaving the grid.
- Note **mute** (per-note flag, drawn hollow, skipped by the scheduler) — standard in FL/Ableton.
- Note **probability** (0–100 %, the scheduler rolls per repetition) and **condition**
  (play every Nth time) for generative variation; drawn as a small dice glyph.
- Per-note **release velocity** and **fine pitch** can wait for the synth that uses them.

### 4.4 Editing commands
- **Split at playhead** (⌘E on a selection), **glue** adjacent same-pitch notes (⌘J).
- **Duplicate pattern-length** (⌘B: copy the selection one loop later, the FL favourite).
- Transpose by semitone / octave through the menu with labels, not only arrows.
- **Set length** for the selection to a grid value (chips while notes are selected).
- Nudge by 1 px (⌥-arrows) for off-grid feel; "move to grid" (quantize exists).
- **Invert / reverse / mirror** the selection (the handoff's transform list) — reverse in
  time, invert around the selection's middle pitch, both tier-aware ("reverse but keep chord
  tones on the beat" as an option).
- "Legato to next on same pitch" vs "legato to any next note" switch.
- **Humanize amount dials** (timing ms, velocity %) instead of fixed values.

### 4.5 Seeing more
- **Ghost notes**: other tracks' notes drawn dashed and with their tiers (the handoff drew
  them); pick the ghost track in the toolbar (GHOST chip).
- **Fold**: hide rows that have no notes and aren't chord tones of any chord in the pattern,
  so a 73-row grid collapses to the dozen that matter.
- Bar numbers in the roll ruler follow the section the pattern lives in ("bar 17" not "bar 1").
- Chord names also drawn over the grid at the chord start (faint), not just in the ruler.
- **Minimap** strip under the ruler for long (8-bar+) patterns.
- Note count / density readout per bar in the status bar.
- Cursor shapes: resize cursor on a note's right edge, grab hand on a note, crosshair on empty.

### 4.6 Chord lane QOL
- Double-click an empty slot to add a chord there; ⌘-drag to duplicate a chord.
- **Split** a chord at the playhead; **merge** two adjacent identical chords.
- Chord name editing by typing ("Am7", "F#dim") with the parser filling pitch classes.
- Copy the chord track from another pattern (dropdown), paste a progression as text.
- Audition a chord on hover in the editor popover; "retrigger bed on edit".

### 4.7 Drum grid QOL
- Per-step velocity (click height or ⌥-drag), accent row, **roll / ratchet** per step.
- Row swing, per-row length (polymeter), kit row reordering, shift a row left/right.
- Audition a drum by clicking its name.

---

## 5. Patterns, song & files

- Pattern **colors** independent of track colors; pattern **notes count per track** on hover.
- "Make unique" on a clip (forks the pattern) and **consolidate** a run of clips into one pattern.
- Clip **split** at the playhead, **mute** toggle on the clip (model has it, UI doesn't yet).
- Snap clip resize to the pattern length, with ×n shown while dragging (shown after drop today).
- **Ripple delete / insert time** in SONG mode.
- Song **markers** jump: ⌘← / ⌘→ to previous / next section, clicking a marker name seeks.
- **Templates**: save the current project as a template; the new-project dialog lists them.
- **Export MIDI** (pattern or song, one track per MIDI track) and **import MIDI** into a pattern.
- Project **notes** field (free text in the inspector) and a project **BPM/key summary** in the
  file name suggestion ("untitled · 120 · Am.dissonant").

---

## 6. Polish & robustness

- Toast queue (several at once stack), error toasts with a "copy details" button.
- A **log file** (`dissonant.log`) with engine and MIDI events; "Help → Reveal log".
- First-run screen: new starter / open recent / open file, with the three-tier legend.
- Status-bar hints that change with context (hover a tool → what it does).
- Performance: cull notes outside the viewport in the roll and the velocity lane (needed once
  patterns pass ~2000 notes); throttle playhead redraws to the display refresh rate.
- Accessibility: focusable notes with VoiceOver labels including tier, high-contrast mode,
  every control reachable by keyboard; honor the OS "reduce motion" flag.
- Windows/Linux specifics: Ctrl instead of ⌘ in every label and tooltip (today the help text
  says ⌘ everywhere), menu accelerators, proper file-type association for `.dissonant`.

---

## 7. Suggested order

1. **Section 0** in full (one sprint; the close prompt and autosave stop data loss).
2. Settings window with Audio + Editing defaults (1.1, 1.3), sample rate / buffer size.
3. Metronome + count-in + pre-roll, swing, BPM field QOL.
4. Roll: ⌥-bypass-snap, ⇧ constrain, wheel transpose, follow playhead, select-by-tier,
   velocity ramp, split / glue / duplicate-loop, note mute.
5. Ghost notes and fold.
6. Export options + stems, recent files, templates, MIDI export.
7. Tempo lane.

Each step is independent of the big roadmap's Phase 3+ work and can ship between them.
