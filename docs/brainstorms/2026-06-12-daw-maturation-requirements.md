---
name: daw-maturation
status: draft
created: 2026-06-12
origin: none
---

# Dissonant: From Prototype to Fully-Fledged DAW

## Problem frame

Dissonant 1.0 proves the core bet — the harmonic-guidance piano roll lets a beginner
place in-key notes and see dissonance — but it still *feels* like a prototype the moment
you try to shape a real track. The first and hardest wall: **you cannot move a note after
placing it.** The roll is place-only (left-click paints, right-click erases); notes
themselves ignore the mouse. Past that wall sit the rest of the gaps between "a loop that
sounds good" and "a finished track": no real mixing, no automation, no way to bounce stems
or polish an arrangement to done.

This document scopes maturing Dissonant into a fully-fledged **composition & production**
DAW — deliberately *not* an audio-recording DAW yet (see Deferred).

## Target outcome

A user can take a guided loop and carry it all the way to a finished, shareable track
without hitting a wall: edit notes fluidly, mix and automate, and export. The product
stays true to STRATEGY.md — every feature serves "beginner-player → finished track," not
generic DAW feature-parity.

## Persona note (assumption)

STRATEGY.md's primary persona is a *player* who records DI + vocals. By parking audio
capture, this phase serves a slightly narrowed persona: **someone composing and producing
with guided MIDI.** This is a sequencing choice, not a redefinition — audio capture remains
load-bearing and returns in a later phase. Recorded by user decision (2026-06-12).

## Scope

### In scope (this effort)
- **M1 — Note editing** (the wall; built first)
- **M2 — Mixing & production**
- **M3 — Finishing & escape velocity**
- **Undo/redo** as a cross-cutting foundation introduced in M1

### Deferred for later (still on the roadmap)
- **Audio capture** — guitar/bass/DI + vocals through an interface, low-latency
  monitoring, audio clips/waveform editing. Load-bearing in STRATEGY.md; a phase of its own.
- MIDI hardware input (play notes from a MIDI keyboard).

### Outside this product's identity
- Generic DAW feature-parity for its own sake (matching Ableton/Logic feature lists).
- Pro features that don't serve a beginner-player reaching "finished": deep modulation
  matrices, advanced routing/sidechain graphs, surround, video scoring.
- Tool-mode UIs that reintroduce the "why won't clicking draw a note?" mode confusion the
  guidance thesis exists to remove.

---

## Milestone 1 — Note editing (modeless direct manipulation)

**Interaction model decision:** modeless direct manipulation, *not* tool modes. You grab a
note to move it and drag empty space to draw — no pencil/arrow toolbar. Chosen because the
beginner persona is harmed most by mode errors. (See `Dissonant/PianoRoll/PianoRollView.swift`
+ `RollMouseHandler`, which already does modeless left-paint / right-erase; this extends it.)

- **R1 — Move a note.** Left-drag on a note's body moves it in pitch and time, snapping to
  the active note-length grid; it auditions the pitch as it moves.
- **R2 — Resize a note.** Left-drag on a note's right edge changes its length (grid-snapped),
  with a hit zone large enough to grab comfortably.
- **R3 — Disambiguate body vs edge vs empty.** A press on an existing note's body = move;
  on its right edge = resize; on empty grid = place (current behavior preserved). Right-click
  still erases.
- **R4 — Marquee multi-select.** Dragging a selection rectangle from empty space selects the
  notes inside it (instead of painting); selected notes are visibly highlighted.
- **R5 — Operate on a selection.** Move all selected notes together (R1 semantics), and
  delete the whole selection at once.
- **R6 — Copy / paste / duplicate.** Copy the selection and paste it (at the playhead or a
  sensible offset); a one-gesture duplicate for quick repetition.
- **R7 — Undo / redo (cross-cutting).** Every destructive or structural edit — place, move,
  resize, delete, paste, and the existing chord/track/pattern edits — is undoable and
  redoable. Foundational: the guidance thesis wants people to experiment freely ("going
  wrong stays a deliberate choice"), which requires a safety net.
- **R8 — Editing preserves harmonic guidance.** Moved/resized notes recolor by tier against
  the chord at their new position, exactly as placed notes do today.

**Open within M1:** how marquee-select coexists with empty-drag-to-paint (modifier key vs.
a lightweight select affordance) — an interaction detail for planning/prototyping.

## Milestone 2 — Mixing & production

- **R9 — Per-track insert-effect chain.** Each track can host an ordered chain of insert
  effects beyond the current fixed tone/reverb (e.g., delay, compression, chorus/EQ), with
  per-effect controls. Builds on the existing per-track FX bus in `TrackVoices`.
- **R10 — Parameter automation.** Track and effect parameters (volume, pan, reverb, cutoff,
  effect params) can change over time via automation lanes drawn against the timeline.
- **R11 — Dedicated mixer view.** A channel-strip mixer (per-track fader, pan, meters,
  mute/solo, FX sends) as an alternative to the inline FX rows, for mixing the whole song
  at once.
- **R12 — Wire up pan.** The `Track.pan` model field already exists but is inert (the
  SoundpipeAudioKit `Panner` had a linker error and was removed); pan must actually affect
  the stereo field via a working node.

## Milestone 3 — Finishing & escape velocity

- **R13 — Arrangement polish.** Reorder/duplicate/repeat song blocks, per-block repeat
  counts, and loop/section markers, so building a full structure is fluid.
- **R14 — Master chain.** A master-bus effect/limiter stage so exports are loud and polished
  rather than raw.
- **R15 — Export beyond a single WAV.** Stems (per-track audio) and MIDI export, alongside
  the existing whole-song WAV bounce.
- **R16 — Starter templates.** Genre/skeleton starting points (drums + chord bed + a lead
  track pre-wired) so a blank project isn't a cold start — directly serves
  time-to-first-good-loop and finish rate.

---

## Success criteria

Tie back to STRATEGY.md metrics:
- **Finish rate (primary):** a guided loop can reach a polished, exported track without
  hitting a workflow wall — the explicit goal of M1–M3.
- **Time-to-first-good-loop (primary):** unaffected or improved (templates in R16; editing
  must not slow first-note placement).
- **Deliberate dissonance use (primary):** preserved — editing keeps tier coloring (R8) and
  never blocks "wrong" notes.
- **No new mode confusion:** the modeless model means a first-time user can still place a
  note by clicking, with zero new concepts required before they can draw.

## Known issues / backburner

- **AU load crash.** Loading an Audio Unit instrument crashes the app — likely a regression
  from the per-track FX refactor (AU attached to a live engine). Tracked for investigation;
  fix direction noted in project memory. Not part of M1–M3 scope but blocks the AU path.

## Dependencies & assumptions

- **Assumption:** audio capture stays deferred for this whole effort; the persona is treated
  as composing-with-guided-MIDI until then.
- **Assumption:** undo/redo is best introduced as the M1 foundation rather than retrofitted
  later — every subsequent milestone's edits inherit it.
- **Dependency:** M2's pan fix (R12) needs a working pan node (AudioKit `Fader` stereo or
  an `AVAudioMixerNode` pan path) since the Soundpipe `Panner` linker issue is unresolved.
- **Dependency:** undo/redo likely hinges on routing model mutations through the document's
  `UndoManager`; current edits mutate `@Published`/binding state directly and probably don't
  register undo (verify during planning).

## Open questions

1. Marquee-select vs. paint-drag coexistence (M1 interaction detail).
2. Automation (R10): freehand-drawn curves vs. point/breakpoint envelopes for the first cut.
3. Mixer view (R11): does it replace the inline FX rows or live alongside them?
4. Velocity editing was cut from M1's first pass — fold into M1 later, or into M2?
