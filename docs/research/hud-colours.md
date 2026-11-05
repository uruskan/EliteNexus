# HUD colours and cockpit lighting

Research of 2026-10-12/13 behind the HUD & cockpit page (`crates/nexus-hud`) and the cockpit-light dimmer.
Game version checked: 4.4 (Odyssey/Live, Steam). "Unverified" marks what no source confirms.

## The game's own setting: the GUI colour matrix

**Files**
- Override: `%LOCALAPPDATA%\Frontier Developments\Elite Dangerous\Options\Graphics\GraphicsConfigurationOverride.xml`.
  One path for every client (Live and Legacy alike, as far as anyone reports). The game creates it as
  `<?xml version="1.0" encoding="UTF-8" ?>` + `<GraphicsConfig />` (61 bytes, CRLF, no BOM) when it is missing.
- Base: `<install>\Products\elite-dangerous-odyssey-64\GraphicsConfiguration.xml` (UTF-8 with BOM, CRLF, tabs).
  Updates and "verify files" overwrite it, which is why themes belong in the override.
- The override replaces values of elements that exist in the base file (same paths); it can't add new elements.

**Base `<GUIColour>`**: `<Default>` (identity, `LocalisationName` "Standard"), plus `<RedToBlueTest>` (rows
`0,0,1 / 0,1,0 / 1,0,0`) and `<DesaturateTest>` (0.33 everywhere). `Custom.4.4.fxcfg` has `<GUIColourQuality>0`,
which by analogy with the other `*Quality` indexes selects `<Default>`. Every tool targets `<Default>`; whether other
indexes select the test presets is unverified, so don't expose them.

**Maths** (confirmed by Arkku's editor source, COVAS:NEXT, ED Scout, EDHM-UI and in-game reports): each row is one
input channel and says where it goes.

```
out = r·MatrixRed + g·MatrixGreen + b·MatrixBlue
out.R = MR[0]·r + MG[0]·g + MB[0]·b   (and so on)
```

- Negative values and values above 1 work (community themes use −3 to 3.5). Above 1 clips toward white.
- White comes out as the sum of the three rows, so a column summing above 1 clips white.
- Linear or gamma space in the game: unverified. Arkku (browser default) and EDHM-UI (explicitly) preview in linear
  light; Nexus does too (`HudMatrix::apply_srgb`).
- Arkku's editor floors pasted values to whole percent (0.57 becomes 0.56): round, don't floor.

**Reference input colours** (no official palette): HUD orange #FF7500 (COVAS:NEXT; others use #FF8000 or
(1, 0.5, 0)), highlight blue #69D9DA (shields, throttle blue zone), radar hostile red, neutral orange, allied green,
wingmen cyan, white.

**What it changes**: the whole 2D interface in one go: HUD, panels, scanner contacts (friend and foe move with
everything else; the most common complaint), shields, station menus and terminals, the on-foot HUD (mostly white,
renders differently), mission-board and comms portraits (pre-inverted since 2.1, so strong themes band them), the
HUD glow cast on the cockpit. Odyssey renders the same matrix differently from Horizons (reds less saturated).
**Not changed**: the main menu, the store's livery previews, the cockpit's own ambient lighting. Galaxy map stars
were separated from the GUI in 2015; 4.x unverified.

**When it is read**: at launch only. Every guide says to quit and relaunch; nobody reports a reload on graphics
Apply or at the main menu.

**Robustness**: root `<GraphicsConfig>`, exact case, `.` decimals, any whitespace in rows. A malformed file is said
to be ignored (possibly reset): unverified, so Nexus never rewrites one. Updates don't touch a populated override.

**Other tools**: COVAS:NEXT, ED Scout, Elite VR Cockpit and EDAutopilot read the override to match their own colours;
EDProfiler and EDHM-UI write it (EDHM-UI resets it to identity when it installs EDHM).

**Themes**: the Frontier forums' NO2O list (archived:
http://web.archive.org/web/20190920205846/https://forums.frontier.co.uk/threads/no2o-the-definitive-list-of-1-7-2-2-compatible-hud-colour-color-configs-please-add-yours.259311/)
records each theme's friend/foe colours. Others: Arkku (https://arkku.com/elite/hud_editor/), newp.io/hudcolor,
team2xh.net/elite, Prima Games' VR set, Windows Central's "Outrun".

## Glare and exposure: `<HDRNode_Reference>`

The base file's lighting values (all 36 sections read, 4.4) are global: `IBL` (ambient light), `HDRNode` and
`HDRNode_Reference` (exposure and tone mapping), `Bloom`, `LightCones`. Nothing is specific to the cockpit.

**The complaint**: Odyssey's auto-exposure reacts to the cockpit's own lights, so with a bright dashboard in view
everything outside is drawn darker ("Focused Feedback Thread - Cockpit Brightness",
https://forums.frontier.co.uk/threads/focused-feedback-thread-cockpit-brightness.588309/). The often-quoted line "the
lighting per se is alright in Odyssey, BUT the auto-exposure is so sensitive..." is CMDR Lyamecron's; Senior
Community Manager Sally Morgan-Moore quoted it on 1 Oct 2021 only to collect it as lighting feedback.

**The values** (base file, 4.4): `GlareCompensation` 1.3333, `Percentiles` 0.7,0.8,0.9, `ExposureType` 2,
`ToneMapType` 1 (ACES), `ACES_A..E` 2.51, 0.03, 2.4543, 0.295, 0.42 (`HDRNode`, apparently the Horizons-era node,
has percentiles 0.01,0.54,0.999 and a Hable curve).
- CMDR Morbad, Feb 2022 (https://forums.frontier.co.uk/threads/brightness-adaptation-setting.598409/): the
  brightness adaptation can't be disabled or fundamentally altered, but turning `GlareCompensation` down reduces
  the peak brightness of the glare from illumination.
- CMDR Morbad, May 2021 (https://forums.frontier.co.uk/threads/odyssey-in-game-graphics-too-bright.576191/,
  attachment archived by the Wayback Machine): `Percentiles` 0.6,0.540000,0.9000, `GlareCompensation` 1.0,
  `ACES_C/D/E` 2.43, 0.59, 0.14 (the standard Narkowicz ACES fit), for commanders finding Odyssey too bright.
- Maths: with A = 2.51 and B = 0.03, the standard fit draws a dark tone (0.03) about 2.7× and mid-grey (0.18)
  about 1.7× as bright as the game's C/D/E, and white the same (`tone::curve_ratios`).
- What `ExposureType` and each percentile mean is undocumented; nobody reports switching the adaptation off.
- Bloom (in-game Graphics option) controls the glow around bright lights.

## Cockpit lights: EDHM

The game has no option for cockpit or cabin lighting (players still ask for one; a forum reply says the base game
never had a way to switch the interior lights off). Frontier reportedly sells ARX "Cockpit Lights" that only recolour
them (third-party store listing, unverified). Dimming them takes code inside the game's renderer: EDHM today.

**How EDHM works**: a 3Dmigoto build (modified by MrVaad) placed next to `EliteDangerous64.exe`: `d3d11.dll`,
`d3dcompiler_47.dll`, `d3dx.ini`, `EDHM-Uninstall.bat`, `EDHM-ini\` and `ShaderFixes\` (`<hash>-ps.txt` shader
replacements). The DLL wraps Direct3D 11 and swaps game shaders by hash while the game runs; no game file is
modified. Settings are 3Dmigoto IniParams (`x69 = 0.9131`: component x of element 69, bound to shader register
`t120`) in `EDHM-ini\*.ini`.
- Cockpit lighting lives in `EDHM-ini\Advanced.ini`: `w69` overall brightness (0–2, the "dimmer"), `x69..z69`
  primary colour, `x70..z70` secondary colour (linear RGB), `w159`/`x190..z190` background glow, `y111` style.
- Live reload: F11 in game, or (since EDHM v21.02 / EDHM-UI 3.0.49) the DLL polls
  `EDHM-ini\ThemeSettings.json` every ~2 s and reloads when it changes. 3Dmigoto hotkeys only work while the game
  has focus.
- EDHM-UI (Electron, `%LOCALAPPDATA%\EDHM-UI-V3`, data in `%USERPROFILE%\EDHM_UI`) makes `EDHM-ini` and
  `ShaderFixes` NTFS junctions into its data folder and regenerates every ini from its JSON themes on each apply,
  so outside edits get overwritten. It treats EDHM as installed when `d3dx.ini` exists and switches it off by
  renaming `d3d11.dll` to `.disabled`.
- Patches change shader hashes: elements revert to orange until a new EDHM ships, which has happened after almost
  every 2025–26 update (v22.03 for 4.4.1.1, Sept 2026).
- EDHM needs the game's own matrix left at identity; it has its own "XML profile" matrix (`x150..z152`).

**Licences**: EDHM is not open source; its licence forbids modification and redistribution except by EDHM-UI and
Nixpkgs (`LICENSE.md`, `REDISTRIBUTION_EXCEPTION.md` in psychicEgg/EDHM). EDHM-UI is GPL-3.0. 3Dmigoto is GPL-3.0.
So Nexus can't bundle EDHM; it could only edit an existing install's documented ini values.

**Frontier's stance**: no statement names EDHM. Its authors say Frontier tolerates it; Frontier's moderation guide
restricts "client manipulation" software capable of hooking into the game. No known bans (unverified).

**Doing it natively** means shipping our own 3Dmigoto shader mod. Hash-keyed fixes (EDHM's `ShaderFixes`) need
redoing after nearly every patch, but 3Dmigoto also has an "on the fly regular expression shader patching engine"
(`[ShaderRegex]`, `[ShaderRegex….Pattern]`, `.Pattern.Replace`, PCRE2 over the shader assembly; see the template
https://github.com/bo3b/3Dmigoto/blob/master/Dependencies/d3dx.ini): a fix keyed on what the lighting code looks
like keeps matching when a patch only recompiles shaders, and does nothing (safely) when the code really changes.
Finding the shader once: frame analysis (`analyse_frame`, default F8; `analyse_options = dump_rt ...` dumps each
draw call's render targets and the shaders used) or hunting mode, with the game in the cockpit.

### Captures and a test dimmer, 2026-10-13 (game 4.4, 2560×1440, near and on a fleet carrier)

3Dmigoto 1.4.9 (`3Dmigoto_v1.4.9.7z`, the official release; loads fine with `load_library_redirect=2`, no
`nvapi64.dll`), `analyse_options = dump_rt jpg mono clear_rt`, `export_shaders=1`. The frame and 614 exported
shaders are kept outside git (Frontier's code: never commit it).
- JPEG dumps skip floating-point targets (R16G16B16A16_FLOAT HDR buffers), so draws ~206-1450 (shadow maps and
  HDR passes) left no images. A follow-up should dump only chosen draws/targets with `dds` (a full DDS dump of
  every draw would take tens of GB at this resolution).
- Deferred renderer: G-buffer for the main view at draws 1451-1549 (R10G10B10A2 normals + two RGBA8 targets),
  a 256×256 interior reflection map (the `Envmap` size) at 1550-1618, then lighting per view: a full-screen pass,
  compute light clustering (22×12×32 clusters), instanced light volumes (84-index spheres, 14-index cube strips,
  36-index cubes) and compute/full-screen resolves.
- The cockpit lamps' light lands in the light buffer at draws 1637-1638: instanced cube light volumes drawn with
  pixel shader `3f366702d540af3e` (ps_5_0, ~150 lines). Per-light data arrives as constant-interpolated inputs
  (v0, v1); it writes `o0.xyz` (HDR light colour) and `o1.x` (luminance, weights 0.3086/0.6094/0.082), both
  scaled at the end by `cb2[0].x`.
- The same shader draws other lights of its type (3,741 instances in this frame, mostly outside the ship), so a
  dimmer must scale only lights near the camera: decode v0/v1 (light position/radius, colour) with a capture
  using `dump_cb`/`dump_vb` on those draws, then a `[ShaderRegex]` on the final `mul o0.xyz, …, cb2[0].xxxx` that
  multiplies by an IniParam when the light is within a few metres. Unknown yet: whether that radius works for
  every ship, and docked or on foot.
- The light volumes' vertex shader keeps its input names: `CORNERPOSITION` (cube corner), per light
  `POSITIONANDRANGE` and `COLOURINNERRANGESHADOWINDEXSPECULARSCALE` (uint-packed). It hands the light shader the
  light's position transformed by `cb2[2..4]` (camera space, presumably: then `length(v0.xyz)` is the light's
  distance from the eyes) and its range in `v0.w`.
- Capture 2 (same day, docked on a fleet carrier's pad; per-light buffers recorded with `dump_vb dump_cb txt
  buf` on the light shaders only, 216 MB): 3,995 box lights in view. Per light: position (float bits in uints)
  and range in `POSITIONANDRANGE`; `COLOURINNERRANGESHADOWINDEXSPECULARSCALE` = colour packed 10:10:10:2 (red in
  the low bits: `000003FF` red, `000FFC00` green, `00A337FF` orange ≈ 1.0/0.64/0.01, `3FFFFFFF` white), then
  floats that look like intensity (6.4-192), inner range (0.01-2) and a specular/shadow word. Camera-space
  distance (`cb2[2..4]` applied to the position): 13 lights within 2 m, 74 within 5 m, 98 within 8 m (the
  cockpit and the ship's front: ranges 0.2-3.8 m), then 2-4 per metre further out (hull, carrier). No clean
  gap: the dimmer's reach must be adjustable (about 5 m by default).
- The other light families (spheres `4034d618…`, `0d055cd7…`, `9e4f083d…`) read their light data from a
  structured buffer (`t0`, indexed by `SV_INSTANCEID`) instead of instance data; all four families end with the
  same `mul o0.xyz, rN.xyzx, cb2[0].xxxx`. In capture 1 the cockpit's light buffer only changed at the box-light
  draws, so a first dimmer can patch `3f366702d540af3e` alone.
- **Test, 2026-10-13: it works.** `ShaderFixes\3f366702d540af3e-ps.txt` = the exported asm plus
  `dcl_resource_texture1d (float,float,float,float) t120`, `dcl_temps 11` and, before the final
  `mul o0.xyz, r0.xyzx, cb2[0].xxxx`: load IniParams (`ld_indexable(texture1d)… t120`), `dp3` of `v0.xyz` with
  itself, compare with reach², `movc` the strength or 1, multiply `r0.xyz` (colour) and `r1.xyz` (luminance
  source). `[Constants] x = 0.35, y = 5, z = 1`; keys F9 toggle `z`, F10 cycle `x`, F11 cycle `y` (the shipped
  dimmer later moved `x` to F7 and `y` to F8: F10 is the game's own screenshot key, outside its controls, so every
  screenshot also stepped the light kept; play-test 2026-11-04). Confirmed in
  the game: only the cockpit lights dim, strength and on/off behave, nothing else changes. A 3 m reach let
  some of this ship's cockpit lights back (they sit up to ~5 m away), so the reach should not go below ~5 m
  by default and should be adjustable per ship.
- **In Elite Nexus** (`nexus_hud::dimmer`, HUD & cockpit › Cockpit lights): the test's hash-named fix held the
  game's whole shader, which can't be shipped, so the dimmer is a `[ShaderRegex]`: the pattern requires the box
  light's four input declarations and its last three lines (`mul o0.xyz, rN.xyzx, cb2[0].xxxx`, the luminance
  `dp3` with 0.3086/0.6094/0.082 and `mul o1.x`), and the replacement inserts the distance test before them with
  two `temps` and `t120` declared through `InsertDeclarations`. Checked offline against the 309 pixel shaders the
  game loaded (Rust `regex`, case-insensitive, as 3Dmigoto's PCRE2): it matches exactly `3f366702d540af3e` and
  `7d85f63f9ab11daf`, a box-light variant with the same inputs. Pinned: the release archive (SHA-256 `d5d05e78…`),
  `x64\d3d11.dll` (`791aa68b…`) and `x64\d3dcompiler_47.dll` (`a05f9973…`).
- HUD: the panels are first drawn into their own textures (draws 3-192) by three small shared shaders that output
  texture × colour + colour, the colours coming from per-element data. The compositor `f2ce89f2c5d390ce` then
  places each panel in the cockpit (with a per-panel tint `cb2[8]`), and other HUD parts are drawn straight into
  the scene (`4a533b975e4998bf` and others).
- **The GUI colour matrix is applied in the HUD shaders at draw time**, from the frame's global constant buffer:
  `out = r·cb1[85] + g·cb1[86] + b·cb1[87]` (MatrixRed, MatrixGreen, MatrixBlue), after each element has its
  standard colour. 12 of the 614 exported shaders read all three rows (the compositor, the 3D HUD elements and
  HUD screens not on screen in this frame), always as three consecutive lines in the same order (green `mul`,
  red `mad`, blue `mad`); registers, write masks and component order vary. That is the hook for colour rules
  (recolour the orange, keep hostile red and shield blue) and per-panel colours, and it depends only on how the
  game applies its own matrix. Colours arriving there (capture 2, `dump_cb`, linear light): HUD orange
  (1, 0.068, 0.025), dark orange (0.234, 0.044, 0.025), white; the panel tint `cb2[8]` is white (panel colours
  come from the panel textures). Hostile red wasn't in view, so its value in the game is not measured.

### Keep hostile red, friendly green and shield blue (Elite Nexus, 2026-10-13)

- **The rule** ("is this the HUD's orange?"): red is the largest channel, green ≤ red·`max_gr`, green ≥
  red·`min_gr` and blue ≤ red·`max_br` (defaults 0.6, 0.035 and 0.3). Orange and its shades take the theme;
  everything else (hostile red, friendly green, shield blue, white, black) keeps its standard colour. Shares of
  red don't depend on brightness or the HDR scale. The measured orange has green at 7 % of red; the reference
  hostile red #FF2010 has 1.4 % in linear light, under the 3.5 % floor (its real value in the game is unmeasured).
- **Settings**: 3Dmigoto IniParams index 1, `[Constants]` `x1` (on), `y1` (`max_gr`), `z1` (`max_br`), `w1`
  (`min_gr`), read in the shader with `ld_indexable(texture1d)… l(1, 0, 0, 0), t120`. The dimmer keeps index 0.
- **Pattern** `[ShaderRegexEliteNexusHudColours]` (`nexus_hud::dimmer::HUD_PATTERN` / `HUD_REPLACE`): the three
  lines with named groups for the red, green and blue inputs and the destination. The replacement copies the
  inputs first (the third line can overwrite one: `mad r0.xzw, r0.wwww, …`), keeps the three lines, evaluates
  the rule with three `temps`, and `movc`s the standard colour back into the destination components when the
  colour isn't the orange. It is only written to `d3dx.ini` while the switch is on, so the HUD shaders are left
  alone otherwise. `palette::is_hud_orange` is the same rule (same comparisons in the same order) for the preview.
- **Offline checks** (Rust `regex`, case-insensitive, as 3Dmigoto's PCRE2): the pattern matches exactly the 12
  shaders (`08517b7d32fa1f80`, `1a696e8bae6eec02`, `1d5a317f0e831e9d`, `3582a66bcb0903ae`, `4a533b975e4998bf`,
  `70e91536d34607c7`, `9798f7cba8194a6c`, `ad5ad611d07db185`, `c4f04894e3f6ae79`, `cc0e441e849ebe4e`,
  `ef44eee0c6ab9506`, `f2ce89f2c5d390ce`) once each, and never a shader the dimmer's pattern takes. A small
  interpreter in the tests runs the inserted instructions through each real shader's own registers and swizzles
  (3 themes × 3 settings × 1,010 colours) and agrees with `is_hud_orange`; the dimmer's inserted code is checked
  the same way on a made-up box light.
- **In game**: not tested yet. Since the crash below, the pattern runs in Elite Nexus during a calibration, not in the game.

### The in-game patterns crashed the game; calibrated fixes instead (2026-10-13)

- **Crash**: with the 0.3.0 config (both `[ShaderRegex]` patterns in `d3dx.ini`) the game crashed four times in a row at
  `EnteringLocation` near LP 493-64 2 a, each time right after the netLog loaded a newer (squadron) fleet carrier
  (`carrier01squadronhublevel`, `carrier01squadron_bridge`, ...). Squadron carriers had loaded fine on 10-11 October,
  before 3Dmigoto was in the game folder; with `d3d11.dll` renamed the game loaded in at the same spot. ShaderRegex
  disassembles every pixel shader the game creates to try its patterns, so something in that path (3Dmigoto's
  disassembler or assembler, PCRE2, or our replacement) fails on one of that carrier's shaders. Which one isn't known.
- **Calibrated fixes** (`nexus_hud::dimmer::start_recording`, `finish_calibration`): the config holds no patterns. For
  one game session it sets `export_shaders=1` (the game runs without our fixes, so the originals are exported, about
  21 MB); Elite Nexus then runs the same patterns on the exported assembly in Rust and writes
  `ShaderFixes<hash>-ps.txt` for each match (the export, `t120` declared before the inputs, `dcl_temps` grown by the
  replacement's temps, the replacement in place of the match), keeping copies in `EliteNexus-shaders` (the HUD
  fixes go into ShaderFixes only while the protection is on) and the game build in `EliteNexus-calibration.json`.
  In the game 3Dmigoto only looks up fingerprints: unknown shaders, such as a new carrier's, are never taken apart.
  A game update that changes a fixed shader leaves the dimmer off (the fingerprint no longer matches) until the next
  calibration; the page compares the build with the journal's and asks for one.
- **Checked offline** against the 309 exported pixel shaders: the calibration finds `3f366702d540af3e` and
  `7d85f63f9ab11daf` and the 12 HUD shaders, and the fix it writes for `3f366702d540af3e` is the hand-made fix tested in
  the game that morning, line for line (`real_shaders` with `NEXUS_SHADER_DIR` and `NEXUS_HAND_FIX`).
- **To check in the game**: calibrate, then the dimmer; then fly near a newer fleet carrier. If it still crashes there,
  3Dmigoto itself (not the patterns) can't take that carrier's shaders.

Sources: https://github.com/psychicEgg/EDHM (Odyssey README, patch notes, licence),
https://github.com/BlueMystical/EDHM_UI (`source_v3/src/Helpers/SettingsHelper.js`, `ThemeReloadSignal.js`,
`data/ODYSS/*.ini`, `ThemeTemplate.json`, `MainWindow/Components/XmlEditor.vue`), https://github.com/bo3b/3Dmigoto
(`util.h`, `DirectX11/IniHandler.cpp`), COVAS:NEXT `src/lib/HudColorMatrix.py`, ED Scout `HudColourAdjuster.py`.
