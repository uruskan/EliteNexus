# Inara API: implementation spec for Elite Nexus

Research date: **2026-10-12**. Status: implementation-ready, with the unverified items marked **[UNVERIFIED]**.

## 0. Sources and verification

The live pages on `inara.cz` currently return an "Access check required … Confirm and continue" bot-check page (HTTP 503) to scripted clients. I did not click through it. The documentation was read from Internet Archive snapshots of the official pages:

| Official page | Snapshot used |
|---|---|
| https://inara.cz/elite/inara-api/ (overview and app list) | https://web.archive.org/web/20260916090852/https://inara.cz/elite/inara-api/ |
| https://inara.cz/elite/inara-api-devguide/ (dev guide and rules) | https://web.archive.org/web/20260308173440/https://inara.cz/elite/inara-api-devguide/ (identical to the 2025-05-23 snapshot) |
| https://inara.cz/elite/inara-api-docs/ (full reference) | https://web.archive.org/web/20260308173432/https://inara.cz/elite/inara-api-docs/ |

Required properties are marked in the HTML with `<span class="negative">` ("red"). They were extracted programmatically and are shown as **req** below.

Client implementations read in full:

- **EDMC 6.1.2** (`main` at commit `89f410c9`):
  - https://github.com/EDCD/EDMarketConnector/blob/main/plugins/inara.py
  - `monitor.py`
  - `ChangeLog.md`, which holds the rate-limit history.
- **EDDiscovery:**
  - https://github.com/EDDiscovery/EliteDangerousCore/blob/master/EliteDangerous/3rdPartyInterfaces/Inara/Inara.cs
  - `InaraSync.cs` in the same folder.

**No requests were sent to the Inara API**: no POSTs, no API keys and no accounts.

---

## 1. Endpoint and transport

```
POST https://inara.cz/inapi/v1/
Content-Type: application/json            (set it; EDMC omits it and Inara still accepts)
Body: uncompressed JSON  { "header": {...}, "events": [ ... ] }
Response: JSON           { "header": {...}, "events": [ ... ] }   (same order as sent)
```

- There is a **single endpoint for everything**: writes and reads.
- If the response is empty or contains no data, something failed. Retry later with a sane timeout and retry rate; Inara may be down for maintenance.
- EDMC: timeout 20 s; UA `EDCD-EDMarketConnector-<ver>`. Its `try_send_data` loop allows 3 attempts, but returns on the first exception. Events have already been dequeued by then, so **a failed POST loses that batch** in EDMC. Don't copy that; keep events queued until a response is parsed.
- EDD posts with `Content-Type: application/json`.

### 1.1 Header (request)

| Property | Type | Req | Notes |
|---|---|---|---|
| `appName` | string | **req** | Your application name, **exactly as it was whitelisted** (see §2). |
| `appVersion` | string | **req** | Plain semver-ish, e.g. `1.2.4`. Inara says version control may be enforced, so keep the format standard. |
| `isBeingDeveloped` | bool | opt | Set `true` while developing or testing. It makes Inara **skip global events such as `setCommunityGoal`** so test data cannot pollute shared data. Newly accepted apps have it forced on server-side while "in development", but set it yourself anyway. |
| `APIkey` | string | **req** | Either the **user's personal API key** or your **generic application key** (read-only events only). Note the capitalisation: `APIkey`. |
| `commanderName` | string | opt\* | In-game CMDR name, taken from the journal or CAPI rather than typed by the user. Recommended whenever a personal key is used. **Required for the combat-log events** (they don't work without it). |
| `commanderFrontierID` | string | opt | Journal FID, e.g. `F123456` or `123456`. Omit it if unknown. |

### 1.2 Events (request)

| Property | Type | Req | Notes |
|---|---|---|---|
| `eventName` | string | **req** | Event or method name (§5). |
| `eventTimestamp` | string | **req** | ISO-8601 UTC, e.g. `2017-05-02T17:30:49Z`. Use the journal timestamp of the underlying event; if there is none, use now. **It must not be older than 30 days.** |
| `eventCustomID` | int | opt | Echoed back in the output, for pairing. |
| `eventData` | object or array | **req** | Event properties. Many events accept **an array of objects** to set multiple items at once (noted per event). |

### 1.3 Response

```json
{ "header": { "eventStatus": 200, "eventData": { "userID": 1, "userName": "Artie" } },
  "events": [ { "eventStatus": 200, "eventData": { … } },
              { "eventCustomID": 1234, "eventStatus": 204, "eventStatusText": "No results found." } ] }
```

- **`header.eventStatus`:**
  - Authentication or app-level status.
  - **400 at header level means the whole batch was cancelled**, for example on failed authorisation.
  - `header.eventData` (`userID`, `userName`) is returned only when a personal key authenticated successfully.
- **`events[i]`:** `eventStatus`, `eventStatusText` (only on warnings and errors), `eventData` (event-specific output) and the echoed `eventCustomID`.
- **Status codes:**

  | Code | Meaning |
  |---|---|
  | 200 | OK |
  | 202 | Warning. Processed, but e.g. multiple matches. |
  | 204 | "Soft" error. Formally OK, but no results or nothing done. Example: `"No results found."`. EDD treats a 204 whose text contains `in-game name` on add/del friend as "commander not known to Inara". |
  | 400 | Error. Missing or invalid properties; the event is skipped. At header level, the whole batch is cancelled. |

- **Known texts:**
  - `"Too much requests, slow down, cowboy. ;) ..."` is returned as 400 when you exceed the rate limit (quoted in the EDMC ChangeLog 4.0.4).
  - `"Everything was alright, the near-neutral status just wasn't stored."` is a harmless reputation warning that EDMC filters from its logs.
- **EDMC handling:**
  - If `header.eventStatus // 100 != 2`, show "Error: Inara \<text\>".
  - Otherwise, log every event whose `eventStatus != 200` and show an error only when it is not `2xx`.
  - EDMC uses the `eventData` of travel and ship events to tell other plugins the Inara system/station/ship IDs and URLs.

---

## 2. Application whitelisting, key types and approval

**Status: approved.** Artie whitelisted the app name "Elite Nexus" on **2026-10-13**, replying to a request sent
through Inara's private messages (app name, what it does and what it sends, the short description for the apps list,
the GitHub repo as homepage). He confirmed that the earlier refusals were only because the name wasn't whitelisted
yet. He didn't mention a generic application key.

From the Dev guide (paraphrased):

- **Whitelisting is mandatory.** Before releasing your app, tell Artie (Inara's author) the app name/identifier you will use, because **it must be white-listed first**. Artie says this is not meant to exclude anyone; it identifies problems and lists the app for users. Send him:
  1. The application name **exactly as it will appear in `appName`**.
  2. What the app does and what you are trying to achieve (for his information).
  3. A short description, used in Inara's public list of apps.
  4. A homepage or download URL, if any.

  The contact link on the Dev guide points to Artie's Inara profile, https://inara.cz/elite/cmdr/1/. Inara also has a contact section at https://inara.cz/elite/policies/#contact.
- **Check the scope first.** Before applying, make sure Inara's API actually provides what you need. It does **not** offer markets, minor-faction influence, star systems or stations as outputs.
- **Test accounts are allowed.** You may create an extra Inara account for development and testing. The user must do this themselves.
- **No beta data.** Never send game-beta data, except to your own test accounts.
- **Enforcement.** Inara reserves the right to revoke access for apps or keys that have high error rates, break the rules, or misuse the API (e.g. mass scraping of commander data).
- **Does an unapproved app work?**
  - The docs only say the name must be whitelisted first, and that newly accepted apps start in development mode server-side (`isBeingDeveloped` forced on).
  - The docs don't say what happens to requests with an unknown `appName`. **Verified 2026-10-13:** every request is refused at header level with `eventStatusText` "This application has no access allowed", confirmed by Artie as the not-whitelisted answer. Elite Nexus pauses sync on it and Accounts & sync explains why.
  - "Elite Nexus" was whitelisted on 2026-10-13 (see above). Since 0.7.3 it sends no `isBeingDeveloped` at all: test runs (`--journal-dir`, `--screenshot`) never upload, so everything it sends is live play. Use a test account for development work that has to upload.
- **Personal vs generic API key:**
  - **Personal key:** each user generates it in Inara Settings → API (https://inara.cz/elite/cmdr-settings-api/; EDMC links `https://inara.cz/settings-api`). It identifies the user and is required for all `add*`/`set*`/`del*` events and for `getCommanderProfile` without `searchName`.
  - **Generic application key:** your app's own key, usable only for **read-only general events**. Per the docs these are `getCommanderProfile` (with `searchName`) and `getCommunityGoalsRecent`. How it is issued is not documented; presumably Artie provides it on approval **[UNVERIFIED]**.
  - **commanderName:** with a generic key, omit `commanderName`.

## 3. Rate limits, batching and etiquette

- **Documented rules** (Dev guide, paraphrased):
  - Be polite.
  - Batch events.
  - Send on session start/end and on `Docked` and/or `FSDJump`; usually there is no need to send more often.
  - Cache on your side.
  - Never send events one by one in separate requests: *excessive requests can get the API key invalidated*.
  - Don't resend the same data repeatedly.
  - Send Live data only (no Legacy 3.8, no beta).
- **The actual limit** is not stated in the docs. From the EDMC ChangeLog:
  - Release 4.0.4: sending more than allowed produced `400 Too much requests, slow down, cowboy. ;) ...` and **locked the user out of the API for an hour**.
  - A later release: EDMC delays up to 35 s "to avoid breaking the **'2 messages a minute'** limit on the Inara API".
- **What clients do:**
  - **EDMC:** one worker loop with `WORKER_WAIT_TIME = 35` s. Each loop sends all queued events for each credentials set in **one POST per (cmdr, FID, API key)**, then sleeps 35 s.
  - **EDDiscovery:** waits 10 s to collect, sends **≤ 50 events per POST**, then waits 30 s.
- **Recommendation:** at most 1 POST per 35 s per API key, as many events as needed per POST (EDD's 50 is a safe cap), and drop superseded "set" events before sending:
  - Keep only the newest `setCommanderShip` per ship, `setCommanderShipLoadout` per ship, `setCommanderStorageModules`, `setCommanderInventory*` and `setCommunityGoal` per CG. EDMC filters its queue the same way.
- **Timestamps:**
  - ≤30 days old.
  - Use the real journal time.
  - EDD notes that Inara rejects or dedupes identical events with the same timestamp. When synthesising a historic upload, EDD spaces events 1 s apart **[UNVERIFIED]**.
- **Credits:** send `setCommanderCredits` only at session start/end, on big changes, or hourly. It writes a visible credits log.
- **Docking at session start:** do **not** send `addCommanderTravelDock` at session start (after `Location`); it spams the flight log. EDMC suppresses the first `Docked`.

---

## 4. Read (get) events

### `getCommanderProfile` (works with the generic key)

- **eventData:** `searchName` (string, optional).
  - Inara first tries an exact match on in-game CMDR names, then a search on Inara user names, returning the first hit.
  - If `searchName` is empty and a personal key is used, Inara returns the current user's profile.
- **Output `eventData`:**

  ```json
  { "userID": 1, "userName": "Artie", "commanderName": "Artie",
    "commanderRanksPilot": [ { "rankName": "combat", "rankValue": 7, "rankProgress": 0.4 }, … ],
    "preferredAllegianceName": "Independent", "preferredPowerName": "Felicia Winters",
    "commanderMainShip": { "shipType": "federation_corvette", "shipName": "Spectre", "shipIdent": "X-0QAR", "shipRole": "" },
    "commanderSquadron": { "SquadronID": 5, "SquadronName": "…", "SquadronMembersCount": 8,
                           "SquadronMemberRank": "Squadron commander", "inaraURL": "https://inara.cz/squadron/5/" },
    "preferredGameRole": "Freelancer", "avatarImageURL": "…", "inaraURL": "https://inara.cz/cmdr/1/",
    "dataImportActive": false, "otherNamesFound": ["…"] }
  ```

- `commanderName` is present only on an exact in-game-name match. Otherwise `otherNamesFound` lists up to 20 candidates.
- Note the PascalCase keys inside `commanderSquadron`.

### `getCommunityGoalsRecent` (works with the generic key)

- **eventData:** `[]` (empty).
- **Output `eventData`:** an array of CG objects:

  ```json
  [ { "communitygoalName": "…", "starsystemName": "…", "stationName": "…", "goalExpiry": "2017-11-09T16:00:00Z",
      "tierReached": 4, "tierMax": 0, "contributorsNum": 408, "contributionsTotal": 2838230000, "isCompleted": false,
      "lastUpdate": "2017-11-08T14:10:05Z", "goalObjectiveText": "…", "goalRewardText": "", "goalDescriptionText": "…",
      "inaraURL": "https://inara.cz/galaxy-communitygoals/" } ]
  ```

Write events also return data where noted in §5 (ship and travel IDs and URLs).

---

## 5. Complete input-event reference (50 events)

These are all the events in the 2026-03 docs, in Inara's own grouping.

**Conventions:**

- `[]` after an event name means `eventData` may be a single object **or** an array of such objects.
- Types are Inara's: `string`, `integer`, `float`, `boolean`, `array`, `mixed`, `timestamp`.
- **req** marks a required property.
- Names such as items, modules, ships and missions are "as in the journal" (FDev symbols), and are case-insensitive where noted.

### 5.1 Commander's profile and misc

| Event | eventData fields | Notes |
|---|---|---|
| `addCommanderFriend` | `commanderName` string **req**; `gamePlatform` string **req** (`pc`, `xbox`, `ps4`) | Adds a friend request on Inara. Not performed if the target's in-game name is unknown to Inara. |
| `delCommanderFriend` | `commanderName` **req**; `gamePlatform` **req** | Removes a friend. |
| `getCommanderProfile` | `searchName` string | Read event; see §4. Works with the generic key. |

### 5.2 Credits, assets and permits

| Event | eventData fields | Notes |
|---|---|---|
| `addCommanderPermit` | `starsystemName` string **req** | For permits gained other than by rank (e.g. mission rewards). Rank permits are automatic. Inara's permit list was "not implemented yet" at doc time. |
| `setCommanderCredits` | `commanderCredits` int **req**; `commanderAssets` int; `commanderLoan` int | Adds a credits-log record if the value differs. Without `commanderAssets`, Inara computes assets; without `commanderLoan`, the loan is set to 0. Send rarely. |

### 5.3 Ranks, reputation and stats

| Event | eventData fields | Notes |
|---|---|---|
| `setCommanderGameStatistics` | the whole journal `Statistics` object (minus `event` and `timestamp`) **req** | Always overwrites everything, so a partial object erases the rest. |
| `setCommanderRankEngineer` `[]` | `engineerName` string **req**; `rankStage` string (`Invited`, `Acquainted`, `Unlocked`, `Barred`); `rankValue` int 1..5 | At least one of stage or value. A `rankValue` implies `Unlocked`. Updates older than the stored timestamp are ignored. |
| `setCommanderRankPilot` `[]` | `rankName` string **req** (`combat`, `trade`, `explore`, `cqc`, `soldier`, `exobiologist`, `federation`, `empire`); `rankValue` int (0..13 PF, 0..14 navy); `rankProgress` float 0..1 | At least one of value or progress. Older timestamps are ignored. Rank permits are awarded automatically. |
| `setCommanderRankPower` | `powerName` string **req**; `rankValue` int **req**; `meritsValue` int | **Leaving a power: `rankValue: -1`.** Defecting: just set the new power, and the old one is cleared automatically. Older timestamps are ignored. |
| `setCommanderReputationMajorFaction` `[]` | `majorfactionName` string **req** (`federation`, `empire`, `independent`, `alliance`); `majorfactionReputation` float -1..1 **req** | Journal value / 100. |
| `setCommanderReputationMinorFaction` `[]` | `minorfactionName` string **req**; `minorfactionReputation` float -1..1 **req** | From `FSDJump`/`Location` `Factions[].MyReputation` / 100. |

### 5.4 Inventory: general (all item types)

`itemType` (string) takes FDev types: `Encoded`, `Raw`, `Manufactured`, `Item`/`Items`, `Component`/`Components`, `Data`, `Consumable`/`Consumables`, plus `Material` (any of the three material types) and `Commodity` (ship cargo hold).

`itemLocation` (string) is currently only `ShipLocker`; this is the default when omitted.

| Event | eventData fields | Notes |
|---|---|---|
| `addCommanderInventoryItem` | `itemName` **req**; `itemCount` int **req**; `itemType` **req**; `itemLocation`; `isStolen` bool; `missionGameID` int | Adds a count. `isStolen` and `missionGameID` are stored but unused. |
| `delCommanderInventoryItem` | same as above | Removes a count. |
| `resetCommanderInventory` `[]` | `itemType` **req**; `itemLocation` | Clears inventory of that type. Resetting `Raw`, `Encoded` or `Manufactured` resets **all** materials. |
| `setCommanderInventory` `[]` | `itemName` **req** (case-insensitive); `itemCount` **req**; `itemType` **req**; `itemLocation`; `isStolen`; `missionGameID` | Replaces the whole inventory of every `itemType` present in the array. If a type may be empty (e.g. `ShipLocker` materials), send `resetCommanderInventory` first. The same item may appear twice with different stolen/mission flags. |
| `setCommanderInventoryItem` | `itemName` **req**; `itemCount` **req**; `itemType` **req**; `itemLocation`; `isStolen`; `missionGameID` | Sets the absolute count. 0 removes the item; an unknown item is added. |

### 5.5 Inventory: ship cargo and materials (older, type-specific API)

| Event | eventData fields | Notes |
|---|---|---|
| `addCommanderInventoryCargoItem` | `itemName` **req**; `itemCount` **req**; `isStolen`; `missionGameID` | Also account for `MiningRefined`, `ScientificResearch`, `SearchAndRescue`, `Synthesis`, `EngineerCraft`, `EngineerContribution`, … |
| `addCommanderInventoryMaterialsItem` | `itemName` **req**; `itemCount` **req** | |
| `delCommanderInventoryCargoItem` | `itemName` **req**; `itemCount` **req**; `isStolen`; `missionGameID` | |
| `delCommanderInventoryMaterialsItem` | `itemName` **req**; `itemCount` **req** | |
| `setCommanderInventoryCargo` `[]` | `itemName` **req** (case-insensitive); `itemCount` **req**; `isStolen`; `missionGameID` | Replaces the whole cargo hold. Limpets count as cargo; passengers do not. |
| `setCommanderInventoryCargoItem` | `itemName` **req**; `itemCount` **req**; `isStolen`; `missionGameID` | 0 removes. |
| `setCommanderInventoryMaterials` `[]` | `itemName` **req**; `itemCount` **req** | Replaces all materials (Raw, Manufactured and Encoded together). |
| `setCommanderInventoryMaterialsItem` | `itemName` **req** (case-insensitive); `itemCount` **req** | 0 removes. |

### 5.6 Storage

| Event | eventData fields | Notes |
|---|---|---|
| `setCommanderStorageModules` `[]` | `itemName` string **req** (module symbol as in the journal); `itemValue` int; `isHot` bool; `starsystemName`; `stationName`; `marketID` int; `engineering` object {`blueprintName`, `blueprintLevel`, `blueprintQuality`, `experimentalEffect`} | Always replaces all storage; an empty list empties it. `marketID` is the primary location key, but still send the names. A module with no location is "in transit". |

### 5.7 Fleet and ships

| Event | eventData fields | Output | Notes |
|---|---|---|---|
| `addCommanderShip` | `shipType` **req** (e.g. `sidewinder`); `shipGameID` int **req** | `shipInaraID`, `shipInaraURL` | Adds the ship and makes it current. Does not handle selling or storing the old ship, or credits. |
| `delCommanderShip` | `shipType` **req**; `shipGameID` **req** | – | May be ignored if the ship was entered manually on Inara without a gameID. |
| `setCommanderShip` | `shipType` **req**; `shipGameID` **req**; `shipName`; `shipIdent`; `shipRole`; `isCurrentShip` bool; `isMainShip` bool; `isHot` bool; `shipHullValue` int; `shipModulesValue` int; `shipRebuyCost` int; `shipMaxJumpRange` float; `shipCargoCapacity` int; `starsystemName`; `stationName`; `marketID` int | `shipInaraID`, `shipInaraURL` | Upserts (auto-adds an unknown ship). At least one optional field is needed. `isCurrentShip`/`isMainShip` set to `false` does nothing. Location fields mean where the ship is docked when not flown. |
| `setCommanderShipLoadout` | `shipType` **req**; `shipGameID` **req**; `shipLoadout` array of modules | – | Each module has `slotName`, `itemName`, `itemValue`, `itemHealth` (0..1), `isOn`, `isHot`, `itemPriority`, `itemAmmoClip`, `itemAmmoHopper`, and optionally `engineering` {`blueprintName`, `blueprintLevel`, `blueprintQuality`, `experimentalEffect`, `modifiers`: [{`name`, `value`, `originalValue`, `lessIsGood`} or {`name`, `value`(string)}]}. No `shipLoadout` resets the configuration. |
| `setCommanderShipTransfer` | `shipType` **req**; `shipGameID` **req**; `starsystemName` **req**; `stationName` **req**; `marketID`; `transferTime` int (seconds) | `shipInaraID`, `shipInaraURL` | Docked or transfer destination for a ship that is not being flown. For `ShipyardTransfer`, the destination is the commander's **current** location. |

### 5.8 Suit loadouts (Odyssey)

| Event | eventData fields | Notes |
|---|---|---|
| `delCommanderSuitLoadout` | `loadoutGameID` int **req** | |
| `setCommanderSuitLoadout` | `loadoutGameID` **req**; `loadoutName`; `suitGameID` int; `suitType` (e.g. `utilitysuit_class3`); `suitMods` string[]; `suitLoadout` [{`slotName`, `itemName`, `itemClass`, `itemGameID`, `engineering`: [{`blueprintName`}]}] | Use for full-loadout events (`SuitLoadout`, `CreateSuitLoadout`). |
| `updateCommanderSuitLoadout` | same fields as `setCommanderSuitLoadout` | Partial update, e.g. a rename (`RenameSuitLoadout`) or one slot changing (`LoadoutEquipModule`). |

### 5.9 Flight log

All of these events return `starsystemInaraID` and `starsystemInaraURL`. Dock and location events also return `stationInaraID`, `stationInaraURL` and `marketInaraURL`.

| Event | eventData fields | Notes |
|---|---|---|
| `addCommanderTravelCarrierJump` | `starsystemName` **req**; `starsystemCoords` [x,y,z]; `stationName`; `marketID`; `jumpDistance` (don't set; unknown); `shipType`; `shipGameID` | Flight log entry plus current location. |
| `addCommanderTravelDock` | `starsystemName` **req**; `starsystemCoords`; `stationName` **req**; `marketID`; `starsystemBodyName`; `starsystemBodyCoords` [lat,lon]; `shipType`; `shipGameID`; `isTaxiShuttle` bool; `isTaxiDropship` bool | Do not send at session start. |
| `addCommanderTravelFSDJump` | `starsystemName` **req**; `starsystemCoords`; `jumpDistance` float; `shipType`; `shipGameID`; `isTaxiShuttle`; `isTaxiDropship` | |
| `addCommanderTravelLand` | `starsystemName` **req**; `starsystemCoords`; `starsystemBodyName` **req**; `starsystemBodyCoords`; `shipType`; `shipGameID`; `isTaxiShuttle`; `isTaxiDropship` | For `Touchdown`, `DropshipDeploy` and similar. |
| `setCommanderTravelLocation` | `starsystemName` **req**; `starsystemCoords`; `stationName`; `marketID`; `starsystemBodyName`; `starsystemBodyCoords` | Sets the current location without a flight-log record. |

### 5.10 Mission log

| Event | eventData fields | Notes |
|---|---|---|
| `addCommanderMission` | `missionName` **req** (e.g. `Mission_Massacre_Legal_Military`); `missionGameID` **req** (documented as string, sent as int by all clients); `missionExpiry` ISO-8601; `influenceGain` (`+`..`+++++`); `reputationGain`; `starsystemNameOrigin` **req** (current system at accept time); `stationNameOrigin`; `minorfactionNameOrigin`; `starsystemNameTarget`; `stationNameTarget`; `minorfactionNameTarget`; `commodityName`; `commodityCount` int; `targetName`; `targetType`; `killCount` int; `passengerType`; `passengerCount` int; `passengerIsVIP` bool; `passengerIsWanted` bool | Does not move cargo or rewards. Multi-destination systems like `A$MISSIONUTIL_MULTIPLE_FINAL_SEPARATOR;B` are allowed; Inara uses the first. |
| `setCommanderMissionAbandoned` | `missionGameID` **req** | |
| `setCommanderMissionCompleted` | `missionGameID` **req**; `donationCredits` int; `rewardCredits` int; `rewardPermits` [{`starsystemName`}]; `rewardCommodities` [{`itemName`, `itemCount`}]; `rewardMaterials` [{`itemName`, `itemCount`}]; `minorfactionEffects` [{`minorfactionName`, `influenceGain`, `reputationGain`}] | Display only. Credits, items and permits must be sent separately. |
| `setCommanderMissionFailed` | `missionGameID` **req** | |

### 5.11 Combat log (header `commanderName` required)

| Event | eventData fields | Notes |
|---|---|---|
| `addCommanderCombatDeath` | `starsystemName` **req**; `opponentName`; `wingOpponentNames` string[]; `isPlayer` bool | One of `opponentName` or `wingOpponentNames` is needed; without either (e.g. a collision), the event is accepted but not recorded. Leave `isPlayer` unset to let Inara decide. |
| `addCommanderCombatInterdicted` | `starsystemName` **req**; `opponentName` **req** (`Interdictor`, else `Power` or `Faction`); `isPlayer` **req**; `isSubmit` bool | |
| `addCommanderCombatInterdiction` | `starsystemName` **req**; `opponentName` **req** (`Interdicted`, else `Power` or `Faction`); `isPlayer` **req**; `isSuccess` bool | |
| `addCommanderCombatInterdictionEscape` | `starsystemName` **req**; `opponentName` **req**; `isPlayer` **req** | |
| `addCommanderCombatKill` | `starsystemName` **req**; `opponentName` **req** | PvP kills only (journal `PVPKill`). |

### 5.12 Community goals

| Event | eventData fields | Notes |
|---|---|---|
| `getCommunityGoalsRecent` | `[]` | Read; see §4. Works with the generic key. |
| `setCommanderCommunityGoalProgress` | `communitygoalGameID` int **req**; `contribution` int **req**; `percentileBand` int 0..100 **req**; `percentileBandReward` int **req**; `isTopRank` bool | One event per goal in `CommunityGoal.CurrentGoals`. Send `setCommunityGoal` before it. |
| `setCommunityGoal` | `communitygoalGameID` **req**; `communitygoalName` **req**; `starsystemName` **req**; `stationName` **req**; `goalExpiry` ISO-8601 **req**; `tierReached` int (strip the "Tier " prefix); `tierMax` int; `topRankSize` int; `isCompleted` bool; `contributorsNum` int; `contributionsTotal` int; `completionBonus` string | Auto-adds an unknown CG. **Ignored when `isBeingDeveloped` is set.** |

---

## 6. Journal → Inara mapping (as EDMC `plugins/inara.py` does it)

**Global gates:** send nothing unless all of these hold:

- Inara output is enabled.
- The game is not a beta.
- The commander is not in multicrew (`Role` set).
- An API key exists for the CMDR.
- The galaxy is Live (`gameversion >= 4.0.0`).

Otherwise EDMC shows "Inara only accepts Live galaxy data", at most every 5 minutes.

**Queueing:**

- Events are queued per (cmdr, FID, API key).
- Every 35 s the worker POSTs everything queued for each key.
- Superseded `setCommanderShip`, `setCommanderShipLoadout` (same ship), `setCommanderStorageModules`, `setCommunityGoal` and `setCommanderCommunityGoalProgress` events are removed from the unsent queue before new ones are added.

| Journal trigger | Inara event(s) | Field mapping / conditions |
|---|---|---|
| Session start: `StartUp`, or first `Cargo` after `LoadGame`, or API key just changed | `setCommanderReputationMajorFaction` `[]`; `setCommanderRankEngineer` `[]`; `setCommanderShip`; `setCommanderShipLoadout` | Reputation from the `Reputation` event, `{majorfactionName: key.lower(), majorfactionReputation: v/100}`. Engineers from tracked `EngineerProgress` state. Current ship: `{shipType, shipGameID, shipName, shipIdent, isCurrentShip:true, shipHullValue?, shipModulesValue?, shipRebuyCost}`. Loadout from tracked `Loadout`. |
| API key changed in settings | `getCommanderProfile` `{searchName: cmdr}` | Response not used beyond logging. |
| `Progress` (arrives after `Rank`) | `setCommanderRankPilot` `[]` | For each tracked rank: `{rankName: key.lower(), rankValue: Rank, rankProgress: Progress/100}`. |
| `Promotion` | `setCommanderRankPilot` (per promoted rank) | `{rankName, rankValue, rankProgress: 0}`. |
| `EngineerProgress` (single-engineer form) | `setCommanderRankEngineer` | `{engineerName: Engineer, rankValue: Rank, rankStage: Progress}`. |
| `PowerplayJoin` | `setCommanderRankPower` | `{powerName: Power, rankValue: 1}`. |
| `PowerplayLeave` | `setCommanderRankPower` | `{powerName: Power, rankValue: 0}`. **The docs say use `-1` to leave.** EDD sends `-1`. Use `-1`. |
| `PowerplayDefect` | `setCommanderRankPower` | `{powerName: ToPower, rankValue: 1}`. EDD additionally sends `FromPower` with -1. |
| `Powerplay` | `setCommanderRankPower` | `{powerName: Power, rankValue: Rank, meritsValue: Merits}`. |
| `PowerplayMerits` | `setCommanderRankPower` | `{powerName: state.Power, rankValue: state.Rank, meritsValue: TotalMerits}`. |
| `PowerplayRank` | `setCommanderRankPower` | `{powerName: Power, rankValue: Rank}`. |
| `Docked` | `addCommanderTravelDock` | `{starsystemName, stationName, shipType, shipGameID, marketID}`. **Skipped** (once) after `Location`-while-docked (i.e. a session that starts docked), `ShipyardNew`, `ShipyardSwap` or `CarrierJump`. Also skipped when re-docking after `Undocked` with no `SupercruiseEntry`/`FSDJump` in between. With `Taxi:true`, the ship fields are removed and `isTaxiDropship` or `isTaxiShuttle` is set. |
| `SupercruiseExit` | `setCommanderTravelLocation` | `{starsystemName: StarSystem, starsystemBodyName: Body if BodyType=='Planet'}`. |
| `ApproachSettlement` | `setCommanderTravelLocation` | `{starsystemName, stationName: Name, starsystemBodyName: BodyName, starsystemBodyCoords: [Latitude, Longitude], marketID?}`. |
| `FSDJump` | `addCommanderTravelFSDJump`; `setCommanderReputationMinorFaction` `[]` | `{starsystemName, starsystemCoords: StarPos, jumpDistance: JumpDist, shipType, shipGameID}`; taxi → no ship fields, plus `isTaxiDropship`/`isTaxiShuttle`. Factions: `{minorfactionName: Name, minorfactionReputation: MyReputation/100}`. |
| `CarrierJump` | `addCommanderTravelCarrierJump`; `setCommanderReputationMinorFaction` `[]` | `{starsystemName, stationName, marketID, shipType, shipGameID, starsystemCoords?}`; then suppresses the next `Docked`. |
| `Location` | `setCommanderTravelLocation` | `{starsystemName, starsystemCoords}` + (if `Docked`) `stationName`, `marketID` + (docked at a planetary base or landed) `starsystemBodyName` + (`Latitude`/`Longitude` present) `starsystemBodyCoords`. |
| `DropshipDeploy` | `addCommanderTravelLand` | `{starsystemName, starsystemBodyName: Body, isTaxiDropship: true}`. |
| `Touchdown` (`PlayerControlled` and not `OnPlanet:false`) | `addCommanderTravelLand` | `{starsystemName, starsystemBodyName, starsystemBodyCoords, shipGameID, shipType, isTaxiShuttle:false, isTaxiDropship:false}`. EDMC misspells the last key as `isTaxiDropShip`; use the documented name. |
| any event, after the above, if changed | `setCommanderInventoryCargo` `[]`; `setCommanderInventoryMaterials` `[]` | Cargo `{itemName, itemCount}` sorted. Materials = Raw + Manufactured + Encoded. Only when different from the last values sent. |
| `Statistics` | `setCommanderCredits`; `setCommanderGameStatistics` | `{commanderCredits, commanderLoan, commanderAssets: Bank_Account.Current_Wealth}`; statistics from the tracked `Statistics`. (Credits are only sent here.) |
| `ShipyardNew` | `addCommanderShip` | `{shipType: ShipType, shipGameID: NewShipID}`; then the next `Loadout` sends the loadout. |
| `ShipyardBuy`, `ShipyardSell`, `SellShipOnRebuy`, `ShipyardSwap` | `setCommanderShip` or `delCommanderShip` | With `StoreShipID`: `setCommanderShip {shipType: StoreOldShip, shipGameID: StoreShipID, starsystemName, stationName}` (current location). With `SellShipID`: `delCommanderShip {shipType: SellOldShip or ShipType, shipGameID: SellShipID}`. |
| `SetUserShipName` | `setCommanderShip` | `{shipType, shipGameID, shipName, shipIdent, isCurrentShip: true}` from tracked state. |
| `ShipyardTransfer` | `setCommanderShipTransfer` | `{shipType, shipGameID: ShipID, starsystemName, stationName (current), transferTime: TransferTime}`. |
| `StoredShips` | `setCommanderShip` (one per ship) | Ships here: `{shipType, shipGameID, shipName, isHot, starsystemName, stationName, marketID}`. Ships remote: `{…, starsystemName?, marketID: ShipMarketID?}` (absent while in transit). Sent only when the fleet changed. |
| `Loadout` | `setCommanderShipLoadout`; `setCommanderShip` | Loadout from tracked modules: `{slotName: Slot, itemName: Item, itemHealth: Health, isOn: On, itemPriority: Priority, itemAmmoClip?, itemAmmoHopper?, itemValue?, isHot?, engineering{blueprintName, blueprintLevel, blueprintQuality, experimentalEffect?, modifiers[{name: Label, value, originalValue, lessIsGood} or {name, value: ValueStr}]}}` (only if changed). Ship: `{shipType, shipGameID, shipName, shipIdent, isCurrentShip: true, shipMaxJumpRange: MaxJumpRange, shipCargoCapacity: CargoCapacity, shipHullValue?, shipModulesValue?, shipRebuyCost?}`. |
| `StoredModules` | `setCommanderStorageModules` `[]` | Ordered by `StorageSlot`: `{itemName: Name, itemValue: BuyPrice, isHot: Hot, starsystemName?, marketID?, engineering?{blueprintName: EngineerModifications, blueprintLevel: Level, blueprintQuality: Quality}}`; only if changed. |
| `MissionAccepted` | `addCommanderMission` | `{missionName: Name, missionGameID: MissionID, influenceGain: Influence, reputationGain: Reputation, starsystemNameOrigin: current system, stationNameOrigin: current station, minorfactionNameOrigin: Faction}` + optional `missionExpiry←Expiry`, `starsystemNameTarget←DestinationSystem`, `stationNameTarget←DestinationStation`, `minorfactionNameTarget←TargetFaction`, `commodityName←Commodity`, `commodityCount←Count`, `targetName←Target`, `targetType←TargetType`, `killCount←KillCount`, `passengerType←PassengerType`, `passengerCount←PassengerCount`, `passengerIsVIP←PassengerVIPs`, `passengerIsWanted←PassengerWanted`. |
| `MissionAbandoned` | `setCommanderMissionAbandoned` | `{missionGameID}`. |
| `MissionCompleted` | `addCommanderPermit` (per `PermitsAwarded`); `setCommanderMissionCompleted` | `{missionGameID, donationCredits←Donation, rewardCredits←Reward, rewardPermits[{starsystemName}], rewardCommodities[{itemName, itemCount}]←CommodityReward, rewardMaterials←MaterialsReward, minorfactionEffects[{minorfactionName, influenceGain (longest + string), reputationGain}]}`. |
| `MissionFailed` | `setCommanderMissionFailed` | `{missionGameID}`. |
| `Died` | `addCommanderCombatDeath` | `{starsystemName}` + `wingOpponentNames←Killers[].Name`, or `opponentName←KillerName` or `KillerShip`. Dropped if no opponent. |
| `Interdicted` | `addCommanderCombatInterdicted` | `{starsystemName, isPlayer: IsPlayer, isSubmit: Submitted, opponentName: Interdictor or Faction or Power or 'Thargoid'}`. Dropped if no opponent. |
| `Interdiction` | `addCommanderCombatInterdiction` | `{starsystemName, isPlayer, isSuccess: Success, opponentName: Interdicted or Faction or Power or 'Thargoid'}`. |
| `EscapeInterdiction` | `addCommanderCombatInterdictionEscape` | `{starsystemName, isPlayer, opponentName: Interdictor or Faction or Power or 'Thargoid'}`. |
| `PVPKill` | `addCommanderCombatKill` | `{starsystemName, opponentName: Victim}`. |
| `ShipLocker` (uses `ShipLocker.json` when the event is a stub) | `resetCommanderInventory` `[]`; `setCommanderInventory` `[]` | Reset `[{itemType:'Items'},{itemType:'Components'},{itemType:'Data'},{itemType:'Consumables'}]`, then set all `{itemName: Name, itemCount: Count, itemType}`. |
| `CreateSuitLoadout`, `SuitLoadout` | `setCommanderSuitLoadout` | `{loadoutGameID: LoadoutID, loadoutName, suitGameID: SuitID, suitType: SuitName, suitMods: SuitMods, suitLoadout[{slotName: SlotName, itemName: ModuleName, itemClass: Class, itemGameID: SuitModuleID, engineering[{blueprintName: mod}]←WeaponMods}]}`. |
| `DeleteSuitLoadout` | `delCommanderSuitLoadout` | `{loadoutGameID}`. |
| `RenameSuitLoadout` | `updateCommanderSuitLoadout` | EDMC builds `{loadoutGameID, loadoutName, suitType, suitGameID}` but then **sends `{}` (bug)**. Send the built object. |
| `LoadoutEquipModule` | `updateCommanderSuitLoadout` | `{loadoutGameID, loadoutName, suitType, suitGameID, suitLoadout:[{slotName, itemName, itemGameID, itemClass, engineering}]}`. |
| `CommunityGoal` | `setCommunityGoal` + `setCommanderCommunityGoalProgress` (per goal) | Goal: `{communitygoalGameID: CGID, communitygoalName: Title, starsystemName: SystemName, stationName: MarketName, goalExpiry: Expiry, isCompleted: IsComplete, contributorsNum: NumContributors, contributionsTotal: CurrentTotal, tierReached?: int(TierReached last word), topRankSize?, tierMax?: int(TopTier.Name last word), completionBonus?: TopTier.Bonus}`. Progress: `{communitygoalGameID, contribution: PlayerContribution, percentileBand: PlayerPercentileBand, percentileBandReward?: Bonus, isTopRank?: PlayerInTopRank}`. |
| `Friends` | `addCommanderFriend` / `delCommanderFriend` | Status `Added`/`Online` → add. `Declined`/`Lost` → delete. `{commanderName: Name, gamePlatform: 'pc'}`. |
| `LoadGame` | (state only) | Resets cached state and starts a new session. |
| `Resurrect`, `ShipyardBuy`, `ShipyardSell`, `SellShipOnRebuy` | (state only) | Marks credits as "changed". |
| `Undocked`, `SupercruiseEntry` | (state only) | Undock/redock suppression. |

### 6.1 Where EDDiscovery differs (useful extras)

- **Inventory** uses the newer generic events:
  - `setCommanderInventory` `[]` with `itemType` (`Commodity`, `Raw`, `Encoded`, `Manufactured`, `Item`, `Component`, `Data`, `Consumable`) for `Cargo`/`Materials`, with `itemLocation:"ShipLocker"` for micro-resources.
  - `setCommanderInventoryItem` on `CollectCargo`, `EjectCargo`, `MarketBuy`, `MarketSell`, `MiningRefined`, `MaterialCollected`, `MaterialDiscarded`, `MaterialTrade`, `EngineerCraft`, `Synthesis`, `EngineerContribution`, `CargoDepot`, `MissionCompleted` rewards and `Buy`/`Sell`/`TradeMicroResources`.
- **Rank:** sends `setCommanderRankPilot` for all 8 ranks (`combat`, `trade`, `explore`, `soldier`, `exobiologist`, `empire`, `federation`, `cqc`) from `Rank` + `Progress`.
- **Credits:** `setCommanderCredits` whenever credits change by more than 500,000, and on `ShipyardBuy`/`ShipyardSell`/`Statistics`.
- **Header:** sends `"isDeveloped": false`, which is the wrong key name (the documented key is `isBeingDeveloped`).

### 6.2 EDMC quirks to *not* copy

- Session-start `setCommanderRankEngineer` sends `rankStage` as a `[rank, progress]` array for unlocked engineers, because of its internal tuple. Send `rankStage` as the stage string and `rankValue` as an int.
- `PowerplayLeave` sends `rankValue: 0`; use `-1`.
- `Touchdown` sends `isTaxiDropShip`; the documented key is `isTaxiDropship`.
- `RenameSuitLoadout` sends `{}`.

---

## 7. Rust implementation notes

- **Models:** `serde` structs for the header, event and response.
  - `eventData` is `serde_json::Value` or a per-event enum with `#[serde(untagged)]` for single vs array.
  - Skip `None` fields (`#[serde(skip_serializing_if = "Option::is_none")]`), because Inara treats present-but-null differently from absent **[UNVERIFIED]**.
- **Queue:** one persistent queue per API key with coalescing of "set" events. One sender task per key with a 35 s minimum interval. Exponential backoff on HTTP errors or an empty response.
- **Fatal header errors:** on a header-level 400, stop the key, show `eventStatusText`, and do not retry automatically until the user acts. Invalid keys and the hourly rate-limit lock both arrive this way.
- **Settings UI:**
  - API-key field.
  - Show `header.eventData.userName` after the first successful sync, as a "connected as …" check.
  - Link to https://inara.cz/elite/cmdr-settings-api/.
- **Header defaults:** `appName: "Elite Nexus"` (exactly what you register) and `appVersion` from `CARGO_PKG_VERSION`. `isBeingDeveloped` is left out (it defaults to false): it was a setting, on by default, for the first weeks after approval, and was removed after 0.7.3-beta.2.

## 8. [UNVERIFIED] / open questions

- How the generic app key is issued. (The response to a non-whitelisted `appName` is now known, see §2.)
- The exact current numeric rate limit. Documented by EDMC as "2 messages a minute", with a 1-hour lock on violation.
- Whether null-valued optional properties are accepted or rejected.
- The duplicate-timestamp dedupe behaviour reported by EDDiscovery.
