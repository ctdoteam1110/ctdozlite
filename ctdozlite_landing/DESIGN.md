# Design System: ctdozlite Landing Page (Atom One Dark Edition)

## 1. Visual Theme & Atmosphere
A refined developer-first aesthetic built on the iconic **Atom One Dark** color palette. Balances technical precision with the warm, ergonomic dark tones that programmers have trusted for a decade.
- **Density:** Daily App Balanced (Score: 6/10)
- **Variance:** Offset Asymmetric (Score: 7/10)
- **Motion:** Fluid CSS & Weighted Springs (Score: 6/10)

## 2. Color Palette & Roles (Atom One Dark + One Light Adaptive)

### Dark Theme (Default — Atom One Dark)
- **Editor Canvas (`#282C34`)** — Primary page background surface.
- **Sunken Deep (`#21252B`)** — Navigation bar, footer, code blocks, sunken areas.
- **Gutter Border (`#181A1F` / `rgba(171, 178, 191, 0.12)`)** — 1px structural dividing lines.
- **Surface Elevation (`#2C313A`)** — Card containers, modal boxes, table headers.
- **Surface Hover (`#353B45`)** — Interactive card hover and raised elements.
- **Foreground Text (`#ABB2BF`)** — Body text, descriptions, secondary copy.
- **Bright Headline (`#DCDFE4` / `#E5E5E6`)** — High-contrast display headlines and active titles.
- **Comment Grey (`#5C6370`)** — Metadata, captions, inactive indicators.
- **One Dark Blue Accent (`#61AFEF`)** — Primary action CTA, focus rings, link accents.
- **One Dark Cyan (`#56B6C2`)** — Benchmark numbers, success metrics, status tags.
- **One Dark Green (`#98C379`)** — OK indicators, terminal prompt success states.

### Light Theme (Atom One Light)
- **Canvas White (`#FAFAFA`)** — Primary background.
- **Surface Light (`#FFFFFF`)** — Cards and containers.
- **Sunken Light (`#EAEAEB`)** — Nav, table headers, code backdrop.
- **Text Primary (`#202227`)** — High-contrast text.
- **Text Secondary (`#383A42`)** — Body copy.
- **Text Muted (`#A0A1A7`)** — Captions and metadata.
- **One Light Blue Accent (`#4078F2`)** — Primary actions and focus.
- **One Light Cyan (`#0184BC`)** — Metrics and highlights.

## 3. Typography Rules
- **Display & Headlines:** `Geist` — track-tight (-0.03em), controlled scale.
- **Body & Prose:** `Geist` / System Sans — 1.6 line-height, comfortable reading.
- **Monospace Engine:** `JetBrains Mono` — used for commands, CLI snippets, hex values, file trees, table metrics.

## 4. Component Stylings
- **Buttons:**
  - *Primary:* Solid Atom Blue (`#61AFEF` dark / `#4078F2` light), off-black or white text, 8px border-radius, zero neon bloom.
  - *Ghost / Secondary:* Surface fill with subtle border, hover state shifts to Surface Hover (`#353B45`).
- **Cards & Surfaces:**
  - 1px border using One Dark gutter tones (`#181A1F` or border wire).
  - Subtle dark shadows (`0 32px 70px -28px rgba(0, 0, 0, 0.65)`).
- **Code & Terminal:**
  - Background set to Sunken Deep (`#21252B`), code styled with Atom One Dark syntax highlights (Cyan `$`, Blue commands, Green OK tags).
