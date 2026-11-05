# Combat: where fighting is legal, and where conflicts are found

Research date: **2026-11-05** (game version 4.4, before the Law & Disorder update). Status: the data-source part was
tested live; the rules part is checked against Frontier's patch notes, dev posts and the community wiki, with every
claim that couldn't be confirmed marked **[UNVERIFIED]**. Numbers in brackets refer to the sources in section 7.

Written for play-test finding 19 (2026-11-04): a commander new to combat wants to know where the nearest fights are,
and where they can kill without a bounty or a trip to the detention centre. The Combat page (`pages/combat.rs`), the
galaxy database's conflict list (`nexus-galaxy/src/conflicts.rs`) and Ask's `conflicts_near` tool are built on this.

---

## 1. Where conflicts can be read

### 1.1 The journal

`FSDJump`, `Location` and `CarrierJump` carry the system's conflicts [14]:

```json
"Conflicts":[ { "WarType":"civilwar", "Status":"active",
  "Faction1":{ "Name":"Leesti United Steelworks", "Stake":"Villeneuve Manufacturing Enterprise", "WonDays":0 },
  "Faction2":{ "Name":"Leesti Alliance Union", "Stake":"Pandey's Point", "WonDays":2 } } ]
```

(a real entry, the maintainer's journal, 2026-11-04).

* `WarType`: `war`, `civilwar` or `election`. Elections are in the list too.
* `Stake`: the asset the faction stands to lose, `""` when none; a colonisation ship comes as a `$EXT_PANEL_…` symbol
  (`clean_station_name` turns it into the name).
* `Status`: `active`, `pending` or `""`. **`""` means the conflict is over**: on the live feed (2026-11-05) every one
  of the 11 conflicts listed with an empty status had both factions showing it among their `RecoveringStates`
  (Wolf 397 4–1, Deciat 4–0, Epsilon Eridani 4–1 and an election 3–4, Kurmodiae 0–1, Niflhel 1–2, Almad 1–1), and no
  active one did. Low scores like 0–1 are wars whose seven days ran out with most days drawn; 1–1 is a draw.
* The key is written only when the system has conflicts, so a report with `Factions` and no `Conflicts` means none.
* A faction's main `FactionState` can lag: Wolf 397 Independents reported `"FactionState":"War"` with no active states
  and `RecoveringStates:[CivilWar]`. Only `ActiveStates` and `PendingStates` say a conflict is on.
* `ThargoidWar{CurrentState, NextStateSuccess, NextStateFailure, SuccessStateReached, WarProgress, RemainingPorts,
  EstimatedRemainingTime}` appears only for systems with a Thargoid state [14].

### 1.2 EDDN

`journal/1` keeps `Conflicts`: it isn't on the schema's list of keys to remove (`eddn.md` §3.1), and EDMC and EDDI send
the event whole. Ten minutes of the live relay into a copy of the maintainer's database (2026-11-05, 06:10–06:20 UTC):
249 populated systems reported with their factions, 69 of them with conflicts, 82 conflicts in all:

| Kind | Active | Pending | Ended (`""`) |
|---|---|---|---|
| war | 22 | 5 | 3 |
| civilwar | 21 | 4 | 5 |
| election | 15 | 4 | 3 |

The maintainer's database, fed only while the app ran, had faction states for 5,599 systems, 5,510 of them reported
in the previous 24 hours, and 631 systems with an active war or civil war reported in the last three days. A few hours
of the feed covers the populated bubble.

### 1.3 Spansh

* Dumps (`galaxy*.json.gz`) carry every faction's `activeStates`, `pendingStates` and `recoveringStates`, not the
  conflict list: no pairs, stakes or days won (`spansh.md` §7).
* `/api/systems/search` can filter on a faction's main state. Tested 2026-11-05:
  `{"filters":{"minor_faction_presences":[{"state":{"value":["War","Civil War"]}}]},"sort":[{"distance":{"direction":"asc"}}],"reference_system":"Sol"}`
  returned 5,632 systems (nearest: Alpha Centauri, a civil war; Barnard's Star; Luhman 16), each with its factions'
  active and pending states and the system's `updated_at`. The filter reads the main state only, so pending wars
  can't be searched, and the count includes systems nobody has visited since their war ended (add an `updated_at`
  range).
* Elite Nexus doesn't use it for the Combat page: the local database has the game's own list, fresher for any system
  visited while it ran. It would help a commander who has just installed the app; see section 6.

### 1.4 EDSM

`GET https://www.edsm.net/api-system-v1/factions?systemName=Leesti` (tested 2026-11-05) returns each faction's
`state`, `activeStates`, `pendingStates`, `recoveringStates`, `happiness`, `isPlayer` and `lastUpdate`. No conflict
list.

### 1.5 What Elite Nexus stores

* `system_conflicts` (galaxy database): one row per conflict, keyed by system and the two factions, with kind, status,
  stakes, days won and the report time. A few thousand rows at most.
* `systems.conflicts_updated`: when the system's list was last replaced.
* The list goes with the factions. A report that replaces a system's factions (newer than what's stored) replaces its
  conflicts too: with the game's list, or with none when the report has no `Conflicts` and no faction is in a war or
  election state. A report whose factions show such a state but has no list (a sender dropped it), and a Spansh dump,
  clear the stored list and leave `conflicts_updated` NULL; the query then works the conflicts out from the faction
  states (factions in the same state, no stakes or days won).
* Nothing new is uploaded. The table fills from what the app already receives.

---

## 2. How a conflict works

* **What starts one**: two factions in a system reach equal influence while an asset is at risk, or an invasion [6][28].
  A faction under 7 % influence can't start one [28]. Both factions native to the system: a civil war; otherwise a
  war [28].
* **Length**: up to seven days since 3.3 [5]. Each day goes to the faction with the higher daily score at the tick [6].
  It ends early once the trailing side can no longer reach a draw [6]: in practice the first to win four days, or 3–0
  when enough days were drawn. Even days won at the end is a draw, and both keep their assets [36].
* **Pending**: the journal's `pending` status. Frontier doesn't document how long it lasts; players report one to three
  days, and up to a week in 2025 **[UNVERIFIED]**.
* **What counts in a war**: conflict-zone objectives most of all; most combat missions, combat bonds, bounty hunting and
  violent crimes also count [6]. A 2025 community guide says the day goes to whoever won the most conflict zones, with
  bonds only breaking ties [36] **[UNVERIFIED]** against Frontier.
* **Stakes**: the winner takes the loser's highest-value asset [28]; Odyssey settlements go to the faction that won the
  most conflict zones at that settlement [7]. Influence moves ±4 % after a conflict (2019) [6].
* **Elections** have no conflict zones: they're decided by non-combat missions, trade (smuggling too), search and rescue,
  exploration data and scenarios at installations and megaships [6]; combat has no effect on them [28].

---

## 3. Legal combat, as of game version 4.4

### 3.1 Conflict zones in space
* Only wars and civil wars create them [15][28]. Intensities: low, medium, high; a capital ship can appear in high
  only [15]. They show in the navigation panel in supercruise within 1,000 ls [15].
* Picking a side: a tab in the **Comms panel** when you drop in [15][17]; in multicrew the helm's choice counts [18].
  Older guides put it under the right-hand Functions tab, an old interface **[UNVERIFIED]** in the current game.
* "Any action against any ship while officially participating" gives no notoriety [18]. Zone ships aren't Wanted, so
  destroying one before joining would be murder under the general rules (inference); no conflict-zone-specific
  source covers friendly fire **[UNVERIFIED]**.
* Since 3.3 zones are fought in rounds, with objectives: captains, Spec Ops wings, war correspondents (protect),
  propagandists (destroy) [5][15]. Ships destroyed there drop no materials [15].
* **Combat bonds**: issued by your side [17], `FactionKillBond{AwardingFaction, VictimFaction}` in the journal [14].
  Where to cash them, sources disagree: stations controlled by the issuing faction [17] or any station in the system
  [15] **[UNVERIFIED]**; the page advises a station of your side. Interstellar Factors take them from anywhere, minus
  25 % [20]. They're lost when the ship is destroyed [23]; Operations store them for that reason [11]. No source says
  they expire.

### 3.2 Conflict zones on foot
* At settlements of factions at war [7][16]. Join at the **Frontline Solutions** desk in a port of that system (pick the
  settlement and the side, then the dropship, up to six players), or fly there and accept the Comms-panel prompt
  [16][18]. 12 against 12, 1,000 reinforcement points a side [16]. Bonds at Frontline Solutions or a combat voucher
  contact [17]. "Bounties do not apply in conflict zones" [16].

### 3.3 Wanted ships: extraction sites and nav beacons
* Resource extraction sites: low, normal, high, hazardous; more ships the higher; system security answers by site type
  and system security, never at hazardous sites, where crimes still count outside anarchies [25]. A compromised nav
  beacon has no security response [26].
* Only ships Wanted in the current jurisdiction are legal targets; a ship with bounties elsewhere only ("Warrant") isn't
  [22][24]. Hitting a ship before it shows Wanted gives you a bounty: let the scan finish [18][22][23].
* The Kill Warrant Scanner reveals bounties from other jurisdictions and pays them all on the kill [2][24].
* Bounties are cashed with the issuing faction's security contact at its station [23], or at Interstellar Factors for any
  jurisdiction, minus 25 % [20].

### 3.4 Anarchy
* No fines, no bounties, no reputation loss, no authority ships [19][27]; destroying clean ships there adds no
  notoriety [18]; the Pilots' Federation bounty for killing a player doesn't apply [4].
* On foot, killing at a settlement of a non-anarchy faction in an anarchy system still adds notoriety [18].
* `SystemSecurity` has both "Anarchy" and "Lawless" values [40]; the community reads Lawless as an inhabited anarchy
  and Anarchy as an uninhabited system **[UNVERIFIED]**. Elite Nexus tests the system's government.

### 3.5 Powerplay 2.0 (live since 31 October 2024 [10][29])
* Crime and punishment stays as it was [9]. Between pledged commanders, crimes bring a **Power bounty**: no authority
  ships, only the issuing Power's pledges can claim it, no detention centre [1][2]. Whether the 2018 "attack with
  impunity in your own space" rule survives is **[UNVERIFIED]**.
* Killing ships aligned with another Power earns merits in acquisition, reinforcement and undermining systems [29][34].
  Frontier calls it "technically illegal" until December [12]; players report small assault or murder bounties and no
  notoriety [35] **[UNVERIFIED]**.
* **Power conflict zones** (`$Warzone_Powerplay_Low/Med/High`, "Power Conflict Zone [… Intensity]") came with
  Ascendancy [9][34]; they form in contested acquisition systems [34][37]; merits per kill [37]; bonds only at the
  combat bond contact on the Power's stronghold carrier [17]. Whether you choose a side or get your Power's is disputed
  [17][37], and their legality has no explicit source **[UNVERIFIED]**.

### 3.6 Thargoids
* Thargoid kills pay combat bonds [17]. No Frontier text says outright that attacking them can never be a crime
  **[UNVERIFIED]**; they belong to no faction.
* The war is over: the last Titan, Cocijo, fell over Earth on 18 December 3310 (2024) [30][31]. Thargoids still held
  parts of the Pleiades, Witch Head, Coalsack and California nebulae in January 2025 [31]. Whether AX conflict zones
  still spawn in 2026 is **[UNVERIFIED]**.

### 3.7 Missions
* Pirate massacre targets are Wanted. Some missions are illegal, which the mission details "generally (not always)" say
  in a SPECIAL line [32]; assassinations can be either [33]. A clean target is murder whoever asked [18].
* Massacre missions for a side in a war are done in conflict zones [15].
* Stacking (community knowledge, not Frontier's): missions from different issuing factions against the same target
  faction all count each kill [33].

### 3.8 Operations (4.4.0.0, 30 June 2026)
* Started from the ship panel while docked (starports, outposts, fleet and squadron carriers, market planetports); solo
  or up to four; Mercenary or Powerplay mode [11]. Bounties, fines and bonds are stored before you go in [11]; nothing
  done there adds notoriety [18]. Pays credits, materials, Merc Coins and, for pledged commanders in Powerplay mode,
  merits [11].

### 3.9 Crimes, notoriety and the detention centre
* `CommitCrime` carries a fine or a bounty [14][42]. Fines: smuggling and illegal goods, loitering, high-speed
  collisions in the no-fire zone, abandoned missions, stolen goods, reckless weapons discharge [1][8][22]. Bounties:
  damaging or destroying clean ships or SRVs, killing people, trespass, destroying property [8][22]. Which one firing in
  a no-fire zone, failing a scan, piracy and interdiction bring is **[UNVERIFIED]**.
* Fines can turn into bounties over time [8] (Law & Disorder ends that [12]). Bounties never expire [1]. Bounties with
  five or more factions of one superpower add a superpower bounty [2].
* **Notoriety**: 0 to 10, +1 per murder (less on foot), −1 per two hours logged in, docked included [1][3][8][18].
  Above 0, Interstellar Factors won't clear bounties [1][8]; murder bounties grow 10 % per point [1].
* **Paying**: fines at a security contact [8]; bounties at Interstellar Factors with notoriety 0 [1][8] (in Odyssey not
  where the issuing faction is present [20]; Factors are in low-security and anarchy systems [8]), or by handing
  yourself in at a security contact, which also sends you to the nearest detention centre [3].
* **Detention centre**: destroyed or killed where you're wanted, and the bounty claimed (security, response ships or a
  player) [1][7][19]. You pay the rebuy plus that jurisdiction's fines and bounties; the rest stay [1]. In Odyssey hot
  ships are impounded with a release fee [7]. Can't pay: a Sidewinder [19][21]. Self-destruct's outcome
  **[UNVERIFIED]**.
* **Other players**: an ordinary crime unless the target is Wanted, in an anarchy, or under Powerplay rules. PvP murder
  adds a Pilots' Federation bounty, valid everywhere but anarchies [4]. Players can switch off "Report crimes against
  me"; switching it back on takes 15 seconds [4][12].

---

## 4. Law & Disorder (December 2026)

Frontier's October 2026 dev log [12] announces a free update for December that rewrites most of section 3: crimes count
only once authorities see and confirm them (until then an "unconfirmed bounty" that leaving wipes); the Kill Warrant
Scanner is removed and an ordinary scan shows every crime; crimes attach to the commander, not the ship; Interstellar
Factors are removed (pay at any valid location, the top two tiers at a Record Cleaner in a criminal port); notoriety as
it is goes, with five criminal tiers instead; inhabited anarchies are renamed criminal systems and get enforcer ships;
a pledged commander killing another Power's ships is no longer a crime; PvP crime moves to Pilots' Federation tiers.
The Combat page's guide says so at the top. **Re-check this note and the guide after the update.**

---

## 5. Signal names (`FSSSignalDiscovered`)

| `SignalName` | Shown as | `SignalType` |
|---|---|---|
| `$Warzone_PointRace_Low/Med/High:#index=n;` | Conflict Zone [Low/Medium/High Intensity] | Combat |
| `$Warzone_Powerplay_Low/Med/High:#index=n;` | Power Conflict Zone [… Intensity] (Medium seen in the maintainer's journal) | Combat |
| `$Warzone_TG…` (`_Low`, `_Med`, `_High`, `_VeryHigh`) | AX Conflict Zone **[UNVERIFIED]** wording | Combat |
| `$MULTIPLAYER_SCENARIO14_TITLE;` / `77` / `78` / `79` | Resource Extraction Site / [Low] / [High] / [Hazardous] | ResourceExtraction |
| `$MULTIPLAYER_SCENARIO42_TITLE;` | Nav Beacon | NavBeacon |
| `$MULTIPLAYER_SCENARIO80_TITLE;` | Compromised Nav Beacon | NavBeacon |

Odyssey writes a system's signals just before the jump event that reaches it (`eddn.md` §3.9), so "signals since
arriving" starts a little before the arrival's timestamp.

---

## 6. What other tools show, and what's left

* The play-test's reference: another companion tool's "Conflict zones nearby" card, listing the closest wars, civil
  wars and elections, who fights and whether each is active or pending.
* Spansh's system search filters on faction states (section 1.3); EDSM shows each system's faction states.
* Left for later: an online fallback through Spansh's search for a commander whose database is still empty; AX conflict
  zones, once their 2026 status is known; reading the commander's own unredeemed bonds and bounties from the journal
  (`FactionKillBond`, `Bounty`, `RedeemVoucher`) to remind them before a risky jump.

---

## 7. Sources

1. https://forums.frontier.co.uk/threads/crime-and-punishment-in-beyond-chapter-one.407608/#post-6420464 (Frontier, 16 Feb 2018)
2. https://forums.frontier.co.uk/threads/update-3-0-3.416165/#post-6536077 (3.0.3, 19 Mar 2018)
3. https://forums.frontier.co.uk/threads/elite-dangerous-beyond-chapter-two-3-1.435267/#post-6818003 (3.1, 27 Jun 2018)
4. https://forums.frontier.co.uk/threads/2-4-the-return-patch-notes.378706/#post-5945501 (2.4, 26 Sep 2017)
5. https://forums.frontier.co.uk/threads/beyond-chapter-four-3-3-patch-notes.463020/ (3.3, 11 Dec 2018)
6. https://forums.frontier.co.uk/threads/background-simulation-update-01-03.479111/ (Frontier, 1 Mar 2019)
7. https://forums.frontier.co.uk/threads/elite-dangerous-odyssey-bgs-and-crime-details.575842/#post-9179877 (Frontier, 20 May 2021)
8. https://forums.frontier.co.uk/threads/elite-dangerous-crime-fines-bounties-and-notoriety.587014/#post-9437220 (Frontier, 30 Jul 2021)
9. https://www.elitedangerous.com/news/powerplay-20-qa (26 Sep 2024)
10. https://www.elitedangerous.com/update-notes/4-0-19-0 (Ascendancy)
11. https://www.elitedangerous.com/update-notes/4-4-0-0 (Operations, 30 Jun 2026)
12. https://www.elitedangerous.com/news/elite-dangerous-october-dev-log-2026-law-disorder (14 Oct 2026)
13. https://forums.frontier.co.uk/threads/elite-dangerous-community-goal-update-and-powerplay-conflict-zones.637447/ (8 May 2025)
14. Journal manual v38 (https://hosting.zaonce.net/community/journal/v38/Journal_Manual_v38.pdf), as in `journal.md` §5.10
15. https://elite-dangerous.fandom.com/wiki/Conflict_Zone_(Signal_Source)
16. https://elite-dangerous.fandom.com/wiki/Conflict_Zone_(On_Foot)
17. https://elite-dangerous.fandom.com/wiki/Bonds
18. https://elite-dangerous.fandom.com/wiki/Notoriety
19. https://elite-dangerous.fandom.com/wiki/Crime_%26_Punishment
20. https://elite-dangerous.fandom.com/wiki/Interstellar_Factor
21. https://elite-dangerous.fandom.com/wiki/Detention_Centre
22. https://elite-dangerous.fandom.com/wiki/Bounty
23. https://elite-dangerous.fandom.com/wiki/Bounty_Hunter
24. https://elite-dangerous.fandom.com/wiki/Kill_Warrant_Scanner
25. https://elite-dangerous.fandom.com/wiki/Resource_Extraction_Site
26. https://elite-dangerous.fandom.com/wiki/Nav_Beacon
27. https://elite-dangerous.fandom.com/wiki/Anarchy
28. https://elite-dangerous.fandom.com/wiki/Influence
29. https://elite-dangerous.fandom.com/wiki/Powerplay
30. https://elite-dangerous.fandom.com/wiki/Thargoid_Titan
31. https://elite-dangerous.fandom.com/wiki/Second_Thargoid_War
32. https://elite-dangerous.fandom.com/wiki/Mission_Board
33. https://elite-dangerous.fandom.com/wiki/Assassin
34. https://forums.frontier.co.uk/threads/powerplay-2-0-activities.629227/ (3 Nov 2024)
35. https://forums.frontier.co.uk/threads/powerplay-commit-crimes-in-undermining-systems.630089/ (player reports, Nov 2024)
36. https://forums.frontier.co.uk/threads/announcing-early-access-to-the-bgs-guide-2025-draft-for-review-and-comment.635917/
37. https://wiknow.pages.dev/pcz-guide.html (community)
38. https://sites.google.com/view/ed-pve-combat/location-catalogue/conflict-zones
39. https://github.com/EDSM-NET/Alias/blob/master/System/FSSSignal.php, https://github.com/EDCD/EDDI/blob/develop/DataDefinitions/Properties/SignalSource.resx
40. https://github.com/EDCD/FDevIDs/blob/master/security.csv
41. https://www.elitedangerous.com/update-notes/4-0-18-06
42. https://github.com/EDCD/FDevIDs/blob/master/crimes.csv

Not reached: Frontier's support article on crime and punishment (404); Inara (503); EDSM's site pages (bot check, not
bypassed; its API answered).
