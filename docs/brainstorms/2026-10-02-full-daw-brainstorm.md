---
name: full-daw-brainstorm
status: draft
created: 2026-10-02
origin: docs/brainstorms/2026-06-12-daw-maturation-requirements.md
---

# Dissonant → a Fully-Featured, Harmony-Native DAW

A large-scale brainstorm. The goal is not to clone Ableton Live or FL Studio, but to reach
their depth on the things a producer actually touches every session (piano roll, arrangement,
mixer, devices, recording) while making Dissonant *better* at the one thing neither of them
does: understanding harmony and putting that understanding under every tool.

This doc is deliberately wide. Section 13 narrows it into a phased roadmap; Section 0 is an
honest audit of where the codebase stands so the roadmap is grounded in what's real.

---

## 0. Where we actually are (audit of the code, 2026-10-02)

~3.6k lines of Swift across `DissonantCore` (pure model + theory, well tested) and the
`Dissonant` app (SwiftUI + AudioKit). What's genuinely there:

| Area | State | Notes |
|---|---|---|
| Three-tier classifier, key detection, highlight engine | **Solid** | Pure, tested, the product's core. Major/minor only, triads only. |
| Piano roll | **Prototype** | Hand-rolled `Canvas`. Place on click, erase on right-click. **No move / resize / select / velocity / zoom / copy / undo.** Notes render as one SwiftUI view each (`ForEach`) — will fall over past a few hundred notes. |
| Chord lane + editor | **Prototype** | 3 starters, free-build editor. **Hardcoded to C major** (`ChordLaneView.keyRoot = 0`, `ChordEditorView.swapToNextDiatonic`) — ignores the locked key. Chords can't be dragged, resized, split or added individually. |
| Patterns + song | **Basic** | Patterns (2/4/8 bars) with per-pattern chords and per-track notes; song is a flat `[UUID]` list of patterns played back-to-back. No clips, no lanes, no markers, no overlap. |
| Drums | **Basic** | 4 procedural one-shots (kick/snare/hat/clap), 8th-note step grid only. No velocity, no swing, no samples. |
| Instruments | **Basic** | 8-voice oscillator + ADSR built from AudioKit `Oscillator` + `AmplitudeEnvelope` nodes (one node pair per voice). Out-of-process AU instrument hosting works, with the plugin's own editor in a window. AU choice is **not persisted**. |
| Mixer | **Basic** | Per-track bus: low-pass → reverb → fader. Master: HPF → 3-band EQ → LPF → reverb → fader. **Pan is inert** (field exists, no node). Master FX values live in `@State` — **not saved**. No meters, no sends, no inserts, no AU *effects*. |
| Transport | **Prototype** | Playhead advanced by a **60 Hz wall-clock `Timer`**, scaled by tempo. `AppleSequencer` is started but nothing is scheduled into it. Note-ons fire from the UI tick → up to ~16 ms jitter, drift under load, no swing possible, fixed velocity 90. |
| Export | **Prototype** | Real-time tap on the master → WAV, stopped by `asyncAfter`. Can't render faster than real time, no stems, no MIDI. |
| Persistence | **Good** | JSON `FileDocument` with tolerant decoding + v1→v2 migration. |
| Undo | **None** | All edits mutate the model directly. |
| Architecture | **Strained** | `ContentView` (622 lines) owns the engine, transport, every mutation and every piece of chrome. Views call `trackVoices?.setVolume(...)` directly rather than the engine mirroring the model. |

Concrete defects found while reading (worth fixing regardless of roadmap):

- Deleting a track never removes its audio entry — `TrackVoices.sync` only adds, so buses leak.
- Switching a track's synth (`setSynth`) adds a new node to the bus without detaching the old one.
- Starters and "next ▸ diatonic" ignore the project key (hardcoded C major).
- Export duration is a `DispatchQueue.asyncAfter` guess; an underrun during the bounce lands in the file.
- "clear" wipes a track's notes with no confirmation and no undo.
- Window minimum is 1380 px wide, which excludes 13" laptops at default scaling.

The previous brainstorm (`2026-06-12-daw-maturation-requirements.md`) scoped M1–M3 (note
editing, mixing, finishing). Everything in it still applies; this doc absorbs it and goes much
further.

---

## 1. The thesis: what "better than Ableton / FL" means *for this product*

Ableton and FL are 20-year-old codebases with hundreds of engineer-years in them. Chasing their
feature lists head-on loses. The winning frame:

> **Reach parity on the daily-driver surfaces, then be the only DAW that understands harmony.**

What each incumbent is actually loved for, and what we take from each:

| From FL Studio | From Ableton Live | Only Dissonant |
|---|---|---|
| The piano roll: stamp, chop, strum, arpeggiate, ghost notes, scale highlight, slide notes, per-note velocity/pan lanes, the *feel* of drawing music fast | Session view (launch clips, jam, capture the performance into the arrangement) | A chord track that is a first-class timeline object every tool consults |
| Pattern → playlist model (we already have the seed of this) | Device chains, racks, macro knobs; everything is a device | Three-tier guidance under every editor: roll, step sequencer, chord editor, MIDI input, audio-to-MIDI |
| Channel rack / step sequencer with per-step everything | Warping; audio is as malleable as MIDI | Reharmonize-from-melody (you hum it, we propose the chords) |
| Mixer with free routing and sidechain everywhere | Automation + clip envelopes that just work | Function-preserving transpose (move a riff to a new chord, keep its *role*) |
| Riff machine / randomizers | Capture MIDI (never lose the jam) | Explain-this-note in plain language |

Every item below passes one gate, carried from `PRODUCT.md`: **does it shorten the path from a
blank project to a finished track for a player who doesn't have the theory?** Pro depth is
welcome when it's made legible; feature-parity-for-its-own-sake is not.

---

## 2. Foundations (do these first or pay for them forever)

These are not features. They are the floor every feature in Sections 3–10 stands on, and
several of them are *blocking* for anything timing- or edit-related.

### 2.1 Sample-accurate sequencing engine

**Problem.** The playhead is a UI timer. Note-ons fire from `onChange(of: positionBeats)` at
~60 Hz. That caps timing accuracy at one video frame, drifts under UI load, makes swing/groove
impossible, and ties export to real time.

**Target.** An audio-clock-driven scheduler:

- A **lookahead scheduler** (~100–200 ms window) that reads the engine's output sample time
  (`outputNode.lastRenderTime` or a render-tap-derived clock) and emits *sample-stamped* events.
- **AU instruments** take sample-stamped MIDI via `scheduleMIDIEventBlock(sampleTime, …)` —
  the hosting layer already uses that block with `AUEventSampleTimeImmediate`; switch to real
  timestamps.
- **Drums** take `AVAudioPlayerNode.scheduleBuffer(at: AVAudioTime)` with sample-exact times.
- **Native synths** must move off node-per-voice AudioKit graphs onto a **render-block design**
  (`AVAudioSourceNode` or a custom in-process `AUAudioUnit`) that consumes a lock-free event
  queue and renders with sub-buffer accuracy. This is the one unavoidable rewrite in the
  instrument layer and it unlocks everything (velocity, glide, per-note modulation, swing).
- **Time in integer ticks** (e.g. 960 PPQ, `Int64`) in the model instead of `Double` beats.
  Kills float drift in long songs and makes equality/snap trivial. Convert at the UI edge.
- **Tempo map + time signature** as first-class model objects (tempo ramps, 3/4, 6/8, 7/8).
- The UI playhead becomes a *reader* of the audio clock (display-link polling, as the original
  plan intended) rather than the *source* of truth.

**Payoff.** Tight timing, swing/groove templates, humanize, offline export, stems, MIDI
recording with correct timestamps, tempo changes.

### 2.2 Offline rendering

`AVAudioEngine.enableManualRenderingMode(.offline, …)` renders faster than real time,
deterministically, without a tap. Needed for: export (whole song, loop, selection), **stems**
(solo each track, render, repeat), freeze/bounce-in-place, and "render pattern to audio clip".
Also fixes the asyncAfter-guess bug.

### 2.3 Command bus + undo/redo + store

Replace direct `document.model.x = y` mutation from views with a `ProjectStore` that accepts
**intents** (`moveNotes`, `setChord`, `addClip`, `setParam`) and produces inverse intents for
`UndoManager`. Views dispatch; the store mutates; the engine *mirrors* the model.

- Every edit becomes undoable for free, including the chord/pattern/track edits that exist now.
- Transactions group a drag into one undo step.
- The audio layer subscribes to diffs (track added → build bus; param changed → set node). No
  more `trackVoices?.setVolume(...)` sprinkled through views.
- Deterministic, headless tests of "do X then undo" against the model.
- Later: macro recording, scripting, remote control, collaboration are all "replay intents".

### 2.4 Schema v3

Grow the model in one coordinated migration rather than piecemeal:

- `NoteEvent`: `velocity`, `releaseVelocity`, `tick`/`lengthTicks`, `microOffset` (for groove),
  `probability`, `condition` (every Nth loop, FL-style), `muted`, `slide` (FL slide note / pitch
  glide target), `colorGroup`, and an `intentional` flag (see 3.3 — "yes, I meant that
  dissonance").
- `Track`: `kind` (`instrument | drum | audio | bus | return | master | chord`), `color`,
  `inserts: [DeviceInstance]`, `sends: [Send]`, `pan`, `inputSource`, `armed`, `monitoring`,
  AU `fullState` blob so plugins come back with their presets.
- `Clip`: a pattern *instance* (or audio clip) on a lane with `start`, `length`, `loopOffset`,
  `transpose`, `gain`, `muted`. The playlist is `[Lane: [Clip]]`, not `[UUID]`.
- `ChordEvent`: `bassPitchClass` (slash chords), `inversion`/`voicing`, `extensions`,
  `romanNumeral` (relative to section key), `function` tag, `isSuggested` (from detection,
  awaiting confirmation).
- `Section`/`Marker`: name, start, `key` (per-section modulation), color.
- `AutomationLane`: target param path + `[Breakpoint(tick, value, curve)]`, both clip-scoped
  and arrangement-scoped.
- `MasterSettings` persisted (today's master FX are `@State`).
- `TempoMap`, `TimeSignature`.

### 2.5 Audio graph that can change while running

- Make `pan` real (`AVAudioMixerNode.pan` on the track output, or AudioKit `Fader` stereo).
- Insert chains as ordered node lists that can be rebuilt without a full engine stop (AVAudioEngine
  can `connect`/`disconnect` live if the node is detached from a quiescent path; otherwise do the
  swap inside a short mute + crossfade).
- Return/bus tracks, pre/post-fader sends, sidechain-capable routing (a device can take a second
  input bus).
- **Plugin delay compensation** using `AUAudioUnit.latency`.
- Tear-down on track delete (fixes the current leak).

### 2.6 MIDI input (CoreMIDI, optionally MIDIKit)

Play the selected track live from a controller; record into patterns (overdub / replace /
loop-record with takes); input quantize; **MIDI learn** on any parameter; sustain pedal; MPE
later. Capture-MIDI buffer (Ableton's "I wasn't recording but that was good" — the last 60 s of
input is always kept).

### 2.7 Rendering performance

The roll draws every note as a SwiftUI view. Move notes, grid, automation and waveforms to
`Canvas`/CoreGraphics with culling (only draw the visible range), and plan a Metal path for
waveforms if needed. Target: 10k notes on screen at 60 fps while scrolling.

---

## 3. The piano roll (FL-grade, then beyond)

The roll is where the persona lives. FL's is the benchmark; Dissonant's must *feel* as fast and
know more.

### 3.1 Table stakes (absorbs M1 of the previous brainstorm)

- Modeless direct manipulation: drag body = move (pitch + time, grid-snapped, auditions as it
  goes), drag right edge = resize, drag left edge = trim start, drag empty = draw, shift-drag
  empty = marquee select, right-click = erase, alt-drag = duplicate.
- Multi-select; move/transpose/stretch/delete selection; ⌘C/⌘V/⌘D; paste at playhead.
- Keyboard: arrows nudge by grid, shift+arrows transpose octave, ⌘A, delete, ⌘Z/⇧⌘Z.
- **Velocity lane** under the roll (drag bars, draw ramps, scale selection); velocity shown as
  note opacity/brightness too. Later: per-note pan / fine-pitch / mod lanes (FL's "note
  properties" lanes).
- Horizontal + vertical zoom (pinch, ⌘-scroll), scroll, follow-playhead modes (page / smooth / off).
- Snap grid: 1/1 … 1/64, triplets, dotted, "none", plus **swing amount** on the grid.
- Loop region brace in the ruler; set loop to selection; play from click in ruler.
- Note mute (greyed, still editable), note color groups (FL), lock a group.
- Mini-keyboard gutter is playable (click = audition, drag = glissando).

### 3.2 FL power tools (and how each gets a harmony upgrade)

| FL tool | Dissonant version |
|---|---|
| Stamp / chord tool | Stamps the **current chord** (from the chord track at the cursor), with a popover for inversion / spread / add 7th — every stamped note is a chord tone by construction |
| Arpeggiate | Arp patterns (up/down/random/order) over the selection *or* over the chord track: "arpeggiate the chord bed into this track" |
| Strum | Timing + velocity ramp across stacked notes (down/up/alternate) |
| Chop | Split notes into a rhythm; chop-to-grid |
| Quantize | With strength %, swing, and "keep chord tones on strong beats" option |
| Randomize / Riff machine | **Guided riff machine**: generates melodies that sit on solid/tension tiers by default, with a "weird" dial that admits dissonance on purpose |
| Legato / glue / flam | Standard |
| Scale highlighting | Already better (per-chord tiers). Add FL's "snap to scale" as a *magnet*, never a wall (see 3.3) |
| Ghost notes | Show other tracks' notes greyed **with their tiers** — see how your bass and lead agree |
| Slide notes | Pitch glides between notes (needs the render-block synth) |

### 3.3 Dissonant-only superpowers in the roll

These are the "much better" part. All of them honor **guidance, never guardrails**.

1. **Tier magnet (modifier-held).** Hold a key while drawing or dragging and the cursor is
   pulled toward solid rows, then tension rows. Release the key and it's free. Never a block.
2. **Function-preserving transpose.** Alt-drag a selection vertically and notes move *by chord
   role*, not semitones: a 3rd stays a 3rd when the chord changes under it. Also: "move this riff
   to the next chord" as a command. This is how a beginner reuses one good idea across a whole
   progression.
3. **Resolve / keep.** Select a flagged dissonant note → inline chips: "resolve down to G",
   "resolve up to B", "keep it (mark intentional)". Marking intentional sets `intentional=true`:
   the hatch stays (information), the "!" flag calms down (no nagging). The track's dissonance
   count then distinguishes deliberate from accidental, which is the whole product thesis made
   into a data point.
4. **Explain this note.** Hover/⌥-click: "F over Cmaj7 — a ♭9 against E, the harshest tier. It
   wants to fall to E." Plain words, no jargon required, theory names available.
5. **Live tier keyboard.** The "playable now" readout becomes a *playable* keyboard: QWERTY
   (and MIDI controllers in "guided" mode) maps to the current chord's solid and tension notes,
   remapping as the playhead moves. A beginner can jam in-harmony on laptop keys with no theory.
   Record it straight into the pattern.
6. **Ghost chord bed.** Faint silhouettes of the chord-track voicing drawn in the roll, so you see
   what you're writing *against*.
7. **Voice-leading hints.** Select a note at a chord boundary → thin arrows to the nearest chord
   tones of the next chord. Optional; off by default.
8. **Humanize by tier.** Randomize timing/velocity, but weight chord tones onto strong beats and
   tensions onto weak ones.
9. **Melody readout strip.** Range, contour, note-density, "you're circling the 5th a lot",
   "the chorus never touches the root" — observations, not corrections.
10. **Harmonic landscape v2.** Keep the whole-progression map; add intensity gradient for how far
    each cell is from a chord tone, and a per-bar "spice meter" summary above the roll.

---

## 4. The chord track as the DAW's harmonic spine

Today the chord track is a per-pattern lane with three starters. It should become the thing
that makes Dissonant a different *kind* of DAW: an object on the timeline, like the tempo map,
that every tool can ask "what's the harmony at tick T?" and "what's the key here?"

- **Key-aware everything.** Fix the hardcoded C major. Starters, diatonic swaps, suggestions all
  derive from the locked (or per-section) key. Per-section **modulation**: a chorus in the
  relative major, a bridge up a step.
- **Chord vocabulary.** 7ths, sus2/sus4, add9, 6ths, slash/bass notes, dim/aug, borrowed chords
  (modal interchange), secondary dominants, tritone subs, Neapolitan. Each carries a **spice
  label** in the same three-tier voice (solid / spicy / weird), so a beginner can reach for a
  borrowed iv knowing what it is.
- **Scales/modes.** Modes, harmonic/melodic minor, pentatonic, blues, whole-tone. The classifier
  generalizes (chord tones; in-scale-not-adjacent = tension; adjacent or out-of-scale =
  dissonance). Exotic scales are where the persona's music lives.
- **Next-chord suggestion** ranked by a transition table learned from a corpus of common
  progressions (I→IV, V→I, vi→IV…), with a **surprise dial** that re-ranks toward the unusual.
  Audition on hover.
- **Reharmonize from melody.** Select melody notes → "suggest chords that make these chord
  tones". Reverse of the current flow and the single most persona-aligned feature on this list:
  the player hums a line, the tool proposes the harmony.
- **Voicing engine.** Inversions, drop-2, spread, close; minimize voice-leading distance between
  consecutive chords; assign a bass note. Drives the bed *and* the stamp tool.
- **The bed becomes an instrument.** Rhythm patterns (pad / strum / arp / stabs / Rhodes
  comping), humanize, its own device chain. It's a real track you can mute, mix and bounce.
- **Direct manipulation.** Drag chords to move/resize, double-click a gap to add, split, right-
  click to swap; chord lane visible across the *whole song* in the arrangement view, not just
  per pattern.
- **Import.** Type `Am F C G`, drop a MIDI file, paste a chord sheet. Later: from audio (Section 8).
- **Cadence / phrase awareness.** Mark phrase ends; suggestions prefer cadential moves there.

---

## 5. Arrangement: pattern/playlist (FL) + session launch (Ableton)

Keep patterns as the unit of creation (that's FL's great idea and it's already here), and put a
real timeline and a real jam surface around them.

### 5.1 Playlist (replaces `arrangement: [UUID]`)

- **Lanes of clips.** A clip is a pattern instance with its own start/length/loop-offset/
  transpose/mute, or an audio clip. Clips overlap across lanes; patterns play in parallel.
- Clip operations: split at playhead, trim, loop-extend by dragging the right edge, consolidate,
  **make unique** (fork a pattern), duplicate, "render to audio".
- **Sections/markers** (verse / chorus / bridge) with color, loop-section, jump-to, and the
  per-section key from Section 4. The whole-song chord lane sits in the ruler.
- Tempo + time-signature lanes.
- Arrangement-level **automation lanes** (Section 6).
- Multi-lane marquee, ripple delete/insert time, duplicate-section.

### 5.2 Session / launch view

A grid: tracks across, pattern slots down, **scenes** as rows. Click a slot to launch it on the
next bar (quantized); launch a scene to fire a row. Hit record and the launches are **captured
into the playlist** as clips. Harmony-aware twist: a slot shows a small tier badge for how its
content fits the chord of the currently-playing chord clip, so you can see which patterns will
sit well together *before* you launch them.

### 5.3 Variation & structure helpers

- **Variation generator**: from pattern A make A' (same contour, different rhythm / inverted /
  retrograde / re-voiced to a new chord), harmony-aware.
- Song skeletons: drop an "intro–verse–chorus–verse–chorus–bridge–chorus" structure as empty
  sections with lengths and suggested keys, then fill.
- Arrangement follow actions: "after this clip play X" for generative jams.

---

## 6. Mixing, devices and automation

### 6.1 Mixer view

Channel strips (fader, pan, meters with peak/RMS/LUFS, mute/solo/arm, inserts, 2–4 sends, output
routing), **group buses**, return tracks, master with a limiter and LUFS readout. Narrow/wide
strips, fold to selected, color-coded. Keyboard-operable.

### 6.2 Device chains (Ableton's model)

Every track has an ordered chain. Devices are native or hosted AU **effects** (we only host AU
instruments today). Drag to reorder, bypass, hot-swap, save chains as presets.

**Native device set** — small, characterful, on-brand ("tape grit"):

- EQ (8-band parametric with analyzer), Compressor (with external sidechain + mix knob),
  Gate/Expander, Limiter, Transient shaper
- Saturator / Tape (wow, flutter, hiss, head-bump — this is the brand), Bitcrusher / Lo-fi
  (sample-rate reduce, vinyl noise, dust)
- Delay (tape / ping-pong / grain), Reverb (plate / room / shimmer), Chorus / Flanger / Phaser
- Filter (with drive + envelope follower), Utility (gain / width / phase / mono)
- **Mix guidance device** (see 6.5)

### 6.3 Racks and macros

Group devices into a rack with **8 macro knobs** mapped to many parameters at once. Instrument
racks (layer/split synths), effect racks (parallel chains). Macros are what make presets playable
for a beginner: one knob labeled "dust", one labeled "wide".

### 6.4 Automation and modulation

- Automation lanes on **every** parameter (track, device, AU via `AUParameterTree`, chord bed,
  tempo). Breakpoints with curve tension; freehand draw; record from knob moves (latch/touch);
  clip envelopes vs arrangement automation.
- Light **modulation**: LFO / envelope-follower / random devices that map to parameters with
  depth. (Deep modulation matrices were ruled out before; a few modulators with a simple map
  stay legible and give the persona the movement their music needs.)
- Sidechain routing (kick → bass compressor) as a first-class, discoverable thing.

### 6.5 Mix guidance (the thesis applied to the mix)

Same voice as the three tiers: observations, never auto-fixes.

- Frequency-collision view: two tracks fighting in 200–400 Hz, shown as a shared band.
- Gain staging flags (clipping into a device, headroom on master).
- Loudness target readout (streaming −14 LUFS) with "you're 6 dB under / 3 dB hot".
- "Masking" hints between bass and kick, vocal and pad.

---

## 7. Instruments and sound

### 7.1 Synth

Replace the 8-voice osc+ADSR with a real **subtractive synth** on a render block: 2 oscillators
(saw/square/tri/sine/noise, PWM, sync), sub, unison with detune/spread, multimode filter with
drive, 2 ADSRs, 2 LFOs, glide, velocity → filter/amp, mono/legato modes. Later: wavetable and a
2-op FM for lo-fi texture. Presets with on-brand names.

### 7.2 Sampler and drums

- **Drum machine**: 16 pads, per-pad sample or procedural voice, pitch/decay/filter/pan/choke
  group, round-robin, velocity layers. Kit browser. More procedural voices (toms, rim, perc,
  808-style sub, noise sweeps).
- **Step sequencer upgrade**: 16th/32nd resolution, per-step velocity / probability /
  ratchet / micro-timing, per-row swing, **polymeter** (rows of different lengths), fills,
  pattern chaining. Also usable on *melodic* tracks, where steps pick from chord tones by
  default (tier-aware step sequencer).
- **Sampler**: drop a WAV onto a track; multisample / SFZ / SF2; loop points; slice a break onto
  pads (transient detection) and play slices from the roll.
- **Slicing + chopping** of loops into the playlist.

### 7.3 Plugins

AU effect hosting; AU preset persistence (`fullState`); AU parameter automation; latency
reporting; sandboxed out-of-process (already); a plugin manager with favorites and a scan.

### 7.4 Preset and browser system

A browser panel: instruments, device presets, chains, drum kits, samples, chord progressions,
pattern templates — tagged, searchable, drag-and-drop everywhere, with a "lo-fi / grit / clean"
character filter that matches the brand.

---

## 8. Audio tracks and recording (the deferred, load-bearing pillar)

`PRODUCT.md`'s persona *records DI and vocals*. This has been parked twice. The plan is to bring
it back not as a generic recorder but as another surface the guidance engine lives on.

- **Recording**: input device/channel selection, arm, **low-latency monitoring** (small buffer
  path + optional direct monitoring), count-in, metronome, punch in/out, **loop recording with
  takes and comping**, pre-roll.
- **Audio clips**: cached waveform peaks, trim/fade/gain/split, crossfades, reverse, normalize,
  clip gain envelopes.
- **Warp / stretch**: `AVAudioUnitTimePitch` first; transient-aware warp markers; later a
  higher-quality stretcher. Pitch shift. Quantize audio to grid.
- **Guitar/DI path**: tuner, native amp + cab sim (or AU), noise gate — a DI recorded through a
  bare interface sounds terrible without this, and that's a finish-rate killer.
- **Audio → notes.** Monophonic pitch tracking (pYIN-class) on a vocal or bass take produces
  notes in the roll **with tiers**. The player sees the harmony of what they already played.
  This is the bridge between "I can play" and "I can arrange".
- **Audio → chords.** Chroma features + template/HMM chord estimation landing in the chord track
  as `isSuggested` chords to confirm, one by one. The original dream, in a correctable place.
- **Vocal tools**: simple pitch correction via AU; doubling; de-breath. Later.
- **Freeze / bounce in place** (needs 2.2).

---

## 9. Workflow, UX and feel

- **Command palette** (⌘K) with every action, fuzzy search, shows shortcuts.
- Shortcut presets: Dissonant / FL / Ableton / Logic.
- Panels: browser, mixer, roll, playlist, session, device chain, inspector — dockable, detachable
  windows, saved layouts, two-monitor friendly. Lower the 1380 px floor.
- Consistent ruler and zoom model across roll / playlist / automation; trackpad pinch everywhere.
- **Templates and starters**: "lo-fi bedroom", "post-punk", "ambient texture", "guitar + vocal
  demo" — tracks, bed, drums and a chain pre-wired (previous R16).
- **First loop in 60 seconds** onboarding: template → starter progression → live tier keyboard →
  record → done, with the three tiers introduced *by doing*.
- Autosave, versions (Time Machine-style browsing of the document), crash recovery.
- **Export**: WAV / AIFF / FLAC / MP3 / AAC, stems, MIDI file (patterns and song), loop export
  with tail, dither, normalize to LUFS, share sheet. Later: a `.dissonant` bundle with embedded
  audio.
- **Accessibility as mechanic**: VoiceOver labels per note including tier ("E4, chord tone"),
  full keyboard note editing, reduced motion (static tier updates), high-contrast tier
  patterns, resizable UI.
- Learn-by-doing layer: a toggleable "why?" strip that names what just happened in plain words
  ("you moved to the IV — that's the lift in the chorus").

---

## 10. The "much better" list (where Dissonant wins outright)

1. **Harmony-native timeline** — the chord track is a real timeline object every tool consults.
2. **Reharmonize from melody** — hum a line, get chords.
3. **Function-preserving transpose** — move a riff across the progression and keep its role.
4. **Live tier keyboard** — jam in-harmony on QWERTY/MIDI with mapping that follows the playhead.
5. **Audio → notes / chords** that land as *tiered, correctable* data, not a black box.
6. **Resolve / keep** with the `intentional` flag — deliberate dissonance becomes a tracked
   creative choice.
7. **Explain this note** and the learn-by-doing strip.
8. **Harmony-aware generators** — riff machine, variations, step sequencer, stamp.
9. **Mix guidance** in the same labeled-never-blocked voice.
10. **Lo-fi character baked into the device set** — the brand is audible, not just visual.

---

## 11. Target architecture

```
DissonantCore        pure model + theory + time (ticks, tempo map), zero deps     ← exists
DissonantEngine      scheduler, graph, devices, render-block synths, offline render
DissonantDocument    ProjectStore: intents → mutations → undo; persistence; migrations
DissonantUI          roll, playlist, session, mixer, devices, browser (SwiftUI + Canvas/AppKit)
```

Thread model: UI on main; a high-priority scheduler thread fills a lookahead window; the
render thread consumes lock-free ring buffers for events and parameter changes and never
touches Swift collections or locks. Model → engine is one-way via diffs; engine → UI is
polled meters and clock. Everything in Core and Document is testable headless.

---

## 12. Hard problems and risks (name them now)

- **Synth rewrite is unavoidable.** AudioKit node-per-voice can't take sample-stamped events.
  Budget it as a real project; it unblocks swing, velocity, glide and timing in one move.
- **Live graph edits on AVAudioEngine** are fiddly; design the insert-chain swap with a mute +
  crossfade fallback from day one.
- **Time-stretch quality** without a commercial library is mediocre; ship the Apple unit, label
  it, upgrade later.
- **Chord/pitch detection accuracy** is never perfect — hence `isSuggested` and confirm-by-ear.
  Never let detection write directly to highlights (the original decision holds).
- **SwiftUI at DAW density.** Canvas with culling first; AppKit/Metal hosted views where needed.
  Measure early with a 10k-note fixture.
- **Scope.** The gate stays: blank → finished for a player without theory. Anything that doesn't
  pass it goes to a "someday" list, not the roadmap.
- **Licensing**: soundfonts, sample packs, any ported DSP.

---

## 13. Phased roadmap

Each phase ships a usable app. Phase 0 is mostly invisible and entirely necessary.

**Phase 0 — Foundations (invisible, blocking)**
Audio-clock scheduler + lookahead; render-block synth; ticks + tempo map + time signature;
`ProjectStore` with intents and undo; schema v3 (velocity, clips, inserts, sections, automation,
master settings, AU state); working pan; track teardown; key-aware starters; offline render
replacing the real-time tap.

**Phase 1 — The roll**
Section 3.1 in full (M1 of the previous brainstorm), velocity lane, zoom, Canvas rendering
with culling, stamp/arp/strum/chop/quantize, ghost notes with tiers, tier magnet, resolve/keep,
explain-this-note, live tier keyboard. MIDI input + capture.

**Phase 2 — Arrangement**
Lanes of clips; sections/markers; whole-song chord lane; per-section key; tempo/time-sig lanes;
ripple edits; variation generator v1. Song skeleton templates.

**Phase 3 — Mix and devices**
Mixer view with meters, sends, groups, master limiter; device chains; native device set v1
(EQ, comp, saturator/tape, delay, reverb, filter, utility, lo-fi); AU effects; automation lanes
and recording; sidechain.

**Phase 4 — Instruments**
Subtractive synth v1 with presets; drum machine + step-sequencer upgrade (velocity, probability,
ratchet, polymeter, swing); sampler + slicing; browser with tags; racks and macros.

**Phase 5 — Audio**
Recording with monitoring, takes, comping; audio clips; warp v1; DI amp/cab + tuner; stems and
freeze; audio → notes (pitch tracking) into tiered roll.

**Phase 6 — Harmony superpowers**
Reharmonize from melody; function-preserving transpose; voicing engine + bed rhythms; chord
vocabulary + modes + modulation; next-chord suggestions with surprise dial; audio → chords as
suggested; mix guidance device.

**Phase 7 — Jam and polish**
Session view with capture; modulators; learn-by-doing strip; shortcut presets; command palette;
layouts; MP3/FLAC export; onboarding path; accessibility pass to the standard in `PRODUCT.md`.

---

## 14. Decisions to make (owner's call)

1. **Patterns-first or clips-first?** Recommend patterns stay the creative unit (FL); clips are
   instances on lanes (Section 5.1). Session view launches patterns.
2. **Native devices vs lean on AU?** Recommend a small, characterful native set (the lo-fi/tape
   family *is* the brand) plus AU hosting for everything else.
3. **Audio recording vs harmony superpowers first?** Persona says recording; thesis says
   harmony. Recommend Phase 5 before Phase 6 but pull **audio → notes** forward into Phase 5
   as the bridge that serves both.
4. **How hard to lean on AudioKit going forward?** Keep it for the graph plumbing and effects;
   own the synth and the scheduler. Revisit if the render-block synth makes AudioKit optional.
5. **Collaboration / cloud / iPad** — real asks eventually; explicitly "someday" here.

## 15. Someday / explicitly not now

Video scoring, surround, Max-for-Live-style scripting, notation view, full MPE, plugin SDK,
network collaboration, iPad companion, hardware controller scripts beyond generic MIDI learn,
AI-generated full arrangements (the product generates *guidance*, not songs).
