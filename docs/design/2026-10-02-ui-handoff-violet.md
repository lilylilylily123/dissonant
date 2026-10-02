<!-- Imported from the Dissonant DAW UI handoff bundle (Claude Design, violet direction). The .dc.html mock itself is not in the repo; this is the written spec the UI follows. -->

# Handoff: Dissonant DAW — Arrangement (Home) + Piano Roll

## Overview
Dissonant is a desktop DAW aimed at hard electronic music (gabber, hardcore, distorted electroclash). This package specifies two views:

1. **Arrangement / Home** — timeline of tracks and clips, sample browser, and a bottom panel that switches between a **device chain** and a **mixer**.
2. **Piano Roll** — MIDI note editor with an inspector, keyboard, note grid, velocity lane and CC automation lane.

The visual language borrows on purpose from Ableton Live (device chain, clip coloring, inspector), FL Studio (piano roll slide/accent, PAT/SONG mode), Logic and Reaper (track headers, mixer strips). Don't reinvent standard DAW conventions.

## About the Design Files
The files in this bundle are **design references created in HTML**. They are prototypes that show the intended look and behavior, not production code to copy. The task is to **recreate these designs in the target codebase's environment** (e.g. React + Canvas/WebGL, JUCE, Qt, Tauri, egui, whatever the app uses) with its own patterns. If there's no codebase yet, choose a framework that fits a real-time audio app. Grid, waveform, note and meter rendering should almost certainly go to Canvas/GPU, not DOM.

Open `Dissonant DAW - Violet.dc.html` in a browser. It needs `support.js` next to it. Both artboards render at 1920×1080.

## Fidelity
**High-fidelity.** The colors, type, sizes and layout are final. Match them pixel for pixel at 1920×1080, 1× scale. The mock data (track names, clips, notes, knob values) is illustrative content. It isn't a spec.

---

## Design Tokens

### Color — surfaces (darkest → lightest)
| Token | Hex | Use |
|---|---|---|
| bg-void | `#08080a` | LCD displays, scopes, meter wells, graph backgrounds |
| bg-0 | `#0b0b0d` | App root, inset fields, row separators |
| bg-0b | `#0e0e11` | Menu bar, status bar, scrollbar troughs |
| bg-lane | `#0f0f12` | Track area background, bottom panel |
| bg-editor | `#101013` | Piano roll editor column |
| bg-1 | `#121215` | Side panels (browser, inspector), ruler |
| bg-lane-row | `#131316` | Clip lane background (selected track: `#16161b`) |
| bg-transport | `#141417` | Transport bar, piano-roll toolbar, lane headers |
| bg-2 | `#16161a` | Track headers, devices, mixer strips |
| bg-2-sel | `#202027` | Selected track header (`#1c1c22` selected mixer strip) |
| bg-3 | `#1c1c21` | Device title bars |
| bg-control | `#1e1e24` / `#202026` | Buttons, knob caps, inactive toggles |

### Color — lines
| Token | Hex |
|---|---|
| line-0 | `#1e1e24` (subtle dividers) |
| line-1 | `#26262d` (panel borders, default) |
| line-2 | `#2a2a31` (device borders, knob track) |
| line-3 | `#2e2e36` (button outlines) |
| line-strong | `#3a3a44` (selected strip border) |

### Color — text
| Token | Hex | Use |
|---|---|---|
| text-0 | `#f2f2f4` | Logo |
| text-1 | `#e6e6ea` | Primary text, values |
| text-2 | `#b8b8c0` | Secondary values |
| text-3 | `#9a9aa4` | Menus, labels, inactive button text |
| text-4 | `#7a7a84` / `#6a6a74` | Section labels, metadata |
| text-5 | `#5f5f68` | Small caps headings, track numbers |
| text-6 | `#4a4a52` | Faint scale labels |

### Color — accent (VIOLET — the chosen direction)
| Token | Hex | Use |
|---|---|---|
| accent | `#b48cff` | Play button, active toggles, BAR position LCD, loop brace, selected browser item, active tool, active tab underline, power LEDs, CC lane curve, scale root |
| accent-rgb | `180,140,255` | For glows: `box-shadow: 0 0 14px rgba(180,140,255,.35)` on the play button |
| accent-bg | `#261c3a` | Background of an "on" toggle chip (text = accent) |
| accent-border | `#5a3f8f` | Outline of an "on" toggle (e.g. MET, AUTO preview) |
| on-accent | `#0b0b0d` | Text on a solid accent background |

### Color — functional
| Token | Hex | Use |
|---|---|---|
| rec | `#ff3b30` | Record, clip indicator, arm, distortion device, GR on limiter |
| rec-bg / rec-border | `#3a1412` / `#5a1d1a` | Armed record button bg / outline; text on that bg `#ff6a5e` |
| warn | `#ff6a2a` | BPM readout, sampler knob arcs, comp GR |
| mute | `#ffb02a` | Mute button ON (text `#0b0b0d`) |
| accent-note | `#ff9d2a` | Accent marker on piano roll notes; accent chip bg `#3a2412` |
| ok | `#3dffb0` | READY status, CPU bar, meter bottom |
| info | `#3dc8ff` | EQ curve, ghost notes, limiter arcs, RAM bar |

### Meter gradient
`linear-gradient(to top, #3dffb0 0%, #b48cff 70%, #ff3b30 100%)` (vertical). The horizontal master meter is `linear-gradient(90deg,#3dffb0,#b48cff 70%,#ff6a2a 88%,#ff3b30)`. The unlit part is a cover of `#0b0b0d`/`#08080a` drawn from the top (or right) down to the level.

### Track color palette (12)
`#ff3b30` `#ff6a2a` `#ff9d2a` `#ffd02a` `#e2ec3e` `#b8f53a` `#3dffb0` `#3dc8ff` `#5a8cff` `#9a7bff` `#ff4fd8` `#8a8a94` (the last is for returns)
- Clip header: solid track color, text `#0b0b0d`.
- Clip body: track color at **~14% alpha** (`hex + "24"`).
- Clip outline: inset 1px, track color at 40% alpha (`hex + "66"`). When selected, `#ffffff`.
- Waveform / MIDI preview fill: track color, opacity .9.

### Typography
- **UI:** `IBM Plex Sans Condensed` 400/500/600/700
- **Numeric / data:** `JetBrains Mono` 400/500/700. Use it for every number, timecode, dB value, filename, note name and shortcut.

| Role | Spec |
|---|---|
| Logo | Plex Cond 700, 11.5px, letter-spacing .22em, uppercase, preceded by 9×12 accent parallelogram (skewX −16°) |
| Menu | Plex Cond 400, 12px, `#9a9aa4`, item padding 4×8, hover bg `#1f1f25` |
| Section heading (small caps) | Plex Cond 600, 8.5px, letter-spacing .14em, uppercase, `#5f5f68` |
| Field label | Plex Cond 600, 9px, ls .1em, `#6a6a74` |
| Track name | Plex Cond 600, 12px, `#e6e6ea` (muted: `#6a6a74`) |
| Device title | Plex Cond 700, 10.5px, ls .08em, uppercase |
| Button text | Plex Cond 600–700, 9–10px, ls .04–.1em, uppercase |
| Clip name | Plex Cond 700, 9px/12px, ls .03em |
| Position LCD | JetBrains Mono 500, 22px, accent |
| BPM LCD | JetBrains Mono 500, 20px, `#ff6a2a` |
| Values | JetBrains Mono 400, 9–10.5px |
| Base body | 11px |

### Radius, spacing, shadows
- Radius: **2px** for chips, clips, fields and notes. **3px** for buttons, devices and panels. 50% for LEDs and meters' dots. Nothing larger.
- Spacing is dense. Gaps of 2/3/4/6/8/10/12px. Panel padding is 8px (devices) or 10–12px (inspector sections).
- Shadows are only used as glows: the play button `0 0 14px rgba(accent,.35)`, LEDs `0 0 5px accent`, the playhead `0 0 6px rgba(255,255,255,.5)`, the record dot `0 0 8px #ff3b30`. No drop shadows on panels.

### Knob (shared component)
- 34×34 SVG (30×30 in mixer). Center 17,17.
- Track: circle r=13, stroke `#2a2a31` 3px, 270° sweep. Use dasharray `61.26 200` rotated 135°, so the arc starts bottom-left.
- Value arc: same circle, dasharray `(p × 61.26) 200`. Its color depends on the device: sampler `#ff6a2a`, distortion `#ff3b30`, comp/multiband = accent, limiter `#3dc8ff`, instrument `#3dffb0`, mixer `#9a9aa4`.
- Cap: circle r=8 fill `#202026`. Pointer: line from center to radius 9 at angle `135° + p·270°`, stroke `#e6e6ea` 1.6px, round cap.
- Under the knob: label (Plex Cond 600 8.5px ls .08em `#7a7a84`), then the value (Mono 9px `#e6e6ea`).
- Interaction (to implement): vertical drag, shift = fine, double-click = reset, scroll = step.

---

## Global Chrome (both screens)

App frame: 1920×1080, column flex.

### 1. Menu bar — 28px
bg `#0e0e11`, bottom border `#222228`, padding 0 10px.
- Left: logo, then the menus: File, Edit, Create, View, Track, Clip, MIDI, Mixer, Options, Window, Help.
- Right (Mono 10.5px `#6a6a74`, gap 18px): project file `VOID_SIGNAL_v7.dsn` (`#b8b8c0`), `48 kHz · 24-bit`, audio device + buffer + latency, `● AUTOSAVED 2m` (`#3dffb0`).

### 2. Transport — 56px
bg `#141417`, bottom border line-1, padding 0 12px, gap 12px, items centered. From left to right:
1. **MODE** label, then a segmented control `PAT | SONG`. The active segment is accent bg with `#0b0b0d` text, the inactive one `#1a1a1f`/`#7a7a84`.
2. 1px divider, 36px tall.
3. **Transport buttons** (30px tall, gap 4px, `#1e1e24` bg, line-3 border, radius 3): Return-to-start 34w, Stop 34w (11px square glyph), **Play 44w** (solid accent, black triangle, glow), Record 34w (red dot with glow, border `#5a1d1a`), Loop 34w (on: accent-bg with accent-border and accent glyph).
4. Toggle chips: `MET` (on), `CNT-IN 1`, `OVR`, `AUTO W` (red outline, `#ff6a5e`).
5. **Position LCD** (42px tall, bg-void, border line-1): `BBB.B.SS` (e.g. `022.2.01`) in Mono 22px accent, with micro-labels BAR/BEAT/16TH below it (7.5px, ls .14em, `#4a4a52`). A divider, then the time `MM:SS.mmm` (Mono 13px `#b8b8c0`).
6. **BPM LCD**: `180.000` (Mono 20px `#ff6a2a`) + `TAP` button.
7. Two 2×2 grids: SIG `4 / 4`, KEY `F min`, SWING `54%`, GRID `1/16`.
8. flex spacer.
9. **Scope** 180×36 (master oscilloscope, accent stroke 1.2px, center line `#1c1c22`).
10. **Spectrum** 164×36: 40 bars, 3px wide with 1px gaps. Bar color by height: low = `#3dffb0`, mid = accent, high = `#ff6a2a`.
11. CPU / RAM / DISK rows: label + 60px bar (5px tall) + Mono value.
12. **Master meter** 150w: header `MASTER` + `CLIP -0.1` (red), two 6px horizontal bars, and a scale `-48 -24 -12 -6 0`.

### 3. Status bar — 22px
bg `#0e0e11`, top border `#222228`, Mono 10px `#6a6a74`. Left: `● READY` (green) and a context string. Right: `UNDO: …`, `PDC 2.1 ms`, `48 kHz`, `CPU 38%`.

The content area between the transport and status bar is **974px** tall.

---

## Screen 1 — Arrangement / Home

Layout: row flex → **Browser 248px** | **Center column (flex 1 = 1672px)**.

### Browser (248px, bg-1, right border line-1)
- **Tabs** (28px): FILES (active: `#e6e6ea`, 2px accent bottom border), PLUGINS, PRESETS, CLIPS (`#6a6a74`). Plex Cond 600 10px ls .08em.
- **Search** (24px field, bg-0, border line-2, radius 3): magnifier icon, query text, then `⌘F` hint on the right.
- **Tag chips**: active ones are accent bg + black text, inactive `#1e1e24`/`#9a9aa4`. 2×6 padding, 9.5px 600.
- **PLACES** list (20px rows): a 6px color square, the name, and a Mono count on the right.
- Divider, then a **file tree** (20px rows, 12px indent per level). Folders get a ▾/▸ chevron and a square grey dot. Files get a round dot (`#ff3b30` at 50%) with Mono duration on the right. The selected row has bg accent-bg and accent text.
- **Preview** (bottom, bg `#0f0f12`, top border): an AUTO (on) / SYNC toggle, a 54px waveform (red fill) with a white play cursor, then filename/duration and format/root/peak lines in Mono 9–9.5px.

### Center column
#### Ruler — 44px (bg-1)
- **Left corner, 220px:** row 1 has `11 TRACKS` (nowrap) plus `+ AUDIO` and `+ MIDI` mini buttons. Row 2 has `FOLLOW` (on), `SNAP BAR`, `A` (automation view) and a zoom readout `1:48`.
- **Timeline, 1440px wide = 48 bars @ 30px/bar** (beat = 7.5px):
  - y 2–15: **Arrangement markers** (INTRO, BUILD, DROP A, BREAK, DROP B). Each has a 2px left border in the marker color, a fill of that color at 10% alpha, and its label in Plex Cond 700 9px ls .1em in the marker color. Drops are `#ff3b30`, build `#ff9d2a`, break `#3dc8ff`, intro `#9a9aa4`.
  - y 17–25: **Loop brace**, solid accent at 85% opacity (bars 21–25 in the mock).
  - y 26–44: bar numbers in Mono 9px. Every 4th bar is brighter (`#c8c8d0`, tick `#4a4a52`); the others are `#5f5f68` with tick `#26262d`.
  - The playhead marker is a 10×7 white downward triangle.
- 12px vertical scrollbar column on the right.

#### Track list (flex 1)
Each row is a horizontal flex: **header 220px** + **lane 1440px**. Track rows are 50px; automation sub-lanes are 40px; row separators are 1px `#0b0b0d`.

**Track header (220×50)**
- 4px track-color strip on the left.
- Line 1: Mono 9px track number `01`, then the name (600 12px, ellipsis), then a type badge `AUDIO|MIDI|RETURN` (8px, 1px line-2 border).
- Line 2: `M` / `S` / `R` buttons, each 17×15, radius 2, 9px 700.
  - M on: `#ffb02a` with black text.
  - S on: `#3dc8ff`.
  - R armed: bg `#3a1412` with a 6px red dot. R off: `#202026` with a `#4a4a52` dot.
  - Then a volume mini-slider (4px track `#26262d`, fill `#6a6a74`, 3×10 white handle) and the dB value (Mono 9px, 30px right-aligned).
- Right edge: stereo meter, two 5×38 bars with the meter gradient.

**Lane (1440×50)**
- Background has three repeating vertical gridlines: every 120px (4 bars) `#2a2a31`, every 30px (bar) `#1e1e24`, and every 7.5px (beat) `#17171b`.
- **Clip:** absolute, top 2, height 45, radius 2, overflow hidden.
  - 12px header strip in the track color, holding the clip name.
  - 33px body showing the audio waveform (a mirrored filled envelope) or a MIDI note preview (3px-tall note bars). Both are drawn in the track color.

**Automation lane (40px)**
- The header is indented 22px with a dimmed color strip at 40%. It shows `↳ Filter Cutoff` in the track color, a `READ` badge (solid track color), and the device path + current value in Mono.
- The lane shows a breakpoint polyline (1.5px stroke) with a 10% fill below it. Nodes are 7px circles with a `#0b0b0d` fill and a 1.5px border in the track color.

After the last track there's a 40px "+ drop plugin or sample to create track" row.

**Playhead:** a 1px white line spanning the ruler and all lanes, with a glow.

#### Bottom panel — 300px
Top border is 2px line-1, bg `#0f0f12`.
- **Tab bar (26px, bg-1):** `DEVICES` and `MIXER` tabs. The active tab is `#e6e6ea` with a 2px accent underline (inset shadow). After a divider comes the selected track chip (color square + `01 GABBER KICK`), then Mono metadata (`6 devices · 4× OS · latency 2.1 ms`). The right side has `MACROS`, `RACK ▾`, `SAVE CHAIN`.

**DEVICES view.** A horizontal chain with padding 8 and gap 6. Each device is a card: bg-2, border line-2, radius 3. Its 22px title bar (bg-3) has a power LED (7px accent with glow), the name, and right-aligned Mono metadata.
1. **KICK SAMPLER (330w):** 78px waveform display (red 1.1px stroke, distorted kick), with an accent start marker labeled `S` and the end region dimmed (black at 55%, `#ff6a2a` left edge). Mode chips: ONE-SHOT (on), LOOP, REV, ROOT F#1, CHOKE 1. Six knobs: TUNE, DECAY, ATTACK, START, VEL, GAIN.
2. **TORTURE distortion (300w):** title bar tinted `#1f1513` with border `#3a221e` and title `#ff6a5e`. Mode segmented `HARD|FOLD|TUBE|CRUSH|RECT`, where the active one is a red fill with black text. A 104×104 transfer-curve display (red 1.8px curve, dashed unity diagonal). A 3×2 knob grid: DRIVE, TONE, BIAS, FOLD, MIX, OUT. IN/OUT mini meters along the bottom.
3. **PARA EQ (360w):** 160px graph on a log frequency axis from 20 Hz to 20 kHz, ±18 dB.
   - Horizontal gridlines at ±6/±12 (`#18181d`); 0 dB is `#2a2a31`.
   - Vertical lines at 50, 100, 500, 1k, 5k, 10k, labeled in Mono 8px.
   - Analyzer fill is white at 5%; the EQ curve is a `#3dc8ff` 1.8px stroke with a 12% fill.
   - Band handles are 15px circles with a `#3dc8ff` border. The selected handle is filled `#3dc8ff` with a black number.
   - Below the graph, a 4-column band summary (type / freq / gain). The selected band uses bg `#10232b`.
4. **GLUE COMP (250w):** 3×2 knobs (THRESH, RATIO, ATTACK, RELEASE, MAKEUP, KNEE), detector chips PEAK / RMS (on) / LOOK 1ms, and a vertical GR meter (orange, filled from the top, value `-7.4`).
5. **MULTIBAND (210w):** HIGH/MID/LOW rows, each a 12px bipolar bar from a center line. Downward compression (red) extends left, upward (accent) extends right. Three knobs: DEPTH, TIME, OUT.
6. **LIMITER (150w):** CEIL and GAIN knobs and a red GR meter.
7. The remaining space is a dashed add-device slot with a `+`.

**MIXER view.** Strips sit in a row (padding 8, gap 4). Track strips are 104w; the master strip is 132w. From top to bottom, each strip has:
- a 3px color cap
- an 18px row with the number + name
- 4 insert slots (14px, bg-0, 4px LED; empty slots show `—` in `#3a3a42`)
- a row with two 30px knobs (Send A, PAN)
- the fader area (flex): a 3px track and a 23×12 cap with a gradient `#5a5a62→#2e2e34`, a `#6a6a74` border and a white center line, plus an L/R meter pair on the right
- a 16px dB readout (Mono on bg-void)
- an M / S / R button row

The selected strip uses bg `#1c1c22` with border line-strong.

---

## Screen 2 — Piano Roll

Layout: row flex → **Inspector 300px** | **Editor (1620px)**.

### Inspector (300px, bg-1, sections separated by line-1, padding 10×12)
1. **CLIP**
   - Header `MIDI · 4 BARS`.
   - Clip name bar: 28px, solid clip color, black 700 13px text.
   - A row of 12 swatches, 12px tall. The selected one gets a double ring: `0 0 0 1px #0b0b0d, 0 0 0 2px #fff`.
   - Fields: TRACK, START, END, LENGTH, LOOP (accent value), SIGNATURE. Each is a label with a right-aligned 96px Mono value box (bg-0, border line-1, radius 2).
2. **NOTES** (`12 SELECTED` badge: light bg `#e6e6ea`, black text)
   - Range rows: PITCH, VELOCITY, LENGTH, CHANCE, RELEASE, PAN. Each has a 64px label, a 5px range bar showing the min–max span of the selection, and a 62px Mono value.
   - Chips: `SLIDE 4` (bg `#13342a`, text `#3dffb0`), `ACCENT 6` (bg `#3a2412`, text `#ff9d2a`), `MUTE`.
3. **SCALE**
   - Root and mode selectors (`F`, `Minor ▾`).
   - A 12-cell chromatic strip, 30px tall. The root cell is solid accent, in-scale cells are accent-bg with accent text, out-of-scale cells are `#16161a`/`#3a3a42`.
   - Chips: HIGHLIGHT (on), FOLD, SNAP TO KEY (on).
4. **TRANSFORM:** a 3×3 grid of buttons: QUANTIZE, LEGATO, CHOP, FLAM, RANDOM, REVERSE, ×2 SPEED, ½ SPEED, INVERT. Hover is accent border + accent text. Below are Q AMOUNT and HUMANIZE sliders.
5. **INSTRUMENT:** a device chip (`ACID 303` · preset `SQUELCH_FM_04`) and 5 knobs (CUTOFF, RESO, ENV, DECAY, ACCENT) with `#3dffb0` arcs.
6. The bottom of the column is pinned with **TRACK OVERVIEW**: a 22px mini-map of the track's clips across the song. The clip being edited is solid with a white ring; the others are at 33% alpha.

### Editor
- **Toolbar (36px, bg-transport):**
  - Title: color square, `PIANO ROLL`, then Mono `07 ACID 303 / ACID_LINE_A`.
  - Tool segmented: SELECT V, DRAW P, PAINT B, SLICE C, ERASE D, MUTE T. The active tool is solid accent with black text; the others are `#1a1a1f`/`#9a9aa4`. Shortcut letters are Mono at 60% opacity.
  - Option chips (label + value): SNAP `1/16 ▾`, GHOST `REESE BASS ▾` (info-blue tint: bg `#0f1f26`, border `#1d4a5c`), CHORD `OFF ▾`, ARP `OFF`, STRUM `0%`, LEGATO.
  - Right side: a ZOOM slider and a `● MIDI IN · <device>` chip (red outline).
- **Ruler (28px):** a 64px corner showing the grid value, then a 1536px timeline.
  - A 6px clip-loop bar along the top in the clip color.
  - Bar labels `21` (Mono 600 11px `#e6e6ea`, tick `#5a5a62`) and beat labels `21.2` etc. (9px `#6a6a74`).
  - A white playhead triangle.
- **Grid row (666px = 37 rows × 18px, C2–C5):**
  - **Keyboard, 64px.** Rows are uniform at 18px. White keys are `#d4d4d9`. Black keys are a 40px `#0e0e10` block over the white. Row separators are 1px `#8a8a92`. C rows are labeled (`C3`) in Mono 600 8.5px; the root (F) is marked `F`. A sounding key is filled with accent.
  - **Note grid, 1536px = 4 bars; 1 bar = 384px, beat = 96px, 16th = 24px.**
  - Row backgrounds when scale highlight is on:

    | | white-key row | black-key row |
    |---|---|---|
    | Root | `#261d3a` | `#1f1830` |
    | In scale | `#1a1622` | `#141119` |
    | Out of scale | `#121214` | `#0e0e10` |

    With highlight off, rows are `#17171b` (white) / `#121215` (black).
  - Row separator is `#0b0b0d`; octave boundary (at C) is `#2e2e36`.
  - Vertical gridlines overlay: bar `#3a3a44`, beat `#26262e`, 16th `rgba(255,255,255,.035)`.
  - The current bar of interest gets a faint `rgba(255,255,255,.025)` band.
  - **Notes:** 17px tall, radius 2, 1px border.
    - Unselected: fill `rgba(61,255,176, .45 + .55·vel/127)` (opacity tracks velocity), border `#1a9e6c`.
    - Selected: fill `#c9ffe9`, border `#fff`, outer 1px white ring at 40%.
    - The note name is drawn inside in Mono 600 8px `#04140d` when the width allows.
    - **Accent** (vel ≥ 118): a 3px `#ff9d2a` bar on the left edge.
    - **Slide** (FL-style portamento): a 7px dark triangle in the top-right corner.
  - **Ghost notes** from another track: a 1px dashed `rgba(61,200,255,.45)` border with a 6% fill and the label `REESE F2`. They aren't interactive.
  - Marquee selection: 1px dashed white at 40%.
  - Playhead: 1px white with a glow. It continues through the velocity and CC lanes at 60%.
  - Vertical scrollbar: a 20px column with a `#2a2a31` thumb, radius 4.
- **Velocity lane:** a 22px header bar with tabs VELOCITY (active), RELEASE, CHANCE, PAN, FINE and a Mono summary on the right. The 110px lane (bg `#0c0c0f`) has a 64px scale column (127/64/0). Each note gets a lollipop: a 3px stem plus a 7×4 head. Selected stems are `#e6fff4`; the others are `#3dffb0`.
- **CC lane:** a 22px header (`CC · 303 CUTOFF ▾` in accent, alternatives RESO / ENV MOD / PITCH BEND, current value right). The 88px lane holds the breakpoint polyline (accent 1.6px, 10% fill) and 8px nodes.

---

## Interactions & Behavior
Implemented in the mock:
- Bottom-panel tabs switch DEVICES ↔ MIXER.
- Piano-roll tool buttons set the active tool.

To build (standard DAW behavior, follow Ableton/FL conventions):
- **Transport:**
  - Space plays/stops.
  - Enter returns to start.
  - R records.
  - L toggles the loop.
  - The position LCD and time update live (BPM 180, 4/4).
  - Clicking the ruler moves the playhead; dragging the loop brace sets the loop range.
  - PAT/SONG switches between playing the selected pattern and the full arrangement.
- **Arrangement:**
  - Drag clips, edge-drag to resize, Alt-drag to duplicate. Snap follows the grid.
  - Double-clicking a MIDI clip opens the Piano Roll.
  - Selecting a track updates the bottom panel.
  - M/S/R toggle, the volume slider drags.
  - You can drag from the browser onto a lane.
- **Piano roll:**
  - DRAW click adds a note at the grid length; drag moves it; drag the right edge to resize.
  - Velocity stems drag vertically.
  - Ctrl/⌘-drag marquee selects.
  - Arrow keys transpose (Shift = octave).
  - Q quantizes; S toggles slide on the selected notes.
  - The scale toggles change row highlighting and constrain input.
- **Hover:** menu items get bg `#1f1f25`; transform buttons get the accent border. Default transitions should be instant or ≤80 ms, since a DAW should feel immediate.
- **Meters / scope / spectrum** animate in real time (~60 fps, peak hold ~1.5 s).

## State Management (suggested)
- Transport: `isPlaying`, `isRecording`, `loopEnabled`, `loopStart/End`, `positionBeats`, `bpm`, `timeSig`, `mode: 'pattern'|'song'`.
- Project: `tracks[] {id, name, type, color, volumeDb, pan, mute, solo, armed, devices[], clips[], automation[]}`.
- Clip: `{id, start, length, name, color, kind:'audio'|'midi', loop, notes[] | audioRef}`.
- Note: `{start, length, pitch, velocity, slide, release, chance, pan, fine}`.
- UI: `selectedTrackId`, `selectedClipIds`, `selectedNoteIds`, `bottomPanel:'devices'|'mixer'`, `pianoRollTool`, `snap`, `ghostTrackId`, `scale {root, mode, highlight, fold, snap}`, zoom/scroll per view.

## Assets
- Fonts: IBM Plex Sans Condensed and JetBrains Mono (Google Fonts / OFL).
- There are no image assets. Icons are minimal vector glyphs (play, stop, record, loop, return-to-start, search) and should be replaced with the codebase's icon set at matching stroke weights (1.3–1.6px).
- Waveforms, curves and meters in the mock are procedurally generated placeholders. In the real app they come from audio and analysis data.

## Files
- `Dissonant DAW - Violet.dc.html` is the **final reference** (violet accent). It shows artboard 1a (Arrangement) and 1b (Piano Roll).
- `support.js` is the runtime needed to open the .dc.html file in a browser.
- The tweakable props in the mock are `bottomPanel` (Devices/Mixer), `playheadBar` (1–49), `showGhostNotes`, and `scaleHighlight`.
