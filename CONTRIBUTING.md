# Contributing to Elite Nexus

This page is for anyone who wants to build Elite Nexus from the source, report a bug well, or change the code. If you
just want to use the app, the [README](README.md) and the [user guide](docs/USER_GUIDE.md) are for you.

## Building from source

Elite Nexus is a Rust workspace. A stable toolchain from [rustup.rs](https://rustup.rs) is all it needs on Windows,
Linux and macOS: CI builds on all three with nothing else installed.

```sh
cargo run -p nexus-app                 # debug build, started right away
cargo build --release -p nexus-app     # target/release/EliteNexus (EliteNexus.exe on Windows)
```

* **macOS**: `tools/macos/build-app.sh` builds and packs `target/macos/Elite Nexus.app` and a zip of it (it needs
  Xcode's Command Line Tools); `--install` also copies the app to Applications. Releases carry an Apple Silicon build
  only, so this is how an Intel Mac gets the app.
* **Windows, a release build for yourself**: `tools\build-release.ps1` installs into `dist\EliteNexus`, or stages
  `EliteNexus.new.exe` while the app runs (`Update-EliteNexus.cmd` swaps it in once the app is closed).
  `Start-EliteNexus.cmd` starts that copy, building it first when there is none. Neither touches `nexus-data`.
* Every build keeps its data (settings, galaxy database, logs) in `nexus-data` next to its own executable, so a debug
  build (`target/debug/nexus-data`) never shares anything with the copy you play with. On macOS the app bundle keeps
  it in `~/Library/Application Support/EliteNexus` instead, and `ELITE_NEXUS_HOME` points any build at another folder.

## Command-line options

| Option | What it does |
|---|---|
| `--page <name>` | Start on a sidebar page (`Dashboard`, `Exploration`, `Exobiology`, `Navigation`, `Market`, `Settings`, …). |
| `--tab <name>` | Open a tab on that page: Market sections (`abnormal`, `database`, …), the whole-galaxy map (`galaxy`), Exploration's `find`, `history` and `unsold`, Exobiology › Find planets (`untouched`, `known`), HUD & cockpit's `glare` and `cockpit`, or a HUD colours preset to start from (`mocha`, `royalpurple`, …). |
| `--journal-dir <dir>` | Read another journal folder (a test run; it never uploads). |
| `--mute` | No voice. |
| `--demo` | Add a sample Ask conversation and an alert banner (for screenshots). |
| `--overlay <moment>` | The overlay with sample content: the main window shows the moment compact and expanded, over the game and as the opaque panel, and (except in a screenshot run) the overlay's own window turns between the two every four seconds. Moments: `jump`, `hyperspace`, `arrival`, `approach`, `sampling`, `close`, `mining`, `trade`, `docked`, `carrier`, `two`, `alert`, `quiet`. |
| `--screenshot <png> [--shot-delay <s>] [--size <w>x<h>]` | Render the page off-screen after loading, save the PNG and exit. The window is never visible and nothing is uploaded. `--size 1000x760` checks a narrow layout; `--shot-delay 8` or more waits for the galaxy image or a Spansh search (5 to 20 s). |
| `--install-desktop` / `--uninstall-desktop` | Linux only: add Elite Nexus and its icon to the desktop's app menu, or take it out. |

The images in `docs/screenshots/` were made this way, e.g.
`EliteNexus.exe --page market --tab abnormal --screenshot docs\screenshots\market.png --shot-delay 8 --mute`. Check a
narrow (1000 px) and a wide window whenever a layout changes.

## Checks

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI (`.github/workflows/ci.yml`) runs both on `windows-latest`, `ubuntu-latest` and `macos-latest` for every push to
`main` and every pull request; keep them green. Tests that need the internet are `#[ignore]`d:
`cargo test -p nexus-app untouched -- --ignored --nocapture`. The Spansh planners can be exercised live with
`cargo run -p nexus-spansh --example smoke -- --only riches,exobio` (all steps without `--only`).

## Developer tools

Each crate ships examples that work on your own journals and database:

| Command | What it does |
|---|---|
| `cargo run --release -p nexus-journal --example scan_journals` | Parse statistics over your journal folder. |
| `cargo run --release -p nexus-model --example summary` | Everything the model knows about your commander. |
| `cargo run --release -p nexus-journal --example replay -- <journal> <dir> [speed]` | Replay a journal into a folder to simulate a live session. |
| `cargo run --release -p nexus-model --example predictions` | Exobiology predictions compared with what you actually sampled. |
| `cargo run --release -p nexus-uplink --example validate` | Every upload your journals would produce, checked against the EDDN schemas. Sends nothing. |
| `cargo run --release -p nexus-galaxy --example seed_journal -- <db>` | Build a galaxy database from your journals. |
| `cargo run -p nexus-eddn --example live -- <db> <seconds>` | Watch the live EDDN feed flow into a database. |

## How it stays fast

Elite Nexus is pure Rust: no Python, no Node.js, no browser engine, one executable with a GPU-rendered interface
(egui/wgpu). Measured on one development PC:

| | Elite Nexus |
|---|---|
| Full journal history load | **~40–60 ms** for 16.5k events (parallel parse, ~260 MB/s, 600k events/s); EDDiscovery takes minutes on big histories |
| Live latency | journal + Status.json polled every **100 ms** (no missed writes, no notification lag) |
| Market data | **direct EDDN firehose** (~550 msgs/min) into a local SQLite database: prices the moment any commander docks, before websites ingest them |
| Market queries | best price within 50 ly in **~1 ms**, galaxy-wide best price 0.2 ms, trade routes in 8–22 ms on an 80k-station / 4.4M-price database |
| Startup to usable | about one second |

Keep it that way: long work (parsing, database queries, HTTP) runs off the UI thread and is polled each frame, and
the galaxy database has one writer thread with pooled readers. `docs/ARCHITECTURE.md` describes the crates and threads.

## Reporting a bug

Open a [GitHub issue](https://github.com/uruskan/EliteNexus/issues) (questions and ideas go to
[Discussions](https://github.com/uruskan/EliteNexus/discussions)) with:

* the version (Settings › About), your OS, and how the game runs there (natively, Proton, Wine, CrossOver, Whisky);
* what you did, what you expected and what happened;
* `nexus.log` and, if the app closed unexpectedly, `crash.log`, both from the data folder (Settings › About › Data
  folder › Open). Neither is sent anywhere by itself.

What works on Linux and what doesn't yet is collected in [issue #3](https://github.com/uruskan/EliteNexus/issues/3).

## Sending changes

* Read [AGENTS.md](AGENTS.md) first: the conventions (no `unwrap()` on external data, `#[serde(default)]` on external
  structs, UI text that says where data comes from and what it can't show, limits that are real, uploads that stay
  opt-in), the build and UI checks, commits and releases.
* Keep pull requests to one logical change. Commit subjects are imperative and about 70 characters, often "Area: what
  changed"; the body says what and why in plain words.
* When behaviour changes, update `docs/USER_GUIDE.md` and `CHANGELOG.md` (under "Unreleased") in the same commit. The
  user guide is compiled into the in-app assistant's knowledge base, so it has to stay true. The website in `site/`
  and its `llms.txt` describe features too.
* Build pages from `widgets.rs` and `theme.rs` (no raw egui styling, no text below 12 pt), and check the UI with a
  screenshot run at a narrow and a wide size.
* Don't run `cargo fmt` over the workspace: much of the code is hand-formatted with long lines. Match the surrounding
  style.
* Never commit `nexus-data`, databases, logs or keys (`.gitignore` covers them; don't force-add).
* Before touching an integration, read its spec in `docs/research/` (EDDN, EDSM, Inara, CAPI, Spansh, the journal,
  engineering, exploration, HUD colours and cockpit lights). The live EDDN service differs from its GitHub docs, for
  one.
* Contributions are licensed under GPL-3.0-or-later like the rest of the code. Data taken from community sources is
  credited in [NOTICE.md](NOTICE.md); add a row there when you bring in more.

## Releases

Pushing a tag such as `v0.7.2` runs `.github/workflows/release.yml`: it builds the Windows exe, the Linux binary (on
Ubuntu 22.04, so the binary starts on older distributions) and the Apple Silicon Mac app, packs each with the README,
NOTICE, licence and user guide, and publishes a GitHub Release with that version's `CHANGELOG.md` section as the notes. A
hyphenated version (`v0.8.0-beta.1`) becomes a pre-release. The steps are in AGENTS.md › Releases.

## Accounts and APIs

* **Inara** only accepts whitelisted app names; it approved "Elite Nexus" on 2026-10-13, so a user's personal API key
  is all sync needs. Developer mode (`isBeingDeveloped`) is still on by default; see `docs/research/inara.md`.
* **Frontier Companion API (CAPI)** needs a client ID approved by Frontier (`docs/research/capi.md`). Not supported
  yet; the journal provides almost everything.
* **EDDN** uploads are validated against the schemas in `docs/research/eddn-schemas/` and must stay strictly
  compliant: `cargo run --release -p nexus-uplink --example validate` checks every message your journals would send.

## Where things are

* `AGENTS.md`: working rules for this repository.
* `docs/ARCHITECTURE.md`: crates, threads, the design system, code conventions.
* `docs/USER_GUIDE.md`: every page explained (also the in-app assistant's knowledge).
* `CHANGELOG.md`: what changed in each release.
* `site/`: the website, published to GitHub Pages by `.github/workflows/pages.yml`, with `llms.txt` for AI assistants;
  `tools/site/` builds and checks it.
* `docs/research/`: verified specs for every integration, and a feature matrix of the tools Elite Nexus replaces.
* `NOTICE.md`: third-party data and fonts, with their licences.
