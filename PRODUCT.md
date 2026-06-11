# Product

## Register

product

## Users

A player, not a producer. Someone who can pick up a guitar or bass and sing — and wants
to record DI and vocals through an audio interface — but doesn't have the theory to
compose or arrange around what they played. They hear music in their head (the lo-fi,
experimental, texture-driven world of acts like ear, Bassvictim, and Water From Your Eyes)
and hit a wall in every existing DAW, where the piano roll assumes theory knowledge they
don't have. They're not nervous about depth or weirdness — they're blocked by tools that
demand the exact knowledge they're missing. Their job: turn the parts they *can* play into
finished, fleshed-out tracks, learning as they go.

## Product Purpose

"in key" is a native macOS DAW that surfaces the harmonic landscape directly in the
interface instead of hiding it behind theory. Its core is a familiar piano roll whose notes
re-tier in real time — solid chord tones, spicy-but-good tensions, flagged dissonance —
against a guided chord track the player builds. It makes creating *and* learning happen at
the same time. Success looks like a beginner going from a blank project to a good-sounding
loop in minutes, finishing tracks they'd otherwise abandon, and reaching for flagged-
dissonant notes on purpose. Dissonance is always labeled, never blocked — going "wrong"
stays a deliberate choice, because the music this tool exists to make is built on it.

## Brand Personality

Raw, characterful, opinionated. The app should come across as an instrument with attitude —
a little gritty, a little DIY, with a clear point of view that matches the experimental
music it's for. Three words: **raw, honest, alive.** It respects the user enough to show
them real musical structure rather than smoothing everything into beginner pap. The voice
is direct and unpretentious — it names things plainly ("that note's dissonant — could be
exactly what you want"), never condescends, and treats happy accidents and deliberate
weirdness as the point, not errors to correct.

## Anti-references

- **Childish / toy** — GarageBand-for-kids, big candy buttons, rounded cartoon mascots,
  confetti. Accessible must never mean infantilizing.
- **Generic SaaS dashboard** — cards-on-grey, corporate blue, Inter-everything, the
  soulless startup web-app look. No personality, could belong to any company.
- **Sterile / clinical** — cold all-white, over-minimal "Apple stock" neutrality with no
  warmth or grit. At odds with the music's character.
- (Not an anti-reference: pro-tool depth. Density and real musical structure are welcome —
  the product makes them *legible*, it does not strip them out.)

## Design Principles

- **Legible, not dumbed down.** The fix for an intimidating DAW isn't fewer features — it's
  guidance that makes real musical structure readable. Show the harmony, label it, teach
  through use. Never solve complexity by hiding it.
- **Has a point of view.** Personality over neutrality. The interface should feel like it
  was made by someone who loves this music, not assembled from a component library. Grit
  and character are features, not noise to sand off.
- **Guidance, never guardrails.** Label the dissonant note; never disable it. The "wrong"
  choice is always one click away, and the design should actively invite going weird —
  flagging is an offer, not a fence.
- **Meaning twice, never color alone.** The three-tier note system is the product; it must
  read through shape, pattern, label, or border in addition to color, so it survives color
  blindness and small sizes. Accessibility here is core mechanic, not a later pass.
- **Fast to the first good loop.** Every screen should shorten the path from blank project
  to something that sounds good. Lower the stakes, reward poking around, make finishing feel
  reachable.

## Accessibility & Inclusion

The three-tier note coloring (solid chord tone / tension / dissonance) must be
color-blind-safe and carry a non-color cue — a distinct shape, pattern, border, or label
per tier — since red-green color blindness affects ~8% of men and the tiers are the core
mechanic. Target WCAG AA contrast for text and meaningful UI. Honor reduced-motion: any
playback or transition animation needs a calm, non-motion alternative. Native macOS
expectations apply — VoiceOver labels on interactive controls and full keyboard operability
for note and chord editing.
