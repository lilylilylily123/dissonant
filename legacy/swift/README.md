# Legacy Swift prototype (frozen)

This is the original macOS-only SwiftUI + AudioKit prototype of Dissonant (v0.1), kept for
reference while the cross-platform Rust + Tauri port at the repository root reaches parity.

It is not built or maintained. The harmonic-guidance logic in `DissonantCore/` was ported
one-to-one (with its tests) to `crates/dissonant-core`. See
`docs/decisions/2026-10-02-migrate-to-rust-tauri.md` for why and how.

To build it anyway: install XcodeGen, run `xcodegen generate` in this folder, open
`Dissonant.xcodeproj`.
