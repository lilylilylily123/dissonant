---
name: beginner-flow-and-qol
status: draft
created: 2026-10-07
origin: docs/brainstorms/2026-10-05-quality-of-life-roadmap.md
---

# Beginner flow and quality-of-life, round two

The first QOL roadmap (`2026-10-05-quality-of-life-roadmap.md`) is mostly shipped, and it was
written for someone who already knows DAWs: modifiers, shortcuts, settings, engine options.
Nothing yet owns **the first ten minutes** for the person in `PRODUCT.md`: a player without
theory who wants a good loop fast, then a finished track.

This doc is about that path. Every item has to pass the `PRODUCT.md` gates: **guidance, never
guardrails** · **legible, not dumbed down** · **meaning twice, never color alone** · no toy
aesthetics, no confetti, no mascots. Nothing here hides a feature; it puts a next step in front
of the user and names things in plain words, with the theory name one glance away.

Sizing and priority follow the first roadmap: **S** = an hour or two · **M** = half a day,
Rust + UI · **L** = a day or more. **now** / **next** / **later**.

---

## 0. Fix first: things that undercut the first impression

Found by reading the code as of `0bd8f61`.

**Status (2026-10-07): all four shipped.** The starter is built by `ProjectModel::starter_shaped`
at the new-project size, with a pluck melody, a groove and the key locked to C major, and the
chord bed turns on for a fresh starter. Split moved to ⇧⌘E. Labels go through `keys()` in
`ui/src/platform.ts`. HARMONY shows a hint when there is no key and no chords. Fixing 0.1 also
turned up that ⌘N always opened the starter, ignoring Settings → editing → new project; it now
follows the setting.

| # | Problem | Where | Fix | Size |
|---|---|---|---|---|
| 0.1 | **The starter project is silent.** The new-project picker promises "I–IV–V–vi in C, a melody and a drum track", but the starter only adds chords, an empty `melody` track and an empty `drums` track. The chord bed is off by default (`hearChords: false`), so pressing space plays nothing. | `ProjectModel::starter()` in `crates/dissonant-core/src/model.rs`; `TemplatePicker.tsx`; `store.ts` | Seed a simple drum groove (kick / snare / hat) and a short melody or bassline made of chord tones and one tension. Turn the chord bed on for starter projects (a per-project flag or the store default when the starter loads). Make the picker's description match. | S |
| 0.2 | **⌘E does two things.** In the roll it splits at the playhead when the selection crosses it; otherwise it falls through to the export dialog. A beginner pressing ⌘E with a note selected gets a split they didn't expect, or an export dialog they didn't expect. | `PianoRoll.tsx` (split), `App.tsx` (export), `MenuBar.tsx` lists both | Give split its own binding (⌘⇧E or S-for-split while the roll is focused) and keep ⌘E for export only. Update the help strings and README table. | S |
| 0.3 | **⌘ everywhere on Windows / Linux.** Tooltips, the roll's `?` help and the README say ⌘. Already open in roadmap §6. | `Editor.tsx`, `MenuBar.tsx` (which already computes `mod`), tooltips | One `modKey()` helper used by every label. | S |
| 0.4 | **Empty projects start with no key.** `KeyState::NONE` makes the inspector say "no guidance yet" and the roll shows no tiers until the user finds the key picker or places enough notes for the "X? lock" offer. | `ProjectModel::empty()`, `Inspector.tsx` | See 1.4: an empty-state line that offers the two quickest ways in. Don't force a key. | S |

---

## 1. The first ten minutes

| Pri | Item | Notes | Size |
|---|---|---|---|
| now | **First-run / new-project screen** | Replaces the bare `TemplatePicker` list. Three doors: **start from a vibe** (1.2), **open recent**, **empty**. The three tiers are shown as a small live legend (solid / spicy / flagged, with their shapes, not only colors). Also shown on launch when there is no autosave to recover. Already listed as open in roadmap §6. | M |
| now | **Vibe templates** | Built-in templates for the music the persona makes: *lo-fi bedroom*, *post-punk*, *ambient texture*, *guitar + vocal demo*, *club loop*. Each sets tempo, swing, key, a progression, named tracks with voices, a drum groove, and a mix (reverb, tone). Ship them as JSON next to user templates so they use the existing template loader (`newFromTemplate`). | M |
| next | **"First loop" checklist strip** | A thin, dismissible strip under the transport with six steps that tick themselves off when the user actually does them: pick a progression · press space · play with KEYS · record a take (R) · add drums · make it a song. No modal tour, no blocking, no celebration beyond the tick. Closes for good once dismissed or done (a setting brings it back). | M |
| now | **Empty-state hints** | Each empty surface names one next action in the app's voice. Roll: "click to place a note, or press K and play the home row". Chord lane: "no chords yet. Pick a starter, or just play and lock the key it hears." Drum grid: "click a step, or start from a groove" (3.3). Song view with no clips: "＋ on a pattern drops it here". | S |
| next | **KEYS overlay** | When the live tier keyboard is on, show a small on-screen strip of the QWERTY rows with each key's note, tinted and shaped by tier, updating as the chord changes. The tier keyboard is one of the product's best ideas and today it's invisible unless you read the README. | S–M |

---

## 2. Plain words over theory words

| Pri | Item | Notes | Size |
|---|---|---|---|
| now | **Mood names for progression starters** | `STARTERS` in `ui/src/theory.ts` are labelled only by roman numerals. Lead with a mood ("bright, lifting", "sad but hopeful", "dark loop", "jazzy turnaround") and keep the numerals as small secondary text, so the theory is learned by association. Audition the progression on hover. | S |
| now | **Persona-fit starters** | Add progressions the target music actually uses: two-chord vamps (i–iv, I–♭VII), i–♭VII–♭VI, Dorian i–IV, a one-chord drone. Several need borrowed chords or modes (2.4). | S–M |
| now | **Instrument presets by character** | Voices are named by waveform (saw / square / triangle / sine / pad / pluck). Add named presets ("warm pad", "buzzy bass", "glass pluck", "soft keys", "lead with bite") that set voice + tone + reverb + octave. The waveform stays visible underneath. | S |
| next | **Role-based "+ track"** | Replace `+ inst` / `+ drum` with **+ bass / + lead / + pad / + keys / + drums**. The role picks a preset, a default octave, a color, and scrolls the roll to the right register. Fewer "why is my bass so high" moments. | S–M |
| next | **More scales, with mood labels** | Only Major / Minor exist today. Add Dorian, Mixolydian, Phrygian, harmonic minor, major and minor pentatonic, each with a plain label ("minor, but brighter" for Dorian). Requires generalising the classifier in `crates/dissonant-core/src/theory/` and its TS mirror in `ui/src/theory.ts`, with tests on both sides. | M–L |
| next | **Context hints in the status bar** | Hovering any control shows what it does in one plain sentence in the status bar. Replaces the long `?` tooltip as the main way to learn the toolbar. Already open in roadmap §6. | S–M |

---

## 3. From loop to song

Beginners stall at the eight-bar loop. These items make the jump to a full track reachable.

| Pri | Item | Notes | Size |
|---|---|---|---|
| now | **Make it a song** | One action in SONG mode (and a step in the checklist): lay out a skeleton of named sections (intro / verse / chorus / verse / chorus / bridge / chorus / outro) using the existing patterns, with sensible lengths. Everything it creates is ordinary clips and sections, editable and undoable as one step. | M |
| now | **Drum groove presets** | In the drum grid: four-on-the-floor, boom-bap, half-time, breakbeat, motorik, plus a **fill** button that writes a fill into the last bar. Drums are where beginners give up first. | S–M |
| next | **Make a variation** | From a pattern, make a sibling pattern that keeps the feel: thin out notes, shift the rhythm, move the riff to the next chord, or reverse. Tier-aware, so chord tones stay on strong beats. Feeds "make it a song" with a verse and a chorus that differ. | M–L |
| next | **Next-chord suggestions** | Hovering the end of the chord lane offers three next chords labelled **solid / spicy / weird**, in the same tier voice as notes, with audition. | M |
| next | **Type chords** | Type `Am F C G` into the chord lane and get the progression. Already open as roadmap 4.6. | M |

---

## 4. Sounds good and shareable by default

| Pri | Item | Notes | Size |
|---|---|---|---|
| now | **Master limiter on by default** | Roadmap §2 has this as "later". Promote it: beginners stack tracks and clip the master, and the first export sounds broken. Transparent brickwall at -0.3 dBFS, switchable in DEVICES. | M |
| next | **Share export** | One button: MP3 at streaming loudness (around -14 LUFS), named from the project. Only WAV exists today. Needs an encoder crate and a LUFS meter in the render path. | M |
| next | **Collapsible inspector sections** | The PAT inspector stacks tracks, pattern, notes, transform, harmony, instrument and overview. Let each section collapse, remember the state, and default TRANSFORM to collapsed in new installs. Nothing is removed. Roadmap 1.5. | S |

---

## 5. Everyday QOL worth promoting from the open list

These are already in the first roadmap; each also helps a beginner.

- **Command palette** (⌘K) with plain-word synonyms, so "remove", "erase" and "delete" all find the same action.
- **Cheat-sheet overlay** (hold `?`) generated from the live binding table (roadmap 1.4), so help text can't drift from the code again.
- **Per-project UI state** (roadmap 1.6): reopening a project looks like leaving it.
- **Toast queue** with a "copy details" button on errors (roadmap §6).
- **Chord lane QOL** (roadmap 4.6) and **drum grid per-step velocity and accents** (roadmap 4.7).
- **Clip split and mute in the UI**, and **make unique** on a clip (roadmap §5).

---

## 6. Suggested order

1. **§0** in full: the starter makes sound, ⌘E means one thing, labels match the OS.
2. Empty-state hints, mood-named starters, persona-fit starters, instrument presets.
3. First-run screen and vibe templates.
4. Drum groove presets and "make it a song".
5. Master limiter and share export.
6. The first-loop checklist strip (it can only point at steps that exist, so it comes after 2–4).
7. Role-based tracks, KEYS overlay, status-bar hints, collapsible inspector.
8. More scales, make a variation, next-chord suggestions.

---

## 7. Not QOL, but blocking the persona

`PRODUCT.md` describes someone who wants to record DI guitar, bass and vocals through an audio
interface. **Audio recording is not built** (Phase 5 in `2026-10-02-full-daw-brainstorm.md`).
Every item above makes the MIDI side friendlier, but the persona's core job needs audio tracks.
It is the largest gap between the product brief and the app, and worth scheduling soon after
the §0–§3 work here.
