# Flight Deck — qpv2-gui Design Language

The wallet is styled as a precision instrument: a cockpit / market
terminal, not a consumer app. Every pixel is a reading, a control, or a
hairline separating the two. Security is communicated through rigor and
density, not decoration.

## Principles

- Data-forward and dense. Full hashes, full 8-decimal balances, real
  metrics. Power users are the audience.
- One signal color. Cryo cyan marks everything interactive or
  important — cold, terminal-native, and deliberately far from the
  caution-yellow family (amber was rejected for reading as a warning).
  Green and red appear only with semantic meaning. Nothing else is
  colored.
- Hairlines, not cards. Panels are separated by 1px rules and hairline
  borders on flat surfaces. No rounded corners, no shadows, no glows,
  no gradients (the ambient background sweep is the one exception).
- Motion is feedback. Breathing status dots, a blinking block cursor on
  the active prompt, an accent flash when a value changes, row hover
  ticks. Plus one ambient layer: the slow background sweep.
- All text is monospace.

## Palette (`AppColors` in `src/types.rs`)

| Field         | Value     | Role                                         |
|---------------|-----------|----------------------------------------------|
| `bg`          | `#0a0c0d` | Canvas — cool near-black.                    |
| `surface`     | `#0f1214` | Panel fill.                                  |
| `surface2`    | `#161b1e` | Hover fill, input fields.                    |
| `border`      | `#232a2d` | 1px hairline separators (solid, never alpha).|
| `border2`     | `#3c474c` | Hovered/active hairline.                     |
| `accent`      | `#22d3ee` | Cryo cyan — the signal color.                |
| `accent2`     | `#3dd67c` | Semantic green: online, confirmed, incoming. |
| `accent3`     | `#0e7490` | Dim cyan for secondary emphasis.             |
| `danger`      | `#ff543e` | Semantic red: offline, errors, outgoing.     |
| `warn`        | `#ffd166` | Caution yellow (testnet badge, warnings).    |
| `text`        | `#dde8ea` | Primary text — cool off-white.               |
| `text_muted`  | `#6d7d82` | Labels, secondary text.                      |
| `*_tint`      | low-alpha | Fills for active rows / selected items.      |

## Typography

Registered in `main.rs`; helpers in `src/types.rs`:

- Default (both `FontId::proportional` and `FontId::monospace`) — IBM
  Plex Mono Regular. Body text 11–12.5pt.
- `display_font(size)` — Martian Mono Condensed Bold. Big numerals and
  screen titles (16–30pt).
- `label_font(size)` — Martian Mono Condensed Regular. Tiny UPPERCASE
  labels, badges, module codes (8–12pt). Always uppercase the string.

egui has no faux bold (`.strong()` only brightens color); emphasize
with size, color, or `display_font` instead of weight.

## Shared widgets (`src/ui/utils.rs`)

- `panel_frame(&colors)` — the standard content block: surface fill,
  hairline stroke, sharp corners, 14px padding.
- `section_header(ui, &colors, "01", "Title")` — cyan index code +
  uppercase title + hairline rule filling the row.
- `data_row / data_row_colored` — label-left (tiny uppercase, muted),
  value-right (body mono).
- `badge(ui, text, color)` — tiny uppercase badge in a tinted hairline
  box.
- `accent_button(&colors, text, size)` — THE primary action (one per
  screen): solid cyan, near-black uppercase label.
- `ghost_button(&colors, text, size)` — secondary actions: hairline
  border, cyan label.
- `breathing_dot(painter, pos, color, t, urgent)` — live status.
- `blinking_cursor(painter, left_center, height, color, t)` — terminal
  cursor beside active prompts.
- `value_flash(ui, id, value) -> f32` — 0..1 intensity for ~0.9s after
  `value` changes; lerp text color toward the accent with it.
- `row_hover(painter, rect, &colors)` — accent tint + 2px left tick.
- `ckb_split(shannons) -> (int_part, frac8)` — render the integer part
  bright and `.frac` dim so full precision stays quiet.
- `draw_instrument_bg`, `draw_frame_brackets` — backgrounds and HUD
  corner brackets (lock/setup screens only).

## Patterns

- Screen header: `display_font(16)` uppercase title, then an 11pt muted
  description line, then content panels. Left padding 24px, right 24px.
- Tables: tiny uppercase column headers (`label_font(9)`, muted), 1px
  hairline under the header and between rows, `row_hover` treatment,
  values in 11.5pt mono. No zebra stripes.
- Amounts: `+1,234.00000000` green for incoming, `−1,234.00000000` red
  for outgoing, neutral `text` otherwise; fraction digits in
  `text_muted` at the same size.
- Hashes/addresses: middle-truncated (`0x8a2f…44c1`), 11pt, muted;
  click copies and reports via `self.status`.
- Badges over icons: prefer text codes (`DEP`, `WD`, `IN`, `OUT`,
  `MAIN`, `TEST`) in `badge()` boxes instead of pictographic glyphs.
- Modals/popups: sharp corners, `surface` fill, `border2` stroke,
  hairline-divided sections — same language as panels.

## Anti-patterns

Rounded corners; colored decorative fills; glow meshes; per-widget
animations beyond the sanctioned set; non-mono fonts; lowercase labels
in `label_font`; more than one solid-accent button per screen.
