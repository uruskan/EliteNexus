<p align="center"><img src="site/assets/favicon.svg" width="88" alt="Elite Nexus icon"></p>

<h1 align="center">Elite Nexus</h1>

<p align="center"><strong>The free, open-source Elite Dangerous companion app for Windows, Linux and macOS.</strong><br>
Live trading, exploration, exobiology, engineering, fleet and carrier tracking, Spansh routes, an AI co-pilot,
HUD colours, cockpit lights and glare control: one fast native app instead of a pile of tools and browser tabs.</p>

<p align="center">
  <a href="https://github.com/uruskan/EliteNexus/releases/latest"><img src="https://img.shields.io/github/v/release/uruskan/EliteNexus?label=Download&logo=github" alt="Download the latest release"></a>
  <a href="https://uruskan.github.io/EliteNexus/"><img src="https://img.shields.io/badge/Website-uruskan.github.io%2FEliteNexus-0b7285" alt="Website"></a>
  <a href="https://github.com/sponsors/uruskan"><img src="https://img.shields.io/badge/Sponsor-%E2%9D%A4-ea4aaa?logo=githubsponsors" alt="Sponsor on GitHub"></a>
  <a href="https://buymeacoffee.com/uruskan"><img src="https://img.shields.io/badge/Buy%20me%20a%20coffee-%E2%98%95-ffdd00" alt="Buy me a coffee"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/licence-GPL--3.0-blue" alt="Licence: GPL-3.0-or-later"></a>
  <a href="https://github.com/uruskan/EliteNexus/actions/workflows/ci.yml"><img src="https://github.com/uruskan/EliteNexus/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
</p>

Elite Nexus reads the game's journal as you play and brings together what commanders usually run side by side:
EDDiscovery, ED Market Connector, Odyssey Materials Helper, Observatory, Exploration Buddy, voice alerts, and the
websites you alt-tab to (Inara, Spansh, EDSM, EDSY, Coriolis, EDAstro). It works with Horizons and Odyssey on
Windows 10 and 11, on Linux with the game under Proton or Wine, and on a Mac with the game under NotProton,
CrossOver or Whisky.

It is one executable with no installer, no account and no ads. Written in Rust with a GPU-rendered interface, it
starts in about a second, loads years of journal history in well under one, and idles at a fraction of the memory
the older tools need.

![Elite Nexus, an Elite Dangerous companion app: dashboard with commander ranks, active ship, location and session stats](docs/screenshots/dashboard.png)

## What commanders are saying

![Quotes from commanders on the Frontier Forums, GitHub and Reddit, October 2026. "The engineering section is excellent as is the colonisation section and also the carrier route planning. This app does most of what I need from EDDiscovery in a fraction of memory usage. Keep going." "This is now my goto Elite companion. It does everything I want and more." "Not launched any other 3rd party apps since 0.6.0 dropped." "That abnormal price filtering is a great idea." "It ran without any problems, and immediately detected my Elite Dangerous installation. The Exploration and Exobiology sections of the application worked perfectly." "I love EliteNexus, handsome work you did there." "Works great so far, thanks for making it!"](docs/testimonials.png)

## Get started

1. **[Download the latest release](https://github.com/uruskan/EliteNexus/releases/latest)**:
   `EliteNexus-…-windows-x64.zip`, `EliteNexus-…-linux-x64.tar.gz` or `EliteNexus-…-macos-arm64.zip` (Apple Silicon).
2. **Unpack and run.** Windows: unzip anywhere and run `EliteNexus.exe`. Linux: unpack and run `./EliteNexus`.
   Mac: drag `Elite Nexus.app` to Applications; the first time, allow it once under System Settings › Privacy &
   Security › Open Anyway (it isn't notarized).
3. **Fly.** Elite Nexus finds the game's journal folder by itself and loads your commander. Start it before or after
   the game. Community sync, the AI co-pilot and the cockpit tools are optional and wait until you set them up.

Everything it keeps (settings, the galaxy database, logs) lives in a `nexus-data` folder next to the program (on a
Mac, in `~/Library/Application Support/EliteNexus`). Every page, and the Linux and macOS notes, are in the
**[user guide](docs/USER_GUIDE.md)**. Linux reports are collected in
[issue #3](https://github.com/uruskan/EliteNexus/issues/3).

## Support Elite Nexus

Elite Nexus is made by one commander, with no company behind it, no ads and no subscription. If it has saved you an
evening of alt-tabbing, found you a payday or kept hostile contacts red in your favourite HUD, you can sponsor it.
Nothing in the app depends on it. o7

<p>
  <a href="https://github.com/sponsors/uruskan"><img src="https://img.shields.io/badge/Sponsor-%E2%9D%A4-ea4aaa?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Sponsor on GitHub"></a>
  <a href="https://buymeacoffee.com/uruskan"><img src="https://img.shields.io/badge/Buy%20me%20a%20coffee-%E2%98%95-ffdd00?style=for-the-badge&logo=buymeacoffee&logoColor=black" alt="Buy me a coffee"></a>
</p>

## Make the cockpit yours

![Elite Dangerous HUD colour editor: an Empire blue theme in the live cockpit preview, with hostile contacts still red and friendly ones still green](docs/screenshots/hud-colours.png)

* **HUD colours**: an editor built on the game's own setting, with community themes (credited to their authors), a
  colour wheel, saturation and brightness, and a live cockpit preview that warns when friend and foe would look
  alike. Applying writes only the three colour lines of `GraphicsConfigurationOverride.xml` and keeps your original
  file.
* **Keep hostile red, friendly green and shield blue.** The game's setting moves every HUD colour together, so a blue
  theme turns hostile contacts blue too. Switch this on and your theme colours only the HUD's orange.
* **Cockpit lights.** The game has no setting for them: Elite Nexus dims the cockpit's own lamps and glow and nothing
  else, with in-game keys for on/off (F9), how much light stays (F10) and reach (F11).
* **Glare and exposure.** Odyssey's auto-exposure darkens everything outside when the dashboard is bright; tame the
  glare and lift the darks with the game's own tone-mapping values, starting from community presets.

HUD colours and glare use the game's own settings file. Keeping friend-and-foe colours and dimming the cockpit lights
need an optional graphics hook, [3Dmigoto](https://github.com/bo3b/3Dmigoto) (open source), downloaded from its
official release, checked against pinned checksums, and installed or removed in one click from the Cockpit lights
tab. No game file is changed and Elite Nexus never ships the game's code: its patterns adjust the light and HUD
shaders as the game loads them. The hook can't run alongside EDHM, Frontier tolerates mods like this without having
approved them, and it is untested under Proton and on macOS so far.

| | |
|---|---|
| ![Elite Dangerous cockpit lights dimmer: installed, with light kept, reach and the in-game keys](docs/screenshots/cockpit-lights.png) **Cockpit lights** — dim the cockpit's own lights | ![Elite Dangerous Odyssey glare and auto-exposure fix: Morbad's Odyssey tweak applied, with less glare and the standard ACES curve](docs/screenshots/glare.png) **Glare and exposure** — less glare, brighter darks |

## Screenshots

| | |
|---|---|
| ![Exobiology: live sampling radar with two samples, colony-range circles and "OK to sample"](docs/screenshots/exobiology.png) **Exobiology** — live sampling radar, colony range, species value | ![Exploration: every body in the system with value, mapped value and highlights](docs/screenshots/exploration.png) **Exploration** — every body, value now vs. mapped, highlights |
| ![Ask: the AI co-pilot answering an exobiology question](docs/screenshots/ask.png) **Ask** — the AI co-pilot | ![Market: stations paying far above the galactic average](docs/screenshots/market.png) **Market** — abnormal prices across the galaxy |
| ![Navigation: the plotted route with star classes, scoopable stars and jump distances](docs/screenshots/navigation.png) **Navigation** — route, fuel stars, next system to the clipboard | ![Engineering: blueprint wishlist and the materials shopping list](docs/screenshots/engineering.png) **Engineering** — wishlist and shopping list |
| ![Fleet: every ship with its full engineered loadout](docs/screenshots/fleet.png) **Fleet** — loadouts, engineering, one-click EDSY/Coriolis | ![Fleet carrier: tritium, finances, capacity and crew](docs/screenshots/carrier.png) **Fleet carrier** — tritium, finances, crew |
| ![Galaxy map: everywhere you've been, with your ship and carrier](docs/screenshots/galaxy-map.png) **Galaxy map** — your travels, ship and carrier | ![Statistics: credits over time, income by category, jumps per day](docs/screenshots/statistics.png) **Statistics** — credits, income, jumps |
| ![Logbook: every journal event, searchable](docs/screenshots/logbook.png) **Logbook** — every journal event, searchable | ![Bindings: active presets, conflicts and every bound action](docs/screenshots/bindings.png) **Bindings** — presets, conflicts, reference card |
| ![Accounts & sync: EDDN, EDSM, Inara and EDAstro, all opt-in](docs/screenshots/accounts.png) **Accounts & sync** — EDDN, EDSM, Inara, EDAstro | |

## Thanks

Elite Nexus is one commander's spare-time project, and most of what changed since launch started with someone
else's report, idea or help. Thank you, commanders.

**Code**

* **[leodesca01](https://github.com/leodesca01)**: the native Mac app, `Elite Nexus.app`
  ([PR #2](https://github.com/uruskan/EliteNexus/pull/2)).
* **[Nillecram](https://github.com/Nillecram)**: showed from their own Vista Genomics sales when the first-logged
  bonus is really paid, and fixed it ([PR #7](https://github.com/uruskan/EliteNexus/pull/7)).

**Sponsors**

* **[sjgarth](https://github.com/sjgarth)** (Alvarrune on the Frontier Forums), and one sponsor who chose to stay
  anonymous. Everyone who bought a coffee: thank you too.

**Bug reports and ideas on GitHub**

* **[pioniere](https://github.com/pioniere)**: the first Linux review, on Nobara under Proton and Wayland, and the
  silent Test voice ([issue #3](https://github.com/uruskan/EliteNexus/issues/3)).
* **[docfallingapart](https://github.com/docfallingapart)**: the Linux app-menu entry, and a folder with a space in
  its name.
* **[nhok0169](https://github.com/nhok0169)**: Ask answering from old data, the empty Services tab, the window
  freezing with the live feed, the synthesis and chart drawing, long module names, shared missions, and the "where do
  I get this material" idea ([issue #4](https://github.com/uruskan/EliteNexus/issues/4)).
* **[hangerhead](https://github.com/hangerhead)**: a carrier's docking access in the market lists, how long a buy
  order has been live, and a louder voice.
* **[Chequers2](https://github.com/Chequers2)**: a surface mining map that remembers the body and the site.

**Bug reports and ideas on the Frontier Forums**

* **cmdr chequers**: journals on a network share, the fleet carrier jump and cooldown timers, and the memory
  comparison with EDDiscovery.
* **Alvarrune**: a Steam library on another drive, a colonisation depot that wasn't theirs, the route countdown, and
  the dimmer crash.
* **Asmodeus**: Elite Nexus changing bindings (it never touches them now).
* **Smallfishnz**: the window vanishing, which is why there is a crash log.
* **NetworX**: the BGS tick hour resetting.
* **BigOldChicken**: searching outfitting for a module, and **Jo Bangles**: the first impressions.

**Community knowledge Elite Nexus is built on**

* HUD themes by **CMDR Diomedian** (Empire blue, Federation, Alliance green), **CMDR Alex Traut** (Royal purple),
  **Arkku** (Blue and red, and the HUD colour editor the themes come from) and **Prima Games** (VR yellow);
  **CMDR Morbad**'s Odyssey glare settings.
* Data and research: EDCD's FDevIDs, EDEngineer (msarilar), EDDiscovery, jixxed's journal schemas, Richard Buckle's
  EDRefCard, the exobiology research of Canonn, EDMC-BioScan, SrvSurvey and EDAstro, and MattG's exploration value
  formulae; the services Spansh, EDSM, Inara, EDDN and EDAstro; 3Dmigoto (bo3b); the Inter typeface and Phosphor
  icons. Sources and licences: [NOTICE.md](NOTICE.md).

## What it does

Each page shows the one thing you usually need, with your current system, ship and landing pad as defaults, and
keeps the advanced settings one click away under Options. Here is every page in one line; the
**[user guide](docs/USER_GUIDE.md)** explains them in full.

| Page | What you get |
|---|---|
| **Dashboard** | Commander, ranks and progress, credits and legal state, ship (fuel, hull, cargo, jump range, rebuy), location and system politics, session earnings, the current system's exploration value, holdings, your fleet carrier and your squadron's with their jump countdowns, colonisation, and an activity feed. |
| **Exploration** | The current system as a tree (stars, planets, moons) with each body's value now and if mapped (first discovery and mapping, Odyssey bonus, efficiency), gravity, temperature, atmosphere, materials, rings and hotspots, biology and geology signals, footfall; Observatory-style highlights (Earth-likes, water and ammonia worlds, terraformables, landable atmospheres, close and wide rings, shepherd and nested moons, fast rotation, jumponium…); **Find valuable bodies**, Spansh's Road to Riches, which widens the search out of the bubble by itself; **Unsold data**; a **History** of every system you scanned and whether its data was sold, refused or lost with your ship; the codex. |
| **Exobiology** | A live **sampling radar** turned to your heading, with every previous sample's colony-range circle and a big "OK to sample / too close" readout; the bio bodies and the genera found on them; species **predictions** from a body's atmosphere, gravity, temperature, volcanism and stars before you land, with the expected value; unsold Vista Genomics data, with the first-logged bonus (×5) only where it can still be paid; **Find planets** two ways: a **bonus hunt** for planets where a valuable species should grow and nobody has a record of sampling it (every exclusion counted and explained), or **sure value**, Spansh's exobiology planner; **Tour the best 15** plans the route. |
| **Navigation** | Your in-game route with scoopable, neutron and white-dwarf stars flagged and the longest non-scoopable streak; the **next system copied to the clipboard** after every jump; Spansh's **neutron**, **exact** (your ship's real jump range) and **fleet-carrier** planners, the carrier one with its tritium, jump timer and an arrival estimate; tours and routes the assistant plans land here too and tick off as you arrive. |
| **Galaxy map** | The Milky Way with everywhere you've been, your travel path, the active route, your carriers, parked ships and landmarks; search any visited system; click one to copy its name or plot a route to it. |
| **Market** | A local galaxy database fed live by EDDN, your journal and, if you like, Spansh's daily dump (every station known from day one). **Commodity pages** with where to buy, where to sell, latest prices and price history, filtered by distance (up to the whole galaxy), pad, price age, stock or demand and carriers; **sell my cargo**, the best single stop for your hold, re-planned after every jump; **trade routes** for your ship and credits; **abnormal prices**, stations and carriers paying far above or selling far below the galactic average; **pins** and a **run planner** that buys and sells your pinned orders on the shortest trip; **price alerts** spoken seconds after any commander's update; the nearest material traders, brokers, Interstellar Factors and Vista Genomics; mining hotspots with their best sell prices; any station's whole market, including the one you're docked at; and the live feed, with a pause. |
| **Mining** | Live sessions: tonnes per hour, prospector quality, motherlodes. For Rhino surface mining, every planetary mining location you worked, with the body and the number to pick in the navigation panel to go back, what came out there and your notes, plus a surface radar to the deposits and your ship. |
| **Colonisation** | Construction depots with the remaining tonnage per commodity, what you already carry, and **where to buy** each commodity near the site with enough stock for a full hold; stale depots marked, other commanders' depots kept apart. |
| **Fleet carrier** | Your fleet carrier and your squadron's: tritium, finances, capacity, crew and services, orders, jump schedule and history, a live jump countdown and cooldown, decommissioning. |
| **Engineering** | A blueprint **wishlist** (or quick-add "upgrade to G5" from your ship) that becomes a **shopping list** and a **trader plan**, exactly what to trade at which material trader; engineers, unlocked first then by distance; Odyssey suits and weapons; synthesis; your materials and ship locker. |
| **Fleet** | Every ship and where it is, in transit included, with its full loadout, engineering and modifier deltas; one-click **Open in EDSY / Coriolis** with the exact build. |
| **Missions** | Active missions with expiry and cargo or kill progress, and a **massacre-stack tracker** that counts a kill once per issuing faction, so you see the true kills remaining. |
| **BGS & Powerplay** | What you did for which faction and power since the tick, with your tick hour and period remembered. |
| **Logbook** | Activity, every journal event with its JSON and full-text search, travel, docking, and a credit ledger. |
| **Statistics** | Credits over time (with a log scale), income and spending by category, jumps and distance per day, deaths, and the game's own statistics, over 7, 30 or 90 days or all time. |
| **Bindings** | Your active presets per category, conflicts, missing essentials, a searchable list of every bound action and a printable reference card. Read-only: the app never changes your bindings. |
| **HUD & cockpit** | **HUD colours**, **Glare and exposure** and **Cockpit lights**: see [Make the cockpit yours](#make-the-cockpit-yours). |
| **Ask** | The AI co-pilot: plain questions ("where can I make the most money in the next hour?") answered from your ship, cargo, journal, the market database and Spansh, with tools for price deals, selling your cargo, trade routes, commodities, nearest services, mining hotspots, your surface mining sites, systems and their planets (and who has set foot on them), untouched biology, route and money-route planning, exobiology here and journal search, plus a built-in knowledge base. Google Gemini by default (one pasted key); Claude or a local model (Ollama, LM Studio) optional. Nothing else in the app needs it. |
| **Accounts & sync** | EDDN (schema-checked, with a test mode), EDSM journal sync, Inara profile sync (the app name is approved, so your personal key is enough) and EDAstro, each opt-in, with retry queues that respect the service's limits. |
| **Alerts & voice** | Toasts plus speech (Windows speech, speech-dispatcher on Linux, `say` on macOS) for high-value bodies, likely valuable biology, sample spacing, motherlodes, a non-scoopable star when fuel is low, interdictions, hull critical, mission completion, carrier jumps and more, down to the priority you choose. |
| **Overlay** | One small black island at the top of the game's screen that says what's going on in a word or two ("2 of 3 samples · Far enough", "Scoopable · 12 jumps left"), opens up for the moments that need you (the next jump, an arrival worth it, an approach, a sample, a deposit, docking, an alert) and fades away when nothing does. Click-through, it never takes the focus, and it shows only while the game is in front; see-through or an opaque panel, and a VR mode for mirroring it into a headset. |
| **Settings** | Journal and game folders found by themselves (every Steam library, Epic and Frontier folders; Proton, Wine, CrossOver and Whisky prefixes) or set by hand, even on a network share; voice, toasts and alert thresholds; text size in fixed steps; the app-menu entry on Linux; a settings file that can't be read or a crash is kept and explained, never silently reset. |

## Documentation and links

* **[User guide](docs/USER_GUIDE.md)**: every page explained, with [Linux](docs/USER_GUIDE.md#linux) and
  [macOS](docs/USER_GUIDE.md#macos) notes. It is also what the in-app assistant knows about the app.
* **[Website](https://uruskan.github.io/EliteNexus/)**, one page per topic:
  [HUD colours](https://uruskan.github.io/EliteNexus/hud-colours.html),
  [Cockpit lights](https://uruskan.github.io/EliteNexus/cockpit-lights.html),
  [Odyssey glare](https://uruskan.github.io/EliteNexus/odyssey-glare.html),
  [Trading](https://uruskan.github.io/EliteNexus/trading.html),
  [Exploration & navigation](https://uruskan.github.io/EliteNexus/exploration.html),
  [Exobiology](https://uruskan.github.io/EliteNexus/exobiology.html),
  [Fleet & engineering](https://uruskan.github.io/EliteNexus/commander.html),
  [AI, voice & your data](https://uruskan.github.io/EliteNexus/companion.html),
  [Compared with other tools](https://uruskan.github.io/EliteNexus/compare.html) and
  [Thanks](https://uruskan.github.io/EliteNexus/thanks.html).
* **[Changelog](CHANGELOG.md)** and **[release notes](https://github.com/uruskan/EliteNexus/releases)**: what
  changed in each version.
* **Talk**: the [release thread on the Frontier Forums](https://forums.frontier.co.uk/threads/elite-nexus-free-companion-app-trading-routes-engineering-exobiology-hud-colours-that-keep-hostiles-red-glare-fix-etc.650288/)
  and [GitHub Discussions](https://github.com/uruskan/EliteNexus/discussions) for questions and ideas;
  [GitHub Issues](https://github.com/uruskan/EliteNexus/issues) for bugs, with `nexus.log` (and `crash.log`, if
  there is one) from the data folder (Settings › About › Data folder › Open).
* **Accounts you may want**: an [EDSM API key](https://www.edsm.net/en/settings/api) and an
  [Inara API key](https://inara.cz/elite/cmdr-settings-api/) for sync. The Frontier Companion API isn't supported
  yet; the journal already provides almost everything.

## For developers

Elite Nexus is written in Rust (egui/wgpu) and builds with a stable toolchain on Windows, Linux and macOS.
**[CONTRIBUTING.md](CONTRIBUTING.md)** covers building from source, command-line options, tests, developer tools and
how to send changes. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) tours the crates and threads, [AGENTS.md](AGENTS.md)
holds the working rules for this repository, and [docs/research/](docs/research/) the verified specs behind every
integration (EDDN, EDSM, Inara, Spansh, the journal, engineering, exploration, HUD colours and cockpit lights).

## Licence

Elite Nexus is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
version. It comes with no warranty. See [LICENSE](LICENSE). Third-party data and fonts keep their own licences: see
[NOTICE.md](NOTICE.md).

*Elite Dangerous is a trademark of Frontier Developments plc. Elite Nexus is an unofficial community tool.*
