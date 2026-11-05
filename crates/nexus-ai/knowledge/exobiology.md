# Exobiology

## How sampling works
Land near the organism and, on foot, use the Genetic Sampler: the first sample logs the species, then two more samples of the same species are needed, each at least the genus's colony distance away from the previous samples. After the third sample the species is analysed and its data can be sold at Vista Genomics (station concourses, some fleet carriers). Sampling a different species before finishing abandons the current one. Unsold data is lost if the commander dies. Elite Nexus' Exobiology page shows a sampling radar: where earlier samples were, the colony-range circles, and whether the current spot is far enough.

## Colony distance by genus (minimum distance between samples)
Concha, Clypeus, Recepta, Aleoida, Frutexa: 150 m. Tussock: 200 m. Cactoida, Fungoida: 300 m. Bacterium, Stratum, Fonticulua: 500 m. Osseus, Tubus: 800 m. Electricae: 1,000 m. Fumerola and Thargoid organisms: 100 m.

## Most valuable species
Base values (Vista Genomics): Fonticulua Fluctus 20.0M Cr; Tussock Stigmasis, Stratum Tectonicas, Fonticulua Segmentatus and Concha Biconcavis 19.0M; Recepta Deltahedronix, Cactoida Vermis, Stratum Cucumisis, Clypeus Speculumi and Fumerola Extremus 16.2M; Recepta Conditivus and Tussock Virgam 14.3M; Aleoida Gravis, Recepta Umbrux and Osseus Discus 12.9M; Tubus Cavas and Clypeus Margaritus 11.9M; Frutexa Flammasis 10.3M. Bacterium and Fungoida species are mostly worth 1–4M (Bacterium Informem 8.4M). Stratum Tectonicas is the classic money maker: common on hot, thin-atmosphere high-metal-content worlds and worth 19M.

## The five-times bonus
If the commander is the first to log a species on a body, Vista Genomics pays five times the base value. Undiscovered or unmapped planets far from the populated bubble are the best places to get it; inside the bubble most species are already logged. The best sign is a body nobody had set foot on when the commander first scanned it (no footfall, see below) in an uninhabited system; populated systems never pay the bonus. Elite Nexus counts the bonus only then, and drops it when the journal's ScanOrganic event says WasLogged true. WasLogged false is no proof on its own: in one commander's sales it was false for 67 species and the bonus came for 2. On a body someone landed on, a species nobody sold there can still pay the bonus, but nothing tells which one before the sale.

## First footfall: has anyone landed here?
The game records whether anyone has set foot on a body (first footfall), and since game update 4.2.1 (September 2025) every scan in the journal carries it as WasFootfalled: the state at the moment of that scan. No public website tracks it. Elite Nexus combines what exists, best first: the commander's own journal (authoritative); scans other commanders share live over EDDN (not every program that shares scans keeps the flag, so it is often missing); for biology bodies, archived EDDN scans looked up online on demand at EDGalaxyData (cached for a day); and a rule of thumb, "probably unclaimed", when Spansh has no data at all on the body since before Odyssey launched (19 May 2021), which is when on-foot play began. Every answer has an age: "not footfalled as of 3 days ago" can be out of date, and a commander who scanned or mapped a body often landed right after. In populated systems first footfall is not possible, even when the flag says false. In the assistant, ask about the planets of a system (system bodies).

## Predicting what grows where
Each genus needs specific conditions: atmosphere type (thin carbon dioxide, ammonia, sulphur dioxide, argon, methane, nitrogen, neon, water…), surface gravity, temperature range, volcanism, and sometimes the parent star's class or the galactic region. A detailed surface scan (DSS) reveals which genera are present; before that, Elite Nexus predicts likely species and their value for every body with biological signals, from the scan data. Planets with several biological signals and a thin atmosphere are the most valuable targets.

## Planning an exobiology trip
1. Find targets, one of two ways (Exobiology › Find planets in Elite Nexus). Sure value: Spansh's exobiology route lists nearby planets where commanders have already logged valuable species, so the species are certain but someone logged them first: expect the base value (in the assistant: money routes, kind exobiology). Bonus hunt: planets where a valuable species should grow and nobody has a record of sampling it, for the five-times bonus; the species there is predicted, not certain (in the assistant: untouched biology). Both tours go to the Navigation page.
2. Use a ship with good jump range that lands easily; an SRV helps cover distance between samples on rough terrain.
3. On each body: DSS-map it to reveal the genera, land near the biological zone, sample each species three times.
4. Sell regularly at Vista Genomics; check the unsold value on the Exobiology page.

## Finding untouched planets (first-logged bonus)

Nothing proves a planet is untouched; only the opposite can be proven. Evidence that someone was there: Spansh lists the
planet's genera (someone DSS-mapped it with a tool that uploads to EDDN), biology landmarks (someone's sample added a
codex entry, uploaded by their tool), `WasMapped` true (someone mapped it and sold the data; this lags until they sell),
`WasFootfalled` true. A planet with biological signals and none of that has never been mapped or sampled by a commander
who shares data (EDMC, EDDiscovery…). Two blind spots remain: players without such tools, and samples of a species the
commander already had in their codex for that region (the game writes no codex entry then, so no landmark appears).
The `untouched_biology` tool and Exobiology › Find planets › Bonus hunt search for exactly that around a system for a
chosen species (Stratum Tectonicas by default), say how many planets were left out and why (samples on record, mapped
without the genus, populated system, landed on), and rank the rest by expected first-logged value and distance.
"No record" planets are the best bets; "Mapped, no samples" planets have the genus confirmed, but someone was there and
may have sampled without a trace, so the bonus may be gone. Near the bubble almost everything is mapped and sampled; a
few thousand light years out, a third or more of the planets with the right conditions show no trace.
