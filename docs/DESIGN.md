# Elite Nexus design

How the app looks and why. Every page, card, control and chart follows this, and so does every new feature. The
colours are the app's own (`theme.rs`); what this document fixes is how they are used, the type scale, the components
and the charts.

The target look is drawn in the redesign canvas (Dashboard, Market, Statistics, Navigation, and a UI kit of
Foundations, Components and Charts): https://claude.ai/artifact/Lm36M4DhF6kddgVyoY517V. It is private to the
maintainer and uses sample data. This file is the written source of truth; the canvas is the picture. The app is not
there yet: see [Moving the app to this design](#moving-the-app-to-this-design).

## Principles

1. **Quiet until it matters.** Surfaces are neutral charcoal. A colour always means something, and nothing is coloured
   for decoration: meters, badges and numbers stay neutral until they need attention.
2. **Orange is for acting, and for you.** The primary button (one per view), the selected tab, links, your position
   and your progress. Never card titles, never decoration.
3. **Hierarchy comes from size and weight, not colour.** Card titles are small, semibold and DIM, with no tinted icon.
   At most one large figure per card.
4. **The number reads first.** Units are smaller and DIM (`19.6 ly`). Big amounts are compact (`27.63 B Cr`) with the
   exact value next to them or in a tooltip. A zero is shown in DIM, so the values that matter stand out. Columns of
   numbers are right-aligned with tabular figures.
5. **Explain, don't just show** (the honesty rule in AGENTS.md). Say where data comes from, how fresh it is and what it
   can't show, in a source note under the data.
6. **Names are copyable.** Every system, station or carrier name a commander might paste into the game has a visible
   copy button, not an invisible click-to-copy label.
7. **Legible for everyone.** Text is at least 4.5:1 against what's behind it and at least 12 pt. Controls are 28 to 36
   pt high (44 where touch is likely). A status colour always comes with an icon or a word.

## Colour

The names are `theme::pal`'s. Contrast is measured on a card (`CARD`).

### Surfaces

| Token | Hex | Use |
|---|---|---|
| `WINDOW` | `#1C1C1E` | Behind everything |
| `SIDEBAR` | `#28282B` | The floating sidebar |
| `CARD` | `#2C2C2E` | Cards and grouped lists, no border |
| `CARD_HI` | `#3A3A3C` | Chips, icon tiles, secondary buttons, hairlines inside a card, meter tracks |
| `CONTROL` | `#4E4E52` | The selected segment, pressed controls |
| `FIELD` | `#212124` | Text fields, the track of a segmented control, wells and callouts inside a card |
| `SEPARATOR` | `#3C3C3F` | Hairlines on the window |

Cards have no border; hairlines separate rows inside them. Only floating things (popovers, menus, tooltips, toasts) get
a shadow.

### Text

| Token | Hex | On a card | Use |
|---|---|---|---|
| `TEXT` | `#F5F5F7` | 12.8:1 | Values, names, anything read first |
| `DIM` | `#98989D` | 4.9:1 | Labels, captions, units, card titles, zeros |
| `FAINT` | `#6E6E73` | 2.8:1 | **Not for text.** Hairlines, separator dots, disabled controls |

### Accent

| Token | Hex | Use |
|---|---|---|
| `ACCENT` | `#FF9F0A` | Primary button, selected tab bar, links, progress, the current position |
| `ACCENT_SOFT` | `#FFB340` | Hover on an accent fill |
| `ACCENT_BG` | `#4A3414` | Tinted chips and badges ("Docked", "First logged ×5", the chosen chip) |
| `ON_ACCENT` | `#201402` | Text and icons on an accent fill |

### Meaning

| Token | Hex | Means |
|---|---|---|
| `GOOD` | `#30D158` | Fine, done, open, fresh; a switch that is on |
| `WARN` | `#FFD60A` | Check this: stale prices, low fuel |
| `BAD` | `#FF453A` | Danger, loss, wanted. As text use `#FF6961`: `BAD` is only 4.1:1 on a card |
| `BLUE` | `#0A84FF` | Information. As text on a card use `#409CFF`: `BLUE` is 3.8:1 there |
| `CYAN` | `#64D2FF` | Neutron stars, fuel |
| `INDIGO` | `#7D7AFF` | Fleet carriers |
| `GOLD` | `#FFCC00` | Squadron carriers |

A tinted badge is the colour at 14 to 18% behind the same colour as text. Banners use the colour at 11 to 14% behind
`TEXT` and `DIM` text, with the icon in the colour.

### Data colours (charts only)

Series take these in this order and keep them when a filter removes others:

| # | Hex | From |
|---|---|---|
| 1 | `#0A84FF` | `BLUE` |
| 2 | `#CE7F06` | `ACCENT`'s hue, one step darker |
| 3 | `#01A6BD` | `TEAL`, one step darker |
| 4 | `#FF375F` | `PINK` |
| 5 | `#BF5AF2` | `PURPLE` |
| 6 | `#B48F00` | `GOLD`, one step darker |

The darker steps exist because orange, teal and gold are much lighter than blue and would outweigh it as equal series.
In this order, neighbouring series stay apart for colour-blind readers (checked with simulated protanopia and
deuteranopia on `CARD`), and the first three stay apart from each other too: a scatter or a map uses only those. More
than six series: fold the rest into "Other" or split the chart.

- One series: `#0A84FF`.
- Amount (heat maps), none to most: `CARD_HI`, then `#004389` `#0058B0` `#056FD8` `#1786FF` `#63A6FE`.
- In and out: in `#0A84FF`, out `#CE7F06`, either side of a `#48484A` line.
- Status colours never stand for a series, and series colours never stand for a status.

## Type

Inter, bundled, in its static weights. The canvas's 550 is Medium and its 650 is SemiBold.

| Role | Size / line | Weight | Example |
|---|---|---|---|
| Figure | 40 / 44 | SemiBold, −0.03 em | `27.63 B Cr` |
| Page title | 28 / 34 | SemiBold, −0.02 em | Dashboard |
| Heading | 20 / 26 | SemiBold, −0.015 em | Kestrel Mk II |
| Card title (panel) | 15 / 20 | SemiBold | Money in and out |
| Card title (summary card) | 13 / 18 | SemiBold, `DIM` | Commander |
| Body | 14 / 20 | Regular | |
| Secondary | 13 / 18 | Regular, `DIM` | Docked at TCG BARBAROS in Orrere |
| Label | 12 / 16 | Medium, `DIM` | Prices no older than |
| Mono | 12 / 16 | Regular | `KFK-L3V`, `14:31` |

- Nothing is smaller than 12 pt.
- Letter spacing is `TextFormat::extra_letter_spacing`, only from 20 pt up.
- Tabular figures: egui doesn't apply OpenType features, so `tnum` can't be switched on at run time. Ship a copy of
  Inter with the feature frozen in (fonttools' `pyftfeatfreeze -f tnum`, next to `tools/fonts/strip_private_use.py`) as
  its own family, for table columns and chart axes. Big standalone figures keep Inter's proportional digits.
- Mono is for identifiers and times only. Today it comes from the system (Cascadia Mono or Consolas), which Linux and
  macOS don't have; bundling an open mono font (JetBrains Mono in the canvas) makes it the same everywhere.

## Space and shape

- Spacing on a 4-pt base: 4, 8, 12, 16, 20, 24, 32, 48.
- Cards pad 20 and sit 16 apart (today `card_frame` pads 18 and `widgets::GAP` is 14). Rows inside a card are 9 to 12
  apart with a hairline between them.
- Corners: chips 6, controls 8, callouts 10, cards 14, sidebar 16, pills and toolbar buttons fully round (today cards
  are 16).

## Page anatomy

1. **Header:** the page title and one line under it saying where you are or where the data comes from. On the right,
   the toolbar capsule (credits, the EDDN rate with a small trend line, the live state) and the round toolbar buttons.
2. **Section tabs:** underline tabs under the header, the selected one in `TEXT` with an accent bar. Not a segmented
   control.
3. **Filters:** one card above everything they scope, each control with its label above it; the result count and time
   under it. A filter never sits inside one chart's card.
4. **Content:** cards in a responsive row (`widgets::cards`), 16 apart; one hero figure per view at most.
5. **Notes:** sources and caveats at the end, in `DIM` with an info icon.

## Components

What to use, from `widgets.rs` (and `charts.rs` once it exists). "Restyle" means the function exists and changes to
match this document; "new" means it doesn't exist yet.

| Component | Function | Rule |
|---|---|---|
| Card | `card`, `card_with`, `card_tinted` (restyle) | Title in `DIM`, no tint: the tint argument goes away |
| Figure | `stat` (restyle) | Value in `TEXT`, unit smaller in `DIM`; colour only on a change, with a sign and an arrow |
| Stat tile | new | Label, value, comparison with a named period, trend line in `GRAY` with the latest point in `ACCENT` |
| Key and value rows | `kv` (restyle) | Label `DIM` left, value `TEXT` right, hairline between rows |
| Meter | `bar` (restyle) | 4 to 6 pt; neutral (`TEXT` at 82%) until a threshold, then `WARN` or `BAD` with a word |
| Segmented control | `segmented` | A few exclusive options; `FIELD` track, `CONTROL` for the selected segment |
| Section tabs | `tabs` (restyle) | Underline style; `segmented` keeps the pill |
| Switch | `switch`, `toggle` | Settings that act at once; `GOOD` when on |
| Checkbox, chip toggle | `chip_toggle` | Options inside a form; accent when on |
| Stepper | `stepper` | Fixed steps; never a slider that rescales the page while it's dragged |
| Fields and search | `search_field`, `combobox_grouped` | Label above, help or error below; a search is one box with its results grouped under it |
| Buttons | `primary_button`, `button`, `icon_button` | One primary per view; destructive is red text with a red hairline; icon buttons have a tooltip |
| Badge | `badge` | A state or a tag: tinted only when the colour means something |
| Freshness | new | A dot and "18 h ago": `GOOD` under 1 h, `GRAY` to 12 h, `WARN` older |
| System name | new (`widgets::system_name`) | Name with a visible copy button; "Copied" toast |
| Table | `table`, `th_sort` (restyle) | Hairlines instead of stripes; headers 12 pt `DIM` with the sort arrow; numbers right-aligned and tabular; a name over its system in a two-line cell |
| Banner | new | Info, warning or danger: what happened, what it means, what to do, an optional action |
| Toast, tooltip | new, `on_hover_ui` | Short; a tooltip explains a column or a control |
| Empty state | `empty` (restyle) | Icon in a circle, a title, one line saying how to fill it, an optional action |
| Loading | new | Keep the last results, dimmed, with a thin progress bar; never blank the page |
| Source note | `footnote` | Info icon, where the data comes from and what it can't show |

## Charts

- **Pick the form by the job.** One value is a figure, not a chart. A trend is a line (an area for a single series).
  Comparing amounts is columns or bars. In against out is bars either side of a line. A part of a whole is a segmented
  bar, never a doughnut. An amount over a grid is a heat map.
- **One y axis.** Two measures with different scales are two charts.
- **Thin marks.** 2-pt lines; columns at most 24 pt wide with 4-pt rounded tops; a 2-pt gap between touching marks; an
  area wash under 25% that fades downwards.
- **Quiet frame.** Hairline grid in `CARD_HI`, baseline `#48484A`, 3 to 5 ticks at round values, axis labels 12 pt
  `DIM` with tabular figures.
- **Label the last point or the extreme, never every point.** A legend whenever there are two series or more. Labels
  stay `TEXT` or `DIM`; the coloured mark next to them carries the identity.
- **Hover:** on lines, a crosshair snaps to the nearest day and lists every series; columns and cells show their own
  tooltip; keyboard focus shows the same.
- **Every chart has a table view**, as a Chart/Table switch or a table beside it.
- **Filters scope charts from above**, all of them at once.
- Chart code lives in one module (`charts.rs`, to be added) so every chart follows these rules. egui_plot's default
  look doesn't: grid, lines, bars and tooltips all need the styling above, or a chart of our own drawn with the
  painter, as the sampling radar already is. A gradient wash is a `Mesh` with per-vertex colours.

## A new feature or page: checklist

- [ ] Built from `widgets.rs` and `charts.rs`; no raw `Color32` and no literal font sizes in page code.
- [ ] Orange only on the primary action, the selection, links and "you".
- [ ] Every colour means something and has a word or an icon with it.
- [ ] No text in `FAINT`; nothing below 12 pt; red and blue text use their text tones.
- [ ] Names you might paste into the game have a copy button.
- [ ] The data says where it comes from and how fresh it is; the empty and loading states are written.
- [ ] Checked at 1000 px and at 1600×1000 with `tools\dev\shot.ps1`.
- [ ] `docs/USER_GUIDE.md` and `CHANGELOG.md` updated when behaviour changes.

## Moving the app to this design

Not started (5 November 2026). In order, each step its own change:

1. **Tokens and fonts.** `theme.rs`: text tones for red and blue, the data colours, spacing and corner constants, the
   type roles; Inter with tabular figures; the mono font.
2. **Components.** Restyle and add the functions above in `widgets.rs`. `card_tinted` alone has 60 callers in 10 page
   files, so making it neutral changes most cards at once. Add a kit page (`--page Kit` for `tools\dev\shot.ps1`)
   showing every component and chart, so a change can be checked in one screenshot.
3. **Charts.** `charts.rs`; move `stats.rs`, `bgs.rs` and `market.rs` off egui_plot's default look.
4. **Pages,** one area per change, Dashboard, Market, Statistics and Navigation first. The work is a colour audit:
   the page files have 88 uses of `pal::ACCENT`, 130 of `GOOD`, 111 of `BAD`, 80 of `FAINT` (many as text) and 15
   raw `Color32::from_rgb`; each one becomes neutral, a meaning, or an action.
5. **Screenshots.** Retake those in the README, on the site and in the user guide.
