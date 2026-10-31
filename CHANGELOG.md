# Changelog

What changed in each release, newest first. Add user-visible changes under "Unreleased" as you make them; at
release time that section becomes the new version and the release workflow publishes it as the release notes.

## Unreleased

### Added
- **Settings › Follow my flight** (off by default) changes the page as you fly, the way Elite Dangerous
  Exploration Buddy switches its tabs: it reads what you're doing from the game's Status.json and switches when
  that changes. Flying a system opens Exploration; getting below **Planet view below** (150 km by default) over a
  planet, landing, or stepping outside on foot opens Exobiology when the planet has biological signals you haven't
  finished analysing (Exploration otherwise); climbing back above it returns to Exploration; the galaxy map, a
  charging hyperspace jump or a finished system (every body found, every planet worth the high-value threshold
  mapped, every genus analysed) opens Navigation. It only ever moves between those three pages: on any other page it
  leaves you alone; docked, in an SRV or on foot inside it changes nothing, and it reacts only to what happens while
  the game runs, never to the journals read at start.

### Fixed
- Navigation: once every stop of a planned route was reached, the page kept showing "Route complete" and hid the
  route plotted in the game afterwards (so did "Copy the next system" and the auto-copy after a jump). The game's
  route now takes over, under the finished tour's summary.

## [0.7.3-beta.2] - 2026-10-25

A second beta of 0.7.3: the compact view becomes the **overlay**, a small island at the top of the game's screen
that says what's going on and opens up for the moments that need you, and important alerts can also appear in
XSOverlay in VR.

### Changed
- The compact view is now the **overlay**: one small black island at the top of the game's screen, in a window of its
  own beside the main window, instead of the whole window turning into a panel. It says what's going on in a word or
  two ("2 of 3 samples · Far enough", "Scoopable · 12 jumps left", "Carrier → Colonia 7:42"), opens up for the moments
  that need you and fades away when nothing does. Its moments: **Next jump** (opens while the FSD charges, in
  hyperspace and in the galaxy map), **Arrival** (opens when a system is worth it, and in the FSS and system map),
  **Approach** (conditions, the genera there, likely value and where to land), **Sampling** (how many samples and how
  far to walk, with a radar of your earlier samples), **Rhino mining** (the nearest deposit and which way), **Trade
  stop** (a pinned market or carrier in the system), **Docked** (the data this station buys), your carrier's countdown
  and important alerts. It's an empty capsule while a cockpit panel is open, gone in danger, and shows only while the
  game or Elite Nexus is in front. Locked, clicks go through it and it never takes the focus from the game; unlock it
  (the lock in the toolbar) to drag it, and a click then opens its page. Settings › Overlay sets where it sits, its
  size (100 to 250 %), whether it stays when idle, a **see-through** window (after a restart; off, an opaque black
  panel) and a **VR mode** for mirroring it into a headset. The user guide says what each system can do.

### Added
- **Alerts in XSOverlay** (Settings › Overlay, off by default): the important alerts also appear as XSOverlay
  notifications in your VR headset, sent only to XSOverlay on this computer. The user guide lists the tools that
  mirror the overlay into VR (Desktop+, OVR Toolkit, XSOverlay, SteamVR's desktop view, OpenKneeboard, Meta Quest
  Link's Pin to Experience, WayVR).

## [0.7.3-beta.1] - 2026-10-25

A beta of 0.7.3: Rhino surface mining remembers the mining locations you worked, with the number to pick in the
navigation panel to go back, and a surface radar finds their deposits again.

### Added
- **Mining › Surface mining sites** (an idea from Chequers2): every planetary mining location the Rhino refined at, with
  its system, its body and the number the game's navigation panel shows for it (Planetary Mining Location 4), what
  came out there by commodity (by-products included), visits and the last visit. To go back, jump to the system,
  target the body and pick that location. The number is the location the game names nearest to where your ship lands
  or lifts off, read from the journal, so runs from before this version are listed too. Pick a commodity to see only
  the locations that gave it; copy the system or plot a route there; see how many mining locations your scans found
  on that body; keep a note on each. The live session and earlier ones say where they were.
- **Surface radar** on the Mining page and in the compact view, near a body you've mined: the deposits the Rhino refined
  at, your ship and where it landed last time, with distance and compass bearing, "up" the way you're facing. It
  works from orbit and in glide too. Deposit positions come from Status.json while Elite Nexus is running (the journal
  records none) and are kept between runs; rigs aren't in the journal, so they aren't shown.
- A quiet reminder when your ship lands by a mining location you worked before: what you got there and when. It's a
  banner, spoken only if the voice threshold is set to Low.
- **Ask** knows your surface mining sites ("Where did I mine Iridium?") and the basics of Rhino surface mining.

## [0.7.2] - 2026-10-24

A hotfix: Exobiology no longer promises the first-logged bonus (×5) for species that won't get it.

### Fixed
- **Exobiology** no longer promises the first-logged bonus (×5) for nearly every species you analyse. It used to
  trust the game's `WasLogged: false` alone, which doesn't mean the bonus is coming: across one commander's sales it
  was false for 67 species and the bonus came for 2, never in a populated system. The bonus now counts only in an
  uninhabited system, on a body nobody had set foot on when you first scanned it, for a species not marked as logged,
  as the user guide always said. A species nobody sold on an already-visited body can still pay it; that shows up as
  a good surprise at Vista Genomics rather than a promise the sale breaks. Thanks to Nillecram for the analysis and
  the fix.

## [0.7.1] - 2026-10-24

A small fix: the BGS & Powerplay page now remembers your tick hour.

### Fixed
- **BGS & Powerplay** remembers the tick hour you set and the period you picked; the tick hour used to go back to
  13:00 UTC at every start. Thanks to NetworX on the Frontier Forums for the report.

## [0.7.0] - 2026-10-23

Elite Nexus for Mac, and fixes for what commanders reported after 0.6.0: a settings file that can't be read no
longer costs you your settings, the journal folder can be set right where a journal problem is shown, the in-game
route counts down again, the window no longer freezes with the live market feed on, and a crash log says what went
wrong if it ever closes unexpectedly. Thanks to leodesca01 for the Mac port, and to everyone who reported these on
the Frontier Forums and GitHub.

### Added
- **macOS**: a native `Elite Nexus.app` (Metal rendering, its own icon). Each release now has an Apple Silicon zip:
  unzip it and drag the app to Applications. It isn't notarized, so macOS asks you to allow it once (System Settings ›
  Privacy & Security › Open Anyway). On an Intel Mac, `tools/macos/build-app.sh --install` builds it from the source
  and puts it in Applications. Thanks to leodesca01 for the port. The game runs under NotProton
  (Steam Play for the macOS Steam client), CrossOver or Whisky; Elite Nexus finds its journals in the game's prefix
  in every Steam library (NotProton keeps Proton's `compatdata` layout), then in CrossOver's bottles and `~/.wine`,
  and in Whisky's bottles only when none of those has one (macOS asks before an app looks into Whisky's container).
  Data lives in `~/Library/Application Support/EliteNexus`, never inside the app. Voice alerts use the system's
  `say`. API keys aren't encrypted on macOS (no DPAPI), and the app says so. Cockpit lights under CrossOver is
  untested; the tab names the Steam launch option it may need.
- **Crash log** (after a Frontier Forums report of the window vanishing). A crash is written to `crash.log` in the data
  folder, with where it happened, and the next start says Elite Nexus didn't close normally and where to look. Nothing
  is sent anywhere. Settings › About › Data folder has an **Open** button. A journal entry Elite Nexus can't handle is
  now skipped and listed under Settings › Journal, instead of silently stopping it from following the game.
- Started from inside its zip file, Elite Nexus says so: Windows unpacks it to a new temporary folder each time, so
  settings and the market database didn't last.

### Changed
- The Linux notes (README, user guide, website) say what has been confirmed with the game under Proton so far, and
  link GitHub issue #3, where Linux reports are collected.
- A cockpit-light dimmer still installed by Elite Nexus 0.3.0 (whose patterns crash the game near the newer fleet
  carriers) is pointed out by a notice at start, with a button to Cockpit lights, instead of only on that tab
  (a Frontier Forums report of the game crashing at launch came from such an install).

### Fixed
- **A settings file that can't be read no longer costs you your settings** (a Frontier Forums report). When
  `settings.json` couldn't be read (typically after editing a folder into it by hand with single backslashes, which
  JSON doesn't allow), Elite Nexus started with the default settings and saved them over the file within half a
  minute: API keys, the journal folder and everything else were gone, without a word. Now the file is kept as it was
  (`settings-unreadable-<date>.json` in the data folder), every setting that still reads is kept (from a file that
  isn't JSON at all, the EDSM and Inara keys, and the Ask key with the service it belongs to; uploads stay off until
  you switch them on again), and a banner
  above every page says what happened and how JSON writes a folder. A file that can't be opened at all isn't saved
  over until the next start.
- **A journal folder that can't be read no longer locks you out of Settings** (a Frontier Forums report: a laptop
  reading the main PC's journals over the network). When the journal folder couldn't be read, or held no commander,
  every page showed only the problem, Settings included, so the folder couldn't be changed. Now the journal folder
  setting (with Browse…) is right under the problem on every page, Settings opens as usual, and the message says
  plainly that the folder doesn't exist on this PC and what to do when the game runs on another PC.
- **Navigation's in-game route counts down again** (a Frontier Forums report). After the first jump, the system the
  route was plotted from came back as the next stop: the remaining distance grew instead of shrinking, distances were
  measured from that system, and "copy the next system" copied it after every jump. Arriving on the route now leaves
  every system up to there behind, and the Dashboard's jump count no longer counts the system you plotted from.
- **The window no longer freezes for a second or two with the live market feed on** (GitHub issue #4). While the
  Market page was open, the database counters it shows were counted on the window's own thread every two seconds, and
  one of them, the price history, grows with every live price change: after a few weeks, counting it froze the
  window. The counters are now counted in the background every ten seconds, and the price history count is kept as
  prices are stored. A frame that still takes over a quarter of a second is noted in `nexus.log` with its page.
- **Market › Services shows results as soon as it opens** (GitHub issue #4). It searched only when a service was
  clicked, so the first visit showed nothing, and the list stayed where you were when you clicked. It now searches when
  it opens, when you pick a service or pad, and after every jump, and says so when no known station has the service.
  It has its own landing pad filter; before, it quietly used the pad picked on other Market tabs.
- **Linux: Test voice says why it's silent** (GitHub issue #3, Nobara on Wayland). When speech-dispatcher's
  `spd-say` was missing or failed, alerts stayed silent and nothing said so. Settings now shows, under Test voice,
  which program spoke, that none is installed, or what the program said when it failed (speech-dispatcher also needs
  an output module, such as `speech-dispatcher-espeak-ng` on Fedora and Nobara). Without `spd-say`, Elite Nexus uses
  `espeak-ng` or `espeak`, and a speech program that hangs no longer holds up the alerts after it.
- **Engineering › Synthesis: the Basic grade of every recipe no longer draws over itself** (GitHub issue #4, with a
  screenshot). Basic grades have no effect text, and the empty line sent the materials back over the recipe's name.

## [0.6.0] - 2026-10-18

A live jump timer for your fleet carrier and your squadron's, colonisation that keeps other commanders' depots
apart from yours, journal and game folders you can type or pick, and a Sponsor link. If you tried
0.6.0-beta.1, this is the same plus the Sponsor link.

**Linux:** the Linux build is built and tested on Ubuntu, but nobody has tried it yet with Elite Dangerous itself
running under Proton or Wine. If you play on Linux, please say what works and what doesn't in a GitHub issue.

### Added
- **Fleet carrier jump timer** (asked for on the Frontier Forums): a live countdown to your carrier's scheduled jump
  (time left, destination, departure time, until when you can cancel, when the pre-flight lockdown starts), then the
  cooldown until it can jump again. It's on the Fleet carrier page, next to the Navigation › Fleet carrier planner and
  on the Dashboard. With a carrier route active, Navigation also shows the next leg and an estimate of the time to the
  destination, and the route ticks off its stops as the carrier arrives, wherever you are. The departure time comes
  from the journal; the 5-minute cooldown is Frontier's figure, and the timer's tooltip says where each number comes
  from and what it can't know.
- **Game folder setting** (Settings › Game, and **Choose the game folder…** on HUD colours, Cockpit lights and
  Bindings when the game isn't found): the folder holding the game, its Steam, Frontier or Epic folder, or its
  `Products` folder. HUD & cockpit and Bindings' built-in presets use it.
- **Sponsor link**: Settings › About, the README and the website link the developer's GitHub Sponsors page. Elite
  Nexus stays free with no ads; sponsoring is optional.

### Changed
- Cockpit lights says up front that the game gets every press of F9, F10 and F11 too, so a key your controls use on
  its own does both.

### Fixed
- **Squadron carriers and decommissioned carriers** (reported on Reddit): a commander who had decommissioned their
  own carrier and now uses their squadron's saw the old carrier marked "DECOMMISSIONING" forever, and never the
  squadron carrier, even as its owner. Elite Nexus now reads decommissions from the journal: a carrier is shown as
  decommissioning with its scrap date and refund, and as gone once that date has passed. The squadron carrier is
  tracked on its own (stats, position, jump timer, orders, jumps) and shown on the Fleet carrier page (a switch picks
  which carrier when you have both), on the Dashboard, on the Galaxy map and in Navigation's fleet carrier planner,
  which plans with the picked carrier's tritium and cargo and Spansh's squadron carrier mass. The journal records only
  the jumps and orders you make yourself, and the pages say so for squadron carriers. Credits you give a squadron
  carrier now count as spending (they're the squadron's), unlike transfers to your own carrier.
- The Windows download's checksum file (`….zip.sha256`) ends its line the Unix way, so `sha256sum -c` checks it on
  Linux too; it used to report the file as unreadable although the checksum matched.
- **Colonisation: a depot you only docked at no longer becomes your construction for good** (a Frontier Forums
  report: someone else's colonisation ship at 0 %, in a system the commander never claimed). A depot is now placed
  only by the station you're docked at with its market ID, never by wherever you happen to be. Your constructions
  (in a system you claimed, or that you delivered to) come first; depots you only docked at are kept apart, closed
  by default. A depot not updated for four weeks is marked out of date (the game updates one only while you're
  docked there), and the eye button hides any depot (Hidden, at the bottom of the page, brings it back).
- Station names such as `$EXT_PANEL_ColonisationShip; Hind's Pride` show as "Hind's Pride" in the situation line,
  the activity feed and the Colonisation page.
- **Settings › Journal › Custom journal folder can be typed in again.** Spaces vanished as you typed, so only a
  pasted folder worked (every journal folder has spaces: "Saved Games"). The text now stays as typed and is read when
  you press Enter or leave the field; quotes from Explorer's "Copy as path" are removed. A **Browse…** button picks
  the folder. Elite Nexus then looks for journals in it and reads them at once (using the game's folder inside if
  you picked one above it); a folder without journals is saved, and the page says the old folder stays in use until
  Reload journals. Network folders (`\PC\…`) work as before.
- **HUD colours, Cockpit lights and Bindings find the game when Steam is installed on another drive.** Elite Nexus
  read Steam's library list only from Program Files; it now also asks the registry where Steam is, and looks at
  Windows' list of installed programs (Steam's and the Frontier launcher's entries for the game). Steam's pre-2021
  library list is read too.
- **Bindings says when it isn't showing the bindings your game uses.** When a preset the game uses couldn't be read
  (typically a built-in preset such as KeyboardMouseOnly while the game's folder wasn't found), the page quietly
  showed another preset, which looked as if your bindings had changed. A warning now names the presets the game
  uses, why one couldn't be read and which preset is shown instead. The page also says that Elite Nexus never changes
  your bindings. Cockpit lights' check of F9, F10 and F11 against your controls uses only the presets the game uses,
  and says when it can't read them.

## [0.5.0] - 2026-10-17

Elite Nexus now runs natively on Linux, next to Elite Dangerous under Proton or Wine.

### Added
- **Linux build** (`EliteNexus-…-linux-x64.tar.gz`, 64-bit; X11 or Wayland, Vulkan or OpenGL). The game runs under
  Proton or Wine; Elite Nexus finds its journals in the game's Proton prefix in every Steam library (native, Flatpak
  and Snap Steam), then in Lutris's and Heroic's usual prefixes. For any other prefix, set Settings › Journal ›
  Custom journal folder: Bindings and HUD colours then use the Options folder in that same prefix. Voice alerts use
  speech-dispatcher's `spd-say`. API keys aren't encrypted on Linux (no DPAPI), and the app says so: the settings
  file is readable only by your user account. Cockpit lights under Proton is untested; the tab names the Steam
  launch option it may need. Settings › About › App menu (or `./EliteNexus --install-desktop`) adds Elite Nexus and
  its icon to your desktop's app menu, in your own folders. On Wayland the compact HUD can't stay on top (Wayland
  doesn't allow it); its button says how to run through XWayland instead. Built and tested on Ubuntu; not yet tried
  with the game itself under Proton.

### Changed
- Bindings finds the game's built-in control presets in every Steam library and the Epic and Frontier folders,
  instead of four fixed Windows paths.

## [0.4.0] - 2026-10-13

A rebuilt Market (a page for every commodity, pins, a run planner, lists you sort by clicking a heading), a readable
Statistics page, and a cockpit-light dimmer that no longer runs patterns inside the game, which could crash it near
the newer fleet carriers.

### Added
- Market: click a column heading (price, demand, distance, updated, ...) to sort any list by it; click again to
  reverse it, a third time for the usual order. Each list remembers how it was sorted.
- **Market › Find commodity is a commodity page**: suggestions as you type ("ltd" finds Low Temperature Diamonds,
  "thar pro" Thargoid Probe, "salvage" lists all salvage), the galactic average, how many markets sell and buy it,
  and three lists: where to buy, where to sell, and the latest prices of every market trading it. Click a row for that
  market's details and this commodity's price history there. An empty list explains why ("Nobody in range buys
  Thargoid Probe"), names the nearest market your filters left out and widens them in one click.
- **Pins**: pin any entry (commodity lists, Abnormal prices, a station's market) and it stays on top of the Market
  page with its latest price, distance and age. In a pinned fleet carrier's system, the pin says whether the carrier
  is really there, from the carriers the game listed since you arrived; the Station tab lists them with their markets.
- **Market › Run planner** fills the pinned orders: where to buy what each pinned buyer wants (cargo aboard first),
  a buyer for each pinned seller, what fits the hold and credits, and the stops in an order that buys before it
  sells. Follow this run puts the stops on the Navigation page.
- Clicking a commodity anywhere on the Market page (Abnormal prices, trade routes, Sell my cargo, a station's market)
  opens its page; clicking a station opens its whole market on the Station tab (formerly This station).

### Changed
- **Statistics** reads at a glance: pick 7 days, 30 days, 90 days or all time and every chart fits that period;
  amounts show as B and M instead of "20000 M Cr"; income and spending are one row per category with their amounts
  (a 30 B trade total no longer flattens the rest), moving credits to your own carrier isn't counted as spending, and
  the credits chart has a log scale for climbs from millions to billions.
- Market remembers your last commodity, recent ones, filters, pins and tab between runs. Find commodity re-runs as
  you change a filter, and its defaults are wider: 100 ly, any quantity.
- Price alerts: the commodity field suggests names as you type.
- **Inara sync works**: Inara approved the app name "Elite Nexus" on 13 October 2026, so your personal Inara API key
  is all it needs (0.3.0 already sends the right name). Accounts & sync no longer reads as if approval is pending,
  and still explains if Inara ever refuses the app.
- Rebuilt the GitHub Pages website with a complete feature directory, dedicated exploration, commander and
  companion pages, and trading content focused on the app's trading tools. Screenshots now open in a full-size
  viewer with zoom and keyboard navigation. Added responsive navigation, a product tour and clearer setup guidance.

### Fixed
- **HUD & cockpit › Cockpit lights could crash the game** when it loaded the newer fleet carriers: the dimmer (and
  the HUD colour protection) ran a pattern inside the game on every shader it loaded. Now a one-time **calibration**
  (start the game, get into your cockpit, quit) finds the shaders it needs among the ones your game loaded, and in
  the game 3Dmigoto only swaps exactly those, by fingerprint; anything else, such as a new carrier, is left alone.
  The tab shows which game build it was calibrated for and asks for a new calibration after a game update (until
  then the dimmer is simply off). Installs from 0.3.0 get **Update and calibrate**, which also puts back a
  d3d11.dll renamed to switch the hook off.
- Accounts & sync: an upload error a service has recovered from since (it accepted uploads again) now shows dimmed,
  with its age, instead of staying red until the next error.
- Market: stations listing a good they produce (the game gives it a token demand of 1, "no demand") no longer show
  as buyers of it in searches, Sell my cargo, trade routes or alerts. Existing data is corrected once on start.
- Market › Find commodity no longer finds nothing without saying why: an unknown name says so, and a search with
  no results shows what is out of range.
- Accounts & sync › EDAstro: uploads no longer pause with "unreadable response" when EDAstro writes its reply's
  status as text (`"200"`), the way EDMC already accepts it; any reply that still can't be read now shows what
  EDAstro sent.
- Accounts & sync › Inara: "Check again", or a new key, within 35 seconds of the last Inara request no longer
  leaves the check spinning until your next sync; it now goes out as soon as Inara's request spacing allows.

## [0.3.0] - 2026-10-13

The HUD & cockpit page: HUD colours that keep hostile red, a cockpit-light dimmer, and glare and exposure. (The
v0.2.3 download already had these, but still called itself 0.2.2 and had no release notes.)

### Changed
- **Elite Nexus is now free software** under the GNU General Public License, version 3 or later: use it, share it
  and change it, as long as what you pass on stays under the same licence with its source. The licence is in the
  download (`LICENSE`), and Settings › About says so.
- Each release now has a `.sha256` file next to the zip, so you can check your download
  (`Get-FileHash EliteNexus-…-windows-x64.zip` in PowerShell shows the same value).

### Added
- **HUD & cockpit** (System), tab **HUD colours**: recolour or dim the HUD with the game's own colour setting. Start from the standard
  orange or a community theme (credited to its author), turn the colour wheel, lower saturation or brightness, or set
  the nine matrix numbers; a live cockpit preview shows the result, the swatches show what hostile red, friendly
  green and shield blue become, and the page warns when friend and foe would look alike or the HUD gets too dark.
  Applying edits only the colour lines of `GraphicsConfigurationOverride.xml`, keeps everything else in it, never
  touches a malformed file, and copies your original aside first. Share themes as the `<MatrixRed>` lines forums use,
  and save your own. The page explains what the setting can't do (cockpit lights, single elements) and warns when
  EDHM is installed, since EDHM needs it left at the standard colours.
- **HUD & cockpit › Glare and exposure**: Odyssey's auto-exposure darkens everything outside when the cockpit lights
  fill the view, and it can't be switched off. This tab sets what players change instead, in the same override
  file: glare from lights (`GlareCompensation`), the standard ACES tone curve (the page shows how much brighter darks
  and mid-tones get) and the auto-exposure targets, with presets for the game's values, less glare, and CMDR
  Morbad's 2021 Odyssey settings. The game's own values are read from its install.
- **HUD & cockpit › Cockpit lights**: dim the cockpit's own lights, which the game has no setting for. Elite Nexus
  installs 3Dmigoto (downloaded from its official release and checked against pinned checksums) with a pattern
  that adjusts the game's light shader as it loads, so lights within a reach of your eyes keep only part of their
  light; stations, carriers, planets and the HUD are untouched. Set the light kept, the reach and on/off in the
  tab or with F9, F10 and F11 in the game (the tab warns when your controls use one of those keys too); Remove
  takes out exactly what was installed, and Delete them clears frame captures and exported shaders left over from
  setting 3Dmigoto up. The game's code is never copied, so most game updates don't break it. Leaves EDHM and other
  3Dmigoto mods alone.
- **HUD & cockpit › HUD colours: Keep hostile red, friendly green and shield blue.** The game's colour matrix moves
  every HUD colour together, so a blue theme turned hostile contacts blue too. With Elite Nexus's graphics hook
  (installed from Cockpit lights), a pattern on the HUD shaders that apply the matrix applies it only to the HUD's
  orange: hostile red, friendly green, shield blue and white keep their colours. The preview, swatches and warnings
  follow the same rule; the thresholds sit under Options, because Frontier documents none of this.

## [0.2.2] - 2026-10-12

### Added
- **Exploration › Find valuable bodies**: Spansh's Road to Riches, moved here from Navigation. Spansh skips
  populated systems, so a search from the bubble used to come back empty; it now widens itself (100, 250, 500,
  then 1,000 ly) until it finds something and says so. Pick Earth-likes, ammonia worlds or rocky and metal-rich
  worlds, how far, how many stops, and whether you'll map planets.
- **Exploration › History**: every system you scanned and what became of the data: sold (and when), unsold, not
  sellable, or lost with your ship. Data sales are recorded.
- **Exobiology › Find planets** with two strategies side by side. **Bonus hunt** looks for planets with no record of
  anyone sampling them and spells out what it found: how many planets suit the species, how many already have
  samples on record, were mapped without the genus, are in populated systems or were landed on. Filters for
  evidence, chance, distance from the star and gravity apply instantly, and **Tour the best 15** plans a route
  through them. **Sure value** (moved from Navigation) lists planets where commanders already logged valuable
  species: certain, but expect the base value.

### Changed
- Navigation keeps the travel planners (neutron, exact, fleet carrier) and links to the pages that plan scan
  tours; every tour, and every route the assistant plans, shows up there.
- "Mapped, no samples" planets (formerly "Mapped, not sampled") are hidden by default, with the reason: a sample
  only leaves a record when it adds a codex entry for that commander.
- Limits follow what the services really allow: Spansh planners take jump ranges of 10-100 ly, a radius up to
  1,000 ly and up to 2,000 stops; market searches reach the whole galaxy; trade routes cover up to 1,000 ly.
- A system you haven't scanned, or a populated one, explains itself on the Exploration page instead of showing
  zeros.
- The assistant's money routes widen their search the same way and say that exobiology values are base values.

### Fixed
- A planned route that starts and ends at your current system (every Road to Riches tour) counted as finished the
  moment it was planned; stops now tick off as you arrive.

## [0.2.1] - 2026-10-12

### Added
- **Exobiology › Find untouched planets**: planets where a valuable species should grow (Stratum Tectonicas by
  default) that no commander sharing data has mapped or sampled, so the first-logged bonus (×5) is likely still
  there. The assistant got the matching `untouched_biology` tool (15 tools).

### Changed
- Accounts & sync explains that Inara accepts only apps it has approved by name, with a link to ask for approval.

### Fixed
- Settings › Text and controls size: the slider ran away to 0.75× or 1.75× while dragged; it now comes in fixed
  steps (90-175 %), and Ctrl + / Ctrl − / Ctrl 0 change it too.
- Long values such as folder paths no longer squeeze a settings row's title to one letter per line.
- The release zip includes the Inter font licence.

## [0.2.0] - 2026-10-12

### Added
- **Market › Abnormal prices**: stations and fleet carriers paying far above (or selling far below) the galactic
  average, 1.5× to 10×, across the whole galaxy or a radius.
- A real **galaxy map**: a painted Milky Way with your travels, route, ship and carrier, search, layers and "Plot
  route" to any system.
- Planets: a footfall-aware bodies table, and the assistant's `system_bodies` tool ("has anyone set foot on
  planet X?").
- Inara: the key check shows which commander it belongs to, and the profile is sent as soon as sync starts.

### Changed
- A redesign after macOS 26: tinted cards, grouped settings rows, segmented controls, a floating glass sidebar and
  a toolbar capsule; every page adapts to the window width.
- The interface is set in Inter, bundled with the app.

## [0.1.0] - 2026-10-12

First release: journal-driven dashboard, exploration, exobiology (sampling radar and species predictions), a
live EDDN market database (commodity search, sell my cargo, trade routes, services, hotspots, price alerts),
Spansh route planners, fleet, engineering, missions, mining, fleet carrier, colonisation, BGS, logbook,
statistics, bindings, opt-in EDDN / EDSM / Inara / EDAstro uploads, voice alerts, a compact HUD and the Ask
assistant (Gemini, Claude or a local model).
