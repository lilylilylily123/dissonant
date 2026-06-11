---
date: 2026-06-11
topic: harmonic-guidance-engine
---

# Harmonic Guidance Engine — Requirements

## Summary

A context-aware harmonic guidance layer over a familiar (FL-style) piano roll. The
player builds a guided chord track — starting from a known-good progression, swapping
chords from in-key suggestions, with a "free-build under the hood" escape hatch — and it
plays back as a backing bed. As the playhead moves over each chord, the piano roll
re-tiers every note into solid chord tones, spicy-but-good tensions, and flagged
dissonance. Key is inferred from what the player plays and locked on suggestion;
dissonance is always labeled, never blocked.

## Problem Frame

The target user (per `STRATEGY.md`) is a player, not a producer: they can pick up a
guitar or bass and sing, but lack the theory to compose or arrange around what they
played. In every existing DAW the piano roll assumes that missing knowledge — it expects
you to already know which notes are in key and which sound good together.

The reframe that emerged in dialogue: the piano roll itself isn't the wall — the
*unlabeled* piano roll is. The user wants FL's piano roll, but where the grid tells them
which notes are safe instead of leaving them to guess. So the engine is an overlay on a
familiar piano roll, not a replacement for it.

The crux that makes this more than an existing feature: FL Studio and Ableton already
highlight in-scale notes. Static scale highlighting is parity, not a product. The bet
here is guidance that knows what sounds good *right now, given the chord that's playing* —
and that labels dissonance as a deliberate choice rather than hiding it.

## Key Decisions

- **Overlay, not a new input paradigm.** Guidance renders on top of a familiar FL-style
  piano roll. The user explicitly wants the piano roll — guidance makes it legible, it
  does not replace it.

- **Context-aware over static scale highlighting.** Highlighting keys off the chord
  happening at the playhead, not just abstract scale membership. This is the deliberate
  rejection of FL/Ableton parity — static scale-only highlighting was considered and
  ruled out as the v1 differentiator.

- **The chord track drives context — not audio detection.** Reliability is chosen over
  the player-dream for v1. Detecting chords from recorded audio is error-prone, and wrong
  chords would silently corrupt every highlight. A guided, in-app chord track is the
  authoritative harmonic source for v1.

- **Guided by default, free-build under the hood.** Chord-track building defaults to
  progression starters plus single-chord swaps (fastest path to a finished-sounding bed),
  with an "open the hood" mode to free-build or alter any chord on demand. Serves the
  beginner without capping the experimental user.

- **Hybrid key detection: infer-and-lock, always overridable.** No key is required up
  front. The engine watches the notes the player gravitates toward and suggests "looks
  like you're in X — lock it?" at low confidence until it's sure. The player can override
  anytime. Key detection from sparse notes is ambiguous, so the suggestion stays quiet and
  ignorable until confident.

- **Dissonance is labeled, never blocked.** Off-tier and harsh notes are flagged, not
  removed. Going "wrong" stays a deliberate, available choice — core to the experimental
  music this product exists to make.

## Requirements

**Key detection and locking**

- R1. A new project starts with no key locked. The piano roll is usable immediately
  without the player choosing a key.
- R2. As the player places or plays notes, the engine infers a likely key/scale and
  surfaces it as a low-confidence, dismissable suggestion ("looks like you're in X —
  lock it?"). The suggestion does not act on its own.
- R3. The player can accept a suggested key, set one directly, or override a locked key at
  any time.
- R4. Once a chord track exists, it is the authoritative source of key and harmonic
  context; note-based key inference is the cold-start bridge used before a chord track is
  present (see R12, Cold-start behavior).

**Guided chord track**

- R5. A dedicated chord-track lane lets the player build a chord progression that plays
  back as an audible backing bed.
- R6. The default building flow offers known-good progression starters the player can drop
  in, then swap any single chord for another suggested in-key chord and hear the
  difference.
- R7. Chord suggestions are in-key and ordered by what commonly sounds good next; the
  player auditions by ear before committing.
- R8. An "open the hood" mode lets the player free-build or alter any chord — including
  chords outside the obvious suggestions — with consonance feedback (consonant vs spicy vs
  harsh) on what they stack.
- R9. Theory names (e.g., chord and key names) are available but never required to build a
  progression; the player can work entirely by ear and audition.

**Context-aware piano-roll highlighting**

- R10. As the playhead moves over each chord in the chord track, the piano roll re-tiers
  every note relative to the chord currently sounding: solid chord tones, spicy-but-good
  tensions, and flagged dissonance.
- R11. Dissonant and off-tier notes remain fully playable and placeable; the tiering is
  visual guidance only and never disables a note.
- R12. **Cold-start behavior.** Before a chord track exists, the engine runs a lighter
  mode: it infers/locks the key and highlights at the scale level only. It upgrades to
  full per-chord context the moment a chord progression is present.
- R13. The same context-aware tiering is also surfaced as a live "notes you could play
  right now" readout (the user's original "tab with all the notes you could play"),
  reflecting the chord at the playhead — the same engine surfaced both on the roll and as a
  list.

## Acceptance Examples

- AE1. **Covers R1, R12.** Given a brand-new empty project with no key and no chord track,
  when the player opens the piano roll, then all notes are playable and the roll shows no
  per-chord tiering yet (neutral / scale-level at most), with no forced key choice.
- AE2. **Covers R2, R3.** Given the player has placed a few notes that imply a key, when
  the engine reaches sufficient confidence, then it surfaces a dismissable "looks like X —
  lock it?" suggestion; if the player ignores it, highlighting behavior does not change
  until they act.
- AE3. **Covers R10.** Given a chord track whose progression moves from one chord to
  another, when the playhead crosses the chord boundary, then the set of solid / spicy /
  dissonant notes on the roll updates to match the new chord.
- AE4. **Covers R6, R7.** Given the player drops in a progression starter, when they swap
  one chord for a suggested alternative, then the backing bed plays the new progression and
  the piano-roll tiering under that chord updates accordingly.
- AE5. **Covers R8, R11.** Given the player opens the hood and builds a harsh, out-of-key
  chord on purpose, when they place notes against it, then the roll still tiers and flags
  notes (now relative to that chord) and never prevents placement.

## Scope Boundaries

### Deferred for later (designed-for, not built in v1)

- Active suggestions — auto-generated melodies and "what chord comes next" generation. The
  data model should not preclude layering these on later.
- Guitar/bass chord *detection* — detecting chords from recorded audio to populate the
  chord track. When added, it feeds the same chord-track structure (R5–R9) so detected
  chords land in a correctable place rather than driving highlights directly.

### Outside this product's identity

- Replacing the piano roll with a non-piano-roll input paradigm. The decision is to
  augment the familiar roll, not invent a new primary input surface.
- Blocking or auto-correcting "wrong" notes. The product labels dissonance; it never
  enforces consonance.

## Dependencies / Assumptions

- **Assumption (low-confidence, flagged):** Key inference from sparse note input is
  ambiguous; the infer-and-lock UX depends on keeping suggestions quiet and low-confidence
  until sure. If inference proves too noisy to be useful pre-chord-track, the cold-start
  mode (R12) may need to lean more on an explicit lightweight key pick.
- The chord track is a prerequisite for the full context-aware experience (R10); the
  cold-start path (R12) is the graceful degradation when it's absent.
- Backing-bed playback (R5) implies the chord track produces audible sound — instrument /
  voicing choice for that playback is a planning concern, not decided here.

## Outstanding Questions

### Deferred to planning

- Visual language for the three tiers (color, shape, opacity) and how flagged dissonance
  reads distinctly from "spicy tension" without clutter — a design decision for planning.
- Chord voicing for both suggestions and backing-bed playback (which inversion/register
  the engine picks).
- How the live "playable now" readout (R13) is laid out relative to the piano roll.

## Sources / Research

- `STRATEGY.md` — product strategy: target problem, the player persona, the "surface the
  harmonic landscape" approach, and the Harmonic Guidance Engine as the load-bearing track.
- Prior art noted in dialogue: FL Studio and Ableton already ship static in-scale
  highlighting — the reason static-scale-only was ruled out as the v1 differentiator.
