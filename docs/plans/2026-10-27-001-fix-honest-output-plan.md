# Plan: honest output — make the app produce what it says it produces

Branch: `fix/honest-output`. Follows the capability audit of 2026-10-27.

## Goal

Six defects where the app is not missing a feature but **producing the wrong result, silently**.
Nothing here adds a capability a user would see in a menu; everything here stops the app lying
about what it played, what it wrote, or what it saved. This is the prerequisite for the
automation and native-effects work, because both of those are only worth building on a renderer
that renders the mix.

The test of "done" for the whole phase: **open a project with a CLAP instrument and an insert
chain, export it, and the WAV is what the speakers played** — and nothing between here and there
can be lost by changing the audio device, recording a take, or saving the file.

## The six

| # | Defect | Today | Size |
|---|---|---|---|
| 1 | Export drops CLAP instruments and every insert chain | `render.rs:67-79` sends `SetLooping/SetHearChords/SetAudioPool/SetSequenceWithDrums/Play` and nothing else; `Instrument::for_track` falls back to the built-in voice, `Channel::new` starts with an empty chain | L |
| 2 | Changing the audio device drops instrument plugins | `restart_audio` clears `loaded_insert_chains` and calls `sync_inserts(true)`, never `sync_plugins()`; `loaded_plugin_uids` keeps the stale entry so no later reconcile repairs it | S |
| 3 | The master hard-clamps instead of limiting, and reports clipping after the fact | `Master::process` does `(a * self.gain).clamp(-1.0, 1.0)`; the meter reads the already-clamped sample; the only warning is `peak > 0.99` in the transport | M |
| 4 | Project save is not atomic and has no backup | `save_project` does a bare `fs::write`, then deletes the autosave beside the file — while `autosave()` and `library.rs` both do tmp+rename | S |
| 5 | MIDI recording folds long takes, overwrites earlier passes, and makes one undo step per note | `live_note_off`: `start % loop_ticks`, `notes.retain(…)` against the whole pattern, `apply(SetNotes, false)` per note-off | M |
| 6 | `Sequence` carries insert lists nothing reads | `SequenceTrack.inserts` / `Sequence.master_inserts` are populated from the model and never consumed by the engine — they make the render path look covered | S |

Order: **2, 4, 6** (small, independent) → **3** (self-contained, in the engine) → **1** (the big
one) → **5**. 1 and 5 are the two that need design decisions; they are spelled out below.

---

## 1. Plugins and inserts in the offline render

### The constraint that shapes the design

A CLAP plugin's parameter/state calls must stay on the thread that opened it — `clack` enforces
it by making `HostedPlugin` not `Send`. That is why every plugin lives on `PluginHostThread`
(`src-tauri/src/plugins.rs`) and the app only ever holds the `PluginAudioUnit`, which *is* `Send`
and is what the engine renders. `render_to_buffer` runs inside `spawn_blocking` and builds a bare
`Engine` from a `Sequence` alone — so it has never had a way to get a unit.

A unit is also consumed by the engine it is handed to, so **N stem passes need N sets of units**,
and they must be opened at the *render* sample rate, which may differ from the device's.

### Shape

```rust
// crates/dissonant-engine/src/render.rs — not part of RenderOptions, which stays Clone+Debug.
#[derive(Default)]
pub struct RenderPlugins {
    /// Instrument per track, already opened at the render sample rate.
    pub instruments: Vec<(Uuid, Instrument)>,
    /// Insert chains, in order, per target (track or master).
    pub inserts: Vec<(InsertTarget, Vec<InsertSlot>)>,
}

pub fn render_to_buffer(sequence: Arc<Sequence>, options: &RenderOptions, plugins: RenderPlugins) -> …
pub fn render_wav(sequence: Arc<Sequence>, path: &Path, options: &RenderOptions, plugins: RenderPlugins) -> …
```

`render_to_buffer` sends `SetInstrument` / `AddInsert` / `SetInsertEnabled` before `Play`, in that
order, exactly as `sync_plugins` + `sync_inserts` do live. An empty `RenderPlugins` reproduces
today's behaviour, which keeps every existing engine test unchanged.

### Shell side (`src-tauri/src/lib.rs`)

- `plugin_host: PluginHostThread` becomes `Arc<PluginHostThread>` so the blocking closure can
  open and close per render pass instead of holding N sets alive at once.
- A new `AppState::open_render_plugins(&self, model: &ProjectModel, sample_rate: f64) -> RenderPlugins`:
  for every track with a `plugin`, and every insert in every chain, open under a **fresh `Uuid`**
  (never the live key, or the reconcile caches and the live instance collide), then push state.
- **State comes from the live instance first.** `plugin_host.save_state(live_id)` is the current
  sound; the model's `state` blob is only as fresh as the last edit that wrote it. Fall back to
  the model blob, then to nothing, and log which.
- `export_wav` opens a set per job inside the blocking loop, renders, then closes every id. Stems
  already render sequentially, so peak memory is one set.
- A plugin that fails to open during export is **not** silently substituted: collect the names and
  return them on `ExportResult` so the UI can say "exported without: Dexed, lamb".

### Risks

- **Block size.** Live uses the device block (64–2048), render uses 1024. Plugins with
  block-size-dependent state will differ slightly from live; that is inherent to offline render
  and acceptable, but `MAX_BLOCK_FRAMES = 2048` must keep bounding it.
- **Cost.** A 12-track plugin project exporting stems opens 13 sets sequentially. Slow, correct,
  and visible in the existing progress events.

### Done when

- A project with a CLAP instrument on one track and an insert chain on another exports a WAV whose
  null-test against a real-time capture is silence to within dither.
- An engine test renders a sequence twice, once with a fixture insert that writes a DC offset and
  once without, and the two buffers differ. (`crates/dissonant-engine/tests/` already has CLAP
  fixtures: `fixture/filter_plugin.rs`, `fixture/boost_entry.rs`.)
- Export with a missing plugin reports it instead of swapping in a saw.

---

## 2. `restart_audio` keeps instrument plugins

```rust
self.loaded_insert_chains.lock().unwrap().clear();
self.loaded_plugin_uids.lock().unwrap().clear();   // add
self.sync_plugins();                                // add — it calls sync_inserts(true) itself
```

`sync_plugins` ends with `sync_inserts(true)`, so the existing `sync_inserts` call goes. The
comment above the clear already explains the reasoning for chains; extend it to plugins.

**Done when** a test (or a manual device switch) shows a plugin-instrument track still hosted
after the engine restarts at a different sample rate. The plugin is re-opened at the new rate,
which is the point — a unit opened at 44.1 k cannot be handed to a 48 k engine.

---

## 3. A master limiter, and clipping reported before it happens

### Shape

`MasterSettings` gains two fields, both `#[serde(default)]`, so no schema bump and old files open
unchanged:

```rust
/// Ceiling the limiter holds the output under, in dBFS. −0.3 by default.
pub ceiling_db: f64,
/// Off restores the old behaviour: a hard clamp at ±1.0.
pub limiter: bool,   // default true
```

In `Master::process`, replace the clamp with:

1. **Measure first.** `pl/pr` take the peak *before* the limiter, so the meter shows what the mix
   actually is and a track pushed to +6 dB reads +6 dB rather than a pinned bar.
2. **Limit.** A lookahead peak limiter: a 1.5 ms delay line (`dsp.rs` already has `DelayLine`),
   gain computed from the lookahead window's peak against the ceiling, instant attack, program-
   dependent release (~60 ms), applied to the delayed signal. `dsp.rs` already carries
   `EnvFollower` and the oversamplers if the first version proves too clicky.
3. **Safety clamp last**, at ±1.0, after the metronome and test tone are summed in
   (`engine.rs:1087-1165` adds them post-master today) — the device never receives an over, but
   the monitor sounds are not limited into the music and are not metered.

`Shared` gains `master_over: AtomicBool`, latched when a pre-limiter sample exceeds the ceiling,
cleared by a new `EngineCommand::ClearClip`. `AudioStatus` carries it; the transport's `CLIP`
becomes a latched, clickable indicator (`Transport.tsx:300-311`).

### Done when

- An engine test feeds a sequence hot enough to exceed 0 dBFS and asserts the output peak is at or
  under the ceiling while the reported peak is the true pre-limiter level.
- A test asserts `limiter: false` reproduces the old clamp exactly, so the change is opt-outable.
- Render goes through the same `Master`, so the export is limited too — no extra work, but it
  needs a test, because `render.rs` builds its own `Master` from `options.master`.

---

## 4. Atomic save, and a backup

In `save_project` (`lib.rs:967-986`):

1. Write `<target>.tmp`, `sync_all`, then rename over `<target>` — the pattern `autosave()` and
   `library.rs:93-101` already use.
2. Before the rename, if `<target>` exists, copy it to `<target>.bak` (one rolling generation).
3. Only after the rename succeeds: `mark_saved`, `remember_recent`, `clear_autosave`, and remove
   the autosave beside the file. Today a failed write still drops the safety net.
4. If `target` has no extension, append `.dissonant` — the open dialog filters on it
   (`bridge.ts:364-372`), so an extensionless save is currently unopenable.

**Done when** a test writes to a path whose rename fails (read-only dir) and the original file is
still intact and the autosave still present.

---

## 5. The recorder stops lying

Three separate lies in `live_note_off`:

### a. No silent modulo-fold in song mode

Today: `start_in = start % pattern.length_ticks`, so a 32-bar performance over the song collapses
onto a 4-bar pattern with no warning.

Instead, resolve the song tick **through the clip the user is actually hearing**: find the clip
whose pattern is the context pattern and whose span contains `start`; the pattern tick is
`(start − clip.start_tick + clip.offset_ticks) % pattern.length_ticks`. That is exactly where the
note sounded, so a pass over a looping clip records where it was played.

If the playhead is inside no clip of that pattern, the note is **dropped with a toast** ("nothing
of *melody 1* is playing here") instead of being folded somewhere arbitrary. In pattern mode the
old path is already correct and stays.

### b. A take is one undo step

`RecorderState` gains `take: Option<TakeState>` holding the ids written this take. Each note-off
applies `SetNotes` with `transient: true` (the same coalescing every knob drag uses), and
`commit_gesture()` runs on disarm, on stop, and on a mode/pattern switch. Label: "record".

### c. Overdub layers, it does not overwrite

The `notes.retain(…)` dedupe exists to swallow double triggers. Narrow it to **notes written by
the current take** (the ids in `TakeState`), so a second pass over a loop stacks with the first
instead of erasing it.

### d. Record as played (small, same file)

`RecorderSettings` gains `quantize: Option<i64>` where `None` = as played; `set_record` already
takes a quantize argument, so this is a UI chip next to REC plus removing the
`if quantize > 0 … else TICKS_PER_BEAT / 32` fallback. Without it there is no way to record a
feel, and with no triplet grid (a separate phase-2 item) no way to record one at all.

**Done when** `src-tauri/tests/` covers: a take across a clip at bar 17 lands at the right pattern
ticks; two passes over the same loop keep both notes; a 12-note take is one undo step; a note
played where the pattern is not placed is dropped, not folded.

---

## 6. Delete the insert lists `Sequence` never uses

`SequenceTrack.inserts` and `Sequence.master_inserts` are populated from the model
(`sequence.rs:62, 165, 291`) and read by nothing — the engine's chains are `InsertSlot`s installed
by command. After item 1 the render path gets its chains from `RenderPlugins`, so these fields
stay dead and keep implying the renderer is covered. Delete them and their assignments; the
`ipc_contract` / `persistence` tests do not pin them.

---

## Not in this phase

Automation, native effects, sends/buses, plugin GUIs, latency compensation, LUFS metering, mp3,
portable projects. Each is a capability, not a defect. LUFS and the loudness target in particular
want their own pass once the limiter exists, because the two are measured together.

## Documentation

- README phase 3 drops "inserts in offline export" from Open.
- README: "next-chord suggestions" moves from Open to Done (it ships in `Setup.tsx`), and the
  factory count becomes 125 to match `factory_sounds.json`.
- A line in the transport/export rows about the limiter and the latched clip indicator.
