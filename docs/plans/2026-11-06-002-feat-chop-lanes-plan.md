# Plan: chop lanes, regions and mono samplers

Follows [the sampler plan](2026-11-06-001-feat-sampler-plan.md). Tracked in beads (run `bd list`).

## Problem

A sampler track opens in the piano roll, which has nothing to do with chops:

- 128 chromatic rows, colored by chord tones and tensions.
- Keys with no chop still take notes.
- Chop numbers are tiny text on the keyboard.
- Chord tools that mean nothing for chops: the chord lane, "chords from", the chord bed, ARP, STRUM, INVERT, stamp chord, and the chord-tone hint.
- A note's drawn length is a lie for one-shot chops, which always play out.

Fresh links are chopped at the 16 strongest hits across the whole file. On a 4-minute song that gave chops of 50 s, 25 s and 134 s, and they layer on top of each other.

## Decisions (settled with the user 2026-11-06)

1. **Chop lanes replace the piano roll on sampler tracks.**
   - One row per chop, in chop order. Chop 1 is at the bottom, like a keyboard. Every chop is shown.
   - Rows shrink to fit about 16 chops, then the lanes scroll.
   - A "used only" toggle hides chops the pattern doesn't play.
   - Each row header shows the chop's color, number, name and a mini waveform.
   - No scale or chord coloring, and no rows without a chop.
2. **Notes show the real sound.**
   - A one-shot hit draws a faded tail as long as the chop sounds at the song's tempo.
   - The tail is cut where the next hit on the track cuts it. On a mono track that is any hit; on a poly track it is the same chop or the same choke group.
   - Gate notes keep their draggable length, with the release tail drawn after it.
3. **Dragging a hit up or down moves it to another chop**, lane by lane, and previews the new chop as it lands.
4. **Mono by default.**
   - New `Sampler.voice: "mono" | "poly"`, default `mono`, also for existing files.
   - Mono: any new hit (sequenced, live key, pad or audition) cuts everything the track is sounding over 4 ms, ignoring choke groups. The choke control is hidden.
   - Poly: today's layering, with choke groups.
5. **Pick a region first.**
   - New `Sampler.region: { startSeconds, endSeconds } | null`.
   - The chop tools (auto N, equal N, grid bar/½/beat) act inside the region only.
   - The waveform draws the region with draggable edges, and dragging across the ruler selects a new one. (Built on the ruler rather than the overview, which already pans the view; the lane's drags move chop edges.)
6. **Fresh links come pre-chopped inside a suggested region:**
   - It starts at the first strong downbeat: the first onset with strength ≥ 0.5.
   - With a detected tempo it is 4 bars long; without one, 8 s from the first strong hit, with the bar-based tools disabled.
   - It is clamped to the source and auto-chopped at the hits inside it, capped at 16.
7. **Moving or resizing the region re-chops** with the last chop tool used (stored as `Sampler.chopper`).
   - Chop *i* keeps its settings where a chop *i* still exists: name, gain, reverse, mode, attack, release and choke.
   - The cut positions are new. The whole change is one undo step.
8. **A pattern strip on the SAMPLE page.**
   - The same chop-lanes component, editing the selected pattern of the sampler track. PAT edits the same pattern.
   - A resizable split between the waveform and the strip, saved like the other panels, defaulting to about 60/40.
9. **Recording:**
   - With the record arm on and the transport running, pads and pad keys write hits into the pattern.
   - Hits snap to the recording grid, through the same record path live keys use.
   - Outside recording, pads keep the one-at-a-time preview.
10. **Inspector on PAT for a sampler track: rhythm tools only.**
    - Kept: quantize, humanize, ×2 slower / ½ faster, reverse (note order), legato, and the velocity lane.
    - Hidden: pitch, ARP, STRUM, INVERT, stamp chord, the chord lane, "chords from", the chord-bed toggle, TIERS/MAP, and the chord-tone hint.

## Contracts

### Core (`dissonant-core/src/sampler.rs`), additive, no schema bump

```rust
#[serde(rename_all = "camelCase")] pub enum SamplerVoice { #[default] Mono, Poly }
#[serde(rename_all = "camelCase")] pub struct SampleRegion { pub start_seconds: f64, pub end_seconds: f64 }
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Chopper { Auto { count: u32 }, Equal { count: u32 }, Grid { step: GridStep } }
#[serde(rename_all = "camelCase")] pub enum GridStep { Bar, Half, Beat }

// on Sampler:
#[serde(default)] pub voice: SamplerVoice,
#[serde(default, skip_serializing_if = "Option::is_none")] pub region: Option<SampleRegion>,
#[serde(default, skip_serializing_if = "Option::is_none")] pub chopper: Option<Chopper>,
```

- `sanitized` clamps the region into the source. A region shorter than `MIN_CHOP_SECONDS` becomes `None`.
- New pure functions, mirrored exactly in `ui/src/sampler.ts` and `ui/src/sampleChops.ts`:
  - `pub fn suggested_region(analysis: &SampleAnalysis, duration: f64, beats_per_bar: u32) -> SampleRegion`.
  - `pub fn chop_region(chopper: &Chopper, region: &SampleRegion, analysis: Option<&SampleAnalysis>, beats_per_bar: u32) -> Vec<Chop>`.
  - `pub fn rechop(old: &[Chop], fresh: Vec<Chop>) -> Vec<Chop>`, which keeps the per-index settings.
- The fetch's default chops become `chop_region(Auto{16}, suggested_region(..))`, with `region` and `chopper` stored.
- The existing generators in `sampleChops.ts` take a region, and core gains the grid/equal versions so the fetch and the UI agree.

### Engine

- `SamplerInstrument` reads `voice`. In mono, `note_on` fades every active voice over `CUT_SECONDS` before striking, and choke groups are ignored.

### UI

- **`ChopLanes` component** (`components/sample/ChopLanes.tsx`), shared by PAT (in place of `PianoRoll` when `track.sampler`) and the SAMPLE strip.
  - Canvas lanes.
  - Hits: place by click, move by drag, change chop by dragging vertically (with a preview), resize gate notes, marquee, delete, copy.
  - Reuses `noteEditing`/`clipEditing` helpers where they apply.
  - Tails are computed from the chop's seconds through the song's tempo map and cut by the next hit per the voice mode.
- **`Editor.tsx` / `Inspector.tsx`** hide the chord and pitch tools for sampler patterns (decision 10).
- **SAMPLE page:**
  - The region's edges on the waveform (flags on the ruler), and drag-select along the ruler.
  - The tools act inside the region, and region edits re-chop (one transient gesture plus `commitGesture`).
  - A mono/poly toggle; the choke control is hidden in mono.
  - The resizable strip, using a new `PanelId` `"sampleStrip"` whose default is 40% of the room the waveform and strip share. SAMPLE plays the song, so the strip's playhead is the song tick read through the clip of the pattern being heard, and is hidden where none is (as recording files nothing there).
  - Pads route through the record path while the arm is on and the transport runs (`strikePad`).
- **Mock** (`bridge.ts`): mirrors the new fields in `reduce`/`normalized` and the fetch default.

## Out of scope

Repitch and time-stretch, per the earlier decision.
