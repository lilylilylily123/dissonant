# Plan: sampler (YouTube link → chop → flip)

Tracked in beads (epic "sampler", run `bd list`).

## Goal

Paste a YouTube link. Dissonant downloads the audio, analyses it (onsets, BPM, key), and opens it on
a new **sampler track**: the source is cut into **chops**, chop *i* plays on MIDI pitch
`basePitch + i`, and the track is sequenced from patterns like any instrument. A new **SAMPLE**
screen is where you chop and shape.

## Decisions (settled with the user 2026-11-06)

- **yt-dlp:** use the system one (PATH, `/opt/homebrew/bin`, `/usr/local/bin`), else download the
  official standalone release into `<app data>/bin/` on first use, with progress. ffmpeg is optional:
  we ask for m4a audio, which symphonia already decodes; ffmpeg is only used to transcode when a video
  offers nothing else.
- **Flip tools in v1:** chop + map to keys/pads; per-chop shaping (start/end, gain, reverse,
  one-shot/gate, attack, release, choke); BPM + key detection with a suggested chop grid and
  "use as song key / tempo". **Not in v1:** repitch, time-stretch.
- **Home:** a new `sampler` track kind plus a SAMPLE screen.
- Chops play at the source's own pitch and speed (resampled only for the output rate). Playing a
  chop higher or lower is not a v1 feature.
- Sources longer than **10 minutes** are refused (memory: a source is held as f32 in RAM).
- One fetch at a time; a fetch can be cancelled.

## Contracts

### Core model (`crates/dissonant-core`, new module `sampler.rs`, re-exported from `lib.rs`)

```rust
pub const MAX_CHOPS: usize = 64;
pub const MIN_CHOP_SECONDS: f64 = 0.01;
pub const DEFAULT_SAMPLER_BASE_PITCH: u8 = 36;
pub const MAX_CHOP_GAIN: f64 = 4.0;
pub const MAX_CHOP_ATTACK_SECONDS: f64 = 2.0;
pub const MAX_CHOP_RELEASE_SECONDS: f64 = 5.0;
pub const MAX_CHOKE_GROUP: u8 = 16;
pub const MAX_SAMPLE_SECONDS: f64 = 600.0;

#[serde(rename_all = "camelCase")]
pub struct Sampler {
    pub source_id: Uuid,
    #[serde(default = "default_base_pitch")] pub base_pitch: u8,
    #[serde(default)] pub chops: Vec<Chop>,               // order = key mapping
    #[serde(default, skip_serializing_if = "Option::is_none")] pub analysis: Option<SampleAnalysis>,
}

#[serde(rename_all = "camelCase")]
pub enum ChopMode { #[default] OneShot, Gate }           // "oneShot" | "gate"

#[serde(rename_all = "camelCase")]
pub struct Chop {
    #[serde(default = "Uuid::new_v4")] pub id: Uuid,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub name: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    #[serde(default = "one")] pub gain: f64,             // 0..=MAX_CHOP_GAIN
    #[serde(default)] pub reverse: bool,
    #[serde(default)] pub mode: ChopMode,
    #[serde(default)] pub attack_seconds: f64,           // 0..=2
    #[serde(default = "default_release")] pub release_seconds: f64, // 0..=5, default 0.01
    #[serde(default)] pub choke: u8,                     // 0 = none, 1..=16
}

#[serde(rename_all = "camelCase")]
pub struct SampleAnalysis {
    #[serde(default)] pub bpm: Option<f64>,
    #[serde(default)] pub bpm_confidence: f64,           // 0..1
    #[serde(default)] pub key_root: Option<i32>,         // pitch class 0..11
    #[serde(default)] pub key_scale: ScaleType,
    #[serde(default)] pub key_confidence: f64,           // 0..1
    #[serde(default)] pub onsets: Vec<Onset>,            // ascending by seconds, ≤ 512
}
#[serde(rename_all = "camelCase")]
pub struct Onset { pub seconds: f64, pub strength: f64 } // strength normalised 0..1
```

- `Sampler::sanitized(&self, source_seconds: f64) -> Option<Sampler>`: clamps every chop's fields
  to the ranges above; `start` clamped to `[0, duration]` and `end` to `[start + MIN_CHOP_SECONDS,
  duration]`, a chop that can't fit is dropped; fails (`None`) when there are more than `MAX_CHOPS`
  chops; `base_pitch` clamped so `base_pitch + chops.len() - 1 <= 127`. Chop order is kept.
- `pub fn chops_at(points: &[f64], duration: f64) -> Vec<Chop>`: chops between consecutive cut
  points (sorted, deduped closer than `MIN_CHOP_SECONDS`, 0 is always a cut point), the last one ending
  at `duration`. Defaults everywhere else.
- `pub fn strongest_onsets(onsets: &[Onset], count: usize) -> Vec<f64>`: the `count` strongest
  onset times, ascending. A fetch's default chops are `chops_at(&strongest_onsets(o, 16), dur)`
  (plus a cut at 0).
- `Track` gets `#[serde(default, skip_serializing_if = "Option::is_none")] pub sampler: Option<Sampler>`.
  `TrackKind::Sampler`; `kind()` precedence Audio > Sampler > Drum > Midi. `normalized()`: a sampler
  track has `is_drum = false`, `is_audio = false`, `amp = None`, `plugin = None`, and its sampler
  sanitized against its source (a missing source keeps the sampler untouched; it plays silence, the UI
  says "missing audio").
- `AudioSource` gets `#[serde(default, skip_serializing_if = "Option::is_none")] pub origin_url: Option<String>`.
- No schema bump (additive fields).

### Commands

| Command (wire) | Rule | Label |
|---|---|---|
| `addSamplerTrack { name, source: AudioSource, sampler: Sampler }` | `sampler.sourceId == source.id`, `source` frames/sampleRate/channels > 0 else `InvalidValue`; sampler sanitized (fail → `InvalidValue`); upsert source; append a new track (fresh id, `Track::new(name)` + sampler) | `add sampler track` |
| `setSampler { id, sampler }` | track exists (`NoSuchTrack`), is a sampler track (new `EditError::NotASamplerTrack` = `"not a sampler track"`), `sampler.sourceId` in `audioSources` (`NoSuchAudioSource`), sanitized (fail → `InvalidValue`) | `edit sampler` |

The Tauri `apply` re-syncs the audio pool for `addSamplerTrack` and `setSampler` (source may
change).

### Sequence

`SequenceTrack` gets `pub sampler: Option<Sampler>` (clone of the track's, `analysis` dropped to
`None`). Notes on a sampler track flow exactly like a synth track's, in pattern and song mode.

### Engine

- `Instrument::Sampler(SamplerInstrument)` in `channel.rs`, chosen when `SequenceTrack.sampler` is
  set. The buffer is the `Arc<AudioBuffer>` for `sampler.source_id` from the engine's `AudioPool`,
  rebound whenever the sequence or the pool changes (no lookups on the audio thread beyond that).
- `note_on(pitch, vel)`: chop `pitch - base_pitch` (out of range = silence). Voice reads
  `start..end` (backwards when `reverse`), rate = `source_sr / out_sr`, 4-point interpolation (reuse
  `audio.rs` `catmull_rom`). Gain = `chop.gain * (0.25 + 0.75 * vel / 127)`. Linear attack over
  `attack_seconds`.
- **oneShot**: ignores note-off, plays to the chop's end, with a 4 ms fade ending there.
  **gate**: plays while held; at note-off (or at the chop's end, whichever first) fades over
  `release_seconds`, continuing to read past the end of the chop while it fades, up to the source's edge.
- Retriggering the same chop, or a hit in the same non-zero choke group, fades the old voice over 4 ms.
  32 voices; the oldest is stolen.
- The same path serves audition, preview and offline export.

### Analysis (`crates/dissonant-engine/src/sample_analysis.rs`)

`pub fn analyze(buffer: &AudioBuffer) -> Analysis` (engine-local type; Tauri maps it onto core's
`SampleAnalysis`): onsets with strengths (spectral-flux envelope, peak-picked, 50 ms refractory,
≤ 512 strongest), BPM from the onset envelope's autocorrelation over 60–200 BPM folded to the 70–160
band, with confidence, and key from a whole-file chroma matched against Krumhansl–Schmuckler
major/minor profiles, with confidence. Deterministic; runs on a worker thread.

### Tauri (`src-tauri/src/sample_fetch.rs`)

- `fetch_sample(url: String) -> Result<Snapshot, String>`: accepts only `http(s)` URLs, passes `--`
  before the URL. One at a time (`AtomicBool`). Phases emitted on event `sample-fetch`:

  ```ts
  type SampleFetchPhase = "locating" | "installing" | "downloading" | "converting"
                        | "decoding" | "analyzing" | "done" | "failed" | "cancelled";
  interface SampleFetchProgress { phase: SampleFetchPhase; fraction: number /* 0..1, -1 unknown */;
                                  title: string | null; message: string | null; }
  ```

  Flow: locate or install yt-dlp → read the title and duration (`--print`), refuse > 10 min →
  download `-f "bestaudio[ext=m4a]/bestaudio" --no-playlist` into a temp dir with `--newline`
  progress → transcode with ffmpeg only if the file isn't a decodable extension (error naming ffmpeg
  if it's absent) → `audio_files::decode` → `write_source` → `sample_analysis::analyze` → apply
  `addSamplerTrack` (name = video title, `originUrl` = url, default chops from onsets, analysis
  stored) labelled `"sample from link"` → `sync_engine`, `sync_audio_pool` → snapshot.
- `cancel_sample_fetch()`: kills the running child, emits `cancelled`.
- yt-dlp install: `https://github.com/yt-dlp/yt-dlp/releases/latest/download/<asset>` (`yt-dlp_macos`,
  `yt-dlp_linux`, `yt-dlp.exe`), verified against that release's `SHA2-256SUMS`, made executable.
  `ureq` as in `dissonant-catalog`.

### UI

- Bridge: `fetchSample(url): Promise<Snapshot>`, `cancelSampleFetch(): Promise<void>`,
  `onSampleFetch(cb): () => void`. The mock emits staged progress and adds a sampler track over a
  synthetic 8-second source (synthetic peaks already exist) so the screen is usable in the browser.
- Types mirror the core contract in `types.ts`; `ui/src/sampler.ts` mirrors `sanitized`, `chopsAt`
  and `strongestOnsets`, plus UI-only chop generators (equal N, BPM grid at 1/2/1/4 bar, beat).
- Screen `"sample"` (transport segment **SMP**, View menu, status bar, keyboard bypass like SOUND):
  link field + fetch/cancel + progress; the selected sampler track's big zoomable waveform with chop
  regions (drag edges, double-click to cut, right-click a chop to delete); generators; a pad grid
  (one pad per chop, labelled with its key; click/keys audition); the selected chop's inspector; the
  analysis readout with "use key" (`setKey`, locked) and "use tempo" (`setTempo`).
- Sampler tracks elsewhere: the track tree and `+ SAMPLE` entry (opens the SAMPLE screen), the
  piano roll edits their notes, and the bottom panel shows a sampler device card linking to SAMPLE.
