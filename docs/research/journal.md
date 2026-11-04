# Elite Dangerous Player Journal — research notes for Elite Nexus

_Researched 2026-10-12. Game state: Odyssey 4.4.x live ("Rhino" update, Sept 2026). Everything below was checked against primary sources (Frontier manual PDFs, jixxed/ed-journal-schemas, the EDDiscovery `EliteDangerousCore` source, EDCD/FDevIDs, EDMC source). Items marked **(community)** are not in Frontier's manual and were reverse-engineered from journals by tool authors._

---

## 0. TL;DR for implementers

* **Official manual:** *Journal Manual v38*, last modified **2026-04-28**, changelog "Changes for Odyssey up to update26 (May 2026)", at <https://hosting.zaonce.net/community/journal/v38/Journal_Manual_v38.pdf>. There is no index page; the URL pattern is `…/community/journal/vNN/Journal_Manual_vNN.pdf` (v30, v31, v33–v38 exist; v32 and v39+ return 404 as of today).
  - v38's only change over v37 (May 2023) is the per-passenger-type **Statistics → Passengers** counters. **Frontier has not documented** any Colonisation, Powerplay 2.0, Vanguards (squadron carrier), Status `Flags2` bit ≥ 20, MercCoins, Nomad or Rhino journal output. Use the community references for those.
* **Best maintained machine-readable reference:** **jixxed/ed-journal-schemas** (Apache-2.0) — one JSON Schema (draft 2020-12) per event, 275 events, last updated 2026-09-06 for the Rhino update. It's written for code generation (no enums, few validations, good examples). **Downloaded to `data/journal-schemas/`.**
  - Docs site: <https://jixxed.github.io/ed-journal-schemas/>.
* **Best maintained parser to cross-check behaviour:** EDDiscovery's **EliteDangerousCore** (Apache-2.0, pushed 2026-10-10): `EliteDangerous/JournalStatus/Entry/JournalTypeEnums.cs` (every event name), `JournalStatus/Events/*.cs` (field handling and quirks), `JournalStatus/UIScan/StatusReader.cs` (Status.json flags), `FrontierData/Enumerations/*` (station types, services, powerplay states, release dates).
  - Also useful: EDMarketConnector `monitor.py` (GPL-2.0, so read it but don't copy).
  - `elite-journal.readthedocs.io` is a stale HTML mirror of an old manual.
* Treat the journal as an **open set**. Unknown events, unknown fields and unknown enum strings must not fail parsing (Frontier adds fields without notice, e.g. `CarrierType`, `BuyMercCoinsPrice`, `Label_Localised` all appeared in 2025–26).

---

## 1. Sources and their currency

| Source | What | Licence | Currency |
|---|---|---|---|
| Frontier *Journal Manual* v38 (PDF) | Official prose spec of every documented event, Status.json, startup sequence, appendices (ranks, star/planet classes, codes) | Frontier docs | 2026-04-28; content effectively frozen at Odyssey U14 (2022) plus small additions |
| v37 PDF (same host) | Previous version. Changelog: CarrierJump restored, StartJump in taxi, `IsThargoid`, `SupercruiseDestinationDrop`, ThargoidWar data, organic `Variant` | — | 2023-05-03 |
| **jixxed/ed-journal-schemas** (GitHub) | JSON Schema per event incl. undocumented ones; Status schema documents Flags/Flags2/GuiFocus | Apache-2.0 | 2026-09-06 (commits: "Nomad update" Jun–Aug 2026, "Rhino update" Sep 2026) |
| **EDDiscovery/EliteDangerousCore** | C# parser for every event (`JournalTypeEnum`), Status reader, enumerations | Apache-2.0 | 2026-10-10 |
| EDCD/FDevIDs | CSVs of Frontier IDs/symbols (ships, modules, commodities, materials, microresources…) | **no licence file** (see §9) | 2026-09-05 |
| EDCD/EDMarketConnector `monitor.py` | Reference tailing logic; journal filename regex | GPL-2.0 | current |

Release dates the EDD source uses for version gating (`EliteReleaseDates.cs`) are a handy changelog:

| Date | Release |
|---|---|
| 2022-11-29 | Odyssey Update 14 (live/legacy galaxy split) |
| 2024-08-07 | Type-8 |
| 2024-10-22 | **Ascendancy** (Powerplay 2.0) |
| 2025-02-26 | **Trailblazers** (colonisation) |
| 2025-08-19 | **Vanguards** (squadron overhaul, squadron carriers) |
| 2026-06 | game version 4.4.0.0, "Nomad" / Operations |
| 2026-08-28 / 2026-09-02 | **Rhino** SRV alpha/release, game version 4.4.1.0 |

---

## 2. File-level format

* **Location (Windows):** `%USERPROFILE%\Saved Games\Frontier Developments\Elite Dangerous\`. Under Proton/Wine the same path sits inside the Wine prefix.
* **Journal files:** line-delimited JSON, one event object per line, UTF-8.
  * Name since Odyssey Update 11 (Mar 2022): `Journal.YYYY-MM-DDTHHMMSS.NN.log`, e.g. `Journal.2022-03-15T152503.01.log`.
  * Old (Horizons/legacy) name: `Journal.YYMMDDHHMMSS.NN.log`. EDMC's regex is `^Journal(Alpha|Beta)?\.[0-9]{2,4}(-)?[0-9]{2}(-)?[0-9]{2}(T)?[0-9]{6}\.[0-9]{2}\.log$`.
  * Beta/alpha clients write `JournalBeta.*` / `JournalAlpha.*`; exclude them from real stats, as EDD does.
  * Sort by the parsed timestamp, then the part number.
* **Rollover:** at ~500k lines the game writes `Continued{Part}`, closes the file and opens `.NN+1.log`. Every file starts with `Fileheader{part, language, Odyssey, gameversion, build}` (note the lower-case keys).
  * `gameversion` starting `4.` means the live galaxy. Since U14 a `3.8.x` client is the separate **legacy** galaxy, which most tools ignore.
  * Beta builds are flagged in `gameversion`; EDD also lists specific beta builds such as `4.4.1.0 r332668`.
* **Every event:** `{"timestamp":"ISO-8601 UTC","event":"Name", …}`.
* **Localisation:** symbol values look like `"$government_Corporate;"`. Each one has a sibling `<Key>_Localised` holding the client-language text. The localised key is omitted when it equals the raw value. Commodity and material names are lower-case FD symbols (`"painite"`, `"$painite_name;"` in some events); normalise by stripping `$`/`_name;` and lower-casing.
* **Companion files:** some events (Market, Outfitting, Shipyard, NavRoute, ModuleInfo, Cargo, ShipLocker, Backpack, FCMaterials) write a short journal line plus a full JSON file in the same folder with the **same `timestamp` and `event`**. See §7 for the read algorithm.
* **IDs:** `SystemAddress`, `MarketID`, `CarrierID`, `SuitID`, `SuitModuleID`, `LoadoutID` and `ShipID` are integers up to 2^63. Use `u64`/`i64`, never `f64`. Fleet-carrier `MarketID` == `CarrierID`.
* **Startup order** (after `Fileheader`):
  1. `Commander`
  2. `Materials`
  3. `Rank`
  4. `Progress`
  5. `Reputation`
  6. `EngineerProgress`
  7. `LoadGame`
  8. `Statistics`
  9. `SquadronStartup`
  10. `Powerplay`
  11. `Missions`
  12. `Cargo`
  13. `Loadout`
  14. `ShipLocker` / `SuitLoadout`
  15. `Location`
  16. `CarrierLocation`
  17. `Music`, …

  The exact order varies slightly between builds; don't rely on it.

---

## 3. Event catalogue (all 275 schema events, grouped)

Field lists are generated from jixxed/ed-journal-schemas.

* `?` marks an optional field.
* `X[a,b]` is an array of objects with those keys.
* `X{a,b}` is an object.
* `_Localised` siblings are omitted.

Events that only exist in the manual or EDD are listed at the end.

#### Startup / session

- **Fileheader** — part, language, Odyssey, gameversion, build
- **Commander** — FID, Name
- **LoadGame** — FID, Commander, Horizons, Odyssey?, Ship?, ShipID?, ShipName?, ShipIdent?, FuelLevel?, FuelCapacity?, GameMode?, Credits, Loan, language?, gameversion?, build?, Group?, StartLanded?, StartDead?
  - `Ship` is a suit symbol when the player starts on foot.
- **NewCommander** — Name, FID, Package?
- **ClearSavedGame** — Name, FID
- **Rank** — Combat, Trade, Explore, Soldier?, Exobiologist?, Empire, Federation, CQC
- **Progress** — same keys, as percent to the next rank
- **Promotion** — any subset of the Rank keys
- **Reputation** — Empire?, Federation?, Alliance?, Independent? (−100..100)
- **Statistics** — Bank_Account{…}, Combat{…}, Crime{…}, Smuggling{…}, Trading{…}, Mining{…}, Exploration{…}, Passengers{…}, Search_And_Rescue{…}, Crafting?{…}, Crew?{…}, Multicrew?{…}, Material_Trader_Stats?{…}, FLEETCARRIER?{…}, Exobiology?{…}, TG_ENCOUNTERS?{…}, CQC?{…}, Squadron?{…}. See §5.9.
- **Materials** — Raw[Name, Count], Manufactured[Name, Count], Encoded[Name, Count]
- **Cargo** — Vessel ("Ship"/"SRV"), Count, Inventory?[Name, Count, Stolen, MissionID?]
- **Missions** — Active[MissionID, Name, PassengerMission, Expires], Failed[…], Complete[…]
- **Passengers** — Manifest[MissionID, Type, VIP, Wanted, Count]
- **Loadout** — Ship, ShipID, ShipName, ShipIdent, HullValue?, ModulesValue?, HullHealth, UnladenMass, CargoCapacity, MaxJumpRange, FuelCapacity{Main, Reserve}, Rebuy, Modules[Slot, Item, On, Priority, AmmoInClip?, AmmoInHopper?, Health, Value?, Engineering?], Hot?
  - `Engineering` is {EngineerID, Engineer?, BlueprintID, BlueprintName, Level, Quality, Modifiers[Label, Value?, OriginalValue?, LessIsGood?, ValueStr?], ExperimentalEffect?}.
- **Powerplay** — Power, Rank, Merits, Votes? (removed in Ascendancy), TimePledged
- **SquadronStartup** — SquadronName, CurrentRank, CurrentRankName?, SquadronID?
- **EngineerProgress** — at startup: Engineers?[Engineer, EngineerID, Progress ("Known"/"Invited"/"Acquainted"/"Unlocked"/"Barred"), RankProgress?, Rank?]. On change: the same fields flattened.
- **Friends** — Status, Name
- **Continued** — Part
- **Shutdown** — no fields
- **GameModeChange** **(community, 2026)** — GameMode? ("MainGame", "Operation")
- **Music** — MusicTrack

#### Travel / navigation

- **StartJump** — Taxi?, JumpType ("Hyperspace"/"Supercruise"), StarSystem?, SystemAddress?, StarClass?
- **FSDJump** — the shared system block (§5.10) plus:
  - Taxi?, Multicrew?, Wanted?
  - JumpDist, FuelUsed, FuelLevel, BoostUsed?
- **FSDTarget** — Name, SystemAddress, StarClass, RemainingJumpsInRoute?
- **Location** — the shared system block plus:
  - DistFromStarLS?, Docked, Taxi?, Multicrew?, Wanted?, OnFoot?, InSRV?
  - StationName?, StationType?, MarketID?, StationGovernment?, StationServices?[], StationEconomy?, StationAllegiance?
  - Latitude?, Longitude?
- **SupercruiseEntry** — Taxi?, Multicrew?, StarSystem, SystemAddress, Wanted?
- **SupercruiseExit** — Taxi?, Multicrew?, StarSystem, SystemAddress, Body, BodyID, BodyType
- **SupercruiseDestinationDrop** — Type, MarketID?, Threat (manual v37)
- **ApproachBody** / **LeaveBody** — StarSystem, SystemAddress, Body, BodyID
- **ApproachSettlement** — Name, MarketID?, SystemAddress, BodyID, BodyName, Latitude?, Longitude?, StationFaction?{Name, FactionState?}, StationGovernment?, StationAllegiance?, StationServices?[], StationEconomy?, StationEconomies?[Name, Proportion]
- **Touchdown** / **Liftoff** — PlayerControlled, Taxi?, Multicrew?, StarSystem?, SystemAddress?, Body?, BodyID?, OnStation?, OnPlanet?, Latitude?, Longitude?, NearestDestination?
  - **(community, Rhino 2026)** `NearestDestination` names the planetary mining location nearest the ship: `"$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=4;"` is the one the navigation panel numbers 4. A settlement, another surface signal (`#type=$SAA_SignalType_Geological;` and so on) or `""` when nothing is near. Seen in real journals by EDSMT and ED-Outrider.
- **DockingRequested** — MarketID, StationName, StationType, LandingPads?{Small, Medium, Large}
- **DockingGranted** — LandingPad, MarketID, StationName, StationType
- **DockingDenied** — Reason (FDevIDs `dockingdeniedreasons.csv`: ActiveFighter, Distance, Hostile, NoSpace, Offences, TooLarge, RestrictedAccess), MarketID, StationName, StationType
- **DockingCancelled** / **DockingTimeout** — MarketID?, StationName?, StationType?
- **Docked** — StationName, StationType, Taxi?, Multicrew?, StarSystem?, SystemAddress?, MarketID?, StationFaction?{Name, FactionState?}, StationGovernment?, StationServices?[], StationEconomy?, StationEconomies?[Name, Proportion], DistFromStarLS, LandingPads?{Small, Medium, Large}, Wanted?, ActiveFine?, StationAllegiance?, CockpitBreach?, StationState? ("Damaged", "UnderRepairs", …)
- **Undocked** — StationName, StationType, MarketID?, Taxi?, Multicrew?
- **NavRoute** — Route?[StarSystem, SystemAddress, StarPos[], StarClass]. The full route is in NavRoute.json.
- **NavRouteClear** — Route?[] (empty)
- **USSDrop** — USSType, USSThreat
- **JetConeBoost** — BoostValue
- **JetConeDamage** — Module
- **BookTaxi** — Cost, DestinationSystem, DestinationLocation, Retreat?
- **CancelTaxi** — Refund

#### Combat / crime

- **Bounty** — Rewards?[Faction, Reward], PilotName?, Target, TotalReward?, VictimFaction, SharedWithOthers?, Reward?, Faction?
  - `Reward`/`Faction` (singular) is the on-foot/skimmer form.
- **CapShipBond**, **FactionKillBond** — Reward, AwardingFaction, VictimFaction
- **PVPKill** — Victim, CombatRank
- **Died** — KillerName?, KillerShip?, KillerRank?, Killers?[Name, Ship, Rank] (wing kills)
- **Resurrect** — Option, Cost, Bankrupt
- **Interdicted** — Submitted, Interdictor?, IsPlayer, IsThargoid?, CombatRank?, Faction?, Power?
- **Interdiction** — Success, Submitted?, Interdicted?, IsPlayer, CombatRank?, Faction?, Power?
- **EscapeInterdiction** — Interdictor, IsPlayer, IsThargoid?
- **UnderAttack** — Target? ("You", "Fighter", "Mothership")
- **ShieldState** — ShieldsUp
- **HullDamage** — Health, PlayerPilot, Fighter?
- **HeatWarning**, **CockpitBreached**, **SystemsShutdown**, **SelfDestruct** — no fields
- **HeatDamage** — ID?
- **ShipTargeted** — TargetLocked, Ship?, ScanStage?, PilotName?, PilotRank?, ShieldHealth?, HullHealth?, Faction?, LegalStatus?, SquadronID?, Power?, Bounty?, Subsystem?, SubsystemHealth?
- **Scanned** — ScanType ("Cargo", "Crime", "Cabin", "Data", "Unknown")
- **CommitCrime** — CrimeType (FDevIDs `crimes.csv`), Faction, Victim?, Bounty?, Fine?
- **CrimeVictim** — Offender, CrimeType, Fine?, Bounty?
- **FighterDestroyed** — ID?
- **SRVDestroyed** — ID, SRVType?
- **RebootRepair** — Modules[]
- **PayBounties** / **PayFines** — Amount, AllFines?, ShipID, BrokerPercentage?, Faction?
- **RedeemVoucher** — Type ("bounty", "CombatBond", "trade", "settlement", "scannable", "codex", …), Faction?, Amount, Factions?[Faction, Amount], BrokerPercentage?
- **ClearImpound** — ShipType, ShipID, ShipMarketID, MarketID, System?

#### Exploration / exobiology

- **FSSDiscoveryScan** — Progress, BodyCount, NonBodyCount, SystemName, SystemAddress
- **FSSAllBodiesFound** — SystemName, SystemAddress, Count
- **FSSSignalDiscovered** — SystemAddress, SignalName, SignalType? ("USS", "FleetCarrier", "StationCoriolis", "Installation", "Megaship", "TouristBeacon", "NavBeacon", "Titan", "ResourceExtraction", "Combat", "Outpost", …), IsStation?, USSType?, SpawningState?, SpawningFaction?, SpawningPower?, OpposingPower?, ThreatLevel?, TimeRemaining?
- **FSSBodySignals** — BodyName, BodyID, SystemAddress, Signals[Type, Count]
- **DiscoveryScan** — SystemAddress, Bodies
- **NavBeaconScan** — SystemAddress, NumBodies
- **Scan** — see §5.8
- **ScanBaryCentre** — StarSystem, SystemAddress, BodyID, SemiMajorAxis, Eccentricity, OrbitalInclination, Periapsis, OrbitalPeriod, AscendingNode, MeanAnomaly
- **SAAScanComplete** — BodyName, SystemAddress, BodyID, ProbesUsed, EfficiencyTarget
- **SAASignalsFound** — BodyName, SystemAddress, BodyID, Signals[Type, Count], Genuses?[Genus]
- **CodexEntry** — EntryID, Name, SubCategory, Category, Region, System, SystemAddress, BodyID?, Latitude?, Longitude?, VoucherAmount?, NearestDestination?, IsNewEntry?, NewTraitsDiscovered?, Traits?[]
- **ScanOrganic** — ScanType ("Log"/"Sample"/"Analyse"), Genus, Species, Variant?, WasLogged?, SystemAddress, Body (BodyID)
- **SellOrganicData** — MarketID, BioData[Genus, Species, Variant?, Value, Bonus]
- **SellExplorationData** — Systems[], Discovered[], BaseValue, Bonus, TotalEarnings
- **MultiSellExplorationData** — Discovered[SystemName, NumBodies], BaseValue, Bonus, TotalEarnings
- **BuyExplorationData** — System, Cost
- **Screenshot** — Filename, Width, Height, System?, Body?, Latitude?, Longitude?, Altitude?, Heading?
- **DataScanned** — Type
- **DatalinkScan** — Message
- **DatalinkVoucher** — Reward, VictimFaction, PayeeFaction

#### Trade / mining / materials

- **Market** — MarketID, StationName, StationType, CarrierDockingAccess?, StarSystem, Items?[…]. Items are in Market.json; see §7.
- **MarketBuy** — MarketID, Type, Count, BuyPrice, TotalCost
- **MarketSell** — MarketID, Type, Count, SellPrice, TotalSale, AvgPricePaid, StolenGoods?, IllegalGoods?, BlackMarket?
- **BuyTradeData** — System, Cost
- **CollectCargo** — Type, Stolen, MissionID?
- **EjectCargo** — Type, Count, Abandoned, MissionID?, PowerplayOrigin?
- **CargoTransfer** — Transfers[Type, Count, Direction ("toship", "tocarrier", "tosrv"), MissionID?]
- **CargoDepot** — MissionID, UpdateType ("Collect"/"Deliver"/"WingUpdate"), CargoType?, Count?, StartMarketID, EndMarketID, ItemsCollected, ItemsDelivered, TotalItemsToDeliver, Progress
- **ProspectedAsteroid** — Materials[Name, Proportion], Content, Remaining, MotherlodeMaterial?
- **AsteroidCracked** — Body
- **MiningRefined** — Type
- **LaunchDrone** — Type ("Prospector", "Collection", "Hatchbreaker", "FuelTransfer", "Repair", "Research", "Decontamination", "Recon")
- **MaterialCollected** / **MaterialDiscarded** — Category ("Raw"/"Manufactured"/"Encoded"), Name, Count
- **MaterialDiscovered** — Category, Name, DiscoveryNumber
- **MaterialTrade** — MarketID, TraderType ("raw"/"manufactured"/"encoded"), Paid{Material, Category, Quantity}, Received{Material, Category, Quantity}
- **Synthesis** — Name, Materials[Name, Count]
- **ScientificResearch** — MarketID, Category, Name, Count
- **SearchAndRescue** — MarketID, Name, Count, Reward

#### Missions / community goals

See §5.6 for missions.

- **MissionAccepted**, **MissionCompleted**, **MissionFailed**, **MissionAbandoned**, **MissionRedirected**
- **CommunityGoal** — CurrentGoals[CGID, Title, SystemName, MarketName, Expiry, IsComplete, CurrentTotal, PlayerContribution, NumContributors, TopTier{Name, Bonus}, TopRankSize?, PlayerInTopRank?, TierReached?, PlayerPercentileBand, Bonus?]
- **CommunityGoalJoin** / **CommunityGoalDiscard** — CGID, Name, System
- **CommunityGoalReward** — CGID, Name, System, Reward

#### Station services: outfitting / shipyard / engineers / crew

- **Outfitting** — MarketID, StationName, StarSystem, Horizons?, Items?[id, Name, BuyPrice, BuyMercCoinsPrice?]. Items are in Outfitting.json.
- **ModuleInfo** — Modules?[Slot, Item, Power, Priority?]. Modules are in ModulesInfo.json.
- **ModuleBuy** — Slot, BuyItem, MarketID, BuyPrice, BuyMercCoinsPrice?, Ship, ShipID, StoredItem?, SellItem?, SellPrice?
- **ModuleBuyAndStore** — BuyItem, MarketID, BuyPrice, BuyMercCoinsPrice?, Ship, ShipID
- **ModuleSell** — MarketID, Slot, SellItem, SellPrice, Ship, ShipID
- **ModuleSellRemote** — StorageSlot, SellItem, ServerId, SellPrice, Ship, ShipID
- **ModuleStore** — MarketID, Slot, StoredItem, ReplacementItem?, Ship, ShipID, Hot?, EngineerModifications?, Level?, Quality?, Cost?
- **ModuleRetrieve** — MarketID, Slot, RetrievedItem, Ship, ShipID, Hot, EngineerModifications?, Level?, Quality?, SwapOutItem?
- **ModuleSwap** — MarketID, FromSlot, ToSlot, FromItem, ToItem, Ship, ShipID
- **MassModuleStore** — MarketID, Ship, ShipID, Items[Slot, Name, Hot, EngineerModifications?, Level?, Quality?]
- **FetchRemoteModule** — StorageSlot, StoredItem, ServerId, TransferCost, TransferTime, Ship, ShipID
- **StoredModules** — MarketID, StationName, StarSystem, Items[Name, StorageSlot, StarSystem?, MarketID?, TransferCost?, TransferTime?, BuyPrice, BuyMercCoinsPrice?, Hot, EngineerModifications?, Level?, Quality?, InTransit?]
- **Shipyard** — MarketID, StationName, StarSystem, Horizons?, AllowCobraMkIV?, PriceList?[id, ShipType, ShipPrice]. The price list is in Shipyard.json.
- **StoredShips** — StationName, MarketID, StarSystem, ShipsHere[ShipID, ShipType, Value, Hot, Name?], ShipsRemote[ShipID, ShipType, Name?, StarSystem?, ShipMarketID?, TransferPrice?, TransferTime?, Value, Hot, InTransit?]
- **ShipyardBuy** — ShipType, ShipPrice, StoreOldShip?, StoreShipID?, MarketID, SellOldShip?, SellShipID?, SellPrice?
- **ShipyardNew** — ShipType, NewShipID
- **ShipyardSell** — ShipType, SellShipID, ShipPrice, MarketID, System?, ShipMarketID?
- **ShipyardSwap** — ShipType, ShipID, StoreOldShip, StoreShipID, MarketID
- **ShipyardTransfer** — ShipType, ShipID, System, ShipMarketID, Distance, TransferPrice, TransferTime, MarketID
- **ShipyardRedeem** **(community, 2024+)** — ShipType, BundleID?, MarketID?. Written when an ARX/store ship (e.g. `python_nx`) is unlocked at a shipyard.
- **ShipRedeemed** **(community)** — ShipType, NewShipID
- **ShipyardBankDeposit** **(community, Vanguards 2025)** — MarketID, ShipType. Probably written when a ship is deposited into the squadron bank; inferred from the name and the Vanguards ship-sharing feature, not confirmed.
- **SellShipOnRebuy** — ShipType, System, SellShipId, ShipPrice
- **SetUserShipName** — Ship, ShipID, UserShipName, UserShipId?
- **EngineerContribution** — Engineer, EngineerID, Type ("Commodity"/"Materials"/"Credits"/"Bond"/"Bounty"), Commodity?, Material?, Quantity, TotalQuantity
- **EngineerCraft** — Slot, Module, Ingredients[Name, Count], Engineer?, EngineerID, BlueprintID, BlueprintName, Level, Quality, Modifiers[Label, Value?, ValueStr?, OriginalValue?, LessIsGood?], ApplyExperimentalEffect?, ExperimentalEffect?
- **EngineerLegacyConvert** — Slot, Module, IsPreview, Engineer, EngineerID, BlueprintID, BlueprintName, Level, Quality, Modifiers[…]
- **TechnologyBroker** — BrokerType ("human", "guardian", "sirius", "salvation", …), MarketID, ItemsUnlocked[Name], Commodities[Name, Count], Materials[Name, Count, Category]
- **BuyAmmo** — Cost
- **BuyDrones** / **SellDrones** — Type, Count, BuyPrice/SellPrice, TotalCost/TotalSale
- **RefuelAll** / **RefuelPartial** — Cost, Amount
- **Repair** — Cost, Item?, Items?[]
- **RepairAll** — Cost
- **RestockVehicle** — Type, Loadout, ID?, Cost, Count
- **AfmuRepairs** — Module, FullyRepaired, Health
- **RepairDrone** — HullRepaired?, CockpitRepaired?, CorrosionRepaired?
- **FuelScoop** — Scooped, Total
- **ReservoirReplenished** — FuelMain, FuelReservoir
- **CrewHire** — Name, CrewID, Faction, Cost, CombatRank
- **CrewFire** — Name, CrewID
- **CrewAssign** — Name, CrewID, Role
- **NpcCrewPaidWage** — NpcCrewName, NpcCrewId, Amount
- **NpcCrewRank** — NpcCrewName, NpcCrewId, RankCombat

#### Powerplay (incl. 2.0)

See §5.2.

- **PowerplayJoin** / **PowerplayLeave** — Power
- **PowerplayDefect** — FromPower, ToPower
- **PowerplayMerits** — Power, MeritsGained, TotalMerits
- **PowerplayRank** — Power, Rank
- **PowerplayCollect** / **PowerplayDeliver** — Power, Type, Count
- **PowerplayFastTrack** — Power, Cost
- **PowerplaySalary** — Power, Amount
- **PowerplayVote** — Power, Votes, VoteToConsolidate, System?
- **PowerplayVoucher** — Power, Systems[]
- **HoloscreenHacked** — PowerBefore?, PowerAfter
- **RequestPowerMicroResources** / **DeliverPowerMicroResources** — TotalCount, MicroResources[Name, Category, Count], MarketID

#### Squadrons

See §5.3.

- **AppliedToSquadron**, **InvitedToSquadron**, **JoinedSquadron**, **LeftSquadron**, **KickedFromSquadron**, **DisbandedSquadron**, **SquadronCreated**, **SharedBookmarkToSquadron** — SquadronID?, SquadronName
- **CancelledSquadronApplication**, **SquadronApplicationApproved**, **SquadronApplicationRejected** **(community)** — SquadronID, SquadronName
- **SquadronPromotion** / **SquadronDemotion** — SquadronID?, SquadronName, OldRank, NewRank, OldRankName?, NewRankName?

#### Fleet / squadron carriers

See §5.4. Every `Carrier*` owner event now carries an optional `CarrierType` ("FleetCarrier" | "SquadronCarrier").

- **CarrierBuy** — CarrierID, CarrierType?, BoughtAtMarket, Location, SystemAddress, Price, Variant, Callsign
- **CarrierStats** — CarrierID, Callsign, CarrierType?, Name, DockingAccess, AllowNotorious, FuelLevel, JumpRangeCurr, JumpRangeMax, PendingDecommission, plus:
  - SpaceUsage{TotalCapacity, Crew, Cargo, CargoSpaceReserved, ShipPacks, ModulePacks, FreeSpace}
  - Finance{CarrierBalance, ReserveBalance, AvailableBalance, ReservePercent?, TaxRate_shipyard?, TaxRate_rearm?, TaxRate_outfitting?, TaxRate_refuel?, TaxRate_repair?, TaxRate_pioneersupplies?, TaxRate?}
  - Crew[CrewRole, Activated, Enabled?, CrewName?]
  - ShipPacks[PackTheme, PackTier], ModulePacks[PackTheme, PackTier]
- **CarrierLocation** **(community)** — CarrierID, CarrierType?, StarSystem, SystemAddress, BodyID
- **CarrierJumpRequest** — CarrierID, CarrierType?, SystemName, Body?, SystemAddress, BodyID, DepartureTime?
- **CarrierJumpCancelled** — CarrierID, CarrierType?
- **CarrierJump** — the shared system block plus Docked, OnFoot?, StationName?, StationType?, MarketID?, StationGovernment?, StationServices?[], StationEconomy?, Taxi?, Multicrew?, Wanted?
- **CarrierFinance** — CarrierID, CarrierType?, CarrierBalance, ReserveBalance, AvailableBalance, ReservePercent, TaxRate_*?
- **CarrierBankTransfer** — CarrierID, CarrierType?, Deposit?, Withdraw?, PlayerBalance, CarrierBalance
- **CarrierDepositFuel** — CarrierID, Amount, Total, CarrierType?
- **CarrierCrewServices** — CarrierID, CrewRole, CarrierType?, Operation ("Activate"/"Deactivate"/"Pause"/"Resume"/"Replace"), CrewName
- **CarrierDockingPermission** — CarrierID, DockingAccess ("all"/"none"/"friends"/"squadron"/"squadronfriends"), AllowNotorious, CarrierType?
- **CarrierTradeOrder** — CarrierID, CarrierType?, BlackMarket, Commodity, PurchaseOrder?, SaleOrder?, CancelTrade?, Price?
- **CarrierShipPack** / **CarrierModulePack** — CarrierID, CarrierType?, Operation ("BuyPack"/"SellPack"/"RestockPack"), PackTheme, PackTier, Refund?, Cost?
- **CarrierNameChange** **(community)** — CarrierID, CarrierType?, Name, Callsign
- **CarrierDecommission** — CarrierID, ScrapRefund, ScrapTime, CarrierType?
- **CarrierCancelDecommission** — CarrierID, CarrierType?
- **FCMaterials** — MarketID, CarrierName, CarrierID (callsign), Items?[id, Name, Price, Stock, Demand]. The full list is in FCMaterials.json.

#### Odyssey on-foot

See §5.5.

- **Embark** / **Disembark** — SRV, Taxi, Multicrew, ID?, StarSystem, SystemAddress, Body, BodyID, OnStation, OnPlanet, StationName?, StationType?, MarketID?, Crew?[Name, Role] (Embark only)
- **SuitLoadout** / **SwitchSuitLoadout** / **CreateSuitLoadout** — SuitID, SuitName, SuitMods[], LoadoutID, LoadoutName, Modules[SlotName, SuitModuleID, ModuleName, Class, WeaponMods[]]
- **DeleteSuitLoadout** / **RenameSuitLoadout** — SuitID, SuitName, LoadoutID, LoadoutName
- **LoadoutEquipModule** / **LoadoutRemoveModule** — LoadoutName, SuitID, SuitName, LoadoutID, SlotName, ModuleName, Class, WeaponMods[], SuitModuleID
- **BuySuit** — Name, Price, SuitID, SuitMods[]
- **SellSuit** — SuitID, SuitMods[], Name, Price
- **UpgradeSuit** — Name, SuitID, Class, Cost, Resources?[Name, Count]
- **BuyWeapon** — Name, Class, Price, SuitModuleID, WeaponMods[]
- **SellWeapon** — Name, Class, WeaponMods[], Price, SuitModuleID
- **UpgradeWeapon** — Name, Class, SuitModuleID, Cost, Resources?[Name, Count]
- **ShipLocker** / **Backpack** — Items?[], Components?[], Consumables?[], Data?[]. Each entry is {Name, Name_Localised?, OwnerID, MissionID?, Count}.
- **BackpackChange** — Added?[Name, OwnerID, Count, Type, MissionID?], Removed?[…]
- **CollectItems** — Name, Type, OwnerID, Count, Stolen
- **DropItems** — Name, Type, OwnerID, Count, MissionID?
- **UseConsumable** — Name, Type
- **Resupply** **(community)** — no fields; re-read Backpack.json
- **BuyMicroResources** — single form: Name?, Category?, Count?. Multi form: TotalCount?, MicroResources?[Name, Category, Count]. Both have Price and MarketID.
- **SellMicroResources** — TotalCount, MicroResources[Name, Category, Count], Price, MarketID
- **TradeMicroResources** — Offered[Name, Category, Count], TotalCount, Received, Count, Category, MarketID
- **TransferMicroResources** — Transfers[Name, Category, LockerOldCount, LockerNewCount, Direction] (removed from the game; old journals only)
- **ShipLockerMaterials** — discontinued early-Odyssey form of ShipLocker
- **BookDropship** — Retreat?, Cost, DestinationSystem, DestinationLocation
- **CancelDropship** — Refund
- **DropshipDeploy** — StarSystem, SystemAddress, Body, BodyID, OnStation, OnPlanet

#### Colonisation (Trailblazers, Feb 2025)

All of these are **(community)**; see §5.1.

- **ColonisationSystemClaim**, **ColonisationSystemClaimRelease** — StarSystem, SystemAddress
- **ColonisationBeaconDeployed** — no fields
- **ColonisationConstructionDepot** — MarketID, ConstructionProgress, ConstructionComplete, ConstructionFailed, ResourcesRequired[Name, RequiredAmount, ProvidedAmount, Payment]
- **ColonisationContribution** — MarketID, Contributions[Name, Amount]
- **CompleteConstruction** — no fields (in jixxed schemas only, not in EDD)

#### Vehicles / crew / wings / comms

- **LaunchSRV** — SRVType?, Loadout, ID, PlayerControlled
- **DockSRV** — SRVType?, ID
- **LaunchFighter** — FighterType?, Loadout, ID, PlayerControlled
- **DockFighter** — ID
- **FighterRebuilt** — Loadout, ID
- **LaunchVessel** **(community, Rhino 2026)** — VesselType, Loadout, ID, PlayerControlled. Used for the Nomad lander, `VesselType` "lander01".
  - EDD notes the Nomad first appeared through `LaunchFighter` with Loadout "base"/"advanced"/"galactic".
- **VehicleSwitch** — To ("Mothership", "Fighter", "SRV")
- **CrewLaunchFighter** — Telepresence, Crew
- **CrewMemberJoins** / **CrewMemberQuits** — Telepresence?, Crew
- **CrewMemberRoleChange** — Telepresence?, Crew, Role
- **ChangeCrewRole** — Role, Telepresence?
- **JoinACrew** / **QuitACrew** — Captain, Telepresence?
- **KickCrewMember** — Crew, OnCrime, Telepresence?
- **EndCrewSession** — OnCrime, Telepresence?
- **WingAdd** / **WingInvite** — Name
- **WingJoin** — Others[]
- **WingLeave** — no fields
- **SendText** — To, Message, Sent
- **ReceiveText** — From, Message, Channel? ("wing", "local", "voicechat", "friend", "player", "npc", "squadron", "starsystem")

#### Misc

- **MarketID** — undocumented, no payload
- **Status** — the Status.json "event"; see §6

**In the manual or EDD but not in the schemas:**

* `EngineerApply` (pre-3.0)
* `PayLegacyFines` (obsolete)
* `WonATrophyForSquadron` (documented, never seen in the wild)
* `BackPackMaterials`, `BackPack` (removed early-Odyssey forms)

EDD also synthesises its own `EDD*` events; ignore them.

---

## 4. What changed 2023 → 2026 (journal-visible)

| When | Change | Source |
|---|---|---|
| v37, May 2023 | `CarrierJump` written again when docked during a carrier jump; `MissionFailed` written on failure; `StartJump` in taxis (`Taxi`); `IsThargoid` on interdiction events; new `SupercruiseDestinationDrop`; `ThargoidWar{…}` in FSDJump/CarrierJump/Location; `ClearImpound` documented; `Variant` on ScanOrganic/SellOrganicData | manual v37 |
| Late 2023 – 2024 | Supercruise Overcharge (SCO) FSDs → Status `Flags2` bit 20. Supercruise Assist bit 21 and NPC Crew Active bit 22 also observed | jixxed, EDD (community) |
| 2024 | `ShipyardRedeem`/`ShipRedeemed` (store/ARX ships), `CarrierLocation` at startup and after jumps; Titan-war materials/commodities | jixxed, EDD |
| 2024-10 Ascendancy | Powerplay 2.0: `PowerplayMerits`, `PowerplayRank`; system fields `ControllingPower`, `PowerplayState`, `PowerplayStateControlProgress`, `PowerplayStateReinforcement`, `PowerplayStateUndermining`, `PowerplayConflictProgress[]`; `HoloscreenHacked`; `Request/DeliverPowerMicroResources`; `Powerplay.Votes` removed; new Power* data microresources | jixxed, EDD |
| 2025-02 Trailblazers | `Colonisation*` events; station types `SpaceConstructionDepot`, `PlanetaryConstructionDepot`, `DockablePlanetStation`; services `colonisationcontribution`, `registeringcolonisation`, `refinery`; `StationName_Localised` "System Colonisation Ship"; ModulesInfo slot `ColonisationSuite`; commodities Steel, Haematite | jixxed, EDD, FDevIDs |
| 2025-08 Vanguards | `CarrierType:"SquadronCarrier"` on all carrier events; squadron ranks moved to a 0–14 scale (`CurrentRankName` localised); `ShipyardBankDeposit`; service `SquadronBank`; `Statistics.Squadron{…}` (bank + leaderboards incl. colonisation/operations) | jixxed, EDD |
| 2026-04 (manual v38) | `Statistics.Passengers` per-type delivered/ejected counters (Lynx Highliner) | manual v38 |
| 2026-06 Operations / "Nomad" (4.4.0.0) | `GameModeChange{GameMode:"MainGame"/"Operation"}`; **MercCoins** currency: `Statistics.Bank_Account.MercCoins_*`, `BuyMercCoinsPrice` on Outfitting/ModuleBuy/StoredModules; FDevIDs outfitting category `mercgear` (pre-engineered modules); `Label_Localised` on engineering modifiers; `StationFaction.Name_Localised`; new allegiance `FrontlineSolutions`; Nomad lander (`lander01`) | jixxed commits, FDevIDs, EDD |
| 2026-09 Rhino (4.4.1.0) | `LaunchVessel`; SRV `mev_rhino` (surface mining rig, refinery, deposit scanner modules); new minable commodities (Iridium, Helium, Helium-3, Bastnasite, Deuterium, Thortveitite, Quartz Pyroxenite, Olivine, Periclase Dunite, Sapphire, Diamond, Ruby, Magnesite); `GameModeChange` field-name fix | jixxed, FDevIDs, EDD |
| 2026-10 | EDD notes new microresources `OperationsStrikeData`, `OperationsCounterAttackData`, `NM_Seed` | EDD `MCMRType.cs` |

---

## 5. Detail for the event families called out in the brief

### 5.1 Colonisation

* `ColonisationSystemClaim{StarSystem, SystemAddress}` — the player paid for a claim at a System Colonisation contact.
  * `ColonisationSystemClaimRelease` — same fields, when the claim is abandoned.
* `ColonisationBeaconDeployed{}` — the colonisation beacon was deployed in the claimed system (no payload; correlate with the last `Location`/`FSDJump`).
* `ColonisationConstructionDepot` — written **every ~15 s while docked** at a construction depot. It's the authoritative build-progress snapshot:
  * `MarketID`
  * `ConstructionProgress` (0..1)
  * `ConstructionComplete`, `ConstructionFailed`
  * `ResourcesRequired[]` of {`Name` "$aluminium_name;", `Name_Localised`, `RequiredAmount`, `ProvidedAmount`, `Payment` (cr/unit)}
  * EDD de-duplicates identical consecutive snapshots.
* `ColonisationContribution{MarketID, Contributions[Name, Name_Localised, Amount]}` — written on delivery.
* `CompleteConstruction{}` — in jixxed only ("upon completing a colonisation construction").
* Related context:
  * `Docked`/`Location` with `StationType` "SpaceConstructionDepot"/"PlanetaryConstructionDepot" and `StationName_Localised` "System Colonisation Ship"
  * Conflict `Stake_Localised` "System Colonisation Ship"
  * `Missions.Active[].Name_Localised` like "Construct primary port" (colonisation architect "missions")
  * `ModuleInfo` slot `ColonisationSuite`
  * module `int_colonisation`
  * `Statistics.Squadron.Squadron_Leaderboard_colonisation_contribution_highestcontribution`

### 5.2 Powerplay 2.0

* `PowerplayJoin/Leave/Defect` as before.
* `Powerplay{Power, Rank, Merits, TimePledged}` at login (`Votes` gone after Ascendancy).
* `PowerplayMerits{Power, MeritsGained, TotalMerits}` on each merit award. EDD parses these as doubles, so be tolerant of fractional values. It names no system: Elite Nexus places each award in the system the commander is in when it's written (BGS & Powerplay's merits per system).
* `PowerplayRank{Power, Rank}` on rank-up. PP2 ranks are numeric and far beyond PP1's 0–5, so don't cap them.
* System state appears in `FSDJump`/`Location`/`CarrierJump`:
  * `Powers[]`
  * `ControllingPower`
  * `PowerplayState` — EDD enum: InPrepareRadius, Prepared, Exploited, Contested, Controlled, Turmoil, HomeSystem, **Fortified**, **Stronghold**, **Unoccupied**
  * `PowerplayStateControlProgress` (0..1+)
  * `PowerplayStateReinforcement`, `PowerplayStateUndermining` (merit totals)
  * `PowerplayConflictProgress[{Power, ConflictProgress}]` for contested and acquisition systems
* On-foot Powerplay:
  * `HoloscreenHacked{PowerBefore?, PowerAfter}`
  * `RequestPowerMicroResources` / `DeliverPowerMicroResources{TotalCount, MicroResources[Name ("powerfinancialrecords"…), Category, Count], MarketID}`
* Legacy PP1 events still defined (`PowerplayCollect/Deliver/FastTrack/Salary/Vote/Voucher`). Keep parsing them for old journals; don't expect most of them now.

### 5.3 Squadrons / Vanguards

* Membership events are unchanged in shape. Jixxed adds application approved/rejected/cancelled.
* **Ranks:** EDD models the old 0–4 rank system (Leader, Senior Officer, Officer, Agent, Rookie) for pre-Vanguards journals. After 2025-08-19 ranks are **0–14 with custom names**, so display `CurrentRankName_Localised`/`NewRankName_Localised` rather than mapping numbers.
* **Squadron carriers:** same event set as fleet carriers, distinguished by `CarrierType:"SquadronCarrier"`. Spansh's router in EDD uses capacity 60 000 t and mass 15 000 for squadron carriers vs 25 000/25 000 for personal carriers.
  * Track both carriers separately: EDD keeps a `FleetCarrier` and a `SquadronCarrier` state.
* **Squadron bank:** `ShipyardBankDeposit{MarketID, ShipType}`, the `SquadronBank` station service, and `Statistics.Squadron` bank counters (`Squadron_Bank_*`) plus leaderboard bests (`…_aegis_…`, `…_bgs_…`, `…_powerplay_…`, `…_operationscore_…`, …).

### 5.4 Fleet carriers

* **CarrierID** equals the carrier's `MarketID`.
* `CarrierStats` is written when the owner opens carrier management and holds the full snapshot (finance, space, crew, packs).
* `CarrierLocation` is written at startup and after each jump (community), so it's the cheapest way to know where your carrier is.
* `CarrierJumpRequest` is written at request time with `DepartureTime` (ISO). `CarrierJumpCancelled` cancels it.
* Jump timings the journal doesn't carry (checked 2026-10-18; used by `nexus_model::carrier::timing`):
  * Countdown: 15 minutes from request to departure, and a **5-minute cooldown** after the jump. Source: Frontier's
    update 1.43 patch notes (2020-06-09, carriers' launch): "Fleet Carriers take 15 minutes to prepare for a jump,
    and have a 5 minute cool-down after" (quoted by mp1st.com from the Frontier forums). No later change found.
    Busy servers set a later `DepartureTime` (players report 45-50 minutes at peak), so trust the event's time.
  * Pre-flight lockdown (pads closed, services stopped) at **3 min 20 s** before departure, and cancelling allowed
    until **10 minutes** before: community wiki (elite-dangerous.fandom.com, Drake-Class Carrier), not Frontier. One
    player's post says the lockdown starts about 2 minutes before, so treat these as a guide.
  * How long the jump itself takes isn't documented; `CarrierJump` (aboard) or `CarrierLocation` marks the arrival.
* `CarrierDecommission{CarrierID, ScrapRefund, ScrapTime}`: `ScrapTime` is a Unix time in seconds (an integer, not
  ISO), when the carrier is scrapped and the refund paid. `CarrierCancelDecommission{CarrierID}` calls it off. A
  later `CarrierStats.PendingDecommission:false` also means it was cancelled. Past `ScrapTime` the carrier is gone;
  the journal writes nothing more for it (Elite Nexus: `Carrier::decommissioned`).
* Squadron carriers (checked 2026-10-18): Elite Nexus routes each event by `CarrierType`, and events without one
  (journals from before Vanguards, a few events) by `CarrierID` (the squadron carrier's, else the personal one), as
  EDDiscovery does. No carrier event says *who* acted: `CarrierJumpRequest`, `CarrierTradeOrder` and the rest are
  written to the journal of the commander who did it, so other members' jumps and orders never appear in yours.
* `CarrierJump` is written to a player **docked on (or on foot aboard) the carrier** during the jump. It's a `Location`-like event without distance or fuel.
* `CarrierTradeOrder` covers buy/sell/cancel orders. `PurchaseOrder`/`SaleOrder` are quantities; `Price` is per unit.
* `CarrierFinance` covers tax/reserve changes. `CarrierBankTransfer` covers credits in and out.
* `CarrierDepositFuel` is fuel donation (tritium), `Total` is the new level.
* Crew roles (`CarrierCrewServices.CrewRole` / `CarrierStats.Crew[].CrewRole`):
  * BlackMarket, Captain, Refuel, Repair, Rearm, Commodities, VoucherRedemption
  * Exploration (Universal Cartographics), Shipyard, Outfitting, CarrierFuel, VistaGenomics, PioneerSupplies, Bartender
* `FCMaterials` + FCMaterials.json is the carrier bartender price list.
* `Market` at a carrier includes `CarrierDockingAccess`.

### 5.5 Odyssey on-foot

* **`ShipLocker`** is the full locker inventory. It's written to ShipLocker.json whenever it changes; the full list also appears in the journal at startup (if in a ship) and when boarding.
  * Categories: `Items`, `Components`, `Consumables`, `Data`. Entries: {Name, Name_Localised?, OwnerID (0 = own), MissionID?, Count}.
* **`Backpack`** has the same structure, written to Backpack.json.
  * **`BackpackChange{Added?[], Removed?[]}`** is the delta (entries add a `Type`). It can be written alongside `UseConsumable`, `CollectItems`, etc.
  * `Resupply` means re-read Backpack.json.
* Suits and weapons:
  * Lifecycle: `BuySuit`/`SellSuit`/`UpgradeSuit` and `BuyWeapon`/`SellWeapon`/`UpgradeWeapon`.
  * Since U14, `Upgrade*` carries `Cost` (credits) and `Resources[]`, which is exactly what a planner needs to verify costs.
  * `Class` is the grade (1–5).
  * Mods appear in `SuitMods[]` / `WeaponMods[]` as fd names, e.g. `suit_increasedo2capacity`, `weapon_stability`.
* Loadouts: `SuitLoadout` (on disembark/startup), `SwitchSuitLoadout`, `Create/Delete/RenameSuitLoadout`, `LoadoutEquipModule`, `LoadoutRemoveModule`.
* Exobiology:
  * `ScanOrganic`: the first sample writes `ScanType` "Log", later samples write "Sample", and completion of the third sample writes "Analyse". EDD handles two consecutive "Sample" events, so key progress on Analyse rather than counting Sample events.
  * It also carries `Genus`, `Species`, `Variant` (since v37), `WasLogged`, `SystemAddress`, `Body` (BodyID).
  * `SellOrganicData.BioData[]` holds `Value` and `Bonus` (first-logged bonus).
  * Planet genus lists come from `SAASignalsFound.Genuses` (after DSS) and `FSSBodySignals.Signals[Type="$SAA_SignalType_Biological;"]`.
* `CodexEntry` carries `EntryID`, `Category`/`SubCategory`/`Name` symbols, `Region` ("$Codex_RegionName_18;" → Inner Orion Spur), `BodyID`/`Latitude`/`Longitude` for surface items, `IsNewEntry` and `VoucherAmount`.
* Transport: `BookTaxi`/`CancelTaxi`, `BookDropship`/`CancelDropship`/`DropshipDeploy`. Status `Flags2` bit 1 (InTaxi).
* Microresources:
  * `BuyMicroResources` (single or multi form), `SellMicroResources`, `TradeMicroResources` (bartender barter), `RequestPowerMicroResources`/`DeliverPowerMicroResources`.
  * Category strings in events can be `"Item"/"Component"/"Data"/"Consumable"` or `$MICRORESOURCE_CATEGORY_Data;`; normalise.
  * The names list (196 entries) is FDevIDs `microresources.csv`.

### 5.6 Missions

* **`MissionAccepted`**:
  * Required: `Faction`, `Name` (internal, e.g. `Mission_Massacre`, `Mission_MassacreWing`, `Mission_Assassinate`, `Mission_Delivery`, `Mission_Collect`, `Mission_Courier`, `Mission_OnFoot_*`), `LocalisedName`, `MissionID`, `Wing`.
  * `Influence`/`Reputation`: the manual still says None/Low/Med/High, but current journals use `"+"`…`"+++++"`.
  * Optional: `Reward`, `Expiry`.
  * Optional by type:
    * Delivery/collect: `Commodity`, `Count`
    * Kill/assassinate: `Target`, `TargetType`, `TargetFaction`, `KillCount`
    * Destination: `DestinationSystem`, `DestinationStation`, `DestinationSettlement` (on-foot)
    * Altruism: `Donation` (string)
    * Passenger: `PassengerCount`, `PassengerVIPs`, `PassengerWanted`, `PassengerType`
    * After redirect: `NewDestinationSystem`, `NewDestinationStation`
  * **Massacre stacking:** group active missions whose `Name` starts with `Mission_Massacre`. Key them by (`TargetFaction`, `DestinationSystem` (the target system), `TargetType` e.g. `$MissionUtil_FactionTag_Pirate;`). Sum `KillCount` per issuing `Faction`. Kills are credited per `Bounty`/`FactionKillBond` where `VictimFaction == TargetFaction`. Remaining counts aren't journaled; you must count yourself.
* **`MissionRedirected`** — MissionID, Name, `NewDestinationStation`/`NewDestinationSystem`, `OldDestinationStation`/`OldDestinationSystem`. Massacre and assassination missions redirect back to the giver when complete.
* **`MissionCompleted`** — `Reward`, `Donated`, `PermitsAwarded[]`, `CommodityReward[Name, Count]`, `MaterialsReward[Name, Category, Count]` (Odyssey microresource rewards use `$MICRORESOURCE_CATEGORY_*;`), `FactionEffects[Faction, Effects[Effect, Trend], Influence[SystemAddress, Trend, Influence], ReputationTrend, Reputation]`.
* **`MissionFailed`** (when the mission fails, v37+) and **`MissionAbandoned`** — MissionID, Name, Fine?.
* **`Missions`** at startup — `Active/Failed/Complete[MissionID, Name, PassengerMission, Expires (seconds remaining)]`. Use it to reconcile your mission store after offline time. The journal does **not** replay the original `MissionAccepted` details, so persist them yourself.
* **`CargoDepot`** — wing-mission delivery progress.
* **Shared wing missions:** your own wing missions (`MissionAccepted` with `Wing: true`) write `MissionCompleted` like any
  other. When a wingmate hands in a mission they shared with you, your journal gets no line for it: no
  `MissionCompleted`, no reward line (the credits and materials arrive anyway; `Status.json`'s `Balance` and the next
  login's `Materials` show them). Reported by edda-app's author, watching the journal live over three shared hand-ins
  (github.com/terakilobyte/edda-app/pull/192, 2026-10-06), and on Frontier's forums in 2018 (`MissionCompleted` written
  for the wing leader but not the members). Not checked against our own journals: none here has a shared mission.
* **`Passengers`** — manifest at startup.

### 5.7 Mining

* **`ProspectedAsteroid`**:
  * `Materials[{Name, Name_Localised?, Proportion (%)}]`
  * `Content` ("$AsteroidMaterialContent_Low;"/"Medium"/"High" + `Content_Localised`)
  * `Remaining` (% of the asteroid left, 100 = untouched)
  * `MotherlodeMaterial?` (core / deep-core asteroid, e.g. "Painite", "LowTemperatureDiamond", "Opal", …)
* **`AsteroidCracked{Body}`** — a motherlode was split by seismic charges ("… A Ring").
* **`MiningRefined{Type "$painite_name;", Type_Localised}`** — one ton refined.
* `LaunchDrone{Type}` for prospector and collector limpets.
* `Statistics.Mining{Mining_Profits, Quantity_Mined, Materials_Collected}`.
* **Rhino (Sept 2026):** the MEV Rhino SRV adds surface mining (modules `int_miningrigdeployment`, `int_refinerysrv_size1_class3`, deposit scanner). New commodities are listed in §4 and in `commodity.csv` ids 129046165–129046177. **(community, checked against real journals by EDSMT, EDLD and ED-Outrider, 4.4.1.x)**
  * No new mining event: the Rhino's refinery writes `MiningRefined`, one per tonne, into the SRV's hold, by-products each under their own name. It names what came out, not the deposit, and carries no position.
  * A body's mining locations: `FSSBodySignals`/`SAASignalsFound` signal `$PlanetaryMiningLocation_Name;` with a `Count` (e.g. 13); the DSS lists them, numbered, in the navigation panel.
  * Which location: `Touchdown`/`Liftoff` `NearestDestination` (see §3), the location nearest the ship, not necessarily the one targeted. Status.json `Destination.Name` names a targeted one, seen both as `"$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=3;"` and `"$SAA_Unknown_Signal:#index=15;"`; it's written only for targets locked from the ship (a rig or deposit picked from the SRV writes nothing) and isn't in the journal.
    * `PlayerControlled:false` on a `Liftoff` is the ship sent away while the commander stays in the SRV, and on a `Touchdown` the ship called back to it (the manual: Latitude/Longitude only when the player flies the ship). The commander hasn't moved, so keep the location the landing named when these name none. A piloted `Liftoff` is leaving: clear it, so a Rhino dropped later from a hover isn't credited to the location left behind. EDRhinoSpotter does both (4.5.2: "Location cleared on takeoff; kept when the ship is sent away from the SRV"); EDSMT keeps the last named location until a jump. No real `PlayerControlled:false` line has turned up, so whether they carry `NearestDestination` is unknown.
    * The manual: `NearestDestination` "is included if within 50km of a location listed in the nav panel"; otherwise the key is absent (a real 2026-08-25 Touchdown has none).
    * The Rhino can be dropped from a hovering ship (EDSMT's guide: deploy "out of glide and under 30 m"; EDRhinoSpotter 4.1.13 "Dropping the SRV from a hover fills Location", from Status.json). Then no `Touchdown` or `Liftoff` is written and the journal names no location at all; only Status.json `Destination`, while the location is still targeted, could say which. Elite Nexus takes the last mining location targeted on the body (kept through a deselection, dropped when something else is targeted or the body is left) at the Rhino's `LaunchSRV` when no landing named one or flew down far from every location, and keeps these drops in settings by the `LaunchSRV`'s time, so a journal read again names those runs too. Such a location ends at `DockSRV`: the ship, still hovering, can fly on without a word in the journal.
  * Not written anywhere: deposits (position, commodity, amount, density) and rigs. Tools take positions from Status.json while the Rhino refines. The game warns at 4 km from a rig and destroys it at 5 km (EDSMT).

### 5.8 Exploration

* **`FSSDiscoveryScan`** ("honk") — `Progress` (0..1 fraction of bodies already known), `BodyCount`, `NonBodyCount`, `SystemName`, `SystemAddress`.
* **`FSSAllBodiesFound`** — `Count`.
* **`FSSSignalDiscovered`** — one per signal: stations/carriers (`IsStation`), USS (`USSType`, `ThreatLevel`, `TimeRemaining`), conflict zones, powerplay signals (`SpawningPower`, `OpposingPower`), Titans/Thargoid signals.
* **`Scan`**:
  * `ScanType`: "AutoScan", "Basic", "Detailed", "NavBeacon", "NavBeaconDetail".
  * `BodyName`, `BodyID`, `StarSystem`, `SystemAddress`, `DistanceFromArrivalLS`.
  * `Parents[]` is an ordered list of single-key objects, nearest first, e.g. `[{"Ring":3},{"Planet":2},{"Star":0}]`. "Null" means a barycentre.
  * Stars: `StarType` (O B A F G K M L T Y TTS AeBe W WN WNC WC WO CS C CN CJ CH CHd MS S D DA DAB DAO DAZ DAV DB DBZ DBV DO DOV DQ DC DCV DX N H X SupermassiveBlackHole A_BlueWhiteSuperGiant F_WhiteSuperGiant M_RedSuperGiant M_RedGiant K_OrangeGiant RoguePlanet Nebula StellarRemnantNebula), `Subclass`, `StellarMass`, `AbsoluteMagnitude`, `Age_MY`, `Luminosity`.
  * Planets: `PlanetClass`, `TerraformState`, `Atmosphere`, `AtmosphereType`, `AtmosphereComposition[Name, Percent]`, `Volcanism`, `MassEM`, `SurfaceGravity` (m/s²), `SurfaceTemperature` (K), `SurfacePressure` (Pa), `Landable`, `Composition{Ice, Rock, Metal}` (fractions), `Materials[Name, Percent]`, `TidalLock`.
  * Orbits: `SemiMajorAxis` (m), `Eccentricity`, `OrbitalInclination`, `Periapsis`, `OrbitalPeriod` (s), `AscendingNode`, `MeanAnomaly`, `RotationPeriod` (s), `AxialTilt` (rad).
  * Common to both: `Radius` (m), `Rings[Name, RingClass, MassMT, InnerRad, OuterRad]`, `ReserveLevel`, `WasDiscovered`, `WasMapped`, `WasFootfalled` (Odyssey).
* **`ScanBaryCentre`** — orbital elements of a barycentre (BodyID = the Null parent id), written when one member of a binary is scanned.
* **`SAAScanComplete`** — DSS mapping finished: `ProbesUsed`, `EfficiencyTarget` (efficient-mapping bonus when ProbesUsed ≤ EfficiencyTarget).
* **`SAASignalsFound`** — `Signals[Type ("$SAA_SignalType_Biological;", "…Geological;", "…Human;", "…Thargoid;", "…Guardian;", "…Other;", or ring hotspots e.g. "Painite", "LowTemperatureDiamond"), Count]` and `Genuses[]` (since U13).
* **`FSSBodySignals`** — per-body signal counts from the FSS (before DSS).
* Selling: `SellExplorationData` (legacy) / `MultiSellExplorationData` (current), `SellOrganicData`.
* `Statistics.Exploration` / `Exobiology` totals.

### 5.9 Statistics

Top-level groups:

* Bank_Account — Current_Wealth, Spent_On_*, Insurance_Claims, Owned_Ship_Count, Suits/Weapons, Premium_Stock, MercCoins_*
* Combat
* Crime
* Smuggling
* Trading
* Mining
* Exploration
* Passengers — v38 adds `Passengers_Missions_<Type>_<vip|bulk>_<delivered|ejected>` for Tourist, Refugee, AidWorker, POW, Soldiers, Protestors, Prisoners, FreedomFighters, Politicians, PoliticalPrisoner, SecurityForces, Businessmen, ScienceTeams, Explorers, Celebrities, HeadOfState, Criminal, Medical, Disgruntled
* Search_And_Rescue
* Crafting — Recipes_Generated_Rank_1..5, Suit/Weapon mods, upgrades
* Crew
* Multicrew
* Material_Trader_Stats
* FLEETCARRIER
* Exobiology
* TG_ENCOUNTERS
* CQC
* Squadron

Keys appear and disappear by account history, so model each group as a map.

### 5.10 The shared "system block" (FSDJump / Location / CarrierJump)

* `StarSystem`, `SystemAddress`, `StarPos[x,y,z]` (ly, Sol-centred), `Body`, `BodyID`, `BodyType` ("Star", "Planet", "PlanetaryRing", "StellarRing", "Station", "AsteroidCluster", "Null").
* `SystemAllegiance` (FDevIDs `systemallegiance.csv`, incl. `PlayerPilots`, `Thargoid`, `Guardian`, and since 2026 `FrontlineSolutions`).
* `SystemEconomy`/`SystemSecondEconomy` ("$economy_X;", see `economy.csv`), `SystemGovernment`, `SystemSecurity`, `Population`.
* `Factions[{Name, FactionState, Government, Influence, Allegiance, Happiness, MyReputation, ActiveStates[State], PendingStates[State, Trend], RecoveringStates[State, Trend], SquadronFaction?, HappiestSystem?, HomeSystem?}]`.
* `SystemFaction{Name, FactionState?}`.
* `Conflicts[{WarType ("war"/"civilwar"/"election"), Status ("pending"/"active"/""), Faction1{Name, Stake, WonDays}, Faction2{…}}]`.
* `ThargoidWar{CurrentState, NextStateSuccess, NextStateFailure, SuccessStateReached, WarProgress, RemainingPorts, EstimatedRemainingTime}`.
* The Powerplay fields from §5.2.
* `Docked` (Location/CarrierJump), `Taxi`, `Multicrew`, `Wanted`.

---

## 6. Status.json — exact specification

Status.json is rewritten whole (not appended) every time something changes, roughly every few seconds. It has the journal shape with `"event":"Status"`.

* At the main menu it's `{timestamp, event, Flags:0}` only.
* Fields appear and disappear by context: ship vs SRV vs on foot vs not in game.
* Read it with retries; the game may hold it open for writing.
* Only `Flags` is guaranteed.

### Flags (ship/SRV), bit → meaning (manual v38)

| Bit | Value | Hex | Meaning |
|---|---|---|---|
| 0 | 1 | 0x00000001 | Docked (on a landing pad) |
| 1 | 2 | 0x00000002 | Landed (on planet surface) |
| 2 | 4 | 0x00000004 | Landing Gear Down |
| 3 | 8 | 0x00000008 | Shields Up |
| 4 | 16 | 0x00000010 | Supercruise |
| 5 | 32 | 0x00000020 | FlightAssist Off |
| 6 | 64 | 0x00000040 | Hardpoints Deployed |
| 7 | 128 | 0x00000080 | In Wing |
| 8 | 256 | 0x00000100 | Lights On |
| 9 | 512 | 0x00000200 | Cargo Scoop Deployed |
| 10 | 1024 | 0x00000400 | Silent Running |
| 11 | 2048 | 0x00000800 | Scooping Fuel |
| 12 | 4096 | 0x00001000 | SRV Handbrake |
| 13 | 8192 | 0x00002000 | SRV using Turret view |
| 14 | 16384 | 0x00004000 | SRV Turret retracted (close to ship) |
| 15 | 32768 | 0x00008000 | SRV DriveAssist |
| 16 | 65536 | 0x00010000 | FSD MassLocked |
| 17 | 131072 | 0x00020000 | FSD Charging |
| 18 | 262144 | 0x00040000 | FSD Cooldown |
| 19 | 524288 | 0x00080000 | Low Fuel (< 25%) |
| 20 | 1048576 | 0x00100000 | Over Heating (> 100%) |
| 21 | 2097152 | 0x00200000 | Has Lat Long |
| 22 | 4194304 | 0x00400000 | Is In Danger |
| 23 | 8388608 | 0x00800000 | Being Interdicted |
| 24 | 16777216 | 0x01000000 | In Main Ship |
| 25 | 33554432 | 0x02000000 | In Fighter |
| 26 | 67108864 | 0x04000000 | In SRV |
| 27 | 134217728 | 0x08000000 | HUD in Analysis mode |
| 28 | 268435456 | 0x10000000 | Night Vision |
| 29 | 536870912 | 0x20000000 | Altitude from Average radius |
| 30 | 1073741824 | 0x40000000 | FSD Jump (in hyperspace) |
| 31 | 2147483648 | 0x80000000 | SRV High Beam |

Example: `16842765 = 0x0101000D` decodes to bits 24, 16, 3, 2, 0: in main ship, mass locked, shields up, gear down, docked.

### Flags2 (mostly on-foot), bit → meaning

| Bit | Value | Hex | Meaning | Source |
|---|---|---|---|---|
| 0 | 1 | 0x00000001 | OnFoot | manual |
| 1 | 2 | 0x00000002 | InTaxi (or dropship/shuttle) | manual |
| 2 | 4 | 0x00000004 | InMulticrew (in someone else's ship) | manual |
| 3 | 8 | 0x00000008 | OnFootInStation | manual |
| 4 | 16 | 0x00000010 | OnFootOnPlanet | manual |
| 5 | 32 | 0x00000020 | AimDownSight | manual |
| 6 | 64 | 0x00000040 | LowOxygen | manual |
| 7 | 128 | 0x00000080 | LowHealth | manual |
| 8 | 256 | 0x00000100 | Cold | manual |
| 9 | 512 | 0x00000200 | Hot | manual |
| 10 | 1024 | 0x00000400 | VeryCold | manual |
| 11 | 2048 | 0x00000800 | VeryHot | manual |
| 12 | 4096 | 0x00001000 | Glide Mode | manual |
| 13 | 8192 | 0x00002000 | OnFootInHangar | manual |
| 14 | 16384 | 0x00004000 | OnFootSocialSpace | manual |
| 15 | 32768 | 0x00008000 | OnFootExterior | manual |
| 16 | 65536 | 0x00010000 | BreathableAtmosphere | manual (its hex column has a typo "0001 0001") |
| 17 | 131072 | 0x00020000 | Telepresence Multicrew | manual v33 |
| 18 | 262144 | 0x00040000 | Physical Multicrew | manual v33 |
| 19 | 524288 | 0x00080000 | FSD Hyperdrive Charging (charging a hyperspace jump, not a supercruise one) | manual v36 |
| 20 | 1048576 | 0x00100000 | Supercruise Overcharge active (EDD names it "SupercruiseOverdrive") | **community** (jixxed + EDD) |
| 21 | 2097152 | 0x00200000 | Supercruise Assist active | **community** |
| 22 | 4194304 | 0x00400000 | NPC Crew Active | **community** |
| 23–31 | | | undefined (EDD: "23-31 not defined") | |

EDD's mode logic:

* **Vehicle:** bits 24/25/26 of `Flags` (main ship / fighter / SRV), or `Flags2` bit 0 (on foot).
* **On-foot location:** `Flags2` bits 3/4/13/14/15.
* **Taxi / multicrew:** `Flags2` bits 1/2, plus 17/18.

### GuiFocus

| Value | Meaning |
|---|---|
| 0 | NoFocus |
| 1 | InternalPanel (right) |
| 2 | ExternalPanel (left) |
| 3 | CommsPanel (top) |
| 4 | RolePanel (bottom) |
| 5 | StationServices |
| 6 | GalaxyMap |
| 7 | SystemMap |
| 8 | Orrery |
| 9 | FSS mode |
| 10 | SAA mode |
| 11 | Codex |

GuiFocus disappears in some landed states; treat a missing value as 0.

### LegalState

`"Clean"`, `"IllegalCargo"`, `"Speeding"`, `"Wanted"`, `"Hostile"`, `"PassengerWanted"`, `"Warrant"`. Accept unknown strings.

### Fields

| Field | Type | Present when | Notes |
|---|---|---|---|
| `timestamp`, `event` | string | always | `event` is `"Status"` |
| `Flags` | u32 | always | table above |
| `Flags2` | u32 | Odyssey client | table above |
| `Pips` | [u8;3] | in ship/SRV | **half-pips** [SYS, ENG, WEP], sum 12 |
| `FireGroup` | int | in ship | 0-based |
| `GuiFocus` | int | in ship | table above |
| `Fuel` | {FuelMain, FuelReservoir} | in ship/SRV | tons (f64) |
| `Cargo` | number | in ship/SRV | tons carried |
| `LegalState` | string | in game | see above |
| `Latitude`, `Longitude` | f64 | near/on a planet (Flags bit 21) | updates every 0.02° flying, 0.0005° in SRV |
| `Altitude` | f64 (schema says int) | near planet | m. Bit 29 set: above average radius; clear: raycast to surface |
| `Heading` | number | near planet | degrees |
| `BodyName` | string | near a body / on foot in station | e.g. "Harow A 1" or the station name |
| `PlanetRadius` | f64 | near planet | m |
| `Balance` | i64 | in game | credits (since v32) |
| `Destination` | {System, Body, Name, Name_Localised?} | when a destination is selected | `System` = SystemAddress, `Body` = BodyID, `Name` = target name (may be a `$…;` symbol; a planetary mining location is `$SAA_Unknown_Signal:…#index=N;`, see §5.7) |
| `Oxygen` | f64 0..1 | on foot | |
| `Health` | f64 0..1 | on foot | |
| `Temperature` | f64 | on foot | kelvin |
| `SelectedWeapon` (+`_Localised`) | string | on foot | e.g. `$humanoid_fists_name;`, `$wpn_m_sniper_plasma_charged_name;` |
| `Gravity` | f64 | on foot | relative to 1 G |

---

## 7. Companion files

**Read algorithm (as EDD does it):**

1. When the journal line for one of these events arrives, read the JSON file.
2. Retry up to ~2.5 s if it's locked, missing, or its `timestamp` is older than the journal line.
3. Accept only if `event` matches and `timestamp` equals the journal line's timestamp.
4. Discard it if the file is newer than the event, since the event has already been superseded.
5. Don't delete Frontier's files (EDD deletes only corrupt ones).

| File | Written when | Journal event | Content (beyond timestamp/event) |
|---|---|---|---|
| **Status.json** | continuously, on change | (none in journal) | §6 |
| **Cargo.json** | cargo changes; at startup | `Cargo` (first per session has `Inventory`, later ones only `Vessel`/`Count`) | `Vessel` ("Ship"/"SRV"), `Count`, `Inventory[{Name, Name_Localised?, Count, Stolen, MissionID?}]` |
| **Market.json** | commodity market opened | `Market` (no `Items`) | `MarketID, StationName, StationType, CarrierDockingAccess?, StarSystem, Items[{id, Name ("$gold_name;"), Name_Localised, Category ("$MARKET_category_metals;"), Category_Localised, BuyPrice, SellPrice, MeanPrice, StockBracket, DemandBracket, Stock, Demand, Consumer, Producer, Rare}]`. `id` = FDevIDs `commodity.csv`/`rare_commodity.csv` id |
| **NavRoute.json** | route plotted; emptied on clear | `NavRoute` / `NavRouteClear` | `Route[{StarSystem, SystemAddress, StarPos[3], StarClass}]` (first entry = current system) |
| **ModulesInfo.json** | RHS Modules panel viewed and data changed | `ModuleInfo` | `Modules[{Slot, Item, Power (MW), Priority}]` in panel order |
| **Outfitting.json** | outfitting opened | `Outfitting` | `MarketID, StationName, StarSystem, Horizons, Items[{id, Name, BuyPrice, BuyMercCoinsPrice?}]`. `id` = FDevIDs `outfitting.csv` |
| **Shipyard.json** | shipyard opened | `Shipyard` | `MarketID, StationName, StarSystem, Horizons, AllowCobraMkIV, PriceList[{id, ShipType, ShipType_Localised?, ShipPrice}]` |
| **Backpack.json** | on-foot backpack changes | `Backpack` (also `BackpackChange` deltas, `Resupply`) | `Items/Components/Consumables/Data[{Name, Name_Localised?, OwnerID, MissionID?, Count}]` |
| **ShipLocker.json** | locker contents change | `ShipLocker` (full list in journal at startup and on boarding) | same four arrays as Backpack |
| **FCMaterials.json** | trading with a fleet-carrier bartender | `FCMaterials` | `MarketID, CarrierName, CarrierID (callsign), Items[{id, Name, Name_Localised?, Price, Stock, Demand}]` |

---

## 8. Ships, vehicles and on-foot item symbols

The journal uses **lower-case** FD symbols in `Loadout.Ship`, `ShipyardSwap.ShipType` and similar fields. FDevIDs `shipyard.csv` uses mixed case; compare case-insensitively.

### 8.1 Ships (FDevIDs `shipyard.csv`, 49 rows) — 2023–2026 additions first

| Journal symbol | FDevIDs id | Name | Size | Released (approx.) | Entitlement SKU |
|---|---|---|---|---|---|
| `python_nx` | 129030464 | Python Mk II | M | ARX early access late 2023, credits early 2024 | ELITE_V_PYTHON_NX |
| `type8` | 129030534 | Type-8 Transporter | M | Aug 2024 (EDD date 2024-08-07) | ELITE_V_TYPE8 |
| `mandalay` | 129030680 | Mandalay | M | late 2024 | ELITE_V_MANDALAY |
| `cobramkv` | 129031229 | Cobra Mk V | S | Dec 2024 | ELITE_V_COBRA_MKV |
| `corsair` | 129031320 | Corsair | M | 2025 | ELITE_V_CORSAIR |
| `panthermkii` | 129034269 | Panther Clipper Mk II | L | 22 Jul 2025 ARX early access (Frontier press release) | ELITE_V_PANTHER_MKII |
| `lakonminer` | 129036130 | **Type-11 Prospector** | M | ~29–30 Sep 2025 ARX early access | ELITE_V_LAKON_MINER |
| `explorer_nx` | 129036569 | **Caspian Explorer** | L | Dec 2025 (FDevIDs 2025-12-02); credits-only from Apr 2026 | ELITE_V_EXPLORER_NX |
| `smallcombat01_nx` | 129039241 | **Kestrel Mk II** | S | Feb 2026 (FDevIDs 2026-02-25) | ELITE_V_SMALL_COMBAT_01 |
| `mediumtransport01` | 129041442 | **Lynx Highliner** (passenger liner, up to 225 passengers) | M | 28 Apr 2026, credits from day one | ELITE_V_MEDIUM_TRANSPORT_01 |
| `anacondamkii` | 129046025 | **Anaconda Mk II** | L (elitedangerous.com: "Landing Pad Requirement Large") | Oct 2026 ARX early access (in FDevIDs by 2026-10-30; Spansh lists it with no credit price) | ELITE_V_ANACONDA_MKII |

The original 38 hulls and their journal symbols:

| Symbol | Name |
|---|---|
| sidewinder | Sidewinder |
| eagle | Eagle |
| hauler | Hauler |
| adder | Adder |
| viper | Viper MkIII |
| cobramkiii | Cobra MkIII |
| type6 | Type-6 |
| dolphin | Dolphin |
| type7 | Type-7 |
| asp | Asp Explorer |
| vulture | Vulture |
| empire_trader | Imperial Clipper |
| federation_dropship | Federal Dropship |
| orca | Orca |
| type9 | Type-9 |
| python | Python |
| belugaliner | Beluga |
| ferdelance | Fer-de-Lance |
| anaconda | Anaconda |
| federation_corvette | Federal Corvette |
| cutter | Imperial Cutter |
| diamondback | Diamondback Scout |
| empire_courier | Imperial Courier |
| diamondbackxl | Diamondback Explorer |
| empire_eagle | Imperial Eagle |
| federation_dropship_mkii | Federal Assault Ship |
| federation_gunship | Federal Gunship |
| viper_mkiv | Viper MkIV |
| cobramkiv | Cobra MkIV |
| independant_trader | Keelback (sic) |
| asp_scout | Asp Scout |
| type9_military | Type-10 Defender |
| krait_mkii | Krait MkII |
| typex | Alliance Chieftain |
| typex_2 | Alliance Crusader |
| typex_3 | Alliance Challenger |
| krait_light | Krait Phantom |
| mamba | Mamba |

* **Taxi variants** seen in journals: `adder_taxi`, `diamondback_taxi`, `viper_taxi`, `vulture_taxi`.
* **On foot:** `Loadout`/`LoadGame` may report a **suit** as the "ship".
* EDD notes the Lynx Highliner hull stats as TBC.

### 8.2 Other vehicles

| Kind | Symbols |
|---|---|
| SRVs | `testbuggy` (SRV Scarab), `combat_multicrew_srv_01` (SRV Scorpion), **`mev_rhino`** (Rhino MEV by Vodel, Sept 2026, surface mining) |
| Fighters | `empire_fighter` (GU-97), `federation_fighter` (F63 Condor), `independent_fighter` (Taipan), `gdn_hybrid_fighter_v1/v2/v3` (Guardian Trident/Javelin/Lance) |
| Lander | **`lander01`** = **Nomad** (Falcon DeLacy, 2026). Launched via `LaunchVessel{VesselType:"lander01"}` (earlier via `LaunchFighter` with Loadout base/advanced/galactic). Modules `vehicle_lander01_*`, `lander01_cockpit` |

### 8.3 Suits and hand weapons (symbols used in Suit*/Weapon* events)

* **Suits:**
  * `flightsuit`
  * `utilitysuit_class1..5` (Maverick, Remlok)
  * `explorationsuit_class1..5` (Artemis, Supratech)
  * `tacticalsuit_class1..5` (Dominator, Manticore)
  * NPC suits (`*suitai_class*`)
* **Weapons:**

| Symbol | Name |
|---|---|
| `wpn_m_assaultrifle_kinetic_fauto` | Karma AR-50 |
| `wpn_m_submachinegun_kinetic_fauto` | Karma C-44 |
| `wpn_m_launcher_rocket_sauto` | Karma L-6 |
| `wpn_s_pistol_kinetic_sauto` | Karma P-15 |
| `wpn_m_sniper_plasma_charged` | Manticore Executioner |
| `wpn_m_shotgun_plasma_doublebarrel` | Manticore Intimidator |
| `wpn_m_assaultrifle_plasma_fauto` | Manticore Oppressor |
| `wpn_s_pistol_plasma_charged` | Manticore Tormentor |
| `wpn_m_assaultrifle_laser_fauto` | TK Aphelion |
| `wpn_m_submachinegun_laser_fauto` | TK Eclipse |
| `wpn_s_pistol_laser_sauto` | TK Zenith |

* **Tools:** `humanoid_fists`, `humanoid_rechargetool` (Energylink), `humanoid_companalyser` (Profile Analyser), `humanoid_sampletool` (Genetic Sampler), `humanoid_repairtool` (Arc Cutter).

---

## 9. EDCD/FDevIDs CSVs (downloaded to `data/fdevids/`)

Upstream: commit `c35612952dd6…` (2026-09-05). **No licence file** in the repo; see `data/fdevids/SOURCE.txt`.

| File | Rows | Header columns | Notes |
|---|---|---|---|
| `commodity.csv` | 270 | `id,symbol,category,name` | `id` = Market.json `id`; categories include Salvage, Minerals, Metals, … (Rhino-update minerals at the end) |
| `rare_commodity.csv` | 142 | `id,symbol,market_id,category,name` | `market_id` = the station that sells it |
| `outfitting.csv` | 1236 | `id,symbol,category,name,mount,guidance,ship,class,rating,entitlement` | category ∈ standard, internal, hardpoint, utility, **mercgear** (2026 pre-engineered MercCoin modules); `ship` set for armour/ship-specific items; player-specific vanity items deliberately omitted |
| `shipyard.csv` | 48 | `id,symbol,name,entitlement` | `entitlement` = SKU needed to buy (see `sku.csv`) |
| `material.csv` | 137 | `id,symbol,rarity,type,category,name` | `rarity` = grade 1–5; `type` ∈ Raw/Manufactured/Encoded; `category` = trader group (Raw 1–7, Manufactured/Encoded group names, `None` for Guardian/Thargoid). Lacks some 2022–23 Thargoid-war materials (e.g. `tg_shutdowndata`) — EDD `MCMRType.cs` has them |
| `microresources.csv` | 196 | `id,symbol,category,English name` | category ∈ Component (33), Consumable (6), Data (114), Item (43) |
| `engineers.csv` | 38 | `id,system_address,market_id,name` | 3000xx ship engineers, 4000xx on-foot; `id` = journal `EngineerID` |
| `economy.csv` | 17 | `id,name` | `$economy_X;` → name (incl. Carrier = "Private Enterprise", Engineer) |
| `government.csv` | 17 | `id,name` | `$government_X;` |
| `security.csv` | 5 | `id,name` | `$SYSTEM_SECURITY_X;` |
| `factionstate.csv` | 27 | `id,name` | BGS states |
| `happiness.csv` | 5 | `id,name` | `$Faction_HappinessBandN;` |
| `systemallegiance.csv` | 9 | `System Allegiance,Name` | includes `FrontlineSolutions` (2026) |
| `factionids.csv` | 9 | `id,name` | superpower/faction symbols |
| `passengers.csv` | 17 | `fdname,name` | passenger types |
| `crimes.csv` | 39 | `id` | CommitCrime `CrimeType` values |
| `dockingdeniedreasons.csv` | 6 | `id` | DockingDenied `Reason` values |
| `rings.csv` | 4 | `id,name` | `eRingClass_*` |
| `terraformingstate.csv` | 3 | `id,name` | |
| `combatrank.csv`, `TradeRank.csv`, `ExplorationRank.csv`, `CQCRank.csv` | 14 each | `number,name` | rank names incl. Elite I–V |
| `EmpireRank.csv`, `FederationRank.csv` | 15 each | `number,name` | navy ranks |
| `sku.csv` | 67 | `sku,requirement` | entitlement explanations |
| `bundles.csv` | 4 | `id,name,sku` | store bundles (e.g. Python Mk II Stellar) |
| `How to determine MatTrader and Broker type` | — | prose | community heuristic: Encoded = HighTech/Military, Raw = Extraction/Refinery, Manufactured = Industrial; Guardian broker = HighTech, Human = Industrial |

---

## 10. Implementation notes for the Rust parser

* Deserialize to `#[serde(tag = "event")]` enums with a catch-all `Unknown(serde_json::Value)` variant.
* Keep `#[serde(flatten)] extra: Map<String, Value>` on every struct; new keys appear without notice.
* Use `Option<T>` liberally. The schemas mark most fields required, but real journals omit them, e.g. `StarSystem` on old `Docked` events or `CarrierType` before Vanguards.
* Use `u64` for SystemAddress, MarketID, CarrierID, SuitID, SuitModuleID and MissionID.
* Use `f64` for `MeritsGained`, `Altitude` and `Heading`.
* Normalise symbols: lower-case; strip `$`, `_name;` and `;`.
* Handle the localised-key duplication.
* Treat `Flags2` absent as 0 (Horizons/legacy client).
* Codegen option: `data/journal-schemas/` is valid JSON Schema 2020-12 (`$ref` to `../_Event.json` and `../common/ShipLockerBackpack.json`). It works with `typify`/`schemafy`-style generators after resolving refs. Keep a hand-written layer for the polymorphic events (`Bounty`, `BuyMicroResources`, `EngineerProgress`).
