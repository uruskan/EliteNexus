# Engineering, materials and engineers — research notes for Elite Nexus

_Researched 2026-10-12, game version 4.4.x (Rhino update)._

This covers ship and on-foot engineering, experimental effects, engineers, the material trader, technology brokers and synthesis. Facts were cross-checked across EDEngineer (MIT), EDDiscovery `EliteDangerousCore` (Apache-2.0), the Elite Dangerous Fandom wiki (CC BY-SA, used for fact checks only) and Frontier's journal manual.

---

## 0. TL;DR

* **What to ship in Elite Nexus**
  * **Ship engineering** (blueprints G1–G5 with materials, engineers per grade, experimentals, synthesis, tech-broker and engineer-unlock costs): use **EDEngineer `blueprints.json` + `entryData.json` (MIT)**, downloaded to `data/engineering/edengineer/`.
    * It's the only permissive, ready-to-use JSON dataset.
    * Its ship data matches the wiki except 4 engineer-grade cells (§6.3).
    * The repo is archived (2024-03), but ship blueprints haven't changed since.
  * **Odyssey (suits/weapons) and engineer locations/requirements:** EDEngineer is **stale**. It has pre-2022 upgrade and mod costs, no credit costs, and no Colonia on-foot engineers.
    * Use the **EDDiscovery `EliteDangerousCore` sources (Apache-2.0)**, downloaded unmodified to `data/engineering/eddiscovery-src/`.
    * The tables below were extracted from them and cross-checked against the wiki.
  * **Material categories, grades and IDs:** FDevIDs `material.csv` (in `data/fdevids/`; licence caveat) or EDD `MCMRType.cs` (Apache-2.0). EDD also covers the newer Thargoid materials.
* **Rejected or reference-only sources**
  * **coriolis-data** says its data is **Frontier IP "subject to their terms"**; only its code is MIT.
  * **EDSY** is CC BY-NC 4.0.
  * **EDOMH** (Odyssey Materials Helper) source is MIT, but all game data now lives in closed-source libraries (`nl.jixxed.ed.data:ed-data-impl`, `nl.edomh.core`) under a proprietary EULA. Nothing usable to copy.
  * **Inara** has no data licence (and blocked our fetches).
* **Material trader:** same category up one grade costs 6:1; down one grade gives 1:3. Crossing categories adds another 6:1, so same-grade cross-category is 6:1 and one grade down cross-category is 2:1. Caps by grade: **300 / 250 / 200 / 150 / 100**.

---

## 1. Candidate sources and licences

| Source | Content | Format | Licence (verified) | Currency | Verdict |
|---|---|---|---|---|---|
| **msarilar/EDEngineer** `EDEngineer/Resources/Data/blueprints.json`, `entryData.json`, `equipment.json` | 1172 recipe rows: ship blueprints G1–G5 + engineers, experimentals, synthesis, engineer unlocks, Guardian/Human tech broker, Odyssey upgrades/mods (old); 371 ingredient definitions | JSON | **MIT**, © 2016 Max (repo LICENSE) | archived 2024-03-22 (commit `2cadd66`) | **Downloaded** → `data/engineering/edengineer/` |
| **EDDiscovery/EliteDangerousCore** `FrontierData/Items/*.cs` | `RecipesEngineering.cs` (802 ship blueprint rows, 87 experimentals, 61 tech-broker rows, 24 suit/weapon upgrade rows, 31 on-foot mods with credits), `RecipesSynthesis.cs`, `MCMRType.cs` (every material/commodity/microresource with grade, trader group, short code, caps), `Engineers.cs` (38 engineers: system, base, body, coords, discovery/meeting/unlock text, permit flag), `Suits.cs`, `HandItems.cs` | C# source | **Apache-2.0** | active, 2026-10-10 (commit `13bb0c0`) | **Downloaded (unmodified reference)** → `data/engineering/eddiscovery-src/` |
| EDCD/coriolis-data `modifications/{blueprints,modules,specials,modifications,modifierActions}.json` | Best numeric data: per-grade **modifier ranges** (`features`), engineers per grade, experimental effects and costs | JSON | LICENSE.md: code (JS/CSS/HTML/SVG) MIT; **"All Data and associated JSON files are intellectual property and copyright of Frontier Developments plc … subject to their terms"** | 2026-09-26 | **Not downloaded** (not permissive). Read-only reference; to use the modifier ranges, fetch at runtime or ask EDCD and Frontier |
| jixxed/ed-odyssey-materials-helper (EDOMH 3.15.6) | Odyssey and Horizons planner | Java | repo **MIT**, but NOTICE + EULA: binaries contain proprietary components; game data is in external libraries (`ed-data-impl`, `edomh-core`), not in the repo | 2026-10-12 | **Nothing to download**; app usable for manual fact checks only |
| taleden/EDSY `eddb.js` | Very complete ship/module/blueprint/experimental data | JS | **CC BY-NC 4.0** (code/markup); game data stated as Frontier property | 2025 | **Not permissive** — reference only |
| Elite Dangerous Fandom wiki | Engineer pages, upgrade tables, trader page | wikitext | CC BY-SA 3.0 | live | Used to **verify facts** (no text copied) |
| EDCD/FDevIDs `material.csv`, `microresources.csv` | IDs, grades, trader categories | CSV | **none declared** | 2026-09-05 | Downloaded under `data/fdevids/` (see licence note there) |

---

## 2. Data formats of the downloaded datasets

### 2.1 EDEngineer `blueprints.json` (array of objects)

```json
{ "Type": "Frame Shift Drive",          // module type, or "Unlock" | "Guardian" | "Human" | "Weapon" | "Suit" | synthesis name
  "Name": "Increased Range",            // blueprint / experimental / engineer / item name
  "Grade": 5,                           // 1..5 for blueprint grades; null for experimentals, unlocks, tech broker, odyssey mods
  "Engineers": ["Felicity Farseer","Elvira Martuuk","Mel Brandon"],   // or "@Synthesis" | "@Technology" | "@Merchant" | "@Bartender"
  "Ingredients": [ {"Name": "Datamined Wake Exceptions", "Size": 1}, ... ],
  "Effects": [ {"Property":"Optimal Mass","Effect":"+55%","IsGood":true}, ... ],   // display strings only
  "CoriolisGuid": "..." }               // optional link to coriolis-data blueprint uuid
```

* **Distinguishing row kinds** (counts in the downloaded file):
  * Ship blueprint grades: `Grade` 1–5, Type is a module type (786 rows = 160 blueprints over 38 module types).
  * Experimental: `Grade == null`, Type is a module type, Engineers are the module's engineers (155 rows, 62 distinct effects).
  * Engineer unlock: `Type == "Unlock"` (26 rows: 20 ship engineers + 6 on-foot, 4 of them tagged `@Bartender`).
  * Tech broker: `Type == "Guardian"` (29) or `"Human"` (21).
  * Synthesis: Engineers contain `"@Synthesis"` (65 rows). Type = synthesis family (e.g. "FSD Injection"), `Grade` 1–3 = Basic/Standard/Premium, Name = the variant's effect text.
  * Odyssey (`Type == "Weapon"` / `"Suit"`): 90 rows — 56 grade upgrades (Engineers `["@Merchant"]`, Grade 2–5) and 34 mods (Grade null). **Outdated values**; use §5.
* `Ingredients[].Name` is the English display name. Map it to journal FD names through `entryData.json`, which holds:
  * `Name`, `FormattedName` (= journal symbol)
  * `Kind` ∈ Material / Data / Commodity / OdysseyIngredient
  * `Subkind` ∈ Raw / Manufactured
  * `Rarity` ∈ VeryCommon / Common / Standard / Rare / VeryRare (= grade 1–5)
  * `Group` (trader category: `Category1`…`Category7`, `Chemical`, `Thermic`, `Heat`, `Conductive`, `MechanicalComponents`, `Capacitors`, `Shielding`, `Composite`, `Crystals`, `Alloys`, `EmissionData`, `WakeScans`, `ShieldData`, `EncryptionFiles`, `DataArchives`, `EncodedFirmware`, `GuardianRuins…`, `Thargoid…`)
  * `OriginDetails[]`, `MaximumCapacity` (override)
  * For Odyssey items: `ValueCr`, `BarterCost`, `BarterValue`, `SettlementType`, `BuildingType`, `ContainerType`
* `equipment.json` maps the 15 suits and weapons to symbols (`utilitysuit`, `wpn_m_assaultrifle_kinetic_fauto`, …).

### 2.2 EDDiscovery sources (parse with simple regexes)

* **Recipes:** `new EngineeringRecipe("<name>", "<fdname>", "<ingredients>", ItemData.ShipModule.ModuleTypes.<Type>, <grade>, "<engineer>,<engineer>")`.
  * `fdname` is the journal `BlueprintName`/`ExperimentalEffect`, e.g. `armour_advanced`, `special_fsd_heavy`.
  * Ingredients are `<count><shortcode>` comma lists, e.g. `"1Fe,1CCo,1HDC"`.
  * Other constructors in the same file cover tech broker `(name, fdname, "Human|Guardian", ModuleTypes.X, ingredients)`, special effects `(name, fdname, ModuleTypes.X | "A,B", ingredients)`, suit/weapon levels `(ModuleTypes.Suit|Weapon, "<manufacturer>", <grade>, ingredients)` and on-foot mods `(ModuleTypes.Suit|Weapon, "<fdname>", "<manufacturer|All>", "<name>", <credits>, ingredients, "<engineers>")`.
* **Short codes** are resolved in `MCMRType.cs`: `Add(CatType.Raw, ItemType.VeryCommon, MaterialGroupType.RawCategory1, MCMR.Carbon, "Carbon", "C")`.
  * Fields: category, grade (`VeryCommon`=1 … `VeryRare`=5), trader group, id, [fdname], English, short code.
  * Microresources use `MR…` codes, e.g. `MRCOL` = Optical Lens, `MRDMI` = Manufacturing Instructions.
* **`Engineers.cs`:** `{"Name", new EngineeringInfo(name, system, base, x, y, z, body, discovery, meeting, unlock, repGain, permit, odyssey)}`.
* **Known EDD data bugs:**
  * Mine-launcher blueprints are typed `ModuleTypes.LifeSupport`, and so is the "Ion Disruption" experimental.
  * Etienne Dorn (scanners, PA, rail), Petra Olmanova (chaff, ECM, missiles, PD, torpedo) and Marsha Hicks (fragment cannon, fuel-transfer, hatch-breaker) top out at **G4 in EDD**, but the game, wiki and EDEngineer say **G5**.
  * Dazzle Shell lists Mechanical Scrap twice.
  * Use EDD for Odyssey and engineer metadata, EDEngineer for ship grades.

---

## 3. Material trader and material rules

### 3.1 Exchange ratios (verified against the wiki "Material Trader" exchange tables)

**Same category** (input grade across, output grade down; "a → b" = give a, receive b):

| out \ in | G1 | G2 | G3 | G4 | G5 |
|---|---|---|---|---|---|
| **G1** | – | 1 → 3 | 1 → 9 | 1 → 27 | 1 → 81 |
| **G2** | 6 → 1 | – | 1 → 3 | 1 → 9 | 1 → 27 |
| **G3** | 36 → 1 | 6 → 1 | – | 1 → 3 | 1 → 9 |
| **G4** | 216 → 1 | 36 → 1 | 6 → 1 | – | 1 → 3 |
| **G5** | 1296 → 1* | 216 → 1 | 36 → 1 | 6 → 1 | – |

**Different category, same material type** (extra ×6 cost):

| out \ in | G1 | G2 | G3 | G4 | G5 |
|---|---|---|---|---|---|
| **G1** | 6 → 1 | 2 → 1 | 2 → 3 | 2 → 9 | 2 → 27 |
| **G2** | 36 → 1 | 6 → 1 | 2 → 1 | 2 → 3 | 2 → 9 |
| **G3** | 216 → 1 | 36 → 1 | 6 → 1 | 2 → 1 | 2 → 3 |
| **G4** | 1296 → 1* | 216 → 1 | 36 → 1 | 6 → 1 | 2 → 1 |
| **G5** | 7776 → 1* | 1296 → 1* | 216 → 1* | 36 → 1 | 6 → 1 |

`*` = impossible in practice because the input exceeds the storage cap.

**Formula:**

* Same category: up Δ grades costs `6^Δ : 1`; down Δ grades gives `1 : 3^Δ`.
* Cross category: multiply the input side by 6.
* Trades happen in whole multiples of the ratio.
* Raw has only grades 1–4.

Other rules:

* **Raw ↔ Manufactured ↔ Encoded cannot be exchanged** (separate traders).
* Guardian, Thargoid and Sensor-Fragment-type materials (category `None` in `material.csv`) are **not tradeable** at material traders.
* The journal records each trade as `MaterialTrade{TraderType, Paid{Material,Category,Quantity}, Received{…}}`.

### 3.2 Storage caps (per material, by grade)

* **G1 300**, G2 250, G3 200, G4 150, G5 100 (EDD `MCMRType.cs` constants `VeryCommonCap … VeryRareCap`).
* The same caps apply to Raw (G1–G4 only), Manufactured and Encoded.
* EDEngineer overrides caps for a few special items (e.g. Guardian obelisk data `MaximumCapacity: 150`).

### 3.3 Where traders are

* **Raw** trader: station economy **Extraction or Refinery**.
* **Manufactured**: **Industrial**.
* **Encoded**: **High Tech or Military**.
* Community pseudocode (FDevIDs `How to determine MatTrader and Broker type`): check the primary economy, then the secondary. The wiki additionally lists Extraction for Manufactured; treat secondary-economy cases as uncertain.
* **Technology brokers:** **Guardian** at High Tech, **Human** at Industrial. Journal `TechnologyBroker.BrokerType` values per EDD: `Guardian`, `Human`, `Mixed`, `Rescue`, `Salvation`, `Sirius`, `TorvalMining`; compare case-insensitively.
* Wiki-reported availability limits: medium/high security systems, population ~1 M – 22 M, no anarchy factions, not at damaged/repairing/lockdown stations.
* `Docked.StationServices` contains `materialtrader` / `techBroker`, but **not the type**. Infer the type from economies, or from a past `MaterialTrade.TraderType`.

### 3.4 Category membership (FDevIDs `material.csv`; matches EDD and the wiki)

| Type | Trader category | G1 | G2 | G3 | G4 | G5 |
|---|---|---|---|---|---|---|
| Raw | Raw category 1 | Carbon (`carbon`) | Vanadium (`vanadium`) | Niobium (`niobium`) | Yttrium (`yttrium`) | – |
| Raw | Raw category 2 | Phosphorus (`phosphorus`) | Chromium (`chromium`) | Molybdenum (`molybdenum`) | Technetium (`technetium`) | – |
| Raw | Raw category 3 | Sulphur (`sulphur`) | Manganese (`manganese`) | Cadmium (`cadmium`) | Ruthenium (`ruthenium`) | – |
| Raw | Raw category 4 | Iron (`iron`) | Zinc (`zinc`) | Tin (`tin`) | Selenium (`selenium`) | – |
| Raw | Raw category 5 | Nickel (`nickel`) | Germanium (`germanium`) | Tungsten (`tungsten`) | Tellurium (`tellurium`) | – |
| Raw | Raw category 6 | Rhenium (`rhenium`) | Arsenic (`arsenic`) | Mercury (`mercury`) | Polonium (`polonium`) | – |
| Raw | Raw category 7 | Lead (`lead`) | Zirconium (`zirconium`) | Boron (`boron`) | Antimony (`antimony`) | – |
| Manufactured | Alloys | Salvaged Alloys (`salvagedalloys`) | Galvanising Alloys (`galvanisingalloys`) | Phase Alloys (`phasealloys`) | Proto Light Alloys (`protolightalloys`) | Proto Radiolic Alloys (`protoradiolicalloys`) |
| Manufactured | Capacitors | Grid Resistors (`gridresistors`) | Hybrid Capacitors (`hybridcapacitors`) | Electrochemical Arrays (`electrochemicalarrays`) | Polymer Capacitors (`polymercapacitors`) | Military Supercapacitors (`militarysupercapacitors`) |
| Manufactured | Chemical | Chemical Storage Units (`chemicalstorageunits`) | Chemical Processors (`chemicalprocessors`) | Chemical Distillery (`chemicaldistillery`) | Chemical Manipulators (`chemicalmanipulators`) | Pharmaceutical Isolators (`pharmaceuticalisolators`) |
| Manufactured | Composite | Compact Composites (`compactcomposites`) | Filament Composites (`filamentcomposites`) | High Density Composites (`highdensitycomposites`) | Proprietary Composites (`fedproprietarycomposites`) | Core Dynamics Composites (`fedcorecomposites`) |
| Manufactured | Conductive | Basic Conductors (`basicconductors`) | Conductive Components (`conductivecomponents`) | Conductive Ceramics (`conductiveceramics`) | Conductive Polymers (`conductivepolymers`) | Biotech Conductors (`biotechconductors`) |
| Manufactured | Crystals | Crystal Shards (`crystalshards`) | Flawed Focus Crystals (`uncutfocuscrystals`) | Focus Crystals (`focuscrystals`) | Refined Focus Crystals (`refinedfocuscrystals`) | Exquisite Focus Crystals (`exquisitefocuscrystals`) |
| Manufactured | Heat | Heat Conduction Wiring (`heatconductionwiring`) | Heat Dispersion Plate (`heatdispersionplate`) | Heat Exchangers (`heatexchangers`) | Heat Vanes (`heatvanes`) | Proto Heat Radiators (`protoheatradiators`) |
| Manufactured | Mechanical Components | Mechanical Scrap (`mechanicalscrap`) | Mechanical Equipment (`mechanicalequipment`) | Mechanical Components (`mechanicalcomponents`) | Configurable Components (`configurablecomponents`) | Improvised Components (`improvisedcomponents`) |
| Manufactured | Shielding | Worn Shield Emitters (`wornshieldemitters`) | Shield Emitters (`shieldemitters`) | Shielding Sensors (`shieldingsensors`) | Compound Shielding (`compoundshielding`) | Imperial Shielding (`imperialshielding`) |
| Manufactured | Thermic | Tempered Alloys (`temperedalloys`) | Heat Resistant Ceramics (`heatresistantceramics`) | Precipitated Alloys (`precipitatedalloys`) | Thermic Alloys (`thermicalloys`) | Military Grade Alloys (`militarygradealloys`) |
| Encoded | Data Archives | Anomalous Bulk Scan Data (`bulkscandata`) | Unidentified Scan Archives (`scanarchives`) | Classified Scan Databanks (`scandatabanks`) | Divergent Scan Data (`encodedscandata`) | Classified Scan Fragment (`classifiedscandata`) |
| Encoded | Emission Data | Exceptional Scrambled Emission Data (`scrambledemissiondata`) | Irregular Emission Data (`archivedemissiondata`) | Unexpected Emission Data (`emissiondata`) | Decoded Emission Data (`decodedemissiondata`) | Abnormal Compact Emissions Data (`compactemissionsdata`) |
| Encoded | Encoded Firmware | Specialised Legacy Firmware (`legacyfirmware`) | Modified Consumer Firmware (`consumerfirmware`) | Cracked Industrial Firmware (`industrialfirmware`) | Security Firmware Patch (`securityfirmware`) | Modified Embedded Firmware (`embeddedfirmware`) |
| Encoded | Encryption Files | Unusual Encrypted Files (`encryptedfiles`) | Tagged Encryption Codes (`encryptioncodes`) | Open Symmetric Keys (`symmetrickeys`) | Atypical Encryption Archives (`encryptionarchives`) | Adaptive Encryptors Capture (`adaptiveencryptors`) |
| Encoded | Shield Data | Distorted Shield Cycle Recordings (`shieldcyclerecordings`) | Inconsistent Shield Soak Analysis (`shieldsoakanalysis`) | Untypical Shield Scans (`shielddensityreports`) | Aberrant Shield Pattern Analysis (`shieldpatternanalysis`) | Peculiar Shield Frequency Data (`shieldfrequencydata`) |
| Encoded | Wake Scans | Atypical Disrupted Wake Echoes (`disruptedwakeechoes`) | Anomalous FSD Telemetry (`fsdtelemetry`) | Strange Wake Solutions (`wakesolutions`) | Eccentric Hyperspace Trajectories (`hyperspacetrajectories`) | Datamined Wake Exceptions (`dataminedwake`) |

**Not tradeable at material traders** (FDevIDs category `None`; grade in brackets):

- Manufactured: Sensor Fragment [G5], Thargoid Carapace [G2], Thargoid Energy Cell [G3], Thargoid Organic Circuitry [G5], Thargoid Technological Components [G4], Bio-Mechanical Conduits [G3], Propulsion Elements [G5], Weapon Parts [G4], Wreckage Components [G3], Guardian Power Cell [G1], Guardian Power Conduit [G2], Guardian Technology Component [G3], Guardian Sentinel Weapon Parts [G3], Guardian Sentinel Wreckage Components [G1]
- Encoded: Thargoid Ship Signature [G3], Thargoid Wake Data [G4], Pattern Delta Obelisk Data [G4], Pattern Alpha Obelisk Data [G4], Pattern Beta Obelisk Data [G4], Pattern Gamma Obelisk Data [G4], Pattern Epsilon Obelisk Data [G4], Thargoid Material Composition Data [G3], Thargoid Residue Data [G4], Thargoid Structural Data [G2], Ship Flight Data [G3], Ship Systems Data [G4], Guardian Weapon Blueprint Segment [G4], Guardian Module Blueprint Segment [G4], Guardian Vessel Blueprint Segment [G5]
- In EDD `MCMRType.cs` but missing from FDevIDs (mostly 2022–23 Thargoid-war items): Caustic Crystal [Manufactured, G4, `tg_causticcrystal`], Caustic Shard [Manufactured, G3, `tg_causticshard`], Corrosive Mechanisms [Manufactured, G3, `tg_causticgeneratorparts`], Hardened Surface Fragments [Manufactured, G1, `tg_abrasion03`], Heat Exposure Specimen [Manufactured, G5, `tg_abrasion01`], Massive Energy Surge Analytics [Encoded, G3, `tg_shutdowndata`], Phasing Membrane Residue [Manufactured, G3, `tg_abrasion02`], Tactical Core Chip [Manufactured, G2, `unknowncorechip`], Thargoid Capital Ship Data [Encoded, G3, `tg_structuraldata02`], Thargoid Interdiction Telemetry [Encoded, G3, `tg_interdictiondata`]

---

## 4. Engineers

### 4.1 Ship engineers — discovery chain

* **Public / known at start:** Felicity Farseer, Elvira Martuuk, The Dweller, Liz Ryder, Tod "The Blaster" McQuinn.
* **Referral:** "Achieve Grade 3 access plus 1/3 progress towards Grade 4 with X" reveals the next engineer:

```
Felicity Farseer ─▶ Juri Ishmaak ─▶ The Sarge, Colonel Bris Dekker
Elvira Martuuk  ─▶ Zacariah Nemo, Marco Qwent (─▶ Professor Palin, Lori Jameson, Chloe Sedesi), Mel Brandon*
The Dweller     ─▶ Lei Cheung (─▶ Ram Tah), Marsha Hicks*
Liz Ryder       ─▶ Hera Tani (─▶ Broo Tarquin, Tiana Fortune), Etienne Dorn*
Tod McQuinn     ─▶ Selene Jean (─▶ Didi Vatermann, Bill Turner), Petra Olmanova*
(* = Colonia region, ~22,000 ly from Sol)
```

* **Reputation:** unlocking gives Grade 1 access. Crafting (and the engineer-specific trades in the table) raises the grade. The journal reports progress in `EngineerProgress{Engineer, EngineerID, Progress, Rank, RankProgress}`; FDevIDs `engineers.csv` maps `EngineerID` → name/system/market.

### 4.2 Engineer table (locations and requirements)

* Locations and ship unlock costs: EDD `Engineers.cs`.
* On-foot meeting and referral requirements: wiki (EDD still has pre-2022 on-foot thresholds, e.g. "sell 10 Opinion Polls" where the current game needs 5).

| Engineer | Type | System | Base (body) | Discovery | Meeting requirement | Unlock (ship) / Referral task (on-foot) | Permit? |
|---|---|---|---|---|---|---|---|
| Bill Turner | Ship | Alioth | Turner Metallics Inc (4 A) | Common knowledge | Gain Friendly status with Alliance. You will also need Allied status with Alioth Independents to get a permit to access the Alioth starsystem. | Provide 50 units of Bromellite. | yes |
| Broo Tarquin | Ship | Muang | Broo's Legacy (5 A) | From Hera Tani (grade 3-4). | Gain combat rank Competent or higher. | Provide 50 units of Fujin Tea. | no |
| Chloe Sedesi | Ship | Shenve | Cinder Dock (A 6) | From Marco Qwent (grade 3-4). | Attain a maximum distance from your career start location of at least 5,000 light years. | Provide 25 units of Sensor Fragments. | no |
| Colonel Bris Dekker | Ship | Sol | Dekker's Yard (Iapetus) | From Juri Ishmaak (grade 3-4). | Friendly with the Federation. | Provide 1,000,000 or 10,000,000 credits worth of federal combat bonds. | yes |
| Didi Vatermann | Ship | Leesti | Vatermann LLC (1 A) | From Selene Jean (grade 3-4). | Gain trade rank Merchant or higher. | Provide 50 units of Lavian Brandy. | no |
| Elvira Martuuk | Ship | Khun | Long Sight Base (5) | Public knowledge | Attain a maximum distance from your career start location of at least 300 light years. | Provide 3 units of Soontill Relics. | no |
| Felicity Farseer | Ship | Deciat | Farseer Inc (6 A) | Public data sources. | Gain exploration rank Scout or higher. | Provide 1 unit of Meta Alloys. | no |
| Hera Tani | Ship | Kuwemaki | The Jet's Hole (A 3 A) | From Liz Ryder (grade 3-4). | Gain rank Outsider or higher with the Empire. | Provide 50 units of Kamitra Cigars. | no |
| Juri Ishmaak | Ship | Giryak | Pater's Memorial (2 A) | From Felicity Farseer (grade 3-4). | Earn more than 50 combat bonds. | Provide 100,000 or 1,000,000 credits worth of combat bonds. | no |
| Lei Cheung | Ship | Laksak | Trader's Rest (A 1) | From The Dweller (grade 3-4). | You have traded in over 50 markets. | Provide 200 units of Gold. | no |
| Liz Ryder | Ship | Eurybia | Demolition Unlimited (Makalu) | Public sources. | Gain Cordial or Friendly status with Eurybia Blue Mafia. | Provide 200 units of Landmines. | no |
| Lori Jameson | Ship | Shinrarta Dezhra | Jameson Base (A 1) | From Marco Qwent (grade 3-4). | Gain combat rank Dangerous or higher. | Provide 25 units of Kongga Ale. | yes |
| Marco Qwent | Ship | Sirius | Qwent Research Base (Lucifer) | From Elvira Martuuk (grade 3-4). | Gain invitation from Sirius Corporation. | Provide 25 units of Modular Terminals. | yes |
| Professor Palin | Ship | Arque | Abel Laboratory (4 E) | From Marco Qwent (grade 3-4). | Attain a maximum distance from your career start location of at least 5,000 light years. | Provide 25 units of Sensor Fragments. | no |
| Ram Tah | Ship | Meene | Phoenix Base (AB 5 D) | From Lei Cheung (grade 3-4). | Gain exploration rank Surveyor or higher. | Provide 50 units of Classified Scan Databanks. | no |
| Selene Jean | Ship | Kuk | Prospector's Rest (B 3) | From Tod McQuinn (grade 3-4). | Mine at least 500 tons of ore. | Provide 10 units of Painite. | no |
| The Dweller | Ship | Wyrd | Black Hide (A 2) | Common knowledge | Deal with at least 5 black markets. | Pay 500,000 credits. | no |
| The Sarge | Ship | Beta-3 Tucani | The Beach (2 B A) | From Juri Ishmaak (grade 3-4). | Gain rank Midshipman or higher with the Federal Navy. | Provide 50 units of Aberrant Shield Pattern Analysis. | no |
| Tiana Fortune | Ship | Achenar | Fortune's Loss (4 A) | From Hera Tani (grade 3-4). | Friendly with the Empire. | Provide 50 units of Decoded Emission Data. | yes |
| Tod 'The Blaster' McQuinn | Ship | Wolf 397 | Trophy Camp (Trus Madi) | Common knowledge. | Earn more than 15 bounty vouchers. | Provide 100,000 credits worth of bounty vouchers. | no |
| Zacariah Nemo | Ship | Yoru | Nemo Cyber Party Base (4) | From Elvira Martuuk (grade 3-4). | Gain invitation from Party of Yoru. | Provide 25 units of Xihe Companions. | no |
| Etienne Dorn | Ship (Colonia) | Los | Kraken's Retreat (A 2 B) | From Liz Ryder (grade 3-4). | Gain trade rank Dealer or higher. | Provide 25 units of Occupied Escape Pods. | no |
| Marsha Hicks | Ship (Colonia) | Tir | The Watchtower (A 2) | From The Dweller (grade 3-4). | Gain exploration rank Surveyor or higher. | Mine 10 units of Osmium. | no |
| Mel Brandon | Ship (Colonia) | Luchtaine | The Brig (A 1 C) | From Elvira Martuuk (grade 3-4). | Gain invitation from Colonia Council. | Provide 100,000 credits worth of bounty vouchers. | no |
| Petra Olmanova | Ship (Colonia) | Asura | Sanctuary (1 A) | From Tod McQuinn (grade 3-4). | Gain combat rank Expert or higher. | Provide 200 units of Progenitor Cells. | no |
| Domino Green | On-foot | Orishis | The Jackrabbit (4) | Domino Green's whereabouts are public knowledge. | Travel 100 light-years in Apex Interstellar Transport shuttles. | Provide 5 units of Push to learn about Kit Fowler. | no |
| Hero Ferrari | On-foot | Siris | Nevermore Terrace (5 C) | Hero Ferrari's whereabouts are public knowledge. | Complete 10 low threat Conflict Zones. | Provide 5 units of Settlement Defence Plans to learn about Wellington Beck. | no |
| Jude Navarro | On-foot | Aurai | Marshall's Drift (1 A) | Jude Navarro's whereabouts are public knowledge. | Complete 10 Restore or Reactivation On Foot missions. | Provide 5 units of Genetic Repair Meds to learn about Terra Velasquez. | no |
| Kit Fowler | On-foot | Capoya | The Last Call (2) | Fulfill Domino Green's referral requirement. | Sell 5 Opinion Polls to Bartenders. | Provide 5 units of Surveillance Equipment to learn about Yarden Bond. | no |
| Oden Geiger | On-foot | Candiaei | Ankh's Promise (9 C) | Fulfilled Terra Velasquez's referral requirement. | Sell 20 total units of Biological Sample, Employee Genetic Data, and/or Genetic Research to Bartenders. | Oden Geiger does not offer a referral. | no |
| Terra Velasquez | On-foot | Shou Xing | Rascal's Choice (1) | Fulfill Jude Navarro's referral requirement. | Complete 6 Covert Heist and/or Covert Theft On Foot missions. | Provide 15 units of Financial Projections to learn about Oden Geiger. | no |
| Uma Laszlo | On-foot | Xuane | Laszlo's Resolve (A 3) | Fulfill Wellington Beck's referral requirement. | Reach Unfriendly reputation or lower with Sirius Corporation. | Uma Laszlo does not offer a referral. | no |
| Wellington Beck | On-foot | Jolapa | Beck Facility (6 A) | Fulfill Hero Ferrari's referral requirement. | Sell 15 total units of Classic Entertainment, Multimedia Entertainment, and/or Cat Media to Bartenders. | Provide 5 units of InSight Entertainment Suites to learn about Uma Laszlo. | no |
| Yarden Bond | On-foot | Bayan | Salamander Bank (7 B) | Fulfill Kit Fowler's referral requirement. | Sell 5 Smear Campaign Plans to Bartenders. | Yarden Bond does not offer a referral. | no |
| Baltanos | On-foot (Colonia) | Deriso | The Divine Apparatus (3 A) | Baltanos's whereabouts are public knowledge. | Attain a Friendly or higher relationship with the Colonia Council minor faction to receive an invitation mission. | Provide 10 units of Faction Associates to learn about Yi Shen. | no |
| Eleanor Bresa | On-foot (Colonia) | Desy | Bresa Modifications (7 A) | Eleanor Bresa's whereabouts are public knowledge. | Dock and disembark at 5 Settlements in the Colonia system. | Provide 10 units of Digital Designs to learn about Yi Shen. | no |
| Rosa Dayette | On-foot (Colonia) | Kojeara | Rosa's Shop (4 B) | Rosa Dayette's whereabouts are public knowledge. | Sell 10 Culinary Recipes and/or Cocktail Recipes to Bartenders in Colonia. | Provide 10 units of Manufacturing Instructions to learn about Yi Shen. | no |
| Yi Shen | On-foot (Colonia) | Einheriar | Eidolon Hold (1 A) | Fulfill the referral requirements of Baltanos, Eleanor Bresa, and Rosa Dayette. | Fulfill the referral requirements of Baltanos, Eleanor Bresa, and Rosa Dayette. | Yi Shen does not offer a referral. | no |

Permit-locked systems: Alioth (Bill Turner), Sol (Bris Dekker), Shinrarta Dezhra (Lori Jameson), Sirius (Marco Qwent), Achenar (Tiana Fortune).

### 4.3 Which modules each engineer offers, and the max grade

Source: wiki engineer pages (checked 2026-10-12). They agree with EDEngineer except where §6.3 notes otherwise.

| Engineer | Modules (max grade) |
|---|---|
| Felicity Farseer | Frame Shift Drive 5, Detailed Surface Scanner 3, Sensors 3, Thrusters 3, Frame Shift Drive Interdictor 1, Power Plant 1, Shield Booster 1 |
| Elvira Martuuk | Frame Shift Drive 5, Shield Generator 3, Thrusters 2, Shield Cell Bank 1 |
| The Dweller | Power Distributor 5, Pulse Laser 4, Beam Laser 3, Burst Laser 3 |
| Liz Ryder | Missile Rack 5, Seeker Missile Rack 5, Torpedo Pylon 5, Mine Launcher 3, Armour 1, Hull Reinforcement Package 1 |
| Tod "The Blaster" McQuinn | Multi-cannons 5, Rail Guns 5, Fragment Cannons 3, Cannons 2 |
| Zacariah Nemo | Fragment Cannon 5, Multi-cannon 3, Plasma Accelerator 2 |
| Lei Cheung | Detailed Surface Scanner 5, Sensors 5, Shield Generator 5, Shield Booster 3 |
| Hera Tani | Detailed Surface Scanner 5, Power Plant 5, Power Distributor 3, Sensors 3 |
| Juri Ishmaak | Detailed Surface Scanner 5, Mine Launcher 5, Sensors 5, Frame Shift Wake Scanner 3, Kill Warrant Scanner 3, Manifest Scanner 3, Missile Rack 3, Seeker Missile Rack 3, Torpedo Pylon 3 |
| Selene Jean | Armour 5, Hull Reinforcement Package 5 |
| Marco Qwent | Power Plant 4, Power Distributor 3 |
| Ram Tah | Chaff Launcher 5, Electronic Countermeasure 5, Heat Sink Launcher 5, Point Defence 5, Collector Limpet Controller 4, Fuel Transfer Limpet Controller 4, Prospector Limpet Controller 4, Hatch Breaker Limpet Controller 3, Guardian Module 1 |
| Broo Tarquin | Beam Laser 5, Burst Laser 5, Pulse Laser 5 |
| Colonel Bris Dekker | Frame Shift Drive Interdictor 4, Frame Shift Drive 3 |
| Didi Vatermann | Shield Booster 5, Shield Generator 3 |
| Professor Palin | Thrusters 5, Frame Shift Drive 3 |
| Lori Jameson | Detailed Surface Scanner 5, Sensors 5, Auto Field-Maintenance Unit 4, Fuel Scoop 4, Life Support 4, Refinery 4, Frame Shift Wake Scanner 3, Kill Warrant Scanner 3, Manifest Scanner 3, Shield Cell Bank 3 |
| Tiana Fortune | Collector Limpet Controller 5, Frame Shift Wake Scanner 5, Fuel Transfer Limpet Controller 5, Hatch Breaker Limpet Controller 5, Kill Warrant Scanner 5, Manifest Scanner 5, Prospector Limpet Controller 5, Sensors 5, Detailed Surface Scanner 3, Frame Shift Drive Interdictor 3 |
| The Sarge | Cannon 5, Collector Limpet Controller 5, Fuel Transfer Limpet Controller 5, Hatch Breaker Limpet Controller 5, Prospector Limpet Controller 5, Rail Gun 3 |
| Bill Turner | Detailed Surface Scanner 5, Plasma Accelerator 5, Sensors 5, Auto Field-Maintenance Unit 3, Frame Shift Wake Scanner 3, Fuel Scoop 3, Kill Warrant Scanner 3, Life Support 3, Manifest Scanner 3, Refinery 3 |
| Etienne Dorn | Detailed Surface Scanner 5, Frame Shift Wake Scanner 5, Kill Warrant Scanner 5, Life Support 5, Manifest Scanner 5, Plasma Accelerator 5, Power Distributor 5, Power Plant 5, Rail Guns 5, Sensors 5 |
| Marsha Hicks | Cannons 5, Collector Limpet Controller 5, Fragment Cannons 5, Fuel Scoop 5, Fuel Transfer Limpet Controller 5, Hatch Breaker Limpet Controller 5, Multi-cannons 5, Prospector Limpet Controller 5, Refinery 5 |
| Mel Brandon | Beam Laser 5, Burst Laser 5, Frame Shift Drive 5, Frame Shift Drive Interdictor 5, Pulse Laser 5, Shield Booster 5, Shield Generator 5, Thrusters 5, Shield Cell Bank 4 |
| Petra Olmanova | Armour 5, Auto Field-Maintenance Unit 5, Chaff Launcher 5, Electronic Countermeasure 5, Heat Sink Launcher 5, Hull Reinforcement Package 5, Mine Launcher 5, Missile Rack 5, Point Defence 5, Seeker Missile Rack 5, Torpedo Pylon 5 |
| Chloe Sedesi | Thrusters 5, Frame Shift Drive 3 |
| Jude Navarro (on-foot) | Added Melee Damage, Damage Resistance, Extra Ammo Capacity, Magazine Size, Reload Speed |
| Domino Green (on-foot) | Enhanced Tracking, Extra Backpack Capacity, Reduced Tool Battery Consumption, Greater Range, Stability |
| Hero Ferrari (on-foot) | Improved Jump Assist, Increased Air Reserves, Increased Sprint Duration, Faster Handling, Noise Suppressor |
| Kit Fowler (on-foot) | Added Melee Damage, Extra Ammo Capacity, Faster Shield Regen, Magazine Size, Stowed Reloading |
| Wellington Beck (on-foot) | Extra Backpack Capacity, Improved Battery Capacity, Reduced Tool Battery Consumption, Greater Range, Scope |
| Terra Velasquez (on-foot) | Combat Movement Speed, Increased Air Reserves, Increased Sprint Duration, Improved Hip Fire Accuracy, Noise Suppressor |
| Uma Laszlo (on-foot) | Damage Resistance, Faster Shield Regen, Headshot Damage, Reload Speed, Stowed Reloading |
| Oden Geiger (on-foot) | Enhanced Tracking, Improved Battery Capacity, Night Vision, Scope, Stability |
| Yarden Bond (on-foot) | Combat Movement Speed, Improved Jump Assist, Quieter Footsteps, Audio Masking, Faster Handling, Improved Hip Fire Accuracy |
| Baltanos (on-foot) | Combat Movement Speed, Improved Jump Assist, Increased Air Reserves, Increased Sprint Duration, Faster Handling, Improved Hip Fire Accuracy, Noise Suppressor |
| Eleanor Bresa (on-foot) | Added Melee Damage, Damage Resistance, Extra Ammo Capacity, Faster Shield Regen, Magazine Size, Reload Speed, Stowed Reloading |
| Rosa Dayette (on-foot) | Enhanced Tracking, Extra Backpack Capacity, Improved Battery Capacity, Reduced Tool Battery Consumption, Greater Range, Scope, Stability |
| Yi Shen (on-foot) | Night Vision, Quieter Footsteps, Audio Masking, Headshot Damage |

On-foot engineers have no grades: each modification is a single upgrade. A suit or weapon of grade G has G−1 modification slots.

---

## 5. Odyssey suits and weapons

#### Suit / weapon grade upgrades (EDD RecipesEngineering.cs 'suit/weapon levels'; matches wiki)

| Item family | G1→G2 | G2→G3 | G3→G4 | G4→G5 |
|---|---|---|---|---|
| Maverick suit (Remlok) | 1 Suit Schematic, 1 Health Monitor, 1 Manufacturing Instructions, 2 Carbon Fibre Plating, 2 Graphene | 2 Suit Schematic, 2 Health Monitor, 2 Manufacturing Instructions, 5 Carbon Fibre Plating, 5 Graphene | 4 Suit Schematic, 4 Health Monitor, 4 Manufacturing Instructions, 9 Carbon Fibre Plating, 9 Graphene | 5 Suit Schematic, 5 Health Monitor, 5 Manufacturing Instructions, 12 Carbon Fibre Plating, 12 Graphene |
| Artemis suit (Supratech) | 1 Suit Schematic, 1 Health Monitor, 1 Manufacturing Instructions, 2 Aerogel, 2 Graphene | 2 Suit Schematic, 2 Health Monitor, 2 Manufacturing Instructions, 5 Aerogel, 5 Graphene | 4 Suit Schematic, 4 Health Monitor, 4 Manufacturing Instructions, 9 Aerogel, 9 Graphene | 5 Suit Schematic, 5 Health Monitor, 5 Manufacturing Instructions, 12 Aerogel, 12 Graphene |
| Dominator suit (Manticore) | 1 Suit Schematic, 1 Health Monitor, 1 Manufacturing Instructions, 2 Titanium Plating, 2 Graphene | 2 Suit Schematic, 2 Health Monitor, 2 Manufacturing Instructions, 5 Titanium Plating, 5 Graphene | 4 Suit Schematic, 4 Health Monitor, 4 Manufacturing Instructions, 9 Titanium Plating, 9 Graphene | 5 Suit Schematic, 5 Health Monitor, 5 Manufacturing Instructions, 12 Titanium Plating, 12 Graphene |
| Kinematic Armaments weapons (Karma AR-50, C-44, L-6, P-15) | 1 Weapon Schematic, 1 Compression-Liquefied Gas, 1 Manufacturing Instructions, 2 Tungsten Carbide, 2 Weapon Component | 2 Weapon Schematic, 2 Compression-Liquefied Gas, 2 Manufacturing Instructions, 5 Tungsten Carbide, 5 Weapon Component | 4 Weapon Schematic, 4 Compression-Liquefied Gas, 4 Manufacturing Instructions, 9 Tungsten Carbide, 9 Weapon Component | 5 Weapon Schematic, 5 Compression-Liquefied Gas, 5 Manufacturing Instructions, 12 Tungsten Carbide, 12 Weapon Component |
| Manticore weapons (Executioner, Intimidator, Oppressor, Tormentor) | 1 Weapon Schematic, 1 Ionised Gas, 1 Manufacturing Instructions, 2 Chemical Superbase, 2 Microelectrode | 2 Weapon Schematic, 2 Ionised Gas, 2 Manufacturing Instructions, 5 Chemical Superbase, 5 Microelectrode | 4 Weapon Schematic, 4 Ionised Gas, 4 Manufacturing Instructions, 9 Chemical Superbase, 9 Microelectrode | 5 Weapon Schematic, 5 Ionised Gas, 5 Manufacturing Instructions, 12 Chemical Superbase, 12 Microelectrode |
| Takada weapons (TK Aphelion, Eclipse, Zenith) | 1 Weapon Schematic, 1 Ionised Gas, 1 Manufacturing Instructions, 2 Microelectrode, 2 Optical Fibre | 2 Weapon Schematic, 2 Ionised Gas, 2 Manufacturing Instructions, 5 Microelectrode, 5 Optical Fibre | 4 Weapon Schematic, 4 Ionised Gas, 4 Manufacturing Instructions, 9 Microelectrode, 9 Optical Fibre | 5 Weapon Schematic, 5 Ionised Gas, 5 Manufacturing Instructions, 12 Microelectrode, 12 Optical Fibre |

#### On-foot modifications (EDD data, Aug-2024 game-data dump; cross-checked against wiki)

| Applies to | Modification (fdname) | Variant | Credits | Materials | Engineers |
|---|---|---|---|---|---|
| Suit | Reduced Tool Battery Consumption (`suit_reducedtoolbatteryconsumption`) | All | 500,000 | 3 Electrical Fuse, 5 Micro Transformer, 8 Electrical Wiring, 5 Reactor Output Review | Domino Green, Wellington Beck, Rosa Dayette |
| Suit | Improved Battery Capacity (`suit_increasedbatterycapacity`) | All | 750,000 | 3 Ion Battery, 5 Micro Supercapacitor, 5 Electrical Wiring, 5 Reactor Output Review, 8 Maintenance Logs | Wellington Beck, Oden Geiger, Rosa Dayette |
| Suit | Faster Shield Regen (`suit_increasedshieldregen`) | All | 750,000 | 3 Ion Battery, 8 Micro Transformer, 8 Electrical Wiring, 5 Reactor Output Review | Kit Fowler, Uma Laszlo, Eleanor Bresa |
| Suit | Damage Resistance (`suit_improvedarmourrating`) | All | 750,000 | 3 Titanium Plating, 3 Carbon Fibre Plating, 8 Epoxy Adhesive, 5 Weapon Inventory, 5 Ballistics Data | Jude Navarro, Uma Laszlo, Eleanor Bresa |
| Suit | Increased Air Reserves (`suit_increasedo2capacity`) | All | 750,000 | 5 Oxygenic Bacteria, 8 pH Neutraliser, 3 Pharmaceutical Patents, 8 Air Quality Reports | Terra Velasquez, Hero Ferrari, Baltanos |
| Suit | Night Vision (`suit_nightvision`) | All | 1,000,000 | 5 Surveillance Equipment, 5 Circuit Switch, 3 Surveillance Logs, 3 Radioactivity Data, 3 NOC Data | Oden Geiger, Yi Shen |
| Suit | Enhanced Tracking (`suit_improvedradar`) | All | 750,000 | 3 Transmitter, 3 Circuit Board, 5 Topographical Surveys, 5 Stellar Activity Logs, 5 Spectral Analysis Data | Domino Green, Oden Geiger, Rosa Dayette |
| Suit | Extra Backpack Capacity (`suit_backpackcapacity`) | All | 750,000 | 5 Epoxy Adhesive, 3 Memory Chip, 5 Weapon Inventory, 5 Chemical Inventory, 5 Digital Designs | Domino Green, Wellington Beck, Rosa Dayette |
| Suit | Extra Ammo Capacity (`suit_increasedammoreserves`) | All | 750,000 | 3 Weapon Component, 8 Recycling Logs, 5 Weapon Test Data, 5 Production Reports | Kit Fowler, Jude Navarro, Eleanor Bresa |
| Suit | Improved Jump Assist (`suit_improvedjumpassist`) | All | 750,000 | 5 G-Meds, 3 Micro Thrusters, 5 Motor, 5 Topographical Surveys | Yarden Bond, Hero Ferrari, Baltanos |
| Suit | Increased Sprint Duration (`suit_increasedsprintduration`) | All | 750,000 | 5 Oxygenic Bacteria, 8 Chemical Catalyst, 3 Troop Deployment Records, 3 Gene Sequencing Data, 3 Clinical Trial Records | Terra Velasquez, Hero Ferrari, Baltanos |
| Suit | Combat Movement Speed (`suit_adsmovementspeed`) | All | 750,000 | 5 Epinephrine, 8 pH Neutraliser, 5 Evacuation Protocols, 3 Genetic Research | Yarden Bond, Terra Velasquez, Baltanos |
| Suit | Quieter Footsteps (`suit_quieterfootsteps`) | All | 1,000,000 | 3 Micro Hydraulics, 8 Viscoelastic Polymer, 3 Settlement Assault Plans, 5 Tactical Plans, 5 Patrol Routes | Yarden Bond, Yi Shen |
| Suit | Added Melee Damage (`suit_increasedmeleedamage`) | All | 500,000 | 5 Epinephrine, 8 Micro Thrusters, 5 Combat Training Material, 5 Combatant Performance | Kit Fowler, Jude Navarro, Eleanor Bresa |
| Weapon | Noise Suppressor (`weapon_suppression_pressurised`) | All | 1,000,000 | 8 Viscoelastic Polymer, 3 Weapon Component, 5 Atmospheric Data, 5 Mining Analytics | Terra Velasquez, Hero Ferrari, Baltanos |
| Weapon | Audio Masking (`weapon_suppression_unpressurised`) | All | 1,000,000 | 5 Scrambler, 8 Transmitter, 3 Circuit Board, 3 Audio Logs, 5 Patrol Routes | Yarden Bond, Yi Shen |
| Weapon | Stability (`weapon_stability`) | All | 500,000 | 5 Viscoelastic Polymer, 5 Micro Hydraulics, 5 Mining Analytics, 8 Risk Assessments | Domino Green, Oden Geiger, Rosa Dayette |
| Weapon | Faster Handling (`weapon_handling`) | All | 500,000 | 3 Viscoelastic Polymer, 5 Operational Manual, 5 Combatant Performance, 5 Combat Training Material | Yarden Bond, Hero Ferrari, Baltanos |
| Weapon | Reload Speed (`weapon_reloadspeed`) | All | 500,000 | 5 Micro Hydraulics, 5 Electromagnet, 5 Operational Manual, 5 Production Reports, 5 Combat Training Material | Jude Navarro, Uma Laszlo, Eleanor Bresa |
| Weapon | Magazine Size (`weapon_clipsize`) | All | 750,000 | 3 Weapon Component, 3 Tungsten Carbide, 5 Metal Coil, 5 Weapon Test Data, 3 Security Expenses | Kit Fowler, Jude Navarro, Eleanor Bresa |
| Weapon | Scope (`weapon_scope`) | All | 500,000 | 5 Optical Lens, 3 Optical Fibre, 5 Spectral Analysis Data, 3 Biometric Data | Wellington Beck, Oden Geiger, Rosa Dayette |
| Weapon | Stowed reloading (`weapon_backpackreloading`) | All | 1,000,000 | 3 Circuit Board, 8 Encrypted Memory Chip, 5 Digital Designs, 5 Operational Manual, 5 Production Schedule | Kit Fowler, Uma Laszlo, Eleanor Bresa |
| Weapon | Higher Accuracy (`weapon_accuracy`) | Kinematic Armaments | 500,000 | 5 Viscoelastic Polymer, 5 RDX, 5 Extraction Yield Data, 3 Biometric Data, 5 Combatant Performance | Yarden Bond, Terra Velasquez, Baltanos |
| Weapon | Greater Range (`weapon_range`) | Kinematic Armaments | 500,000 | 5 Metal Coil, 5 RDX, 3 Weapon Component, 5 Ballistics Data, 5 Topographical Surveys | Domino Green, Wellington Beck, Rosa Dayette |
| Weapon | Headshot damage (`weapon_headshotdamage`) | Kinematic Armaments | 500,000 | 5 Chemical Catalyst, 8 RDX, 3 Weapon Component, 5 Weapon Test Data, 3 Medical Records | Uma Laszlo, Yi Shen |
| Weapon | Higher Accuracy (`weapon_accuracy`) | Manticore | 500,000 | 5 Chemical Catalyst, 5 Electromagnet, 5 Metal Coil, 3 Chemical Patents, 5 Combatant Performance | Yarden Bond, Terra Velasquez, Baltanos |
| Weapon | Greater Range (`weapon_range`) | Manticore | 500,000 | 3 Motor, 5 Electromagnet, 3 Electrical Fuse, 5 Chemical Formulae, 8 Mineral Survey | Domino Green, Wellington Beck, Rosa Dayette |
| Weapon | Headshot damage (`weapon_headshotdamage`) | Manticore | 500,000 | 5 Ion Battery, 5 Electromagnet, 8 Micro Supercapacitor, 5 Chemical Experiment Data, 3 Blood Test Results | Uma Laszlo, Yi Shen |
| Weapon | Higher Accuracy (`weapon_accuracy`) | Takada | 500,000 | 5 Metal Coil, 3 Optical Lens, 8 Electrical Wiring, 3 Radioactivity Data, 5 Combatant Performance | Yarden Bond, Terra Velasquez, Baltanos |
| Weapon | Greater Range (`weapon_range`) | Takada | 500,000 | 8 Micro Transformer, 3 Optical Lens, 3 Circuit Board, 5 Stellar Activity Logs, 8 Risk Assessments | Domino Green, Wellington Beck, Rosa Dayette |
| Weapon | Headshot damage (`weapon_headshotdamage`) | Takada | 750,000 | 5 Ion Battery, 3 Optical Lens, 5 Scrambler, 5 Spectral Analysis Data, 3 Biometric Data | Uma Laszlo, Yi Shen |

### 5.1 Credit costs for grade upgrades

* EDD doesn't store credit costs for upgrades.
* The wiki lists them per item:
  * Artemis / Dominator / Maverick: **600,000 / 2,250,000 / 4,500,000 / 7,500,000** for G2/G3/G4/G5.
  * Karma L-6 and Manticore Executioner: 700,000 / 2,625,000 / 5,250,000 / 8,750,000.
  * Karma AR-50: 500,000 / 1,875,000 / 3,750,000 / 6,250,000.
  * Karma P-15 and TK Zenith: 200,000 / 750,000 / 1,500,000 / (2,500,000).
  * Karma C-44 and TK Eclipse: 300,000 / 1,125,000 / (2,250,000) / –.
  * Wiki tables for several weapons have gaps or typos.
* All complete rows fit **cost = base price × {4, 15, 30, 50}** for G2…G5. Base prices: suits 150k, L-6/Executioner 175k, AR-50/Aphelion/Oppressor 125k, Intimidator 100k, C-44/Eclipse 75k, P-15/Zenith/Tormentor 50k. Treat this as a hypothesis.
* The authoritative per-commander source is the journal: `UpgradeSuit`/`UpgradeWeapon` carry `Cost` and `Resources[]` (since manual v36). Record those to correct the table.
* Pioneer Supplies also sells pre-upgraded and pre-modded items at higher prices (`BuySuit`/`BuyWeapon` `Price`, `SuitMods`/`WeaponMods`).
* The Flight Suit can't be upgraded.

### 5.2 Odyssey material sources (journal)

* `ShipLocker`/`ShipLocker.json` is the full inventory.
* Deltas come from:
  * `BackpackChange`, `CollectItems`, `DropItems`, `UseConsumable`
  * `BuyMicroResources`, `SellMicroResources`, `TradeMicroResources` (bartender)
  * `MissionCompleted.MaterialsReward`
  * `UpgradeSuit/UpgradeWeapon.Resources` (spent)
  * `RequestPowerMicroResources`, `DeliverPowerMicroResources`
* Re-sync on every `ShipLocker` event.
* The 196 microresource ids/symbols are in FDevIDs `microresources.csv` (Item/Component/Data/Consumable). Bartender barter values are in EDEngineer `entryData.json` (`BarterCost`/`BarterValue`, pre-2022 values).

---

## 6. Ship engineering

### 6.1 Mechanics relevant to a planner

* **Blueprint grades:** each blueprint has grades 1–5, and each grade has its own recipe of 1–3 materials (G1: one material, G2: two, G3–G5: three; a few have more). Every **roll** at grade N consumes one recipe of grade N. You can only roll grade N after maxing N−1 on that module (the progress bar).
* **Engineer access:** an engineer offers a blueprint only up to their max grade for that module type (§4.3). You need that engineer's access at or above the grade.
* **Experimental effects:** one per module, applied on a module that has a blueprint. They cost a fixed recipe (Appendix A2). Applying a new experimental replaces the old one. Journal: `EngineerCraft.ApplyExperimentalEffect`, `Loadout…Engineering.ExperimentalEffect`.
* **Journal fields:** engineered modules appear in `Loadout.Modules[].Engineering` as `{EngineerID, BlueprintID, BlueprintName (fdname, e.g. "FSD_LongRange"), Level, Quality (0..1 progress in the current grade), Modifiers[], ExperimentalEffect}`. Stored modules show `EngineerModifications`, `Level` and `Quality`. `EngineerCraft` logs every roll with the `Ingredients[]` consumed. These let the planner subtract materials automatically.
* **Rolls needed per grade:** not published by Frontier. Planners such as EDEngineer and EDOMH let the user choose a roll count; keep that user-configurable.
* **Newer module sources:**
  * **Pre-engineered** items from the tech broker, e.g. "Engineered FSD (SCO) V1" classes 2–7 (needs a Titan Drive Component) and the AX/enzyme weapons (Appendix A4 / EDD list).
  * **MercGear** modules (2026, FDevIDs `outfitting.csv` category `mercgear`) bought with **MercCoins**. Journal: `BuyMercCoinsPrice` on Outfitting/ModuleBuy, `Statistics.Bank_Account.MercCoins_*`.
  * **Merc blueprints** are bought from engineers, and every roll costs Merc Coins as well as materials. Those for a MercGear module start at G2 (the module is bought at G1) and fit only that module; Plasma Conversion (lasers) and Scoop Rate Enhanced (fuel scoops) fit ordinary modules from G1. The planner's data is the `MERC` table in `convert_engineering.rs`.

### 6.2 Module types covered

Armour, AFMU, Beam/Burst/Pulse Laser, Cannon, Chaff Launcher, Collector/Fuel-Transfer/Hatch-Breaker/Prospector Limpet Controllers, ECM, Fragment Cannon, FSD, FSD Interdictor, Fuel Scoop, Heat Sink Launcher, Hull Reinforcement, Kill Warrant Scanner, Life Support, Manifest (cargo) Scanner, Mine Launcher, Missile Rack (incl. seeker), Multi-cannon, Plasma Accelerator, Point Defence, Power Distributor, Power Plant, Rail Gun, Refinery, Sensors, Shield Booster, Shield Cell Bank, Shield Generator, Detailed Surface Scanner, Thrusters, Torpedo Pylon, Frame Shift Wake Scanner.

EDD also lists 4 cosmetic "Decorative" recipes (Remote Release Flak) and Cargo-rack "Increased capacity" rows. The full per-grade recipe tables are in **Appendix A1**.

### 6.3 Dataset discrepancies (and which value to trust)

| Item | EDEngineer | EDD | Wiki / game | Use |
|---|---|---|---|---|
| Juri Ishmaak — Wake / Kill Warrant / Manifest Scanner | not listed | G3 | G3 | add G3 |
| Lori Jameson — Life Support | G5 | G4 | G4 | G4 |
| Etienne Dorn — KWS, Manifest, Wake, PA, Rail | G5 | G4 | G5 | G5 |
| Petra Olmanova — Chaff, ECM, Missile, PD, Torpedo | G5 | G4 | G5 | G5 |
| Marsha Hicks — Fragment, Fuel-Transfer, Hatch-Breaker | G5 | G4 | G5 | G5 |
| Mine Launcher blueprints | Mine Launcher | typed LifeSupport (bug) | Mine Launcher | — |
| Dazzle Shell (PA) cost | 5 Hybrid Cap., 4 Manganese, 5 Mech. Components, 5 Mech. Scrap | 5 Mech. Scrap ×2 (typo) | — | EDEngineer |
| On-foot mod / upgrade costs | pre-2022 (higher) | current | current (matches EDD) | EDD |
| On-foot engineers | 9 bubble | 13 incl. Colonia | 13 | EDD |

Experimental costs agree between EDEngineer and EDD (155 vs 87 rows; EDD lists each effect once with its module list). Naming differs slightly, e.g. "Recycling Cells" vs "Recycling Cell".

---

## 7. Technology brokers

* **Guardian** broker unlocks need Guardian blueprint segments plus Guardian materials, obelisk data and a commodity. They cover the Guardian FSD Booster, Gauss/Plasma/Shard weapons, Guardian HRP/MRP/SRP, hybrid power plant and distributor, and the Guardian fighters Trident/Javelin/Lance.
* **Human** broker unlocks need materials and commodities. They cover shock cannons, flechette launchers, enzyme missiles, corrosion-resistant racks, meta-alloy HRP, pre-engineered FSD/DSS/mining laser/missile rack, Sirius AX items, and (2024+) **Engineered FSD (SCO) V1** classes 2–7 plus TG items.
* EDEngineer has 50 rows (Appendix A4). EDD has 61 rows from Inara (Aug 2024), including the SCO and AX additions (listed after A4).
* Journal: `TechnologyBroker{BrokerType, ItemsUnlocked[], Commodities[], Materials[]}`.

---

## 8. Synthesis

* 20 synthesis families, each with Basic/Standard/Premium recipes (Appendix A3, EDEngineer data; EDD has `RecipesSynthesis.cs`):
  * Ammunition, incl. AX and Guardian munitions
  * FSD Injection (jump boost +25% / +50% / +100%)
  * AFM refill
  * SRV refuel/repair/ammo
  * Limpets
* Journal: `Synthesis{Name ("FSD Basic", …), Materials[Name, Count]}`.

---

## 9. Planner event checklist (journal → inventory)

| Purpose | Events |
|---|---|
| Material snapshot | `Materials` (startup: Raw/Manufactured/Encoded arrays) |
| Material deltas | `MaterialCollected`, `MaterialDiscarded`, `MaterialTrade`, `Synthesis`, `EngineerCraft.Ingredients`, `TechnologyBroker.Materials`, `MissionCompleted.MaterialsReward` (category Raw/Manufactured/Encoded), `ScientificResearch`, `EngineerContribution` (Type Materials) |
| Commodities used in unlocks | `Cargo` / Cargo.json, `MarketBuy/Sell`, `EngineerContribution` (Type Commodity), `TechnologyBroker.Commodities` |
| Engineer state | `EngineerProgress` |
| Module state | `Loadout`, `StoredModules`, `ModuleStore/Retrieve`, `MassModuleStore` |
| Odyssey inventory | `ShipLocker`(.json), `Backpack`(.json), `BackpackChange`, `CollectItems`, `DropItems`, `UseConsumable`, `Buy/Sell/TradeMicroResources`, `UpgradeSuit/UpgradeWeapon.Resources`, `MissionCompleted.MaterialsReward` (`$MICRORESOURCE_CATEGORY_*;`) |
| Suit/weapon state | `SuitLoadout`, `SwitchSuitLoadout`, `BuySuit/BuyWeapon` (`SuitMods`/`WeaponMods`, `Class`), `UpgradeSuit/UpgradeWeapon` (`Class`, `Cost`) |

---

## Appendices (generated from the downloaded data; EDEngineer © 2016 Max, MIT; EDDiscovery © EDDiscovery development team, Apache-2.0)

### A1. Ship blueprints by module type (EDEngineer data; G1..G5 materials; engineers = max grade they offer)

#### Armour

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Blast Resistant | 1 Nickel | 1 Carbon, 1 Zinc | 1 Salvaged Alloys, 1 Vanadium, 1 Zirconium | 1 Galvanising Alloys, 1 Tungsten, 1 Mercury | 1 Phase Alloys, 1 Molybdenum, 1 Ruthenium | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Heavy Duty | 1 Carbon | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Kinetic Resistant | 1 Nickel | 1 Nickel, 1 Vanadium | 1 High Density Composites, 1 Salvaged Alloys, 1 Vanadium | 1 Galvanising Alloys, 1 Tungsten, 1 Proprietary Composites | 1 Phase Alloys, 1 Molybdenum, 1 Core Dynamics Composites | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Lightweight | 1 Iron | 1 Conductive Components, 1 Iron | 1 Conductive Components, 1 High Density Composites, 1 Iron | 1 Germanium, 1 Conductive Ceramics, 1 Proprietary Composites | 1 Conductive Ceramics, 1 Tin, 1 Military Grade Alloys | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Thermal Resistant | 1 Heat Conduction Wiring | 1 Heat Dispersion Plate, 1 Nickel | 1 Heat Exchangers, 1 Salvaged Alloys, 1 Vanadium | 1 Galvanising Alloys, 1 Tungsten, 1 Heat Vanes | 1 Phase Alloys, 1 Molybdenum, 1 Proto Heat Radiators | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |

#### Auto Field-Maintenance Unit

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Core Dynamics Composites, 1 Compound Shielding, 1 Tungsten | Petra Olmanova 5, Lori Jameson 4, Bill Turner 3 |

#### Beam Laser

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Efficient Weapon | 1 Sulphur | 1 Heat Dispersion Plate, 1 Sulphur | 1 Chromium, 1 Exceptional Scrambled Emission Data, 1 Heat Exchangers | 1 Heat Vanes, 1 Irregular Emission Data, 1 Selenium | 1 Cadmium, 1 Proto Heat Radiators, 1 Unexpected Emission Data | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Long Range Weapon | 1 Sulphur | 1 Modified Consumer Firmware, 1 Sulphur | 1 Focus Crystals, 1 Modified Consumer Firmware, 1 Sulphur | 1 Conductive Polymers, 1 Focus Crystals, 1 Modified Consumer Firmware | 1 Cracked Industrial Firmware, 1 Thermic Alloys, 1 Biotech Conductors | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Overcharged Weapon | 1 Nickel | 1 Conductive Components, 1 Nickel | 1 Conductive Components, 1 Electrochemical Arrays, 1 Nickel | 1 Conductive Ceramics, 1 Polymer Capacitors, 1 Zinc | 1 Conductive Polymers, 1 Modified Embedded Firmware, 1 Zirconium | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Short Range Blaster | 1 Nickel | 1 Modified Consumer Firmware, 1 Nickel | 1 Electrochemical Arrays, 1 Modified Consumer Firmware, 1 Nickel | 1 Conductive Polymers, 1 Electrochemical Arrays, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Configurable Components, 1 Cracked Industrial Firmware | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |

#### Burst Laser

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Efficient Weapon | 1 Sulphur | 1 Heat Dispersion Plate, 1 Sulphur | 1 Chromium, 1 Exceptional Scrambled Emission Data, 1 Heat Exchangers | 1 Heat Vanes, 1 Irregular Emission Data, 1 Selenium | 1 Cadmium, 1 Proto Heat Radiators, 1 Unexpected Emission Data | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Focused Weapon | 1 Iron | 1 Conductive Components, 1 Iron | 1 Chromium, 1 Conductive Ceramics, 1 Iron | 1 Focus Crystals, 1 Germanium, 1 Polymer Capacitors | 1 Military Supercapacitors, 1 Niobium, 1 Refined Focus Crystals | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Long Range Weapon | 1 Sulphur | 1 Modified Consumer Firmware, 1 Sulphur | 1 Focus Crystals, 1 Modified Consumer Firmware, 1 Sulphur | 1 Conductive Polymers, 1 Focus Crystals, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Thermic Alloys, 1 Cracked Industrial Firmware | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Overcharged Weapon | 1 Nickel | 1 Conductive Components, 1 Nickel | 1 Conductive Components, 1 Electrochemical Arrays, 1 Nickel | 1 Conductive Ceramics, 1 Polymer Capacitors, 1 Zinc | 1 Conductive Polymers, 1 Modified Embedded Firmware, 1 Zirconium | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Precipitated Alloys, 1 Configurable Components, 1 Technetium | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Short Range Blaster | 1 Nickel | 1 Modified Consumer Firmware, 1 Nickel | 1 Electrochemical Arrays, 1 Modified Consumer Firmware, 1 Nickel | 1 Conductive Polymers, 1 Electrochemical Arrays, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Configurable Components, 1 Cracked Industrial Firmware | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Broo Tarquin 5, Mel Brandon 5, The Dweller 3 |

#### Cannon

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Efficient Weapon | 1 Sulphur | 1 Heat Dispersion Plate, 1 Sulphur | 1 Chromium, 1 Exceptional Scrambled Emission Data, 1 Heat Exchangers | 1 Heat Vanes, 1 Irregular Emission Data, 1 Selenium | 1 Cadmium, 1 Proto Heat Radiators, 1 Unexpected Emission Data | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |
| High Capacity Magazine | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Vanadium | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | 1 High Density Composites, 1 Mechanical Equipment, 1 Tin | 1 Mechanical Components, 1 Military Supercapacitors, 1 Proprietary Composites | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |
| Long Range Weapon | 1 Sulphur | 1 Modified Consumer Firmware, 1 Sulphur | 1 Focus Crystals, 1 Modified Consumer Firmware, 1 Sulphur | 1 Conductive Polymers, 1 Focus Crystals, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Cracked Industrial Firmware, 1 Thermic Alloys | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |
| Overcharged Weapon | 1 Nickel | 1 Conductive Components, 1 Nickel | 1 Conductive Components, 1 Electrochemical Arrays, 1 Nickel | 1 Conductive Ceramics, 1 Polymer Capacitors, 1 Zinc | 1 Conductive Polymers, 1 Modified Embedded Firmware, 1 Zirconium | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Configurable Components, 1 Precipitated Alloys, 1 Technetium | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |
| Short Range Blaster | 1 Nickel | 1 Modified Consumer Firmware, 1 Nickel | 1 Electrochemical Arrays, 1 Modified Consumer Firmware, 1 Nickel | 1 Conductive Polymers, 1 Electrochemical Arrays, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Configurable Components, 1 Cracked Industrial Firmware | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Marsha Hicks 5, The Sarge 5, Tod McQuinn 2 |

#### Chaff Launcher

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| 100% Refill | - | 1 Compact Composites, 2 Filament Composites, 1 Thermic Alloys | - | - | - | @Synthesis 2 |
| 100% Refill, +2 Seconds Duration | - | - | 1 Compact Composites, 2 Filament Composites, 2 Thermic Alloys, 1 Proto Radiolic Alloys | - | - | @Synthesis 3 |
| 50% Refill | 1 Compact Composites, 1 Filament Composites | - | - | - | - | @Synthesis 1 |
| Ammo Capacity | 1 Mechanical Scrap | - | - | - | - | Petra Olmanova 1, Ram Tah 1 |
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Petra Olmanova 5, Ram Tah 5 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Petra Olmanova 5, Ram Tah 5 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Petra Olmanova 5, Ram Tah 5 |

#### Collector Limpet Controller

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |

#### Electronic Countermeasure

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Petra Olmanova 5, Ram Tah 5 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Petra Olmanova 5, Ram Tah 5 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Petra Olmanova 5, Ram Tah 5 |

#### Fragment Cannon

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Double Shot | 1 Carbon | 1 Carbon, 1 Mechanical Equipment | 1 Carbon, 1 Cracked Industrial Firmware, 1 Mechanical Equipment | 1 Mechanical Components, 1 Security Firmware Patch, 1 Vanadium | 1 High Density Composites, 1 Configurable Components, 1 Modified Embedded Firmware | Marsha Hicks 5, Zacariah Nemo 5, Tod McQuinn 3 |
| Efficient Weapon | 1 Sulphur | 1 Heat Dispersion Plate, 1 Sulphur | 1 Chromium, 1 Exceptional Scrambled Emission Data, 1 Heat Exchangers | 1 Heat Vanes, 1 Irregular Emission Data, 1 Selenium | 1 Cadmium, 1 Proto Heat Radiators, 1 Unexpected Emission Data | Marsha Hicks 5, Zacariah Nemo 5, Tod McQuinn 3 |
| High Capacity Magazine | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Vanadium | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | 1 High Density Composites, 1 Mechanical Equipment, 1 Tin | 1 Mechanical Components, 1 Military Supercapacitors, 1 Proprietary Composites | Marsha Hicks 5, Zacariah Nemo 5, Tod McQuinn 3 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Marsha Hicks 5, Zacariah Nemo 5, Tod McQuinn 3 |
| Overcharged Weapon | 1 Nickel | 1 Conductive Components, 1 Nickel | 1 Conductive Components, 1 Electrochemical Arrays, 1 Nickel | 1 Conductive Ceramics, 1 Polymer Capacitors, 1 Zinc | 1 Conductive Polymers, 1 Modified Embedded Firmware, 1 Zirconium | Marsha Hicks 5, Zacariah Nemo 5, Tod McQuinn 3 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Configurable Components, 1 Precipitated Alloys, 1 Technetium | Marsha Hicks 5, Zacariah Nemo 5, Tod McQuinn 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Marsha Hicks 5, Zacariah Nemo 5, Tod McQuinn 3 |

#### Frame Shift Drive

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Faster FSD Boot Sequence | 1 Grid Resistors | 1 Chromium, 1 Grid Resistors | 1 Grid Resistors, 1 Heat Dispersion Plate, 1 Selenium | 1 Cadmium, 1 Heat Exchangers, 1 Hybrid Capacitors | 1 Electrochemical Arrays, 1 Heat Vanes, 1 Tellurium | Elvira Martuuk 5, Felicity Farseer 5, Mel Brandon 5, Chloe Sedesi 3, Colonel Bris Dekker 3, Professor Palin 3 |
| Increased FSD Range | 1 Atypical Disrupted Wake Echoes | 1 Atypical Disrupted Wake Echoes, 1 Chemical Processors | 1 Chemical Processors, 1 Phosphorus, 1 Strange Wake Solutions | 1 Chemical Distillery, 1 Eccentric Hyperspace Trajectories, 1 Manganese | 1 Arsenic, 1 Chemical Manipulators, 1 Datamined Wake Exceptions | Elvira Martuuk 5, Felicity Farseer 5, Mel Brandon 5, Chloe Sedesi 3, Colonel Bris Dekker 3, Professor Palin 3 |
| Shielded FSD | 1 Nickel | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 Shielding Sensors, 1 Zinc | 1 Compound Shielding, 1 High Density Composites, 1 Vanadium | 1 Imperial Shielding, 1 Proprietary Composites, 1 Tungsten | Elvira Martuuk 5, Felicity Farseer 5, Mel Brandon 5, Chloe Sedesi 3, Colonel Bris Dekker 3, Professor Palin 3 |

#### Frame Shift Drive Interdictor

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Expanded FSD Interdictor Capture Arc | 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Unusual Encrypted Files | 1 Grid Resistors, 1 Mechanical Components, 1 Tagged Encryption Codes | 1 Divergent Scan Data, 1 Mechanical Equipment, 1 Strange Wake Solutions | 1 Classified Scan Fragment, 1 Eccentric Hyperspace Trajectories, 1 Mechanical Components | Mel Brandon 5, Colonel Bris Dekker 4, Tiana Fortune 3, Felicity Farseer 1 |
| Long Range FSD Interdictor | 1 Unusual Encrypted Files | 1 Atypical Disrupted Wake Echoes, 1 Tagged Encryption Codes | 1 Anomalous Bulk Scan Data, 1 Anomalous FSD Telemetry, 1 Open Symmetric Keys | 1 Unidentified Scan Archives, 1 Strange Wake Solutions, 1 Atypical Encryption Archives | 1 Classified Scan Databanks, 1 Eccentric Hyperspace Trajectories, 1 Adaptive Encryptors Capture | Mel Brandon 5, Colonel Bris Dekker 4, Tiana Fortune 3, Felicity Farseer 1 |

#### Fuel Scoop

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Core Dynamics Composites, 1 Compound Shielding, 1 Tungsten | Marsha Hicks 5, Lori Jameson 4, Bill Turner 3 |

#### Fuel Transfer Limpet Controller

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |

#### Hatch Breaker Limpet Controller

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 3 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 3 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 3 |

#### Heat Sink Launcher

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| 100% Refill | 2 Basic Conductors, 2 Heat Conduction Wiring | - | - | - | - | @Synthesis 1 |
| 100% Refill, +15% Heat Dissipation | - | 2 Basic Conductors, 2 Heat Conduction Wiring, 2 Heat Exchangers | - | - | - | @Synthesis 2 |
| 100% Refill, +30% Heat Dissipation | - | - | 2 Basic Conductors, 2 Heat Conduction Wiring, 2 Heat Exchangers, 1 Proto Heat Radiators | - | - | @Synthesis 3 |
| Ammo Capacity | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | - | - | - | - | Petra Olmanova 1, Ram Tah 1 |
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Petra Olmanova 5, Ram Tah 5 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Petra Olmanova 5, Ram Tah 5 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Petra Olmanova 5, Ram Tah 5 |

#### Hull Reinforcement Package

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Blast Resistant Hull Reinforcement | 1 Nickel | 1 Carbon, 1 Zinc | 1 Salvaged Alloys, 1 Vanadium, 1 Zirconium | 1 Galvanising Alloys, 1 Tungsten, 1 Mercury | 1 Phase Alloys, 1 Molybdenum, 1 Ruthenium | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Heavy Duty Hull Reinforcement | 1 Carbon | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Kinetic Resistant Hull Reinforcement | 1 Nickel | 1 Nickel, 1 Vanadium | 1 High Density Composites, 1 Salvaged Alloys, 1 Vanadium | 1 Galvanising Alloys, 1 Tungsten, 1 Proprietary Composites | 1 Phase Alloys, 1 Molybdenum, 1 Core Dynamics Composites | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Lightweight Hull Reinforcement | 1 Iron | 1 Conductive Components, 1 Iron | 1 Conductive Components, 1 High Density Composites, 1 Iron | 1 Conductive Ceramics, 1 Germanium, 1 Proprietary Composites | 1 Conductive Ceramics, 1 Military Grade Alloys, 1 Tin | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |
| Thermal Resistant Hull Reinforcement | 1 Heat Conduction Wiring | 1 Heat Dispersion Plate, 1 Nickel | 1 Heat Exchangers, 1 Salvaged Alloys, 1 Vanadium | 1 Galvanising Alloys, 1 Tungsten, 1 Heat Vanes | 1 Phase Alloys, 1 Molybdenum, 1 Proto Heat Radiators | Petra Olmanova 5, Selene Jean 5, Liz Ryder 1 |

#### Kill Warrant Scanner

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Fast Scanner | 1 Phosphorus | 1 Phosphorus, 1 Flawed Focus Crystals | 1 Phosphorus, 1 Flawed Focus Crystals, 1 Open Symmetric Keys | 1 Manganese, 1 Focus Crystals, 1 Atypical Encryption Archives | 1 Arsenic, 1 Refined Focus Crystals, 1 Adaptive Encryptors Capture | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Long Range Scanner | 1 Iron | 1 Iron, 1 Hybrid Capacitors | 1 Iron, 1 Hybrid Capacitors, 1 Unexpected Emission Data | 1 Germanium, 1 Electrochemical Arrays, 1 Decoded Emission Data | 1 Niobium, 1 Polymer Capacitors, 1 Abnormal Compact Emission Data | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Wide Angle Scanner | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Germanium | 1 Mechanical Scrap, 1 Germanium, 1 Classified Scan Databanks | 1 Mechanical Equipment, 1 Niobium, 1 Divergent Scan Data | 1 Mechanical Components, 1 Tin, 1 Classified Scan Fragment | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |

#### Life Support

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| 100% Refill | 2 Iron, 1 Nickel | - | - | - | - | @Synthesis 1 |
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Radiolic Alloys, 1 Proto Light Alloys | Etienne Dorn 5, Lori Jameson 4, Bill Turner 3 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Etienne Dorn 5, Lori Jameson 5, Bill Turner 3 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Core Dynamics Composites, 1 Compound Shielding, 1 Tungsten | Etienne Dorn 5, Lori Jameson 4, Bill Turner 3 |

#### Manifest Scanner

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Fast Scanner | 1 Phosphorus | 1 Phosphorus, 1 Flawed Focus Crystals | 1 Phosphorus, 1 Flawed Focus Crystals, 1 Open Symmetric Keys | 1 Manganese, 1 Focus Crystals, 1 Atypical Encryption Archives | 1 Arsenic, 1 Refined Focus Crystals, 1 Adaptive Encryptors Capture | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Long Range Scanner | 1 Iron | 1 Iron, 1 Hybrid Capacitors | 1 Iron, 1 Hybrid Capacitors, 1 Unexpected Emission Data | 1 Germanium, 1 Electrochemical Arrays, 1 Decoded Emission Data | 1 Niobium, 1 Polymer Capacitors, 1 Abnormal Compact Emission Data | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Wide Angle Scanner | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Germanium | 1 Mechanical Scrap, 1 Germanium, 1 Classified Scan Databanks | 1 Mechanical Equipment, 1 Niobium, 1 Divergent Scan Data | 1 Mechanical Components, 1 Tin, 1 Classified Scan Fragment | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |

#### Mine Launcher

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| High Capacity Magazine | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Vanadium | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | 1 High Density Composites, 1 Mechanical Equipment, 1 Tin | 1 Mechanical Components, 1 Military Supercapacitors, 1 Proprietary Composites | Juri Ishmaak 5, Petra Olmanova 5, Liz Ryder 3 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Radiolic Alloys, 1 Proto Light Alloys | Juri Ishmaak 5, Petra Olmanova 5, Liz Ryder 3 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Configurable Components, 1 Technetium, 1 Precipitated Alloys | Juri Ishmaak 5, Petra Olmanova 5, Liz Ryder 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Juri Ishmaak 5, Petra Olmanova 5, Liz Ryder 3 |

#### Missile Rack

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| High Capacity Magazine | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Vanadium | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | 1 High Density Composites, 1 Mechanical Equipment, 1 Tin | 1 Mechanical Components, 1 Military Supercapacitors, 1 Proprietary Composites | Liz Ryder 5, Petra Olmanova 5, Juri Ishmaak 3 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Liz Ryder 5, Petra Olmanova 5, Juri Ishmaak 3 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Precipitated Alloys, 1 Configurable Components, 1 Technetium | Liz Ryder 5, Petra Olmanova 5, Juri Ishmaak 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Liz Ryder 5, Petra Olmanova 5, Juri Ishmaak 3 |

#### Multi-cannon

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Efficient Weapon | 1 Sulphur | 1 Heat Dispersion Plate, 1 Sulphur | 1 Chromium, 1 Exceptional Scrambled Emission Data, 1 Heat Exchangers | 1 Heat Vanes, 1 Irregular Emission Data, 1 Selenium | 1 Cadmium, 1 Proto Heat Radiators, 1 Unexpected Emission Data | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |
| High Capacity Magazine | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Vanadium | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | 1 High Density Composites, 1 Mechanical Equipment, 1 Tin | 1 Mechanical Components, 1 Military Supercapacitors, 1 Proprietary Composites | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |
| Long Range Weapon | 1 Sulphur | 1 Modified Consumer Firmware, 1 Sulphur | 1 Focus Crystals, 1 Modified Consumer Firmware, 1 Sulphur | 1 Conductive Polymers, 1 Focus Crystals, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Cracked Industrial Firmware, 1 Thermic Alloys | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |
| Overcharged Weapon | 1 Nickel | 1 Conductive Components, 1 Nickel | 1 Conductive Components, 1 Electrochemical Arrays, 1 Nickel | 1 Conductive Ceramics, 1 Polymer Capacitors, 1 Zinc | 1 Conductive Polymers, 1 Modified Embedded Firmware, 1 Zirconium | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Configurable Components, 1 Precipitated Alloys, 1 Technetium | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |
| Short Range Blaster | 1 Nickel | 1 Modified Consumer Firmware, 1 Nickel | 1 Electrochemical Arrays, 1 Modified Consumer Firmware, 1 Nickel | 1 Conductive Polymers, 1 Electrochemical Arrays, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Configurable Components, 1 Cracked Industrial Firmware | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Marsha Hicks 5, Tod McQuinn 5, Zacariah Nemo 3 |

#### Plasma Accelerator

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Efficient Weapon | 1 Sulphur | 1 Heat Dispersion Plate, 1 Sulphur | 1 Chromium, 1 Exceptional Scrambled Emission Data, 1 Heat Exchangers | 1 Heat Vanes, 1 Irregular Emission Data, 1 Selenium | 1 Cadmium, 1 Proto Heat Radiators, 1 Unexpected Emission Data | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |
| Focused Weapon | 1 Iron | 1 Conductive Components, 1 Iron | 1 Chromium, 1 Conductive Ceramics, 1 Iron | 1 Focus Crystals, 1 Germanium, 1 Polymer Capacitors | 1 Military Supercapacitors, 1 Niobium, 1 Refined Focus Crystals | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |
| Long Range Weapon | 1 Sulphur | 1 Modified Consumer Firmware, 1 Sulphur | 1 Focus Crystals, 1 Modified Consumer Firmware, 1 Sulphur | 1 Conductive Polymers, 1 Focus Crystals, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Cracked Industrial Firmware, 1 Thermic Alloys | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |
| Overcharged Weapon | 1 Nickel | 1 Conductive Components, 1 Nickel | 1 Conductive Components, 1 Electrochemical Arrays, 1 Nickel | 1 Conductive Ceramics, 1 Polymer Capacitors, 1 Zinc | 1 Conductive Polymers, 1 Modified Embedded Firmware, 1 Zirconium | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Configurable Components, 1 Precipitated Alloys, 1 Technetium | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |
| Short Range Blaster | 1 Nickel | 1 Modified Consumer Firmware, 1 Nickel | 1 Electrochemical Arrays, 1 Modified Consumer Firmware, 1 Nickel | 1 Conductive Polymers, 1 Electrochemical Arrays, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Configurable Components, 1 Cracked Industrial Firmware | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Bill Turner 5, Etienne Dorn 5, Zacariah Nemo 2 |

#### Point Defence

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Ammo Capacity | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | - | - | - | - | Petra Olmanova 1, Ram Tah 1 |
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Petra Olmanova 5, Ram Tah 5 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Petra Olmanova 5, Ram Tah 5 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Petra Olmanova 5, Ram Tah 5 |

#### Power Distributor

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Charge Enhanced | 1 Specialised Legacy Firmware | 1 Chemical Processors, 1 Specialised Legacy Firmware | 1 Chemical Distillery, 1 Grid Resistors, 1 Modified Consumer Firmware | 1 Chemical Manipulators, 1 Cracked Industrial Firmware, 1 Hybrid Capacitors | 1 Chemical Manipulators, 1 Cracked Industrial Firmware, 1 Exquisite Focus Crystals | Etienne Dorn 5, The Dweller 5, Hera Tani 3, Marco Qwent 3 |
| Engine Focused | 1 Sulphur | 1 Conductive Components, 1 Sulphur | 1 Anomalous Bulk Scan Data, 1 Chromium, 1 Electrochemical Arrays | 1 Unidentified Scan Archives, 1 Selenium, 1 Polymer Capacitors | 1 Classified Scan Databanks, 1 Cadmium, 1 Military Supercapacitors | Etienne Dorn 5, The Dweller 5, Hera Tani 3, Marco Qwent 3 |
| High Charge Capacity | 1 Sulphur | 1 Chromium, 1 Specialised Legacy Firmware | 1 Chromium, 1 High Density Composites, 1 Specialised Legacy Firmware | 1 Modified Consumer Firmware, 1 Proprietary Composites, 1 Selenium | 1 Cracked Industrial Firmware, 1 Military Supercapacitors, 1 Proprietary Composites | Etienne Dorn 5, The Dweller 5, Hera Tani 3, Marco Qwent 3 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Etienne Dorn 5, The Dweller 5, Hera Tani 3, Marco Qwent 3 |
| System Focused | 1 Sulphur | 1 Conductive Components, 1 Sulphur | 1 Anomalous Bulk Scan Data, 1 Chromium, 1 Electrochemical Arrays | 1 Unidentified Scan Archives, 1 Selenium, 1 Polymer Capacitors | 1 Classified Scan Databanks, 1 Cadmium, 1 Military Supercapacitors | Etienne Dorn 5, The Dweller 5, Hera Tani 3, Marco Qwent 3 |
| Weapon Focused | 1 Sulphur | 1 Conductive Components, 1 Sulphur | 1 Anomalous Bulk Scan Data, 1 Hybrid Capacitors, 1 Selenium | 1 Unidentified Scan Archives, 1 Electrochemical Arrays, 1 Cadmium | 1 Classified Scan Databanks, 1 Polymer Capacitors, 1 Tellurium | Etienne Dorn 5, The Dweller 5, Hera Tani 3, Marco Qwent 3 |

#### Power Plant

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Armoured | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Etienne Dorn 5, Hera Tani 5, Marco Qwent 4, Felicity Farseer 1 |
| Low Emissions | 1 Iron | 1 Iron, 1 Irregular Emission Data | 1 Heat Exchangers, 1 Iron, 1 Irregular Emission Data | 1 Germanium, 1 Unexpected Emission Data, 1 Heat Vanes | 1 Niobium, 1 Decoded Emission Data, 1 Proto Heat Radiators | Etienne Dorn 5, Hera Tani 5, Marco Qwent 4, Felicity Farseer 1 |
| Overcharged | 1 Sulphur | 1 Conductive Components, 1 Heat Conduction Wiring | 1 Conductive Components, 1 Heat Conduction Wiring, 1 Selenium | 1 Cadmium, 1 Conductive Ceramics, 1 Heat Dispersion Plate | 1 Chemical Manipulators, 1 Conductive Ceramics, 1 Tellurium | Etienne Dorn 5, Hera Tani 5, Marco Qwent 4, Felicity Farseer 1 |

#### Prospector Limpet Controller

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Marsha Hicks 5, The Sarge 5, Tiana Fortune 5, Ram Tah 4 |

#### Pulse Laser

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Efficient Weapon | 1 Sulphur | 1 Heat Dispersion Plate, 1 Sulphur | 1 Chromium, 1 Exceptional Scrambled Emission Data, 1 Heat Exchangers | 1 Heat Vanes, 1 Irregular Emission Data, 1 Selenium | 1 Cadmium, 1 Proto Heat Radiators, 1 Unexpected Emission Data | Broo Tarquin 5, Mel Brandon 5, The Dweller 4 |
| Focused Weapon | 1 Iron | 1 Conductive Components, 1 Iron | 1 Chromium, 1 Conductive Ceramics, 1 Iron | 1 Focus Crystals, 1 Germanium, 1 Polymer Capacitors | 1 Military Supercapacitors, 1 Niobium, 1 Refined Focus Crystals | Broo Tarquin 5, Mel Brandon 5, The Dweller 4 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Broo Tarquin 5, Mel Brandon 5, The Dweller 4 |
| Long Range Weapon | 1 Sulphur | 1 Modified Consumer Firmware, 1 Sulphur | 1 Focus Crystals, 1 Modified Consumer Firmware, 1 Sulphur | 1 Conductive Polymers, 1 Focus Crystals, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Cracked Industrial Firmware, 1 Thermic Alloys | Broo Tarquin 5, Mel Brandon 5, The Dweller 4 |
| Overcharged Weapon | 1 Nickel | 1 Conductive Components, 1 Nickel | 1 Conductive Components, 1 Electrochemical Arrays, 1 Nickel | 1 Conductive Ceramics, 1 Polymer Capacitors, 1 Zinc | 1 Conductive Polymers, 1 Modified Embedded Firmware, 1 Zirconium | Broo Tarquin 5, Mel Brandon 5, The Dweller 4 |
| Rapid Fire Modification | 1 Mechanical Scrap | 1 Heat Dispersion Plate, 1 Mechanical Scrap | 1 Mechanical Equipment, 1 Precipitated Alloys, 1 Specialised Legacy Firmware | 1 Mechanical Components, 1 Modified Consumer Firmware, 1 Thermic Alloys | 1 Configurable Components, 1 Precipitated Alloys, 1 Technetium | Broo Tarquin 5, Mel Brandon 5, The Dweller 4 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Broo Tarquin 5, Mel Brandon 5, The Dweller 4 |

#### Rail Gun

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| High Capacity Magazine | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Vanadium | 1 Mechanical Scrap, 1 Niobium, 1 Vanadium | 1 High Density Composites, 1 Mechanical Equipment, 1 Tin | 1 Mechanical Components, 1 Military Supercapacitors, 1 Proprietary Composites | Etienne Dorn 5, Tod McQuinn 5, The Sarge 3 |
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Etienne Dorn 5, Tod McQuinn 5, The Sarge 3 |
| Long Range Weapon | 1 Sulphur | 1 Modified Consumer Firmware, 1 Sulphur | 1 Focus Crystals, 1 Modified Consumer Firmware, 1 Sulphur | 1 Conductive Polymers, 1 Focus Crystals, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Thermic Alloys, 1 Cracked Industrial Firmware | Etienne Dorn 5, Tod McQuinn 5, The Sarge 3 |
| Short Range Blaster | 1 Nickel | 1 Modified Consumer Firmware, 1 Nickel | 1 Electrochemical Arrays, 1 Modified Consumer Firmware, 1 Nickel | 1 Conductive Polymers, 1 Electrochemical Arrays, 1 Modified Consumer Firmware | 1 Biotech Conductors, 1 Configurable Components, 1 Cracked Industrial Firmware | Etienne Dorn 5, Tod McQuinn 5, The Sarge 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Etienne Dorn 5, Tod McQuinn 5, The Sarge 3 |

#### Refinery

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Core Dynamics Composites, 1 Compound Shielding, 1 Tungsten | Marsha Hicks 5, Lori Jameson 4, Bill Turner 3 |

#### Sensors

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Light Weight Scanner | 1 Phosphorus | 1 Salvaged Alloys, 1 Manganese | 1 Salvaged Alloys, 1 Manganese, 1 Conductive Ceramics | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Bill Turner 5, Etienne Dorn 5, Juri Ishmaak 5, Lei Cheung 5, Lori Jameson 5, Tiana Fortune 5, Felicity Farseer 3, Hera Tani 3 |
| Long Range Scanner | 1 Iron | 1 Iron, 1 Hybrid Capacitors | 1 Iron, 1 Hybrid Capacitors, 1 Unexpected Emission Data | 1 Germanium, 1 Electrochemical Arrays, 1 Decoded Emission Data | 1 Niobium, 1 Polymer Capacitors, 1 Abnormal Compact Emission Data | Bill Turner 5, Etienne Dorn 5, Juri Ishmaak 5, Lei Cheung 5, Lori Jameson 5, Tiana Fortune 5, Felicity Farseer 3, Hera Tani 3 |
| Wide Angle Scanner | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Germanium | 1 Germanium, 1 Mechanical Scrap, 1 Classified Scan Databanks | 1 Mechanical Equipment, 1 Niobium, 1 Divergent Scan Data | 1 Mechanical Components, 1 Tin, 1 Classified Scan Fragment | Bill Turner 5, Etienne Dorn 5, Juri Ishmaak 5, Lei Cheung 5, Lori Jameson 5, Tiana Fortune 5, Felicity Farseer 3, Hera Tani 3 |

#### Shield Booster

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Blast Resistant | 1 Iron | 1 Conductive Components, 1 Iron | 1 Conductive Components, 1 Focus Crystals, 1 Iron | 1 Germanium, 1 Untypical Shield Scans, 1 Refined Focus Crystals | 1 Niobium, 1 Aberrant Shield Pattern Analysis, 1 Exquisite Focus Crystals | Didi Vatermann 5, Mel Brandon 5, Lei Cheung 3, Felicity Farseer 1 |
| Heavy Duty | 1 Grid Resistors | 1 Distorted Shield Cycle Recordings, 1 Hybrid Capacitors | 1 Distorted Shield Cycle Recordings, 1 Hybrid Capacitors, 1 Niobium | 1 Electrochemical Arrays, 1 Inconsistent Shield Soak Analysis, 1 Tin | 1 Antimony, 1 Polymer Capacitors, 1 Untypical Shield Scans | Didi Vatermann 5, Mel Brandon 5, Lei Cheung 3, Felicity Farseer 1 |
| Kinetic Resistant | 1 Iron | 1 Germanium, 1 Grid Resistors | 1 Focus Crystals, 1 Hybrid Capacitors, 1 Salvaged Alloys | 1 Galvanising Alloys, 1 Untypical Shield Scans, 1 Refined Focus Crystals | 1 Phase Alloys, 1 Aberrant Shield Pattern Analysis, 1 Exquisite Focus Crystals | Didi Vatermann 5, Mel Brandon 5, Lei Cheung 3, Felicity Farseer 1 |
| Resistance Augmented | 1 Phosphorus | 1 Conductive Components, 1 Phosphorus | 1 Conductive Components, 1 Focus Crystals, 1 Phosphorus | 1 Conductive Ceramics, 1 Manganese, 1 Refined Focus Crystals | 1 Conductive Ceramics, 1 Imperial Shielding, 1 Refined Focus Crystals | Didi Vatermann 5, Mel Brandon 5, Lei Cheung 3, Felicity Farseer 1 |
| Thermal Resistant | 1 Iron | 1 Germanium, 1 Heat Conduction Wiring | 1 Focus Crystals, 1 Heat Conduction Wiring, 1 Heat Dispersion Plate | 1 Heat Dispersion Plate, 1 Untypical Shield Scans, 1 Refined Focus Crystals | 1 Heat Exchangers, 1 Aberrant Shield Pattern Analysis, 1 Exquisite Focus Crystals | Didi Vatermann 5, Mel Brandon 5, Lei Cheung 3, Felicity Farseer 1 |

#### Shield Cell Bank

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Rapid Charge | 1 Sulphur | 1 Chromium, 1 Grid Resistors | 1 Hybrid Capacitors, 1 Precipitated Alloys, 1 Sulphur | 1 Chromium, 1 Electrochemical Arrays, 1 Thermic Alloys | - | Mel Brandon 4, Lori Jameson 3, Elvira Martuuk 1 |
| Specialised | 1 Specialised Legacy Firmware | 1 Conductive Components, 1 Specialised Legacy Firmware | 1 Conductive Components, 1 Cracked Industrial Firmware, 1 Exceptional Scrambled Emission Data | 1 Conductive Components, 1 Cracked Industrial Firmware, 1 Yttrium | - | Mel Brandon 4, Lori Jameson 3, Elvira Martuuk 1 |

#### Shield Generator

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Enhanced, Low Power Shields | 1 Distorted Shield Cycle Recordings | 1 Distorted Shield Cycle Recordings, 1 Germanium | 1 Distorted Shield Cycle Recordings, 1 Germanium, 1 Precipitated Alloys | 1 Inconsistent Shield Soak Analysis, 1 Niobium, 1 Thermic Alloys | 1 Military Grade Alloys, 1 Tin, 1 Untypical Shield Scans | Lei Cheung 5, Mel Brandon 5, Didi Vatermann 3, Elvira Martuuk 3 |
| Kinetic Resistant Shields | 1 Distorted Shield Cycle Recordings | 1 Distorted Shield Cycle Recordings, 1 Modified Consumer Firmware | 1 Distorted Shield Cycle Recordings, 1 Modified Consumer Firmware, 1 Selenium | 1 Focus Crystals, 1 Inconsistent Shield Soak Analysis, 1 Mercury | 1 Refined Focus Crystals, 1 Ruthenium, 1 Untypical Shield Scans | Lei Cheung 5, Mel Brandon 5, Didi Vatermann 3, Elvira Martuuk 3 |
| Reinforced Shields | 1 Phosphorus | 1 Conductive Components, 1 Phosphorus | 1 Conductive Components, 1 Mechanical Components, 1 Phosphorus | 1 Conductive Ceramics, 1 Configurable Components, 1 Manganese | 1 Arsenic, 1 Conductive Polymers, 1 Improvised Components | Lei Cheung 5, Mel Brandon 5, Didi Vatermann 3, Elvira Martuuk 3 |
| Thermal Resistant Shields | 1 Distorted Shield Cycle Recordings | 1 Distorted Shield Cycle Recordings, 1 Germanium | 1 Distorted Shield Cycle Recordings, 1 Germanium, 1 Selenium | 1 Focus Crystals, 1 Inconsistent Shield Soak Analysis, 1 Mercury | 1 Refined Focus Crystals, 1 Ruthenium, 1 Untypical Shield Scans | Lei Cheung 5, Mel Brandon 5, Didi Vatermann 3, Elvira Martuuk 3 |

#### Surface Scanner

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Expanded Probe Scanning Radius | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Germanium | 1 Mechanical Scrap, 1 Germanium, 1 Phase Alloys | 1 Mechanical Equipment, 1 Niobium, 1 Proto Light Alloys | 1 Mechanical Components, 1 Tin, 1 Proto Radiolic Alloys | Bill Turner 5, Etienne Dorn 5, Hera Tani 5, Juri Ishmaak 5, Lei Cheung 5, Lori Jameson 5, Felicity Farseer 3, Tiana Fortune 3 |

#### Thrusters

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Clean Drive Tuning | 1 Sulphur | 1 Conductive Components, 1 Specialised Legacy Firmware | 1 Conductive Components, 1 Specialised Legacy Firmware, 1 Unexpected Emission Data | 1 Conductive Ceramics, 1 Decoded Emission Data, 1 Modified Consumer Firmware | 1 Abnormal Compact Emission Data, 1 Conductive Ceramics, 1 Tin | Chloe Sedesi 5, Mel Brandon 5, Professor Palin 5, Felicity Farseer 3, Elvira Martuuk 2 |
| Dirty Drive Tuning | 1 Specialised Legacy Firmware | 1 Mechanical Equipment, 1 Specialised Legacy Firmware | 1 Chromium, 1 Mechanical Components, 1 Specialised Legacy Firmware | 1 Configurable Components, 1 Modified Consumer Firmware, 1 Selenium | 1 Cadmium, 1 Cracked Industrial Firmware, 1 Pharmaceutical Isolators | Chloe Sedesi 5, Mel Brandon 5, Professor Palin 5, Felicity Farseer 3, Elvira Martuuk 2 |
| Drive Strengthening | 1 Carbon | 1 Heat Conduction Wiring, 1 Vanadium | 1 Heat Conduction Wiring, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Heat Dispersion Plate, 1 High Density Composites | 1 Heat Exchangers, 1 Imperial Shielding, 1 Proprietary Composites | Chloe Sedesi 5, Mel Brandon 5, Professor Palin 5, Felicity Farseer 3, Elvira Martuuk 2 |

#### Torpedo Pylon

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Lightweight Mount | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Liz Ryder 5, Petra Olmanova 5, Juri Ishmaak 3 |
| Sturdy Mount | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Liz Ryder 5, Petra Olmanova 5, Juri Ishmaak 3 |

#### Wake Scanner

| Blueprint | G1 | G2 | G3 | G4 | G5 | Engineers (max grade) |
|---|---|---|---|---|---|---|
| Fast Scanner | 1 Phosphorus | 1 Phosphorus, 1 Flawed Focus Crystals | 1 Phosphorus, 1 Flawed Focus Crystals, 1 Open Symmetric Keys | 1 Manganese, 1 Focus Crystals, 1 Atypical Encryption Archives | 1 Arsenic, 1 Refined Focus Crystals, 1 Adaptive Encryptors Capture | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Lightweight | 1 Phosphorus | 1 Manganese, 1 Salvaged Alloys | 1 Conductive Ceramics, 1 Manganese, 1 Salvaged Alloys | 1 Conductive Components, 1 Phase Alloys, 1 Proto Light Alloys | 1 Conductive Ceramics, 1 Proto Light Alloys, 1 Proto Radiolic Alloys | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Long Range Scanner | 1 Iron | 1 Iron, 1 Hybrid Capacitors | 1 Iron, 1 Hybrid Capacitors, 1 Unexpected Emission Data | 1 Germanium, 1 Electrochemical Arrays, 1 Decoded Emission Data | 1 Niobium, 1 Polymer Capacitors, 1 Abnormal Compact Emission Data | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Reinforced | 1 Nickel | 1 Nickel, 1 Shield Emitters | 1 Nickel, 1 Shield Emitters, 1 Tungsten | 1 Molybdenum, 1 Tungsten, 1 Zinc | 1 High Density Composites, 1 Molybdenum, 1 Technetium | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Shielded | 1 Worn Shield Emitters | 1 Carbon, 1 Shield Emitters | 1 Carbon, 1 High Density Composites, 1 Shield Emitters | 1 Proprietary Composites, 1 Shielding Sensors, 1 Vanadium | 1 Compound Shielding, 1 Core Dynamics Composites, 1 Tungsten | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |
| Wide Angle Scanner | 1 Mechanical Scrap | 1 Mechanical Scrap, 1 Germanium | 1 Mechanical Scrap, 1 Germanium, 1 Classified Scan Databanks | 1 Mechanical Equipment, 1 Niobium, 1 Divergent Scan Data | 1 Mechanical Components, 1 Tin, 1 Classified Scan Fragment | Etienne Dorn 5, Tiana Fortune 5, Bill Turner 3, Lori Jameson 3 |

### A2. Experimental effects (EDEngineer data, cross-checked with EDD 'Special Effects')

| Module type | Experimental | Cost | Engineers (any who offer the module) |
|---|---|---|---|
| Armour | Angled Plating | 5 Compact Composites, 3 High Density Composites, 3 Zirconium | Liz Ryder, Selene Jean, Petra Olmanova |
| Armour | Deep Plating | 5 Compact Composites, 3 Mechanical Equipment, 2 Molybdenum | Liz Ryder, Selene Jean, Petra Olmanova |
| Armour | Layered Plating | 5 Heat Conduction Wiring, 3 High Density Composites, 1 Niobium | Liz Ryder, Selene Jean, Petra Olmanova |
| Armour | Reflective Plating | 5 Compact Composites, 3 Heat Dispersion Plate, 2 Thermic Alloys | Liz Ryder, Selene Jean, Petra Olmanova |
| Beam Laser | Concordant Sequence | 5 Focus Crystals, 3 Modified Embedded Firmware, 1 Zirconium | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Regeneration Sequence | 3 Refined Focus Crystals, 4 Shielding Sensors, 1 Peculiar Shield Frequency Data | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Thermal Conduit | 5 Heat Dispersion Plate, 5 Sulphur, 5 Tempered Alloys | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Thermal Shock | 5 Flawed Focus Crystals, 3 Heat Resistant Ceramics, 3 Conductive Components, 3 Tungsten | Mel Brandon, The Dweller, Broo Tarquin |
| Beam Laser | Thermal Vent | 5 Flawed Focus Crystals, 3 Conductive Polymers, 3 Precipitated Alloys | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Concordant Sequence | 5 Focus Crystals, 3 Modified Embedded Firmware, 1 Zirconium | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Inertial Impact | 5 Flawed Focus Crystals, 5 Distorted Shield Cycle Recordings, 5 Atypical Disrupted Wake Echoes | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Phasing Sequence | 5 Focus Crystals, 3 Aberrant Shield Pattern Analysis, 3 Niobium, 3 Configurable Components | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Scramble Spectrum | 5 Crystal Shards, 3 Untypical Shield Scans, 5 Exceptional Scrambled Emission Data | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Mel Brandon, The Dweller, Broo Tarquin |
| Burst Laser | Thermal Shock | 5 Flawed Focus Crystals, 3 Heat Resistant Ceramics, 3 Conductive Components, 3 Tungsten | Mel Brandon, The Dweller, Broo Tarquin |
| Cannon | Auto Loader | 4 Mechanical Equipment, 3 Mechanical Components, 3 High Density Composites | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Dispersal Field | 5 Conductive Components, 5 Hybrid Capacitors, 5 Irregular Emission Data, 5 Worn Shield Emitters | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Force Shell | 5 Mechanical Scrap, 5 Zinc, 3 Phase Alloys, 3 Heat Conduction Wiring | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | High Yield Shell | 5 Mechanical Scrap, 3 Proto Light Alloys, 3 Chemical Manipulators, 5 Nickel | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Smart Rounds | 5 Mechanical Scrap, 3 Security Firmware Patch, 3 Decoded Emission Data, 3 Classified Scan Databanks | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Marsha Hicks, Tod McQuinn, The Sarge |
| Cannon | Thermal Cascade | 5 Heat Conduction Wiring, 4 Hybrid Capacitors, 3 High Density Composites, 5 Phosphorus | Marsha Hicks, Tod McQuinn, The Sarge |
| Fragment Cannon | Corrosive Shell | 5 Chemical Storage Units, 4 Precipitated Alloys, 3 Arsenic | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Dazzle Shell | 5 Mechanical Scrap, 4 Manganese, 5 Hybrid Capacitors | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Drag Munition | 5 Carbon, 5 Grid Resistors, 2 Molybdenum | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Incendiary Rounds | 5 Heat Conduction Wiring, 5 Phosphorus, 5 Sulphur, 3 Phase Alloys | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Screening Shell | 5 Mechanical Scrap, 5 Distorted Shield Cycle Recordings, 5 Modified Consumer Firmware, 3 Niobium | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Fragment Cannon | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Frame Shift Drive | Deep Charge | 5 Atypical Disrupted Wake Echoes, 3 Galvanising Alloys, 1 Eccentric Hyperspace Trajectories | Mel Brandon, Felicity Farseer, Elvira Martuuk, Chloe Sedesi, Professor Palin, Colonel Bris Dekker |
| Frame Shift Drive | Double Braced | 5 Atypical Disrupted Wake Echoes, 3 Galvanising Alloys, 1 Configurable Components | Mel Brandon, Felicity Farseer, Elvira Martuuk, Chloe Sedesi, Professor Palin, Colonel Bris Dekker |
| Frame Shift Drive | Mass Manager | 5 Atypical Disrupted Wake Echoes, 3 Galvanising Alloys, 1 Eccentric Hyperspace Trajectories | Mel Brandon, Felicity Farseer, Elvira Martuuk, Chloe Sedesi, Professor Palin, Colonel Bris Dekker |
| Frame Shift Drive | Stripped Down | 5 Atypical Disrupted Wake Echoes, 3 Galvanising Alloys, 1 Proto Light Alloys | Mel Brandon, Felicity Farseer, Elvira Martuuk, Chloe Sedesi, Professor Palin, Colonel Bris Dekker |
| Frame Shift Drive | Thermal Spread | 5 Atypical Disrupted Wake Echoes, 3 Galvanising Alloys, 1 Heat Vanes, 3 Grid Resistors | Mel Brandon, Felicity Farseer, Elvira Martuuk, Chloe Sedesi, Professor Palin, Colonel Bris Dekker |
| Heat Sink Launcher | Modified Heat Sink Launcher | 8 Mechanical Scrap, 6 Niobium, 6 Vanadium, 4 Mechanical Components | @Technology |
| Hull Reinforcement Package | Angled Plating | 5 Tempered Alloys, 3 Zirconium, 5 Carbon, 3 High Density Composites | Liz Ryder, Petra Olmanova, Selene Jean |
| Hull Reinforcement Package | Deep Plating | 5 Compact Composites, 3 Molybdenum, 2 Ruthenium | Liz Ryder, Petra Olmanova, Selene Jean |
| Hull Reinforcement Package | Layered Plating | 5 Heat Conduction Wiring, 3 Shielding Sensors, 3 Tungsten | Liz Ryder, Petra Olmanova, Selene Jean |
| Hull Reinforcement Package | Reflective Plating | 5 Heat Conduction Wiring, 3 Heat Dispersion Plate, 1 Proto Light Alloys, 4 Zinc | Liz Ryder, Petra Olmanova, Selene Jean |
| Mine Launcher | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Emissive Munitions | 4 Mechanical Equipment, 3 Unexpected Emission Data, 3 Heat Exchangers, 3 Manganese | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Ion Disruptor | 5 Sulphur, 5 Phosphorus, 3 Chemical Distillery, 3 Electrochemical Arrays | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Overload Munitions | 5 Filament Composites, 4 Tagged Encryption Codes, 2 Aberrant Shield Pattern Analysis, 3 Germanium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Radiant Canister | 1 Polonium, 3 Phase Alloys, 4 Heat Dispersion Plate | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Reverberating Cascade | 2 Configurable Components, 3 Classified Scan Databanks, 4 Filament Composites, 4 Chromium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Shift-Lock Canister | 5 Tempered Alloys, 3 Strange Wake Solutions, 5 Salvaged Alloys | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Mine Launcher | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Drag Munition (Seeker only) | 5 Carbon, 5 Grid Resistors, 2 Molybdenum | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Emissive Munitions | 4 Mechanical Equipment, 3 Unexpected Emission Data, 3 Heat Exchangers, 3 Manganese | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | FSD Interrupt (Dumbfire only) | 3 Strange Wake Solutions, 5 Anomalous FSD Telemetry, 5 Mechanical Equipment, 3 Configurable Components | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Overload Munitions | 5 Filament Composites, 4 Tagged Encryption Codes, 2 Aberrant Shield Pattern Analysis, 3 Germanium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Penetrator Munitions (Dumbfire only) | 5 Galvanising Alloys, 3 Electrochemical Arrays, 3 Zirconium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Missile Rack | Thermal Cascade | 5 Heat Conduction Wiring, 4 Hybrid Capacitors, 3 High Density Composites, 5 Phosphorus | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Multi-cannon | Auto Loader | 4 Mechanical Equipment, 3 Mechanical Components, 3 High Density Composites | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Corrosive Shell | 5 Chemical Storage Units, 4 Precipitated Alloys, 3 Arsenic | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Emissive Munitions | 4 Mechanical Equipment, 3 Unexpected Emission Data, 3 Heat Exchangers, 3 Manganese | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Incendiary Rounds | 5 Heat Conduction Wiring, 5 Phosphorus, 5 Sulphur, 3 Phase Alloys | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Smart Rounds | 5 Mechanical Scrap, 3 Security Firmware Patch, 3 Decoded Emission Data, 3 Classified Scan Databanks | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Multi-cannon | Thermal Shock | 5 Flawed Focus Crystals, 3 Heat Resistant Ceramics, 3 Conductive Components, 3 Tungsten | Marsha Hicks, Tod McQuinn, Zacariah Nemo |
| Plasma Accelerator | Dazzle Shell | 5 Mechanical Scrap, 4 Manganese, 5 Hybrid Capacitors, 5 Mechanical Components | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Dispersal Field | 5 Conductive Components, 5 Hybrid Capacitors, 5 Irregular Emission Data, 5 Worn Shield Emitters | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Phasing Sequence | 5 Focus Crystals, 3 Aberrant Shield Pattern Analysis, 3 Niobium, 3 Configurable Components | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Plasma Slug | 3 Heat Exchangers, 2 Modified Embedded Firmware, 2 Refined Focus Crystals, 4 Mercury | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Target Lock Breaker | 5 Selenium, 3 Security Firmware Patch, 1 Adaptive Encryptors Capture | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Plasma Accelerator | Thermal Conduit | 5 Heat Dispersion Plate, 5 Sulphur, 5 Tempered Alloys | Etienne Dorn, Zacariah Nemo, Bill Turner |
| Power Distributor | Cluster Capacitor | 5 Phosphorus, 3 Heat Resistant Ceramics, 1 Cadmium | Etienne Dorn, The Dweller, Marco Qwent, Hera Tani |
| Power Distributor | Double Braced | 5 Phosphorus, 3 Heat Resistant Ceramics, 1 Proprietary Composites | Etienne Dorn, The Dweller, Marco Qwent, Hera Tani |
| Power Distributor | Flow Control | 5 Phosphorus, 3 Heat Resistant Ceramics, 1 Conductive Polymers | Etienne Dorn, The Dweller, Marco Qwent, Hera Tani |
| Power Distributor | Stripped Down | 5 Phosphorus, 3 Heat Resistant Ceramics, 1 Proto Light Alloys | Etienne Dorn, The Dweller, Marco Qwent, Hera Tani |
| Power Distributor | Super Conduits | 5 Phosphorus, 3 Heat Resistant Ceramics, 1 Security Firmware Patch | Etienne Dorn, The Dweller, Marco Qwent, Hera Tani |
| Power Plant | Double Braced | 5 Grid Resistors, 3 Vanadium, 1 Proprietary Composites | Etienne Dorn, Felicity Farseer, Marco Qwent, Hera Tani |
| Power Plant | Monstered | 5 Grid Resistors, 3 Vanadium, 1 Polymer Capacitors | Etienne Dorn, Felicity Farseer, Marco Qwent, Hera Tani |
| Power Plant | Stripped Down | 5 Grid Resistors, 3 Vanadium, 1 Proto Light Alloys | Etienne Dorn, Felicity Farseer, Marco Qwent, Hera Tani |
| Power Plant | Thermal Spread | 5 Grid Resistors, 3 Vanadium, 1 Heat Vanes | Etienne Dorn, Felicity Farseer, Marco Qwent, Hera Tani |
| Pulse Laser | Concordant Sequence | 5 Focus Crystals, 3 Modified Embedded Firmware, 1 Zirconium | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Emissive Munitions | 4 Mechanical Equipment, 3 Unexpected Emission Data, 3 Heat Exchangers, 3 Manganese | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Phasing Sequence | 5 Focus Crystals, 3 Aberrant Shield Pattern Analysis, 3 Niobium, 3 Configurable Components | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Scramble Spectrum | 5 Crystal Shards, 3 Untypical Shield Scans, 5 Exceptional Scrambled Emission Data | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Mel Brandon, The Dweller, Broo Tarquin |
| Pulse Laser | Thermal Shock | 5 Flawed Focus Crystals, 3 Heat Resistant Ceramics, 3 Conductive Components, 3 Tungsten | Mel Brandon, The Dweller, Broo Tarquin |
| Rail Gun | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Etienne Dorn, Tod McQuinn, The Sarge |
| Rail Gun | Feedback Cascade | 5 Open Symmetric Keys, 5 Shield Emitters, 5 Filament Composites | Etienne Dorn, Tod McQuinn, The Sarge |
| Rail Gun | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Etienne Dorn, Tod McQuinn, The Sarge |
| Rail Gun | Multi-Servos | 5 Mechanical Scrap, 4 Focus Crystals, 2 Conductive Polymers, 2 Configurable Components | Etienne Dorn, Tod McQuinn, The Sarge |
| Rail Gun | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Etienne Dorn, Tod McQuinn, The Sarge |
| Rail Gun | Plasma Slug | 3 Heat Exchangers, 2 Modified Embedded Firmware, 2 Refined Focus Crystals, 4 Mercury | Etienne Dorn, Tod McQuinn, The Sarge |
| Rail Gun | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Etienne Dorn, Tod McQuinn, The Sarge |
| Rail Gun | Super Penetrator | 3 Proto Light Alloys, 3 Refined Focus Crystals, 3 Zirconium, 5 Untypical Shield Scans | Etienne Dorn, Tod McQuinn, The Sarge |
| Shield Booster | Blast Block | 5 Inconsistent Shield Soak Analysis, 3 Heat Resistant Ceramics, 3 Heat Dispersion Plate, 2 Selenium | Mel Brandon, Felicity Farseer, Lei Cheung, Didi Vatermann |
| Shield Booster | Double Braced | 5 Distorted Shield Cycle Recordings, 3 Galvanising Alloys, 3 Shield Emitters | Mel Brandon, Felicity Farseer, Lei Cheung, Didi Vatermann |
| Shield Booster | Flow Control | 5 Inconsistent Shield Soak Analysis, 3 Security Firmware Patch, 3 Focus Crystals, 3 Niobium | Mel Brandon, Felicity Farseer, Lei Cheung, Didi Vatermann |
| Shield Booster | Force Block | 5 Unidentified Scan Archives, 3 Shielding Sensors, 2 Aberrant Shield Pattern Analysis | Mel Brandon, Felicity Farseer, Lei Cheung, Didi Vatermann |
| Shield Booster | Super Capacitor | 3 Untypical Shield Scans, 5 Compact Composites, 2 Cadmium | Mel Brandon, Felicity Farseer, Lei Cheung, Didi Vatermann |
| Shield Booster | Thermo Block | 5 Anomalous Bulk Scan Data, 3 Conductive Ceramics, 3 Heat Vanes | Mel Brandon, Felicity Farseer, Lei Cheung, Didi Vatermann |
| Shield Cell Bank | Boss Cells | 5 Chemical Storage Units, 3 Chromium, 1 Polymer Capacitors | Mel Brandon, Elvira Martuuk, Lori Jameson |
| Shield Cell Bank | Double Braced | 5 Chemical Storage Units, 3 Chromium, 1 Yttrium | Mel Brandon, Elvira Martuuk, Lori Jameson |
| Shield Cell Bank | Flow Control | 5 Chemical Storage Units, 3 Chromium, 1 Conductive Polymers | Mel Brandon, Elvira Martuuk, Lori Jameson |
| Shield Cell Bank | Recycling Cells | 5 Chemical Storage Units, 3 Chromium, 1 Configurable Components | Mel Brandon, Elvira Martuuk, Lori Jameson |
| Shield Cell Bank | Stripped Down | 5 Chemical Storage Units, 3 Chromium, 1 Proto Light Alloys | Mel Brandon, Elvira Martuuk, Lori Jameson |
| Shield Generator | Double Braced | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Configurable Components | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Shield Generator | Fast Charge | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Compound Shielding | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Shield Generator | Force Block | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Decoded Emission Data | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Shield Generator | Hi-cap | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Conductive Polymers | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Shield Generator | Lo-draw | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Conductive Polymers | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Shield Generator | Multi-weave | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Aberrant Shield Pattern Analysis | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Shield Generator | Stripped Down | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Proto Light Alloys | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Shield Generator | Thermo Block | 5 Worn Shield Emitters, 3 Flawed Focus Crystals, 1 Heat Vanes | Mel Brandon, Elvira Martuuk, Lei Cheung, Didi Vatermann |
| Thrusters | Double Braced | 5 Iron, 3 Hybrid Capacitors, 1 Proprietary Composites | Mel Brandon, Felicity Farseer, Elvira Martuuk, Professor Palin, Chloe Sedesi |
| Thrusters | Drag Drives | 5 Iron, 3 Hybrid Capacitors, 1 Security Firmware Patch | Mel Brandon, Felicity Farseer, Elvira Martuuk, Professor Palin, Chloe Sedesi |
| Thrusters | Drive Distributors | 5 Iron, 3 Hybrid Capacitors, 1 Security Firmware Patch | Mel Brandon, Felicity Farseer, Elvira Martuuk, Professor Palin, Chloe Sedesi |
| Thrusters | Stripped Down | 5 Iron, 3 Hybrid Capacitors, 1 Proto Light Alloys | Mel Brandon, Felicity Farseer, Elvira Martuuk, Professor Palin, Chloe Sedesi |
| Thrusters | Thermal Spread | 5 Iron, 3 Hybrid Capacitors, 1 Heat Vanes | Mel Brandon, Felicity Farseer, Elvira Martuuk, Professor Palin, Chloe Sedesi |
| Torpedo Pylon | Double Braced | 5 Mechanical Scrap, 5 Compact Composites, 3 Vanadium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Torpedo Pylon | Flow Control | 5 Mechanical Scrap, 3 Hybrid Capacitors, 1 Modified Embedded Firmware | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Torpedo Pylon | Mass Lock Munition | 5 Mechanical Equipment, 3 High Density Composites, 3 Aberrant Shield Pattern Analysis | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Torpedo Pylon | Oversized | 5 Mechanical Scrap, 3 Mechanical Components, 1 Ruthenium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Torpedo Pylon | Penetrator Payload | 3 Mechanical Components, 3 Tungsten, 5 Anomalous Bulk Scan Data, 3 Selenium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Torpedo Pylon | Reverberating Cascade | 2 Configurable Components, 3 Classified Scan Databanks, 4 Filament Composites, 4 Chromium | Liz Ryder, Petra Olmanova, Juri Ishmaak |
| Torpedo Pylon | Stripped Down | 5 Salvaged Alloys, 5 Carbon, 1 Tin | Liz Ryder, Petra Olmanova, Juri Ishmaak |

### A3. Synthesis recipes (EDEngineer data)

| Synthesis | Variant (EDEngineer names the variant by its effect; "None" = Basic) | Ingredients |
|---|---|---|
| AFM Refill | None | 2 Nickel, 2 Zinc, 2 Chromium, 3 Vanadium |
| AFM Refill | +50% Repair Speed | 1 Tin, 2 Manganese, 6 Vanadium, 1 Molybdenum, 1 Zirconium |
| AFM Refill | +100% Repair Speed | 2 Zinc, 4 Chromium, 6 Vanadium, 2 Zirconium, 1 Tellurium, 1 Ruthenium |
| AX Explosive Munitions | None | 4 Carbon, 3 Iron, 3 Nickel, 3 Propulsion Elements |
| AX Explosive Munitions | +5% Damage | 6 Sulphur, 6 Phosphorus, 2 Mercury, 4 Propulsion Elements, 4 Thargoid Organic Circuitry |
| AX Explosive Munitions | +10% Damage Boost | 5 Tungsten, 4 Mercury, 5 Bio-Mechanical Conduits, 2 Polonium, 5 Propulsion Elements, 6 Ship Flight Data |
| AX Remote Flak Munitions | None | 4 Nickel, 3 Carbon, 2 Sulphur |
| AX Remote Flak Munitions | +5% Damage Boost | 2 Tin, 3 Zinc, 1 Arsenic, 3 Thargoid Technological Components, 2 Wreckage Components |
| AX Remote Flak Munitions | +10% Damage Boost | 8 Zinc, 2 Tungsten, 1 Arsenic, 3 Sensor Fragment, 4 Thargoid Technological Components, 1 Wreckage Components |
| AX Small Calibre Munitions | None | 2 Iron, 1 Nickel, 2 Sulphur, 2 Weapon Parts |
| AX Small Calibre Munitions | +5% Damage Boost | 2 Iron, 2 Phosphorus, 2 Zirconium, 3 Sensor Fragment, 4 Weapon Parts |
| AX Small Calibre Munitions | +10% Damage Boost | 3 Iron, 2 Phosphorus, 2 Zirconium, 4 Sensor Fragment, 2 Thargoid Carapace, 6 Weapon Parts |
| Enzyme Missile Launcher Munitions | None | 3 Iron, 3 Sulphur, 4 Bio-Mechanical Conduits, 3 Propulsion Elements, 3 Weapon Parts, 2 Lead |
| Enzyme Missile Launcher Munitions | +15% Damage Boost | 6 Sulphur, 4 Tungsten, 5 Bio-Mechanical Conduits, 6 Propulsion Elements, 4 Weapon Parts, 4 Lead |
| Enzyme Missile Launcher Munitions | +30% Damage Boost | 5 Phosphorus, 4 Tungsten, 6 Bio-Mechanical Conduits, 6 Propulsion Elements, 5 Weapon Parts, 6 Lead |
| Explosive Munitions | None | 3 Iron, 3 Nickel, 4 Carbon, 4 Sulphur |
| Explosive Munitions | +15% Damage Boost | 6 Sulphur, 6 Phosphorus, 4 Arsenic, 2 Mercury |
| Explosive Munitions | +30% Damage Boost | 5 Phosphorus, 4 Arsenic, 5 Niobium, 5 Mercury, 5 Polonium |
| FSD Injection | +25% Jump Range | 1 Carbon, 1 Vanadium, 1 Germanium |
| FSD Injection | +50% Jump Range | 1 Carbon, 1 Vanadium, 1 Germanium, 1 Cadmium, 1 Niobium |
| FSD Injection | +100% Jump Range | 1 Carbon, 1 Germanium, 1 Arsenic, 1 Niobium, 1 Yttrium, 1 Polonium |
| Flechette Launcher Munitions | None | 1 Tungsten, 3 Electrochemical Arrays, 2 Mechanical Components, 2 Boron |
| Flechette Launcher Munitions | +5% Damage Boost | 4 Tungsten, 6 Electrochemical Arrays, 4 Mechanical Components, 4 Boron |
| Flechette Launcher Munitions | +10% Damage Boost | 6 Tungsten, 9 Electrochemical Arrays, 5 Mechanical Components, 6 Boron |
| Guardian Gauss Cannon Munitions | None | 3 Manganese, 2 Focus Crystals, 3 Guardian Power Conduit, 4 Guardian Wreckage Components |
| Guardian Gauss Cannon Munitions | +15% Damage Boost | 5 Manganese, 3 Heat Resistant Ceramics, 5 Focus Crystals, 4 Guardian Power Conduit, 3 Guardian Sentinel Weapon Parts |
| Guardian Gauss Cannon Munitions | +30% Damage Boost | 8 Manganese, 6 Filament Composites, 10 Focus Crystals, 6 Guardian Technology Component |
| Guardian Plasma Charger Munitions | None | 3 Chromium, 2 Heat Dispersion Plate, 3 Guardian Power Conduit, 4 Guardian Wreckage Components |
| Guardian Plasma Charger Munitions | +15% Damage Boost | 4 Chromium, 2 Heat Exchangers, 2 Phase Alloys, 2 Guardian Power Cell, 2 Guardian Technology Component |
| Guardian Plasma Charger Munitions | +30% Damage Boost | 6 Chromium, 2 Zirconium, 4 Heat Exchangers, 2 Phase Alloys, 2 Guardian Power Cell, 3 Guardian Sentinel Weapon Parts |
| Guardian Shard Cannon Munitions | None | 3 Carbon, 2 Vanadium, 3 Crystal Shards, 3 Guardian Power Cell, 5 Guardian Wreckage Components |
| Guardian Shard Cannon Munitions | +15% Damage Boost | 4 Crystal Shards, 2 Guardian Power Cell, 2 Guardian Sentinel Weapon Parts |
| Guardian Shard Cannon Munitions | +30% Damage Boost | 8 Carbon, 6 Guardian Power Cell, 4 Vanadium, 8 Crystal Shards |
| High Velocity Munitions | None | 2 Iron, 1 Vanadium |
| High Velocity Munitions | +15% Damage Boost | 4 Iron, 3 Vanadium, 2 Tungsten, 2 Zirconium |
| High Velocity Munitions | +30% Damage Boost | 4 Vanadium, 4 Tungsten, 2 Zirconium, 2 Yttrium |
| Large Calibre Munitions | None | 4 Nickel, 3 Carbon, 2 Sulphur |
| Large Calibre Munitions | +15% Damage Boost | 2 Tin, 3 Phosphorus, 3 Zinc, 1 Arsenic, 2 Zirconium |
| Large Calibre Munitions | +30% Damage Boost | 8 Zinc, 2 Tungsten, 1 Arsenic, 1 Mercury, 2 Antimony |
| Limpets | 4 Limpets, Cargo Hold Required | 10 Iron, 10 Nickel |
| Plasma Munitions | None | 3 Sulphur, 4 Phosphorus, 1 Manganese |
| Plasma Munitions | +15% Damage Boost | 5 Phosphorus, 3 Manganese, 1 Selenium, 4 Molybdenum |
| Plasma Munitions | +30% Damage Boost | 5 Selenium, 4 Cadmium, 4 Molybdenum, 2 Technetium |
| SRV Ammo Restock | None | 2 Sulphur, 1 Phosphorus |
| SRV Ammo Restock | +15% Damage Boost | 1 Phosphorus, 1 Manganese, 1 Selenium, 1 Molybdenum |
| SRV Ammo Restock | +30% Damage Boost | 2 Phosphorus, 2 Selenium, 1 Molybdenum, 1 Technetium |
| SRV Refuel | None | 1 Sulphur, 1 Phosphorus |
| SRV Refuel | +100% Fuel Efficiency | 1 Sulphur, 1 Phosphorus, 1 Arsenic, 1 Mercury |
| SRV Refuel | +200% Fuel Efficiency | 1 Sulphur, 1 Arsenic, 1 Mercury, 1 Technetium |
| SRV Repair | None | 2 Iron, 1 Nickel |
| SRV Repair | +50% Hull Strength | 3 Nickel, 1 Manganese, 2 Vanadium, 1 Molybdenum |
| SRV Repair | +100% Hull Strength | 1 Zinc, 2 Chromium, 2 Vanadium, 1 Tungsten, 1 Tellurium |
| Shock Cannon Munitions | None | 3 Grid Resistors, 2 Heat Dispersion Plate, 2 Focus Crystals, 2 Phase Alloys, 2 Lead |
| Shock Cannon Munitions | +5% Damage Boost | 5 Grid Resistors, 3 Heat Dispersion Plate, 4 Focus Crystals, 5 Phase Alloys, 3 Lead |
| Shock Cannon Munitions | +10% Damage Boost | 7 Grid Resistors, 4 Heat Dispersion Plate, 6 Focus Crystals, 8 Phase Alloys, 5 Lead |
| Small Calibre Munitions | None | 2 Iron, 1 Nickel, 2 Sulphur |
| Small Calibre Munitions | +15% Damage Boost | 2 Iron, 2 Zinc, 2 Phosphorus, 2 Selenium, 2 Zirconium |
| Small Calibre Munitions | +30% Damage Boost | 2 Sulphur, 2 Phosphorus, 2 Tungsten, 2 Zirconium, 2 Mercury, 1 Antimony |

### A4. Technology broker unlocks (EDEngineer data; Guardian = Guardian broker, Human = human broker)

| Broker | Item | Cost |
|---|---|---|
| Guardian | Guardian FSD Booster | 1 Guardian Module Blueprint Segment, 21 Guardian Power Cell, 21 Guardian Technology Component, 24 Focus Crystals, 8 HN Shock Mount |
| Guardian | Guardian Gauss Cannon (Fixed, Medium) | 1 Guardian Weapon Blueprint Segment, 18 Guardian Power Cell, 20 Guardian Technology Component, 15 Manganese, 6 Magnetic Emitter Coil |
| Guardian | Guardian Gauss Cannon (Fixed, Small) | 1 Guardian Weapon Blueprint Segment, 12 Guardian Power Conduit, 12 Guardian Wreckage Components, 15 Guardian Sentinel Weapon Parts |
| Guardian | Guardian Hull Reinforcement | 1 Guardian Module Blueprint Segment, 21 Guardian Wreckage Components, 16 Pattern Beta Obelisk Data, 16 Pattern Gamma Obelisk Data, 12 Reinforced Mounting Plate |
| Guardian | Guardian Hybrid Power Plant | 1 Guardian Module Blueprint Segment, 18 Guardian Power Conduit, 21 Pattern Epsilon Obelisk Data, 15 Heat Resistant Ceramics, 10 Energy Grid Assembly |
| Guardian | Guardian Module Reinforcement | 1 Guardian Module Blueprint Segment, 18 Guardian Wreckage Components, 15 Pattern Epsilon Obelisk Data, 20 Guardian Power Conduit, 9 Reinforced Mounting Plate |
| Guardian | Guardian Plasma Charger (Fixed, Large) | 1 Guardian Weapon Blueprint Segment, 28 Guardian Power Conduit, 20 Guardian Sentinel Weapon Parts, 28 Chromium, 10 Micro-Weave Cooling Hoses |
| Guardian | Guardian Plasma Charger (Fixed, Medium) | 1 Guardian Weapon Blueprint Segment, 19 Guardian Power Conduit, 16 Guardian Sentinel Weapon Parts, 14 Chromium, 8 Micro-Weave Cooling Hoses |
| Guardian | Guardian Plasma Charger (Fixed, Small) | 1 Guardian Weapon Blueprint Segment, 12 Guardian Power Cell, 12 Guardian Sentinel Weapon Parts, 15 Guardian Technology Component |
| Guardian | Guardian Plasma Charger (Turreted, Large) | 2 Guardian Weapon Blueprint Segment, 26 Guardian Power Conduit, 24 Guardian Sentinel Weapon Parts, 26 Chromium, 10 Articulation Motors |
| Guardian | Guardian Plasma Charger (Turreted, Medium) | 2 Guardian Weapon Blueprint Segment, 21 Guardian Power Conduit, 20 Guardian Sentinel Weapon Parts, 16 Chromium, 8 Articulation Motors |
| Guardian | Guardian Plasma Charger (Turreted, Small) | 1 Guardian Weapon Blueprint Segment, 12 Guardian Power Cell, 12 Guardian Technology Component, 15 Guardian Sentinel Weapon Parts |
| Guardian | Guardian Power Distributor | 1 Guardian Module Blueprint Segment, 20 Pattern Alpha Obelisk Data, 24 Guardian Power Cell, 18 Phase Alloys, 6 Heatsink Interlink |
| Guardian | Guardian Shard Cannon (Fixed, Large) | 28 Guardian Technology Component, 1 Guardian Weapon Blueprint Segment, 20 Guardian Wreckage Components, 20 Carbon, 18 Micro Controllers |
| Guardian | Guardian Shard Cannon (Fixed, Medium) | 18 Guardian Technology Component, 1 Guardian Weapon Blueprint Segment, 20 Guardian Wreckage Components, 14 Carbon, 12 Power Transfer Bus |
| Guardian | Guardian Shard Cannon (Fixed, Small) | 1 Guardian Weapon Blueprint Segment, 12 Guardian Power Conduit, 12 Guardian Technology Component, 15 Guardian Sentinel Weapon Parts |
| Guardian | Guardian Shard Cannon (Turreted, Large) | 2 Guardian Weapon Blueprint Segment, 20 Guardian Wreckage Components, 26 Guardian Technology Component, 28 Carbon, 12 Micro Controllers |
| Guardian | Guardian Shard Cannon (Turreted, Medium) | 2 Guardian Weapon Blueprint Segment, 16 Guardian Wreckage Components, 20 Guardian Technology Component, 15 Carbon, 12 Micro Controllers |
| Guardian | Guardian Shard Cannon (Turreted, Small) | 1 Guardian Weapon Blueprint Segment, 12 Guardian Power Conduit, 12 Guardian Sentinel Weapon Parts, 15 Guardian Technology Component |
| Guardian | Guardian Shield Reinforcement | 1 Guardian Module Blueprint Segment, 17 Guardian Power Cell, 20 Guardian Technology Component, 24 Pattern Delta Obelisk Data, 8 Hardware Diagnostic Sensor |
| Guardian | Javelin (Fighter) | 25 Guardian Power Cell, 18 Guardian Wreckage Components, 1 Guardian Vessel Blueprint Segment, 26 Pattern Epsilon Obelisk Data, 25 Guardian Technology Component |
| Guardian | Lance (Fighter) | 25 Guardian Power Cell, 18 Guardian Sentinel Weapon Parts, 1 Guardian Vessel Blueprint Segment, 26 Pattern Epsilon Obelisk Data, 25 Guardian Technology Component |
| Guardian | Modified Gauss Cannon (Fixed, Medium) | 1 Guardian Weapon Blueprint Segment, 5 Guardian Power Cell, 5 Guardian Technology Component, 4 Niobium, 2 Thermal Cooling Units |
| Guardian | Modified Gauss Cannon (Fixed, Small) | 1 Guardian Weapon Blueprint Segment, 3 Guardian Power Conduit, 3 Guardian Wreckage Components, 4 Guardian Sentinel Weapon Parts, 2 Niobium |
| Guardian | Modified Plasma Charger (Fixed, Medium) | 5 Guardian Power Conduit, 1 Guardian Weapon Blueprint Segment, 5 Guardian Sentinel Weapon Parts, 4 Arsenic, 2 Power Transfer Bus |
| Guardian | Modified Plasma Charger (Fixed, Small) | 1 Guardian Weapon Blueprint Segment, 3 Guardian Power Cell, 4 Guardian Technology Component, 3 Guardian Sentinel Weapon Parts, 2 Arsenic |
| Guardian | Modified Shard Cannon (Fixed, Medium) | 5 Guardian Technology Component, 1 Guardian Weapon Blueprint Segment, 5 Guardian Wreckage Components, 4 Germanium, 2 Power Converter |
| Guardian | Modified Shard Cannon (Fixed, Small) | 1 Guardian Weapon Blueprint Segment, 3 Guardian Power Conduit, 3 Guardian Wreckage Components, 4 Guardian Sentinel Weapon Parts, 2 Germanium |
| Guardian | Trident (Fighter) | 25 Guardian Power Cell, 1 Guardian Vessel Blueprint Segment, 26 Pattern Epsilon Obelisk Data, 18 Pattern Beta Obelisk Data, 25 Guardian Technology Component |
| Human | Corrosion Resistant Cargo Rack (2D and 4D) | 16 Meta-alloys, 26 Iron, 18 Chemical Manipulators, 12 Neofabric Insulation, 22 Radiation Baffle |
| Human | Engineered Detailed Surface Scanner V1 | 26 Mechanical Scrap, 22 Germanium, 28 Mechanical Components, 24 Niobium |
| Human | Engineered FSD V1 | 26 Tellurium, 26 Electrochemical Arrays, 28 Chemical Processors, 18 Datamined Wake Exceptions |
| Human | Engineered Missile Rack V1 | 16 Proto Radiolic Alloys, 28 Phosphorus, 10 Osmium, 24 Conductive Ceramics, 26 Hybrid Capacitors |
| Human | Enzyme Missile Rack | 15 Tungsten, 16 Thargoid Energy Cell, 18 Thargoid Organic Circuitry, 16 Molybdenum, 6 Radiation Baffle |
| Human | Meta Alloy Hull Reinforcement | 16 Meta-alloys, 25 Focus Crystals, 22 Aberrant Shield Pattern Analysis, 20 Configurable Components, 12 Reinforced Mounting Plate |
| Human | Modified Mining Laser | 20 Arsenic, 16 Osmium, 28 Phosphorus, 24 Rhenium |
| Human | Remote Release Flechette Launcher (Fixed) | 22 Rhenium, 30 Iron, 24 Molybdenum, 26 Germanium, 8 CMM Composite |
| Human | Remote Release Flechette Launcher (Turreted) | 20 Rhenium, 28 Iron, 28 Molybdenum, 24 Germanium, 10 Articulation Motors |
| Human | Shock Cannon (Fixed, Large) | 28 Vanadium, 26 Tungsten, 24 Rhenium, 26 Technetium, 8 Power Converter |
| Human | Shock Cannon (Fixed, Medium) | 24 Vanadium, 26 Tungsten, 20 Rhenium, 28 Technetium, 6 Ion Distributor |
| Human | Shock Cannon (Fixed, Small) | 8 Vanadium, 10 Tungsten, 8 Rhenium, 12 Technetium, 4 Power Converter |
| Human | Shock Cannon (Gimballed, Large) | 24 Tungsten, 24 Rhenium, 22 Technetium, 28 Vanadium, 12 Power Transfer Bus |
| Human | Shock Cannon (Gimballed, Medium) | 22 Tungsten, 20 Rhenium, 28 Technetium, 24 Vanadium, 10 Power Converter |
| Human | Shock Cannon (Gimballed, Small) | 10 Vanadium, 11 Tungsten, 8 Rhenium, 10 Technetium, 4 Power Transfer Bus |
| Human | Shock Cannon (Turreted, Large) | 26 Vanadium, 28 Tungsten, 22 Rhenium, 24 Technetium, 10 Ion Distributor |
| Human | Shock Cannon (Turreted, Medium) | 24 Vanadium, 22 Tungsten, 20 Rhenium, 28 Technetium, 8 Power Transfer Bus |
| Human | Shock Cannon (Turreted, Small) | 8 Vanadium, 12 Tungsten, 10 Rhenium, 10 Technetium, 4 Ion Distributor |
| Human | Sirius Modified AX Missile Rack (Large) | 9 Heat Dispersion Plate, 10 High Density Composites, 20 Mechanical Equipment, 10 Military Supercapacitors, 3 Technetium, 13 Vanadium |
| Human | Sirius Modified AX Missile Rack (Medium) | 6 Heat Dispersion Plate, 5 High Density Composites, 12 Mechanical Equipment, 5 Military Supercapacitors, 8 Vanadium |
| Human | Sirius Modified Heat Sink Launcher | 5 Mechanical Components, 8 Mechanical Scrap, 6 Niobium, 6 Vanadium |

EDD-only tech broker rows (EDD data from Inara, Aug 2024; not in EDEngineer):

- Human: Sirius AX Missile Rack Fixed Large — 20 Mechanical Equipment, 9 Heat Dispersion Plate, 13 Vanadium, 10 High Density Composites, 10 Military Supercapacitors, 3 Technetium
- Human: Sirius AX Missile Rack Fixed Medium — 12 Mechanical Equipment, 6 Heat Dispersion Plate, 8 Vanadium, 5 High Density Composites, 5 Military Supercapacitors
- Human: Azimuth Enhanced AX Multi Cannon Gimballed Large — 11 Iron, 16 Zirconium, 9 Bio-Mechanical Conduits, 12 Wreckage Components, 17 Weapon Parts, 6 Ship Systems Data
- Human: Azimuth Enhanced AX Multi Cannon Gimballed Medium — 9 Iron, 11 Zirconium, 6 Bio-Mechanical Conduits, 7 Wreckage Components, 14 Weapon Parts
- Human: Bobblehead — 10 Meta-Alloys, 1 Thargoid Heart
- Human: Corrosion Resistant Cargo Rack Size 4 Class 1 — 16 Meta-Alloys, 26 Iron, 18 Chemical Manipulators, 22 Radiation Baffle, 12 Neofabric Insulation
- Human: Detailed Surface Scanner (Engineered V1) — 22 Germanium, 24 Niobium, 28 Mechanical Components, 26 Mechanical Scrap
- Human: Enzyme Missile Rack Fixed Medium — 16 Thargoid Energy Cell, 18 Thargoid Organic Circuitry, 16 Molybdenum, 15 Tungsten, 6 Radiation Baffle
- Human: Engineered FSD V1 (Class 5) — 18 Datamined Wake Exceptions, 26 Tellurium, 26 Electrochemical Arrays, 28 Chemical Processors
- Human: Sirius Heat Sink Launcher — 8 Mechanical Scrap, 6 Niobium, 6 Vanadium, 5 Mechanical Components
- Human: Meta Alloy Hull Reinforcement Size 1 Class 1 — 16 Meta-Alloys, 15 Focus Crystals, 22 Aberrant Shield Pattern Analysis, 20 Configurable Components, 12 Reinforced Mounting Plate
- Human: Mining Laser (Fixed Small) — 16 Osmium, 20 Arsenic, 24 Rhenium, 28 Phosphorus
- Human: Flechette Launcher Fixed Medium — 30 Iron, 24 Molybdenum, 22 Rhenium, 26 Germanium, 8 CMM Composite
- Human: Flechette Launcher Turret Medium — 28 Iron, 28 Molybdenum, 20 Rhenium, 24 Germanium, 10 Articulation Motors
- Human: Engineered SeekerMissileRack Rack V1 (Fixed, Class 2) — 10 Osmium, 16 Proto Radiolic Alloys, 24 Conductive Ceramics, 26 Hybrid Capacitors, 28 Phosphorus
- Human: Plasma Shock Cannon Fixed Large — 28 Vanadium, 26 Tungsten, 24 Rhenium, 26 Technetium, 8 Power Converter
- Human: Plasma Shock Cannon Fixed Medium — 24 Vanadium, 26 Tungsten, 20 Rhenium, 28 Technetium, 6 Ion Distributor
- Human: Plasma Shock Cannon Fixed Small — 8 Vanadium, 10 Tungsten, 8 Rhenium, 12 Technetium, 4 Power Converter
- Human: Plasma Shock Cannon Turret Large — 26 Vanadium, 28 Tungsten, 22 Rhenium, 24 Technetium, 10 Ion Distributor
- Human: Plasma Shock Cannon Turret Medium — 24 Vanadium, 22 Tungsten, 20 Rhenium, 28 Technetium, 8 Power Transfer Bus
- Human: Plasma Shock Cannon Turret Small — 8 Vanadium, 12 Tungsten, 10 Rhenium, 10 Technetium, 4 Ion Distributor
- Human: Plasma Shock Cannon Gimbal Large — 28 Vanadium, 24 Tungsten, 24 Rhenium, 22 Technetium, 12 Power Transfer Bus
- Human: Plasma Shock Cannon Gimbal Medium — 24 Vanadium, 22 Tungsten, 20 Rhenium, 28 Technetium, 10 Power Converter
- Human: Plasma Shock Cannon Gimbal Small — 10 Vanadium, 11 Tungsten, 8 Rhenium, 10 Technetium, 4 Power Transfer Bus
- Human: TG Pulse Neutraliser — 5 Massive Energy Surge Analytics, 5 Propulsion Elements, 5 Sensor Fragment, 1 Unclassified Relic
- Guardian: Guardian FSD Booster Size 1 — 1 Guardian Module Blueprint Fragment, 21 Guardian Power Cell, 21 Guardian Technology Component, 24 Focus Crystals, 8 HN Shock Mount
- Guardian: Guardian Hull Reinforcement Size 1 Class 1 — 1 Guardian Module Blueprint Fragment, 21 Guardian Wreckage Components, 16 Pattern Beta Obelisk Data, 16 Pattern Gamma Obelisk Data, 12 Reinforced Mounting Plate
- Guardian: Guardian Hybrid Fighter V 1 — 1 Guardian Vessel Blueprint Fragment, 25 Guardian Power Cell, 26 Pattern Epsilon Obelisk Data, 18 Pattern Beta Obelisk Data, 25 Guardian Technology Component
- Guardian: Guardian Hybrid Fighter V 2 — 1 Guardian Vessel Blueprint Fragment, 25 Guardian Power Cell, 26 Pattern Epsilon Obelisk Data, 18 Guardian Wreckage Components, 25 Guardian Technology Component
- Guardian: Guardian Hybrid Fighter V 3 — 1 Guardian Vessel Blueprint Fragment, 25 Guardian Power Cell, 26 Pattern Epsilon Obelisk Data, 18 Guardian Sentinel Weapon Parts, 25 Guardian Technology Component
- Guardian: Guardian Module Reinforcement Size 1 Class 1 — 1 Guardian Module Blueprint Fragment, 18 Guardian Wreckage Components, 15 Pattern Epsilon Obelisk Data, 20 Guardian Power Conduit, 9 Reinforced Mounting Plate
- Guardian: Guardian Power Distributor Size 1 — 1 Guardian Module Blueprint Fragment, 20 Pattern Alpha Obelisk Data, 24 Guardian Power Cell, 18 Phase Alloys, 6 Heatsink Interlink
- Guardian: Guardian Power Plant Size 2 — 1 Guardian Module Blueprint Fragment, 18 Guardian Power Conduit, 21 Pattern Epsilon Obelisk Data, 15 Heat Resistant Ceramics, 10 Energy Grid Assembly
- Guardian: Guardian Shield Reinforcement Size 1 Class 1 — 1 Guardian Module Blueprint Fragment, 17 Guardian Power Cell, 20 Guardian Technology Component, 24 Pattern Delta Obelisk Data, 8 Hardware Diagnostic Sensor
- Guardian Weapons: Guardian Gauss Cannon Fixed Medium Modified — 6 Thermal Cooling Units, 18 Guardian Power Cell, 20 Guardian Technology Component, 15 Niobium, 1 Guardian Weapon Blueprint Fragment
- Guardian Weapons: Guardian Gauss Cannon Fixed Small Modified — 12 Guardian Power Conduit, 12 Guardian Wreckage Components, 15 Guardian Sentinel Weapon Parts, 9 Niobium, 1 Guardian Weapon Blueprint Fragment
- Guardian Weapons: Guardian Plasma Launcher Fixed Large — 1 Guardian Weapon Blueprint Fragment, 28 Guardian Power Conduit, 20 Guardian Sentinel Weapon Parts, 28 Chromium, 10 Micro-Weave Cooling Hoses
- Guardian Weapons: Guardian Plasma Launcher Fixed Medium — 1 Guardian Weapon Blueprint Fragment, 18 Guardian Power Conduit, 16 Guardian Sentinel Weapon Parts, 14 Chromium, 8 Micro-Weave Cooling Hoses
- Guardian Weapons: Guardian Plasma Launcher Fixed Small — 1 Guardian Weapon Blueprint Fragment, 12 Guardian Power Cell, 12 Guardian Sentinel Weapon Parts, 15 Guardian Technology Component
- Guardian Weapons: Guardian Plasma Launcher Turret Large — 2 Guardian Weapon Blueprint Fragment, 20 Guardian Power Conduit, 24 Guardian Sentinel Weapon Parts, 26 Chromium, 10 Articulation Motors
- Guardian Weapons: Guardian Plasma Launcher Turret Medium — 2 Guardian Weapon Blueprint Fragment, 21 Guardian Power Conduit, 20 Guardian Sentinel Weapon Parts, 16 Chromium, 8 Articulation Motors
- Guardian Weapons: Guardian Plasma Launcher Turret Small — 1 Guardian Weapon Blueprint Fragment, 12 Guardian Power Cell, 12 Guardian Technology Component, 15 Guardian Sentinel Weapon Parts
- Guardian Weapons: Guardian Shard Cannon Fixed Medium Modified — 12 Power Converter, 20 Guardian Wreckage Components, 18 Guardian Technology Component, 14 Germanium, 1 Guardian Weapon Blueprint Fragment
- Guardian Weapons: Guardian Shard Cannon Fixed Small Modified — 12 Guardian Power Conduit, 12 Guardian Wreckage Components, 15 Guardian Sentinel Weapon Parts, 6 Germanium, 1 Guardian Weapon Blueprint Fragment
- Guardian Weapons: Guardian Shard Cannon Turret Large — 2 Guardian Weapon Blueprint Fragment, 20 Guardian Wreckage Components, 26 Guardian Technology Component, 28 Carbon, 12 Micro Controllers
- Guardian Weapons: Guardian Shard Cannon Turret Medium — 2 Guardian Weapon Blueprint Fragment, 16 Guardian Wreckage Components, 20 Guardian Technology Component, 15 Carbon, 12 Micro Controllers
- Guardian Weapons: Guardian Shard Cannon Turret Small — 1 Guardian Weapon Blueprint Fragment, 12 Guardian Power Conduit, 15 Guardian Technology Component, 12 Guardian Sentinel Weapon Parts
- Guardian: Anti-Guardian Zone Resistance FSD — 2 Hardened Surface Fragments, 1 Caustic Crystal, 1 Tactical Core Chip
- Guardian: Anti-Guardian Zone Resistance Hull — 2 Hardened Surface Fragments, 1 Caustic Crystal, 1 Tactical Core Chip
- Guardian: Anti-Guardian Zone Resistance Modules — 2 Hardened Surface Fragments, 1 Caustic Crystal, 1 Tactical Core Chip
- Human: Engineered FSD (SCO) V1 (Class 2) — 10 Datamined Wake Exceptions, 14 Tellurium, 6 Electrochemical Arrays, 9 Chemical Processors, 4 Propulsion Elements, 1 Titan Drive Component
- Human: Engineered FSD (SCO) V1 (Class 3) — 12 Datamined Wake Exceptions, 16 Tellurium, 9 Electrochemical Arrays, 11 Chemical Processors, 5 Propulsion Elements, 1 Titan Drive Component
- Human: Engineered FSD (SCO) V1 (Class 4) — 13 Datamined Wake Exceptions, 20 Tellurium, 9 Electrochemical Arrays, 13 Chemical Processors, 6 Propulsion Elements, 1 Titan Drive Component
- Human: Engineered FSD (SCO) V1 (Class 5) — 15 Datamined Wake Exceptions, 24 Tellurium, 12 Electrochemical Arrays, 14 Chemical Processors, 7 Propulsion Elements, 1 Titan Drive Component
- Human: Engineered FSD (SCO) V1 (Class 6) — 15 Datamined Wake Exceptions, 28 Tellurium, 12 Electrochemical Arrays, 16 Chemical Processors, 8 Propulsion Elements, 1 Titan Drive Component
- Human: Engineered FSD (SCO) V1 (Class 7) — 16 Datamined Wake Exceptions, 28 Tellurium, 14 Electrochemical Arrays, 18 Chemical Processors, 12 Propulsion Elements, 1 Titan Drive Component

### A5. Ship engineer max grades — wiki vs EDEngineer differences

- Juri Ishmaak / Frame Shift Wake Scanner: wiki G3, EDEngineer not listed
- Juri Ishmaak / Kill Warrant Scanner: wiki G3, EDEngineer not listed
- Juri Ishmaak / Manifest Scanner: wiki G3, EDEngineer not listed
- Lori Jameson / Life Support: wiki G4, EDEngineer G5
