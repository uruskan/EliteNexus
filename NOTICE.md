# Third-party data & credits

Elite Nexus is original code, licensed under the GNU General Public License, version 3 or later (see `LICENSE`).
It derives **data** (facts, names, numbers) from the community sources below.
Each source's licence text and exact commit are kept next to the data in `data/*/` (`LICENSE`, `SOURCE.txt`).

| Source | Used for | Licence | Notes |
|---|---|---|---|
| EDCD/FDevIDs (`data/fdevids`) | commodity, module, ship, material and micro-resource names/IDs | none published; factual game data | Frontier's own game identifiers, compiled by EDCD. Asked in [EDCD/FDevIDs#105](https://github.com/EDCD/FDevIDs/issues/105) (2025): contributors call the data "free for all, like public domain" and note it belongs to Frontier; closed without a licence or objection. Every EDCD tool ships it. |
| EDEngineer by msarilar (`data/engineering/edengineer`) | ship blueprints, experimental effects, synthesis, technology broker costs | MIT | Archived 2024; Odyssey costs come from EDDiscovery instead. |
| EDDiscovery (`data/engineering/eddiscovery-src`) | Odyssey suit/weapon upgrades & modifications, engineers incl. Colonia (+ journal EngineerIDs), journal blueprint symbols, synthesis recipes, technology broker unlocks, Thargoid-war materials, one blueprint (Short Range Blaster) | Apache-2.0 | Data transcribed (changes listed in `data/engineering/nexus/SOURCE.txt`), no code linked. Ship the Apache-2.0 text with binaries. |
| jixxed/ed-journal-schemas (`data/journal-schemas`) | reference for undocumented journal events (Colonisation, Powerplay 2.0, Flags2 bits) | Apache-2.0 | |
| EDRefCard by Richard Buckle | cross-check for binding action groupings/names (`crates/nexus-binds`) | MIT | Licence reproduced in `crates/nexus-binds/README.md`. |
| Community exobiology research (Canonn, EDMC-BioScan, SrvSurvey, EDAstro, Fandom wiki) | species values, colony ranges, prediction conditions (`data/exobiology`) | facts only | No GPL code is copied; entries cite their sources. |
| MattG's exploration value formulae (Frontier forums) | cartographic value estimates | facts only | |
| Spansh, EDSM, Inara, EDDN, EDAstro | live services the app talks to (uploads opt-in) | their own terms | See `docs/research/` for each API's usage rules. The optional Spansh station import streams Spansh's public daily dump into the local database. |
| EDDN JSON schemas + READMEs (EDCD/EDDN, `docs/research/eddn-schemas`) | validating every upload in tests | BSD-3-Clause | Test-only; not in the binary. Add EDDN's `LICENSE` file there before sharing the source. |
| Inter by Rasmus Andersson (`data/fonts/inter`) | interface typeface, compiled into the exe | SIL Open Font License 1.1 | Licence text in `data/fonts/inter/LICENSE.txt`; ship it with binaries (the release zip includes this NOTICE). |
| Phosphor Icons (egui-phosphor) | UI icons | MIT | |
| egui's default fonts (epaint_default_fonts) | fallback glyphs and emoji | Ubuntu Font Licence 1.0 / SIL OFL 1.1 | Font data compiled into the exe; each font keeps its own licence. |
| SQLite (via rusqlite, bundled) | local galaxy/market database | public domain | |
| zeromq (zmq.rs) | EDDN relay subscriber | MIT | |
| reqwest / rustls / aws-lc-rs | HTTPS | MIT / Apache-2.0 / ISC | |
| egui / eframe / wgpu, tokio and other crates | UI and runtime | MIT / Apache-2.0 | See `cargo tree` / crate licences. |

Not bundled on purpose: coriolis-data (Frontier IP) and EDSY data (CC BY-NC) — Elite Nexus links to those sites instead.

*Elite Dangerous is a trademark of Frontier Developments plc. Elite Nexus is an unofficial, community-made tool and is not
endorsed by Frontier Developments.*
