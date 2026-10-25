# Elite Nexus — Architecture

Native Rust (edition 2024, rustc 1.99, MSVC) desktop companion for Elite Dangerous that replaces
EDDiscovery, ED Market Connector, Odyssey Materials Helper, Observatory, Exploration Buddy, EDCoPilot-style
alerts, OD Elite Tracker and the website tools (INARA/Spansh/EDSM/EDSY/Coriolis/EDAstro/ED Tools/EDRefCard)
with one fast app.

## Build

```powershell
cargo run -p nexus-app                # debug build (deps optimised, fast enough)
cargo build --release -p nexus-app    # → target\release\EliteNexus.exe
cargo test --workspace
```

On Windows, `tools\dev\cargo.ps1` runs cargo at BelowNormal priority (logs in `target\cc\`) so a game on the same PC
isn't slowed, and `tools\dev\shot.ps1` checks the UI with off-screen screenshots; `AGENTS.md` has the working rules.
Parallel builds: set your own `CARGO_TARGET_DIR` (e.g. `target-agents\<name>`) to avoid lock contention.
`EliteNexus.exe --page Exploration --screenshot out.png` renders a page after loading and saves a PNG (used for UI checks;
the window is placed off-screen and never takes focus). `--size 1000x700` tests responsive layouts, `--tab <name>` opens
a Market section (`abnormal`, `database`, …), the whole-galaxy map (`galaxy`), an Exploration tab (`find`, `history`,
`unsold`), Exobiology › Find planets (`untouched`, `known`; these three searches start on the first frame of a
screenshot run), HUD & cockpit's tabs (`glare`, `cockpit`) or a HUD colours preset to start from (`mocha`,
`royalpurple`, …), `--demo` adds a sample Ask conversation and an alert banner, `--shot-delay 8` waits longer (the
galaxy image paints in the background, Spansh searches take 5–20 s). `ELITE_NEXUS_GAME_DIR` points HUD & cockpit at
another game folder (one holding `EliteDangerous64.exe`): test the Cockpit lights installer against a copy, never
a real install.

## Crates

| crate | role | depends on |
|---|---|---|
| `nexus-journal` | journal + companion-file I/O: typed events (`Event::parse`), `load_history` (parallel bulk load), `Watcher` (100 ms live tail), NaN-sanitising parser | — |
| `nexus-data` | static game knowledge compiled in: ranks, ships, exploration values, exobiology, commodities/modules/materials names (FDevIDs), engineering blueprints & engineers, material-trader maths | — |
| `nexus-model` | pure state machine: `Model` (all CMDRs) → `GameState` (location, fleet, inventory, missions + massacre stacks, exploration, exobio, mining, carrier, colonisation, history ledger, session, feed) + `Alerts`; `context`: what the commander is doing right now (docked, supercruise, charging a jump, near a body, landed, SRV, on foot, …, the GuiFocus screen, danger), from Status.json and the journal, for anything that follows the game | journal, data |
| `nexus-galaxy` | local SQLite (WAL) galaxy/market DB: one writer thread, pooled read connections, in-memory station index; commodity search, trade routes, nearest services, hotspots, streaming Spansh dump import | journal |
| `nexus-eddn` | EDDN **subscriber** (ZeroMQ relay → decoder thread → galaxy DB), dedupe, watchdog + reconnect, stats & headlines | galaxy |
| `nexus-uplink` | **uploads**: EDDN (schema-validated, strictly compliant), EDSM journal sync, Inara, EDAstro; persistent retry queues (`nexus-data\uplink\*.jsonl`), per-service rate limits | journal |
| `nexus-spansh` | Spansh API client: neutron/galaxy/carrier/riches/exobio/trade plotters, system/station/body search | — |
| `nexus-binds` | `.binds` parser, conflicts/missing essentials, printable reference card | — |
| `nexus-hud` | HUD colour matrix: maths and previews (linear light), surgical edits of `GraphicsConfigurationOverride.xml` (only the `<Matrix…>` rows, backup once, atomic), credited community presets, friend/foe readability checks, game install and EDHM detection, tone mapping (glare, ACES curve, auto-exposure targets in `<HDRNode_Reference>`), Elite Nexus's graphics hook (`dimmer`: 3Dmigoto from its official release with pinned SHA-256s and a pure-Rust 7z reader; `[ShaderRegex]` patterns that patch the game's box-light shader at load (cockpit-light dimmer) and, when switched on, the HUD shaders that apply the colour matrix (keep hostile red, friendly green and shield blue; `palette::is_hud_orange` is the same rule in Rust for previews); install / settings / removal tracked by a manifest in the game folder) | — |
| `nexus-ai` | built-in assistant: providers (Gemini via its OpenAI-compatible endpoint, Claude Messages API with prompt caching, local/other OpenAI-compatible), tool-calling loop, embedded knowledge base (`knowledge/*.md` + `docs/USER_GUIDE.md`, BM25 search) | — |
| `nexus-app` | egui 0.36 / eframe (wgpu) GUI, services wiring, TTS, toasts | everything |

## Runtime

```
journal-watcher thread ──WatchMsg──► nexus-core thread ── write lock ──► Arc<RwLock<Model>> ◄── read lock ── UI (egui)
                                          │  alerts ──► UI toasts + TTS thread
                                          │  history sink (start-up) ──► galaxy-history thread ─┐
                                          └─ LiveMsg (Event | Companion, + EventContext)        │
                                               ├─► nexus-sync thread ──► nexus-uplink (queues)  │
                                               └─► galaxy-feed thread ──► galaxy-live thread ───┤
EDDN relay ──ZeroMQ──► socket task (tokio) ──► eddn-decoder thread ─────────────────────────────┤
Spansh dump (HTTP stream / file) ──► galaxy-import thread ──────────────────────────────────────┴─► galaxy-writer thread ──► galaxy.db
UI pages ──► background jobs (jobs.rs) ──► galaxy read pool / Spansh API (tokio)
```

* `app.rs` owns the services: `Core` (journal → model), `SyncService` (sync.rs → nexus-uplink),
  `MarketService` (market.rs → nexus-galaxy + nexus-eddn, price alerts, Spansh import), `AssistantService`
  (assistant.rs → nexus-ai on a worker thread; `ai_tools.rs` implements its 16 tools over the model, galaxy DB,
  journal and Spansh), Spansh client, TTS. `finder.rs` holds market lookups shared by pages and tools (sell plan,
  colonisation suppliers).
* Routes and tours: Navigation holds one `PlannedRoute` (pages/navigation.rs) that drives the "next system →
  clipboard" automation and the galaxy map. It tracks progress by arrival (`advance`), so a loop that starts here isn't
  finished at once. Navigation plans travel itself (neutron, exact, carrier); scan tours are planned on the page they
  serve and handed over: `tours.rs` wraps Spansh's Road to Riches (Exploration › Find valuable bodies) and exobiology
  planner (Exobiology › Find planets › Sure value), widening the radius while Spansh answers with nothing (it skips
  populated systems), builds nearest-first tours through planets found here, and holds Spansh's own limits
  (`tours::limits`). `untouched.rs` is the bonus hunt: a Spansh body search classified by the evidence of someone
  having been there (genera = mapped, biology landmarks = sampled, footfall), with every exclusion counted in `Stats`
  so the page can say why planets were left out.
* UI rule: simple surface, hidden depth. Defaults come from the commander's state; advanced options sit behind a
  collapsed "› Options" disclosure, never removed. Say where data comes from and what it can't show.
* Design system (macOS 26 dark, Liquid-Glass sidebar and toolbar): `theme.rs` holds the palette (`pal`), the type scale
  (`size`, nothing below 12 pt) and the egui style; `widgets.rs` the components: `card`/`card_tinted` (Health-style
  headers), `cards` (responsive grid with equal-height rows), `split`, `group` rows with switches (System Settings
  style), `tabs`/`segmented` (falls back to pills when too narrow), `switch`/`toggle`, `pill`, capsule `button`s,
  `search_field`, `badge`, `th` table headings. Pages use these instead of raw egui widgets so the look stays uniform
  and every layout adapts to the window width (the sidebar becomes an icon rail below 1180 px).
* History is folded with alerts **disabled**; only live events raise alerts or get uploaded. Never upload history.
  Test runs (`--journal-dir`, `--screenshot`) never upload at all.
* The UI never blocks: long work (DB queries, HTTP) runs off-thread and results are polled each frame.
* Data dir: `nexus-data\` next to the exe (portable) — fallback `%LOCALAPPDATA%\EliteNexus`, override `ELITE_NEXUS_HOME`.
  Contents: `settings.json` (API keys DPAPI-sealed), `galaxy.db` (+ WAL), `uplink\` queues and caches.
* The galaxy DB only keeps what queries use: EDDN jumps into unpopulated systems are dropped (populated systems,
  carrier jumps, docked arrivals, stations, markets and ring hotspots are kept); the commander's own journal is kept whole.
  The `bodies` table keeps, from EDDN, only bodies with biology, Earth-like/water/ammonia worlds, terraformables and
  on-foot evidence (`GalaxyOptions::all_eddn_bodies` keeps every landable body); from the own journal also every
  landable body. Footfall flags are dated, monotonic, and the commander's journal wins.
* Abnormal prices: partial indexes (`deals::ANOMALY_INDEXES`) hold only market rows ≥ 1.5× off the galactic average and
  every unpriced (fleet-carrier) order, so a whole-galaxy scan reads just those rows.

## Conventions

* Journal timestamps: `nexus_journal::Timestamp` (unix seconds). Commodity symbols: lower-case (`commodity_symbol("$gold_name;") == "gold"`).
* All serde structs for external data use `#[serde(default)]`; unknown fields ignored; never panic on input data.
* No `unwrap()` on I/O or network data. `tracing` for logs. Clippy-clean.
* Research specs live in `docs/research/*.md` (EDDN, EDSM, Inara, CAPI, Spansh, websites, journal, engineering, exploration, HUD colours and cockpit lights, tool feature matrix).

## Licensing notes

* Elite Nexus is free software under GPL-3.0-or-later (`LICENSE`); third-party data and fonts are listed in `NOTICE.md`
  with their licences.
* EDCD/FDevIDs CSVs (data/fdevids) have **no licence file**; releases now distribute binaries, so ask EDCD.
* EDGalaxyData (online footfall lookups, `ai_tools::ONLINE_FOOTFALL_LOOKUP`) publishes no terms: ask its operator.
* EDEngineer data (MIT), EDDiscovery sources (Apache-2.0), ed-journal-schemas (Apache-2.0): attribution in data/*/SOURCE.txt.
* coriolis-data / EDSY data are Frontier IP / CC BY-NC: link to the sites, don't bundle their data.
* Exobiology dataset is a factual compilation incl. facts from GPL tools — no GPL code is copied.
