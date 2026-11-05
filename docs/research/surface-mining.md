# Rhino surface mining: rigs and deposits

Researched 2026-11-05. Game state: Odyssey 4.4.2.1 (the Rhino arrived with 4.4.1.0 on 2 September 2026). What the
journal and Status.json say about surface mining is in `journal.md` §5.7; this file holds the game facts the Mining
page uses that no journal records: how far apart rigs go, how big deposits are, what a rig and a deposit give, and
how often each material turns up on each ground (§4).

Certainty: **Frontier** means Frontier's own update notes; **players** means commanders' reports on Frontier's forums,
with no published measurement method; **codex** means the in-game codex as players quote it (not checked in the game
here). Frontier has published none of the distances below.

## 1. Rigs

| Fact | Value | Certainty | Source |
|---|---|---|---|
| Least distance between two rigs | about 75 m | players | "The distance between rigs is around 75m" (CMDR_UKmouse, 2 Sep 2026, thread 649463); "should be 50m, not 75m" (Kzak, 20 Sep 2026, thread 649458); "looks like 75m to 100m apart" (Capt. Hook, 2 Sep 2026, thread 649461). Kzak had said 100 m on 2 Sep and 75 m by 20 Sep. One post says 35 m, contradicted by every other. |
| The game refuses a rig closer than that | hard limit | players | "I couldn't put a second rig down was only 1 or 2 meters off the current limit" (ebbrell, 20 Sep 2026, thread 649458); "exclusion zone around each mining rig" is a hard limit (Armchair, 22 Sep 2026). |
| Rigs working at once | up to 6 | players | "one mining location where I had 6 mining rigs in action" (thread 649463); "6-rig spots", "spots where you theoretically could place 7 Rigs" (Tessa R Act, 21 Sep 2026, thread 649458). |
| Rigs carried | 12 | players | "My rigs show 12/12" (Zechs Darius, 2 Sep 2026, thread 649463). Synthesis refills them: 3 iron, 2 nickel, 1 mechanical equipment (same post). |
| Chunks a rig extracts | at most 12 (9 before 4.4.1.1) | Frontier | 4.4.1.1 notes, 3 Sep 2026: "Adjusted maximum chunks a mining rig can extract, increased to 12 from 9." |
| Rigs lost | on boarding the ship or relogging | players | "If you relog, you lose your deployed rigs. Tested. If you board ship, you lose your deployed rigs. Tested." (thread 649463). The 4 km warning and 5 km loss are in `journal.md` §5.7. |

No source found says 77 m, a figure mentioned once: the reports run from 75 to 100 m, and the later ones say 75 m.

**Rigs that fit a patch** (Mining page): with rigs at least *s* apart and inside a round patch, the narrowest patch
that takes *n* of them is given by the best packing of *n* points in a disc (known for these *n*):

| Rigs | Width | Arrangement |
|---|---|---|
| 2 | *s* = 75 m | a pair |
| 3 | 2*s*/√3 ≈ 87 m | a triangle |
| 4 | *s*√2 ≈ 106 m | a square |
| 5 | *s* / sin 36° ≈ 128 m | a pentagon |
| 6 | 2*s* = 150 m | one in the middle, five on a ring *s* out (72° apart, 88 m between neighbours) |

Six is also the most that work at once, so a wider patch takes no more working rigs. Patches aren't round, so this is
a guide: a long narrow patch takes fewer.

## 2. Deposits and mining locations

| Fact | Value | Certainty | Source |
|---|---|---|---|
| A deposit's size | a few metres to 300 m across | codex | "The codex reports the mining locations are anywhere from a few meters across, up to 300m max" (CMDR_UKmouse, 2 Sep 2026, thread 649463): the purple patches the deposit scanner shows. |
| A planetary mining location's size | shown in the game; "6 Km or bigger" holds the big patches | players | Tessa R Act, 21 Sep 2026, thread 649458. |
| What a deposit holds | high density about 500 t, medium 800-900 t, low about 1,800 t | players (one commander) | Tessa R Act, 21 Sep 2026, thread 649458. Frontier's 4.4.1.0 notes confirm "Deposit capacity per density level" and "Efficiency based on deposit density" as balance values, without numbers. |
| Refilling | a mined-out deposit refills over weeks | players | "some 3-5 weeks when all the good spots I discovered have refilled" (Tessa R Act, 21 Sep 2026); Frontier lists "Time to replenish a deposit" (4.4.1.0). |
| What a location holds | its target panel lists its materials once targeted after a DSS scan | players | Tessa R Act, 2 Sep 2026, thread 649461 ("select one after the other all possible mining-sites in the lefthand panel, it will show which Mats are to be found at each"). 4.4.2.0 (20 Oct 2026) fixed "Planetary Mining resources were not being displayed in the Info Panel when scanning a planet with FSS". Neither is written to the journal (no schema change since 6 Sep 2026; no local journal with a scan to check). |

## 3. Where Elite Nexus gets a deposit's outline

No journal or Status.json line describes a deposit or a rig. While Elite Nexus runs, each tonne the Rhino refines
(`MiningRefined` in the SRV) is placed at the commander's Status.json position: the first starts a deposit, refines
within 200 m (`SPOT_RADIUS_M`) belong to it, and those at least 20 m from the ones kept (`POINT_GAP_M`, at most 24)
are kept as the places it refined. The Rhino collects at each rig, so with rigs at least 75 m apart those places
most likely show where the rigs stood (an inference: no journal places a rig). The outline drawn is the circle around them: the ground worked, which the deposit may
exceed.

## 4. What each ground holds: the survey behind Where to mine's odds

No journal or feed says what a planetary mining location holds. In the game, a location's target panel lists its
materials once it's targeted after a DSS scan (§2). A survey of what those panels listed is the one source of odds:

* **What it is:** 1,131 mining locations read in the game up to 2026-10-11 14:28 UTC, published as the share of each
  ground's locations whose panel listed each material ("50% means 5 of every 10 locations listed it"). A listing
  "says the material is there to find, not how many patches are in it".
* **Grounds:** the body class as the system map names it, a rocky body split by its volcanism; magma on a metal-rich
  or high metal content body counts under its class. Locations read: metal-rich 114, high metal content 276, rocky
  without volcanism 242, rocky with metallic magma 99, rocky magma 36, silicate vapour geysers 139, rocky ice 47,
  icy 178. Rocky bodies with other volcanism weren't read.
* **Counts:** `nexus_data::ground_odds` keeps, per ground, how many locations listed each material:
  round(share × locations read), 144 of them, one for each share the survey published (19 metal-rich, 19 high metal
  content, 23 rocky without volcanism, 25 metallic magma, 22 rocky magma, 20 silicate vapour geysers, 10 rocky ice,
  6 icy); a material it listed nowhere on a ground counts as 0 there. Each of the 144 gives back the published share
  to a tenth of a percent (checked 2026-11-05), so they are the survey's own counts. Materials are named as the game names the commodities ("Low Temp
  Diamonds" there is Low Temperature Diamonds).
* **What the page does with them:** the share per ground, "x of n read" wherever it matters (36 rocky magma
  locations make rough odds), and share × a body's own locations as "≈ n" (an average, which holds however the
  locations of one body go together). With a material and no class, Spansh is asked only for the classes where it
  turned up; the survey's prices are not used (Elite Nexus has its own market data).
* **Where it was read:** the "Ground Mining Table" at https://wiknow.pages.dev/ref/ground-mining ("Generated from
  EDIntel, 2026-10-11 14:28 UTC"), and with the per-ground location counts in `mining_sheet.json` of
  github.com/Fumlop/EDRhinoSpotter (GPL-3.0; "source": "EDIntel prospecting guide"), both read 2026-11-05. Only the
  numbers are used; no file was copied.

## Sources

* Frontier update notes: https://www.elitedangerous.com/news/updates/4-4-1-0 (2 Sep 2026),
  `.../4-4-1-1` (3 Sep 2026), `.../4-4-2-0` (20 Oct 2026), read 2026-11-05.
* Frontier forums, read 2026-11-05: "The Rhino, and mining rigs." (threads/the-rhino-and-mining-rigs.649463),
  "Mining Rig closer placement should be allowed" (threads/mining-rig-closer-placement-should-be-allowed.649458, two
  pages), "Using the Rhino and surface mining; first impressions"
  (threads/using-the-rhino-and-surface-mining-first-impressions.649461, page 1).
* Not readable from here: Fandom's Mining Rig page (HTTP 402), a news site behind a bot check, Reddit, Inara
  logbooks (HTTP 503). A field-manual wiki says the exact spacing "is not documented".
