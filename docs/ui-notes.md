# UI notes: the look of CadCraft 0.7, measured

> Last reviewed 2026-11-03 · Last updated 2026-11-03 · Change: first version (M17.1, "look like 0.7"): CadCraft 0.7.1's
> app shell and the 0.7 website measured in headless Chromium at 1600 × 1000, CSS read as source.

| Date | Change |
|---|---|
| 2026-11-03 | M17.1: first measurements (app shell, CadCraft window, website /apps, /great-eight, Studio apps page) |

How these were measured: `getcadcraft.com/use/<app>/0.7.1` (PartCraft and TakeoffCraft have no 0.7.1: `/use/<app>/0.7`),
the website pages, each loaded in headless Chromium at 1600 × 1000 (light: `prefers-color-scheme: light`; dark:
`?space=model`), with every visible element's box, computed colours, font and border dumped (scratch tool
`forks/lead/scratch/look07/refshot.mjs`, outside the repo). Values below are CSS pixels = egui points at 100 %. The 0.5
build (`/use/cadcraft/0.5`, the frozen local 0.5 site) has the same shell and branding; the one visible difference
found: 0.7 greys out the Modify, Undo/Redo, Make and Extents buttons while they cannot act (`opacity .45`), 0.5 never
does. The fork keeps 0.5's behaviour (all buttons enabled).

Reference shots: `forks/lead/shots/look07/ref/` (outside the repo).

## Palette (tokens.css; `--sheet` etc.)

| Role | Paper (light) | Model (dark) | Rust token |
|---|---|---|---|
| sheet (stage, hover, input fill) | `#F3F6F8` | `#1D232B` | `panel_darker`, `input`, `canvas_bg` |
| sheet-2 (bars, panels, cards) | `#FFFFFF` | `#252D37` | `app_bar`, `panel` |
| ink (text, strong rules) | `#17202B` | `#E8EEF4` | `text`, `text_strong`, `rule`, `ink` |
| ink-2 (notes, labels) | `#34404E` | `#C3CDD8` | `text_dim` |
| grid (light rules, canvas grid) | `#D5DDE5` | `#36414E` | `divider`, `canvas_grid` |
| line (control borders) | `#7A8796` | `#6C7A8A` | `line` |
| action (links, pressed, tab rule) | `#1E48C8` | `#6E95FF` | `accent` |
| on-action | `#FFFFFF` | `#1D232B` | `on_accent` |

App layer colours (each app's icons, swatch, card edge; tokens.css `--layer-*`):

| App | Layer | Paper | Model |
|---|---|---|---|
| CadCraft | red | `#B3122E` | `#FF5A6E` |
| TakeoffCraft | ochre | `#8A6200` | `#F2C230` |
| CivilCraft | green | `#17703A` | `#4BD37B` |
| PartCraft | cyan | `#00707E` | `#3FD6E6` |
| BimCraft | blue | `#1E48C8` | `#6E95FF` |
| MarkupCraft | magenta | `#9A1F86` | `#F06BDC` |
| ModelCraft | orange | `#A84A00` | `#FF9A3C` |
| DetailCraft | violet | `#5B3CC4` | `#A78BFA` |

The fork's five presets: Light = paper, Dark = model (0.7's values unchanged); Medium light, Medium dark and High
contrast are derived (our own values, theme.rs), and a test checks 4.5:1 text and 3:1 icon/border contrast on all five.

## Type

- 0.7 uses **Archivo** (variable, weights 100–900, width 62–125 %), OFL-1.1: bundled as is (`assets/fonts/Archivo-Variable.ttf`,
  one file, egui 0.37 reads its `wght` / `wdth` axes). Used at 500 (text), 600 (labels, buttons), 700 (headings) and
  700 at 125 % width (the website's and start screen's display type). Inter stays as the fallback for scripts Archivo
  lacks; JetBrains Mono for code.
- Sizes: app name 17/700; app-bar buttons 15/600; toolbar group label 12/600 ink-2; tool label 11.5/600; panel grip
  name 12/600 ink-2; section heading 15/700; panel text and notes 14/500, line height 22; link buttons 14/600 action,
  underlined; props section head 13/700; props sub-heads 13/700 ink-2; history 13/500 ink-2; prompt 14/700; command
  field 15/600; status coords 13/600; toggles 13/600; tabs 13/600; kbd 12/600.
- Website: body 17/500 line 1.6; h1 52/700 width 125 % letter-spacing −0.02 em; card name 26/700 width 125 %; role
  17/600 ink-2; dimension-divider label 20/700 width 112 %; eyebrow 17/600 ink-2.

## Regions (CadCraft 0.7.1 at 1600 × 1000)

| Region | Box | Notes |
|---|---|---|
| App bar | 0,0 1600 × 48 | sheet-2, 1 px ink rule under; 16 px gutters |
| Mark | 16,13 22 × 22 | crosshair with a centre square |
| Swatch, name | 54,15 18 × 18; name at 81 | swatch 1 px ink edge, radius 1; "CadCraft™" |
| Area chip | 180,10 75 × 28 | app colour text and 1 px border, radius 2 ("Browser"; the fork: "Rust test version") |
| Bar buttons | h 32, gap 16 | 1 px ink border, radius 2, icon 15 + label 15/600 |
| Paper/Model switch | 1473,12 111 × 23 | 1 px ink box; the chosen half filled with ink, 11/600 capitals |
| Toolbar | 48 → 206 (2 rows of 79) | sheet-2, groups wrap; 1.5 px ink rule under |
| Toolbar group | padding 4 / 5.6 / 4.8 | label 22 high, buttons 48 high, gap 1.6, 1 px line at the right |
| Tool button | ≥ 41.6 × 48 | icon 18.4 in the app colour at +5, label at +25; hover: 1 px line border + sheet fill |
| Left panel | 0,206 248 wide | grip 28 (1 px line under), sections 16 px gutter, 1 px line between |
| Stage | 248 → 1320 | sheet fill, canvas grid (`--grid`, 45 % minor), axis icon 18 px from the corner |
| Tabs | 30 high under the stage | 1 px line over, open tab on sheet with a 2 px action rule |
| Right panel | 1320,206 280 wide | grip "Properties", heading 15/700, 30 × 30 bordered head buttons |
| Command area | 854 → 968 (114) | 1 px ink rule over; history 74, command line 40 (1 px grid rule over, 1 px action underline) |
| Status bar | 968 → 1000 (32) | 1 px line over; coords left; toggles right: 24 high, 1 px line, pressed = action fill |

Other apps (same shell, measured from their 0.7.1 / 0.7 shots): ModelCraft has a Workspace picker group first and its
tool palette as toolbar groups (File, Draw, Modify, Organize, Edit, Camera), an Outliner panel left (232 wide) and
"Entity info, style and scenes" right; MarkupCraft a one-row toolbar (File, Select, Draw, Revise, Annotate, Measure,
Edit), a Sheets panel left (224), "Markup panels" with tabs right (320), a sheet bar under the stage (zoom, fit,
rotate); every app's status bar is the same 32 px strip.

## Controls

- Radius 2 everywhere (cards, buttons, inputs); 1 px borders; no shadows.
- Select: 25 high, sheet fill, 1 px line border; number input 31 high.
- Menu list (toolbar More): sheet-2, 1 px ink border, items 238 × 34, 14/500, the key at the right in 12 px ink-2.
- Outlined button (`.btn-line`): 2 px action border, action text 15/700, 40 high; filled (`.btn-cc`) action fill.
- Hint card ("Start drawing"): 16 px from the stage corner, ≤ 512 wide, sheet-2, 1.5 px ink border, heading 14/700.

## Icons

0.7 draws Bootstrap Icons (an icon font) at 18.4 px in the app colour. The fork does not ship that pack (CLAUDE.md § 7):
the icons are painter icons drawn in code on a 20 × 20 grid (ribbon.rs `paint_icon`), named after the same icons so
each button shows the same picture idea; stroke ~1.3 px at 18 px.

## Website and Studio apps page

- Header 69 high (sheet), the mark + "CadCraft™" + version chip, nav 15/600, Paper/Model switch, outlined "Use it now".
- Background: sheet with a drafting grid, 1 px lines every 8 px (`--grid` 45 %) and 40 px (95 %); model space: the
  same grid as dots.
- /apps: eyebrow, h1, lead (20 px), a dimension-line divider ("Every app"), then cards in two columns 590 wide with
  a 20 px gap: sheet-2, 1 px ink border, **4 px layer-colour left edge**, padding 24, swatch 28, wide name 26 + ™,
  role 17/600 ink-2, a drawing of the app's work on sheet with a 1 px grid border, "▶ Use CadCraft™" filled button.
- /great-eight and /great-eight/studio/apps: dark hero (`#11161C`-ish with the grid), rainbow rule, the "8" ring of
  the eight layer colours. The fork's start screen follows /apps (light by default), not the dark hero.

## What egui cannot match exactly

- Text: egui shapes without kerning pairs from GPOS and without CSS letter-spacing; Archivo at the same size renders a
  few percent wider or narrower per word. Line heights are set explicitly where 0.7 sets them (notes, cards).
- No sub-pixel antialiasing of text; strokes are not hinted to the pixel grid the way the browser does for 1 px borders.
- Bootstrap's icon font is not used (above): icons are close in idea and size, not identical in shape.
- Native `<select>` arrows: egui's combo box draws its own ▾.
