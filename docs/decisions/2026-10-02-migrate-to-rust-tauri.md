---
title: Migrate from Swift/macOS to Rust + TypeScript (Tauri)
status: accepted
date: 2026-10-02
---

# Decision: cross-platform rewrite on Rust + Tauri

## Context

Dissonant 0.1 was a native macOS SwiftUI app on AudioKit (~3.6k lines). Two things pushed a
stack change at the same time:

1. **Reach.** The product should not be Mac-only. Windows and Linux producers are a large part
   of the FL Studio / Ableton audience the product competes for.
2. **Foundations.** The full-DAW brainstorm (`docs/brainstorms/2026-10-02-full-daw-brainstorm.md`,
   §2) found that the prototype's transport was a UI timer and the synth was built from
   per-voice AudioKit nodes — both had to be rewritten for sample-accurate sequencing anyway.
   Rewriting them once, in the new stack, costs less than twice.

## Options considered

| Option | Verdict |
|---|---|
| **Rust engine + TypeScript UI in Tauri** | **Chosen.** Native desktop on macOS/Windows/Linux; Rust owns audio (cpal), the scheduler and DSP; the UI is TypeScript on Canvas. Everything compiles and tests headlessly in CI. Plugin hosting via CLAP first. The Rust core can compile to WASM later for a browser build. |
| C++ with JUCE | Industry standard with VST3/AU hosting built in, but slower to develop, GPL/commercial license, dated UI toolkit. |
| Browser-first (TS + Rust/WASM AudioWorklet) | Zero install, but no plugin hosting and higher input latency — weak for the DI/vocal recording pillar. Still reachable later from this stack. |
| Stay on Swift | Mac-only, and not buildable outside Xcode. |

## What was ported

- `DissonantCore` → `crates/dissonant-core`: model (now schema v3 with per-note velocity and
  persisted master settings), three-tier classifier, Krumhansl–Schmuckler key detection,
  highlight engine, chord track, pattern arrangement, plus a new `Document` with commands and
  undo/redo. All XCTest suites ported to Rust tests.
- Audio → `crates/dissonant-engine`: a new block processor with sample-accurate event
  scheduling, PolyBLEP synth voices with velocity, procedural drums, per-track bus with a
  working pan, master chain, chord bed, offline WAV render (faster than real time) and a cpal
  output thread.
- App shell → `src-tauri`: commands, playback context, playhead events, file I/O, export.
- UI → `ui/` (Vite + React + TypeScript): every surface of the prototype, plus full piano-roll
  editing (move / resize / select / marquee / copy / paste / duplicate / velocity / zoom /
  ruler seek), chord drag/resize/delete, song reorder, undo/redo, meters.

## What was dropped (for now)

- Audio Unit hosting. Replaced on the roadmap by CLAP (then VST3) hosting in Rust.
- Reading v1/v2 `.dissonant` files written by the Swift app. The JSON layout is the same
  except Swift encoded `notesByTrack` as a flat `[key, value, …]` array; an importer is a
  small follow-up.

## Consequences

- Build: `pnpm install`, `pnpm --dir ui install`, `pnpm dev` (needs Rust + Node; Linux needs
  the webkit2gtk/alsa dev packages listed in the README).
- The UI mirrors ~150 lines of theory in TypeScript for 60 fps rendering; its tests pin the
  same expectations as the Rust tests. Moving to a WASM build of `dissonant-core` removes the
  duplication when the theory grows (brainstorm §4).
- The Swift prototype lives in `legacy/swift/`, frozen, until the port reaches parity.
