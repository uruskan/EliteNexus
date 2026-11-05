# Changelog

What changed in each release, newest first. Add user-visible changes under "Unreleased" as you make them; at
release time that section becomes the new version and the release workflow publishes it as the release notes.

## Unreleased

### Added
- **Combat**, a new page (under Commander) for commanders who want a fight without a bounty. **Fights near you**
  lists the nearest wars and civil wars, the fights with conflict zones: how far, active or pending, the two
  factions with the days each has won and what each stands to lose, the system's government, how old the report is,
  and a **Plot route** button. Choose how far to look and how old a report may be; elections can be shown
  too. Your current system comes first, with its wars, the combat sites your scans logged there and a warning in an
  anarchy. The list comes from the galaxy database, which now keeps each system's conflicts as the game reports them
  through EDDN and your journal, so it fills while Elite Nexus runs, and the page says how many systems it has fresh
  reports for. **How to fight legally** explains where killing isn't a crime and how to pick a side, what makes you
  wanted, and when you end up in the detention centre. Ask can answer "where's the nearest war?" from the same list.
- **AI agents** (Settings › AI agents, off by default): Claude Code, Claude Desktop and other AI programs that speak
  MCP can use the tools Ask uses, to read only: your commander, fleet, materials, ranks and engineers, journal, the
  market database, engineering, where to mine or buy, and Spansh. Copy buttons set them up. The program the agent
  starts (`EliteNexus --mcp`) opens no window and reaches the Elite Nexus you have running over a pipe on this
  computer, never the network; it answers one call at a time, at most 30 a minute. The user guide's Data & privacy
  section lists what an agent can read.
- **Ask** says what it can see and do: **What I can see and do**, under the suggestions or behind the list icon at the
  top, lists every tool it has, from the same list its model gets, grouped by what it reads, what it asks Spansh or
  EDGalaxyData, and what it changes (route plans for Navigation), with what AI agents may read besides.
- **Ask** can be stopped: while it answers, the round button becomes **Stop**, and **New conversation**, now there
  while it works too, stops it as well. A slow model no longer leaves you waiting with no way out, and a stopped
  question changes nothing, not even a route it was planning.
- **Ask** knows more: it can list your whole fleet (where each ship is, even when you left it to ride an Apex taxi,
  when a transfer arrives, jump range, cargo, value, rebuy and the engineered modules with their blueprint, grade and
  experimental effect, from the last loadout the game reported for that ship) and tell you how far you are on every
  rank, which engineers you've unlocked, your superpower reputation, Powerplay, squadron, and whether the game is
  running or at the main menu. It also knows your engineering materials (each with its grade, storage cap and whether
  it's full), by their English names on a game client in another language too, and where to find a raw material: the
  landable bodies that hold it most, or the nearest, within a distance you name (100 ly unless you say otherwise),
  from the scans commanders shared that Spansh holds.
- **Ask** knows engineering, from the Engineering page's data: name a module and it lists the blueprints, experimental
  effects and engineers for it; name a blueprint and it works out what each grade takes against the materials you
  hold, what you're short of, the material-trader swaps that cover it (as the Blueprint planner does) and which
  engineers offer that grade, how far they are and what unlocking them takes. It starts from the grade your current
  ship's module already has of that blueprint, unless you say otherwise.
- **Ask** searches where to surface-mine with the Rhino, as Mining › Where to mine does: landable bodies with
  planetary mining locations near you (nearest first, or the most locations first) or the best system in each stretch
  of the way to a system, by class and volcanism, with what your own runs gave on each ground. It says where the
  counts come from and that nothing records what a location holds.
- **Ask** finds where to buy a build, as Fleet › Where to buy does: the nearest stations that sell all of one of your
  ships' builds (the one you fly unless you name another), a pasted build, some of its modules or a hull ("where can
  I buy a 6A FSD?"), with what it costs there and how old the station's list is; or, when no station has everything,
  the stops that cover it between them. Modules in your storage and a hull you own aren't searched for.
- **BGS & Powerplay** shows the merits you earned in each system, on the system's card beside its factions'
  influence, bonds, profit and data (asked for by NetworX on the Frontier Forums). The journal doesn't say which
  system a merit award counts for, so each award is placed in the system you were in when the game awarded it. The
  page is read from your journal, so merits you earned before this version are placed too.
- **Engineering**: Merc blueprints, bought from engineers with Merc Coins, are in the blueprint planner: Extended cargo
  racks, Heavy Duty module reinforcement, Balanced and Support Focused power distributors, the Merc weapons, seeker
  racks and mining tools, Plasma Conversion for lasers and Scoop Rate Enhanced for fuel scoops. Each grade shows its
  Merc Coins per roll, and the shopping list adds them up against your balance at your last login.
- **Engineering** and **Missions**: where to get what you're short of (asked for by nhok0169 in GitHub issue #4). In
  the blueprint planner and its shopping list, the pin at the end of a material you're short of opens, under it,
  the nearest material trader of its kind, for a raw material the three landable planets within 100 ly richest in
  it (from Spansh), and for a commodity the nearest markets that sell it and rings with hotspots for it. The Missions
  page lists the goods your missions ask you to source, mine or donate, each with the same pin. Every answer says
  where it comes from (Elite Nexus's market database, or Spansh's shared scans) and what it can't know, and a button
  opens the full list on the Market page.
- **Fleet** shows the landing pad each ship needs (S, M or L), in the ship list and on its Loadout card (asked for by
  Alvarrune on the Frontier Forums).
- **Fleet › Where to buy**: paste a ship build (a SLEF export from EDSY or Coriolis, a journal Loadout or an import
  link) or pick one of your ships, and Elite Nexus finds the nearest stations that sell all of it, modules and hull:
  starports, outposts, settlements and fleet carriers with a pad your ship fits. Modules in your storage and the
  bulkheads a new hull comes with start unticked, and unlocks (tech brokers, Powerplay) are marked; a hull no station
  prices in credits is flagged as possibly ARX-only. Every station shows how old its outfitting list is and what the
  build costs there, with a route button. Spansh can't tell a fixed weapon from a gimballed one, so each station's own
  list is checked for the exact modules. When no station sells everything, it suggests two stops, or the nearest
  seller of each missing piece. Ships › **Where to buy** opens it with that ship's build.
- **Fleet › Where to buy: one ship or module.** Type a ship or a module ("Anaconda", "6A FSD", "gimballed
  multi-cannon"), pick it from the suggestions, and Elite Nexus finds the nearest stations that sell it, with its
  price there and how old the station's list is. A module is searched where the ship you fly can land; a hull, where
  it can. A whole build is one click away, under **A whole build**.
- **Market** › Find commodity: a **Category** field beside the search, and a ▾ on both that opens the whole list. The
  category narrows only the list the search field's ▾ opens: typing still finds every commodity. A category's rare
  goods are one item, **Rare Goods**, and the category **Rare Goods** holds them all: one page lists
  every place selling or buying any of them, each row naming its rare good and comparing it with that good's own
  average. Rare goods are marked ★ in yellow, and the lists name the commodity in their first column. Clicking into
  either field selects its text; Tab takes the highlighted entry. The retired event rares (Classified Experimental
  Equipment, Galactic Travel Guide) are left out.
- **Market**: fleet carriers say who may dock right in the lists (Find commodity, Abnormal prices, Sell my cargo,
  your pins and the Station tab's carriers): **open**, **friends**, **squadron**, **squadron, friends** or **owner
  only**, as last reported with the carrier's market, or **?** when no report has said. Until now it showed only
  after clicking a row (asked for by hangerhead in GitHub discussion #1). The owner can change it at any time, so
  it may differ when you get there.
- **Market** › Find commodity: click a fleet carrier's row and it says how long its order's price has stood, up to
  the carrier's latest report: "Price unchanged in every report from 23 Oct to the latest, 4 Nov" (with what the
  report before had), "For sale" or "Wanted at this price" from a date when the report before had none for sale or
  no buy order for it, or unchanged as far back as Elite Nexus's price history reaches. It keeps 30 days of price
  changes, from the first report of that carrier it received; neither the game nor the live feed says when an order
  was set up (asked for by hangerhead in GitHub discussion #1). The price history now also records when a commodity
  first appears on a fleet carrier's market.
- **Market** › Trade routes, while you're docked: **From this station** lists only the routes that start there, what
  to buy at this station and where to sell it within the distance you set (asked for by NetworX on the Frontier
  Forums). Open the commodity market in the game first, so Elite Nexus has the station's latest prices.
- **Mining** › Surface radar numbers the deposits (an idea from Chequers2): 1, 2, 3… on each body in the order the
  Rhino records them, kept between runs, beside each deposit on the radar, in the list under it and on the overlay's
  radar. Each deposit in the list takes a note of your own, such as how many rigs fit, so two deposits of the same
  commodity can be told apart. Deposits kept by 0.7.3-beta.1 are numbered too, oldest first.
- **Mining** › Surface mining sites: a Rhino dropped from a hovering ship, for which the game names no location, takes
  the planetary mining location you had targeted. It's read from Status.json, so only while Elite Nexus runs, and kept
  for later starts.
- **Overlay: where the route ends.** On a plotted route (the galaxy map's, or one planned on Navigation), the open
  island names the route's last system and how far it is in a straight line ("Orrere · 23.4 ly away"), under the
  next jump, and the small island adds the distance to the jumps left ("2 jumps · 23.4 ly"). It showed only the
  next jump and the jumps left.
- **Overlay: docking granted.** When a station grants you docking, the island opens with your landing pad and the
  station, as it did only when docking was denied, then keeps the pad ("Pad 36") until you dock, the permission
  runs out or you leave. The banner in Elite Nexus says it too. A denied request now says why in plain words ("too
  far away: ask again within 7.5 km") instead of the game's code ("Distance"). An alert that comes while a cockpit
  panel is open (docking is usually requested from one) now waits until the panel closes, instead of running out
  while the island is kept small.
- **Settings › Follow my flight** (off by default) changes the page as you fly: it reads what you're doing from
  the game's Status.json and switches when that changes. Flying a system opens Exploration; getting below
  **Planet view below** (150 km by default) over a
  planet, landing, or stepping outside on foot opens Exobiology when the planet has biological signals you haven't
  finished analysing (Exploration otherwise); climbing back above it returns to Exploration; the galaxy map, a
  charging hyperspace jump or a finished system (every body found, every planet worth the high-value threshold
  mapped, every genus analysed) opens Navigation. Exploration opens on the current system and Exobiology on This
  system, whichever tab you left open there. It only ever moves between those three pages: on any other page it
  leaves you alone; docked, in an SRV or on foot inside, only opening the galaxy map changes the page, and it reacts
  only to what happens while the game runs: never to the journals read at start, nor to loading another commander.
- **Settings › Screenshots** (off by default; asked for by Alvarrune on the Frontier Forums): **Save screenshots as
  PNG** saves each screenshot you take while Elite Nexus runs (F10, or Alt+F10 at high resolution) again as a PNG,
  every pixel of the game's uncompressed BMP kept, named after the system, the nearest body and the time, such as
  `Sol (Earth) 2026-11-04 14-32-05.png`. It goes next to the BMP or into a folder you choose, never over another
  file, and **Delete the BMP afterwards** removes the BMP once its PNG is saved. Screenshots already in the folder
  stay as they are.
- **Mining** › **Where to mine** (asked for by NetworX on the Frontier forum): which bodies are worth landing on for
  Rhino surface mining. **Near you** lists landable bodies with planetary mining locations within 25 to 1,000 ly,
  nearest first or most locations first, with each body's ground (its class and its volcanism, as the game reports
  them), its number of mining locations, its distance from the star and its gravity. Narrow it to a class
  (metal-rich, high metal content, rocky, rocky ice, icy), a volcanism (none, metallic or rocky magma, silicate
  vapour geysers, water magma or geysers, carbon dioxide geysers, ammonia, methane or nitrogen magma) and a least
  number of locations; **Tour 10 systems** plans a route through the first ten systems. **Along a route** finds
  stops on the way to a destination (the end of your route, or one you type): the system with the most mining
  locations in each stretch of the trip, up to 1,000 ly from the line, then **Follow this route** puts them on the
  Navigation page. **In this system** lists the landable planets your own scans described, and **Your grounds** what
  your own Rhino runs gave on each ground. Counts come from scans commanders share through EDDN (EDMC,
  EDDiscovery…), collected by Spansh, and only scans since the Rhino update carry them; no journal or feed says what
  a mining location holds, and the page says so.
- **Speech engine and voice** (Windows, Settings › Voice and alerts): speak through Windows speech, as before, or
  through **SAPI**, Windows' classic speech engine, which also has the voices other programs install for it (IVONA,
  CereProc, …). **Voice** picks one of the engine's voices instead of always the Windows default.
- **Back to your carrier**: Navigation › Fleet carrier shows where your carrier is (the system the game last reported,
  and when) and how far that is from you, with **Route my ship here**: one click plans every jump there for your ship.
  While the carrier has a jump scheduled, **Route to** its destination is there too. The Fleet carrier page has the
  same button.
- **Fleet Carrier Administration**: Navigation › Fleet carrier lists the stations with that service nearest your
  carrier, or nearest you, from Spansh's station search, with the nearest picked: plan a route there for your ship or
  your carrier in one click, or pick another station. Without Spansh it falls back to the local database and says what
  that one knows.

### Changed
- **Statistics** is rebuilt in three tabs. **Career** turns the game's own statistics into plain words: time in the
  game and what you earned an hour, your wealth, the distance you jumped; where your credits came from and where they
  went, as bars of their shares; a card per activity (trade, combat, exploration, exobiology, passengers, on foot,
  engineering, fleet, carrier and more) with its rank and only the figures that aren't zero; your records, and the
  round numbers coming up on your counters. **Activity** counts what your journals hold over the period you pick:
  time in the game, money in and out by category with trade counted by its profit (and cargo you buy but don't sell,
  such as a construction's, at cost), a chart by day of jumps, light years, credits or time, a card for each thing you
  did, and arrows against the period before. An empty period says why. **All statistics** replaces the old list of the game's raw names, mostly zeros:
  every figure in plain words, grouped, searchable, with the zeros put away.
- **Engineering** › Blueprint planner is easier to read. Pick the grade to reach and the rolls per grade with plain
  controls, and it says at once what you're short of, with what you already have folded into one row. Each grade's
  recipe and the engineers who can do it are one click away, and the shopping list sits under the wishlist.
- **Engineering** › Suits & weapons and Synthesis read the same way: each recipe (a grade, a modification, a
  synthesis) is one row that says Ready or how many materials you're short of, with its price or effect, and opens
  into each ingredient against what you have, instead of a run of "3× Vanadium (0)" in coloured brackets.
- **Market**: the four lines of live-feed messages under every tab are gone, so each tab starts higher up. The
  **Live** badge stays, with the feed's rate and the database's size beside it.
- **Fleet › Where to buy: a build for a ship you own is a refit of it.** Modules already fitted on your ship of that
  type count as yours, like those in your storage, and start unticked ("Fitted on your Python"). A build of one of
  your own ships says you have all of it, instead of listing every module to buy again. Ask's where to buy counts them
  the same way.
- **Fleet › Where to buy says it in plain words.** It asked for "a journal Loadout event"; it now names what to paste:
  a build exported from EDSY or Coriolis, a link from either that opens a build, or one of your ships' builds copied
  on the Ships tab. The search's settings (near which system, fleet carriers, how old a station's list may be) are
  all in view instead of behind **Options**, and changing one searches again.
- **Fleet › Where to buy offers every ship to start from.** It offered only ships whose modules the game had recorded,
  one of seventeen in a play-test. The others are listed too, greyed, saying why: the game writes a ship's modules to
  its journal only while you fly it.
- **Navigation** says what each planner is for: four cards replace the tabs. **Exact plotter** plans every jump for
  your ship and suits most trips; **Neutron** is for long trips, with only a jump range and no fuel planning; **In-game
  route** says it mirrors the galaxy map's route, and that it's empty because nothing is plotted there. The planners'
  settings are in view under them instead of behind **Options**, and the Fleet carrier planner, which had none, no
  longer offers an empty Options. **From** follows you as you jump (the carrier planner's follows the carrier) until
  you type another system.
- **Exploration** › Current system lists a system's signals by kind (stations, installations, megaships, conflict
  zones, carriers…) with counts, plain names and callsigns, in columns that use the page's width, and says where they
  come from. Signals kept from earlier visits show when they were seen, since carriers come and go. Click one to copy
  its name.
- **Exploration** › Find valuable bodies shows its settings in a bar under the search, each once: Within, Stops, Jump
  range, Bodies worth at least, Furthest from the star, and Values (if you map them, or from the scan alone). Type a
  value or step it with − and +.
- **Inara**: Developer mode is gone from Accounts & sync (Inara options). Elite Nexus no longer marks what it sends
  as "in development", so Inara now takes the community goals you're part of, and your progress in them, as it does
  from any approved app. A settings file that still has the switch reads as before.

### Fixed
- **Exploration** › Current system scrolls as a whole: a long list of signals (124 in a busy system) was cut off at
  the window's edge, and the bodies table scrolled in a box of its own.
- **Engineering** › Materials: the three cards fit the window. Long manufactured and encoded names ("Guardian
  Sentinel Wreckage Components") widened their cards, so at about 1,600 pixels wide the Encoded card ran past the
  window's edge with its counts cut off. A long name now ends in "…" and shows whole when you hover it.
- **Missions** › Recent mission history: a long mission title no longer widens the card past the window. The date,
  outcome and reward come first, and the title takes the room left, ending in "…" (whole on hover).
- **Missions** › Recent mission history: a failed or abandoned mission no longer shows a reward of "0 Cr". It says
  "no reward", or the fine it cost when the game fined you.
- **Market**: the live feed's "updates stored" counts only what the database kept. It counted every item a message
  turned into, so every body scanned anywhere, though from the feed Elite Nexus keeps only bodies with biology,
  Earth-like, water and ammonia worlds and terraformables, and every market report, even one older than the market
  it already had.
- **Ask**: the line under an answer names each source once. Two tools that use the same source (Spansh) made it read
  "Spansh · your journal · Spansh".
- **Ask** answers questions about a page of Elite Nexus from that page's part of the user guide. The guide's section on
  the pages is longer than an answer may be, so Ask was handed only its first part, and most pages' rows never reached
  it. Long sections are now cut to the lines about what was asked. A system with many planets with biology no longer
  makes "Exobiology where you are" too long either: the most valuable come first.
- **Ask**: going back to Gemini or Claude after another OpenAI-compatible service or a local model no longer sends
  your questions, and your key, to that service's address, which made Ask say "The model service rejected the API
  key". Switching provider under "Other providers and model" also clears the old provider's address and model.
- **BGS & Powerplay counts your bounties.** Bounty vouchers you handed in never reached the page, so its Bounties
  column stayed empty: the journal names each faction's share of a voucher differently from the bounty itself, and
  every share was read as 0. Each faction now gets its share, in the system where you hand the vouchers in. The page
  is read from your journal, so bounties you handed in before this version show too. Combat bonds and Statistics
  were counted right.
- **BGS & Powerplay: "All time" says what it counts.** It said "Counting from 01 Jan 02:00", the date computer
  clocks count from (1 January 1970) in your time zone. It now says it counts all your journals, and the date of
  your first activity in them.
- **BGS & Powerplay: a system's factions fit the window.** The table on each system's card had a column for every
  kind of activity, and in a window about 1,000 pixels wide it ran past the edge: Missions failed and Murders were
  cut off. It now has a column only for each kind of activity the system saw in the period, and where even those
  don't fit side by side, each faction lists its activity under its name.
- **Statistics and BGS & Powerplay: every chart labels its side.** The Powerplay merits chart could have "0" as its
  only label when your best day was a few hundred merits, and so could Jumps per day with up to about nine jumps a
  day: the marks went by tens, hundreds and thousands, and the ones close enough together had no room for their
  labels. The charts are now marked in round steps (1, 2, 2.5 or 5 times ten, a hundred, …), as fine as their labels
  have room for. On the Credits chart's **Log scale**, a balance that stayed between two powers of ten (2 to 5
  billion, say) had no label at all; it now gets 2 B, 3 B, 4 B and 5 B.
- **Statistics: the Credits chart's dates are your own.** Its dates stood at midnight UTC, which west of UTC (the
  Americas) is the evening before: your balance from the morning of the 3rd showed just after the mark for the 2nd.
  Each date now marks your own midnight, as on Jumps per day.
- **Statistics and BGS & Powerplay: a bar's number is no longer cut off** (GitHub issue #4, with a screenshot).
  Hovering a tall bar in Jumps per day or the Powerplay merits chart wrote its number above the bar, where the chart's
  edge cut it off (the last day's too). The day and its number now show in a tooltip by the pointer, which always
  fits, and the merits chart no longer slides away from its bars when dragged.
- **Statistics: the Credits chart's axis reads right when your balance stays put.** When your balance didn't change
  over the period, every label up its side was the same ("1.23 B" eleven times): the chart spanned a tenth of a
  credit. A flat or nearly flat balance now sits in a band of a tenth of its value above and below it, with labels
  that differ.
- **Statistics and BGS & Powerplay: per-day charts go by your own days.** Jumps per day and the Powerplay merits chart
  cut the days at midnight UTC and named each after your date when that midnight came, which west of UTC (the
  Americas) is the day before: most of a day's jumps and merits showed under the previous date, on the axis and when
  you hovered a bar. Both now count your calendar days, midnight to midnight on your clock, under the dates it shows,
  and the merits chart no longer writes the same date on every tick when your merits fit in a day or two.
- **Fleet: long module names stay in their column** (GitHub issue #4, with a screenshot). A name such as "8A Mk II
  Supercharge Optimised Frame Shift Drive (SCO)" ran over the module's value. The Module column now widens to the
  longest name when the window has room, and a name that still doesn't fit ends in "…" and shows whole when you hover
  it.
- **Fleet: engineering badges stay inside the loadout table.** In windows about 1,250 to 1,470 pixels wide, a module
  with a long blueprint and an experimental effect ("G5 Power Distributor High Frequency" and "Stripped Down") ran
  past the table's edge. The Engineering column now takes the room its badges need, and where the two don't fit side
  by side, the experimental effect goes under the blueprint. The ship list sits beside the loadout only once the
  window leaves the loadout enough room; in a narrower window the ships are a row of names above it.
- **Fleet: EDSY opens a build pasted from Coriolis.** On Where to buy, a build pasted as Coriolis's SLEF export opened
  in EDSY as "Import failed: Unrecognized JSON format / No builds found": Coriolis leaves out the field that marks a
  ship's build (`"event":"Loadout"`), and EDSY looks for it. The EDSY and Coriolis buttons now always send the build
  marked as one, on Where to buy and on the Ships tab alike.
- **Fleet › Where to buy lists every module of a Type-11 Prospector, Panther Clipper Mk II or Anaconda Mk II.**
  Modules in slots that take only one kind of module (the Type-11's mining hardpoints, limpet controller and hangar
  slots, the cargo slots of the Panther Clipper Mk II and Anaconda Mk II) were left off the shopping list as if they
  came with the ship, and Ask didn't see them in a ship's loadout either. They're listed now, under Hardpoints and
  Optional internals, and the Ships tab names those slots ("Large mining hardpoint 1", "Limpet slot 1").
- **Fleet: a ship's name and its buttons no longer overlap.** In a narrow window the Loadout card ran a long ship
  name under its Where to buy, Copy JSON, EDSY and Coriolis buttons. Where the two don't fit side by side, the
  buttons now sit under the name.
- **Fleet: engineering badges use the game's names.** A blueprint showed as the journal writes it, "G5 FSD Long
  Range" or "G5 Power Distributor High Frequency", where the game, the Engineering page and Ask say "G5 Increased FSD
  Range" and "G5 Charge Enhanced". An experimental effect the journal gives only as a code (special_fsd_heavy) now
  shows its name too (Mass Manager), on the Fleet page and in Ask.
- **Missions and Statistics say why a wingmate's shared mission isn't counted** (GitHub issue #4). Your own missions,
  wing missions included, are counted as always. When a wingmate hands in a mission they shared with you, though, the
  game writes nothing about it in your journal, reward included, so there is nothing for Elite Nexus to count. The
  Missions page and Statistics › Income and spending now say so; the credits still show in your balance.
- **Missions: the Active missions table fits the window.** Below about 1,450 pixels wide it was wider than the window:
  Expires and Progress were cut off, and the cards under it ran past the window's edge too. Its columns now fit any
  window size, and a mission, faction or destination too long for its column ends in "…" and shows whole when you
  hover it.
- **Ask can look back through your whole journal** (GitHub issue #4). Its journal search returned only the newest 15
  matching lines (40 at most, fewer when they were long), so a question about last week's jumps saw only the last day
  or two, and nothing said so. It can now search a period you name ("where did I go from 22 to 29 October?") and page
  back through it, says when it left older matches out and from which day your journal folder starts, and gives times
  in your local time.
- **Mining** › Surface mining sites: a Rhino run in which you sent your ship away (or called it back) is credited to
  the mining location your landing named. When the ship lifted off or landed by itself without naming a location,
  Elite Nexus took that for a landing far from every location and listed the run as "location not named". The list
  is read from the journal, so earlier runs move to their location too. The game names a location only when the ship
  touches down or lifts off within 50 km of one, so a Rhino dropped from a hovering ship gets no number from the
  journal (it takes your target instead, see Added). An unnamed site now says so: target the location, or touch down
  by it, before you deploy. A drop after you lifted off is no longer credited to the location you left. A body you
  mined where the game named no location no longer says "you've worked 0": it says "at least 1". Reported by
  Chequers2.
- **Navigation**: once every stop of a planned route was reached, the page kept showing "Route complete" and hid the
  route plotted in the game afterwards (so did "Copy the next system", the auto-copy after a jump and the overlay's
  next jump). The game's route now takes over, under the finished tour's summary.
- The **Anaconda Mk II** is known by name, with its bulkheads, and as a large ship: until now the Fleet page showed
  its raw game name and market searches offered it stations with only small and medium pads.
- **Fleet: the ship you fly is where you left it, not where you are.** Once you stepped out of it and took an Apex
  taxi or a dropship, or crewed another commander's ship, the Ships list put your ship wherever you went. It now
  shows where you left it (on your own or your squadron's fleet carrier, wherever the carrier has jumped since). A
  ship you transferred showed "In transit" until the game next listed your stored ships; it now arrives when its
  transfer time is up.
- **Dashboard: no stale rank progress after a promotion.** The game reports your progress towards each rank when you
  log in. Promoted during a session, the Commander card paired your new rank with the progress you had made towards it,
  a bar near full. That rank's bar now stays empty until the game reports it again at your next login, and says so
  when you hover it.

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
