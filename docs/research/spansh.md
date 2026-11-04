# Spansh — implementation spec for Elite Nexus

Research date: **2026-10-12**. Status markers used below:

- **[TESTED]**: a real request was made during this research and the response shape was recorded. Samples are in `docs/research/spansh-samples/`.
- **[SITE-JS]**: read from the current spansh.co.uk front-end bundle (app version `0.1.903+d0f304f7`), which is exactly what the website sends.
- **[OPENAPI]**: from Spansh's official OpenAPI document at https://docs.spansh.co.uk/ (`Spansh API` v3.1.1, OpenAPI 3.1.0).
- **[EDD]**: from EDDiscovery's Apache-2.0 Spansh client (`EliteDangerousCore/EliteDangerous/3rdPartyInterfaces/Spansh/*.cs`).
- **[EDMC-SR]**: from the archived EDMC_SpanshRouter plugin.
- **UNVERIFIED**: inferred, or seen only in client code and not observed live.

Request budget used: 12 API requests to `spansh.co.uk/api` (1 neutron job, 1 result poll, 1 stations search, 1 bodies search, 3 `searchable_fields`, 2 autocomplete variants, 1 smart search, 1 nearest, 1 system dump). Separately, 17 `HEAD` requests and 4 ranged `GET`s (≤200 KB each) went to the downloads host.

### 0. Quick map: Elite Nexus feature → Spansh call

| Elite Nexus feature | Spansh call | § |
|---|---|---|
| System name autocomplete (with coordinates and id64) | `GET /api/systems/field_values/system_names?q=` | 5 |
| Global search box (systems, bodies, stations) | `GET /api/search?q=` | 5 |
| System, body and station detail panel | `GET /api/dump/{id64}` (cache it), `/api/system/{id64}`, `/api/body/{id64}`, `/api/station/{market_id}` | 5, 7.3 |
| "Where am I near" from coordinates | `GET /api/nearest?x=&y=&z=` | 5 |
| Neutron route | `POST /api/route` + `GET /api/results/{job}` | 2, 3.1 |
| Exact route for the current ship (journal Loadout) | `POST /api/generic/route` | 3.2 |
| Fleet carrier route | `POST /api/fleetcarrier/route` | 3.3 |
| Road to Riches, ELW/AW/HMC routes | `POST /api/riches/route` | 3.4 |
| Exobiology route | `POST /api/exobiology/route` | 3.5 |
| Trade route | `POST /api/trade/route` | 3.6 |
| Multi-stop tourist route | `POST /api/tourist/route` | 3.7 |
| Nearest commodity, module, ship, material trader, tech broker or Interstellar Factors | `POST /api/stations/search` | 4 |
| Mining hotspot finder | `POST /api/bodies/search` with the `ring_signals` filter | 4.7 B |
| Rhino surface mining: where to mine (Mining › Where to mine) | `POST /api/bodies/search` with the `signals` filter `Planetary Mining Location` | 4.7 C |
| Faction and power system search | `POST /api/systems/search` | 4.6 |
| Offline/local galaxy database | `downloads.spansh.co.uk` dumps plus the daily `galaxy_1day` | 7 |

---

## 1. General conventions

| Topic | Spec |
|---|---|
| Base URL | `https://spansh.co.uk/api` [OPENAPI `servers`]. The website calls it same-origin (`apiServer:""`). EDMC builds *web* links on `https://www.spansh.co.uk/...`. |
| Auth | None for everything in this document. `/api/user/*` and `/api/auth/*` use a Frontier-OAuth cookie session and are out of scope. |
| Encodings | **Route planners**: `POST` with `application/x-www-form-urlencoded; charset=UTF-8` and *traditional* array encoding, meaning an array is sent as a repeated key (`via=A&via=B`, never `via[]=`) [SITE-JS: jQuery `traditional:true`]. EDMC_SpanshRouter sends the same parameters as a URL query string on a `POST` with an empty body, and that also works [EDMC-SR]. **Engineer plotter** and **all searches**: `POST` with a JSON body. **Lookups**: `GET`. |
| Compression | Responses are gzip-encoded when `Accept-Encoding: gzip` is sent [TESTED: `Content-Encoding: gzip`, `Vary: Accept-Encoding`]. The Sol dump is 312 KB over the wire and 2.0 MB decoded. |
| IDs | `id64` (system or body) and `market_id` are unsigned 64-bit and can exceed 2^53. One example is body id64 `2017616565339460314`. The site parses JSON with `json-bigint` [SITE-JS]. In Rust, use `u64` (serde_json handles it), and never route these through `f64`. Search results also include `"id"` as a **string** duplicate (`"id":"128016640"` next to `"market_id":128016640`) [TESTED]. |
| Coordinates | `x,y,z` as `f64` in light-years, Sol at the origin. Values are multiples of 1/32 (for example `-9530.5`, `-910.28125`). |
| Timestamps | Search and lookup records use ISO-8601 with `Z` (`2026-10-12T07:15:43Z`). Job envelopes use `created_at`/`updated_at` **without** `Z` (`2026-10-12T09:17:41`); treat them as UTC [TESTED]. |
| Errors | Planners can return `{"status":"error","error":"<msg>"}`, or an HTTP 4xx with `{"error":"<msg>"}` [SITE-JS, EDMC-SR]. EDMC-SR looks for `"starting system"` / `"finishing system"` in the message to highlight a bad input (message text UNVERIFIED). Lookups return `404 {"error": ...}` for an unknown id [OPENAPI]. |
| Server | nginx/1.31.0. No rate-limit headers were observed (see §6). |

### 1.1 Website deep links (for "Open in Spansh" buttons) [SITE-JS router]

```
https://spansh.co.uk/system/{id64}
https://spansh.co.uk/body/{id64}
https://spansh.co.uk/station/{market_id}
https://spansh.co.uk/station/{market_id}/stock/{commodity}/{start}/{end}     price history
https://spansh.co.uk/search/{query}                                          (EDMC uses www.spansh.co.uk/search/{system_name})
https://spansh.co.uk/{systems|bodies|stations}/search/{search_guid}/{page}   page is 1-based in the UI
https://spansh.co.uk/plotter/results/{job}?efficiency=..&range=..&from=..&to=..&supercharge_multiplier=..
https://spansh.co.uk/{exact-plotter|fleet-carrier|riches|ammonia|earth|rocky-metal|exobiology|trade|tourist|colonisation|engineer-plotter}/results/{job}
https://spansh.co.uk/tourist/search/{search_guid}/results/{job}
https://spansh.co.uk/trade-to-system/{system}/{station}/{commodity}/results/{amount}
https://spansh.co.uk/distance-matrix/{id64}/{id64}/...
https://spansh.co.uk/dumps     https://spansh.co.uk/community-goals
```

The website keeps planner parameters in the results URL query string. If a job has expired (404), the page recomputes it from those parameters (§2).

---

## 2. Job / poll mechanism (all route planners)

1. **Submit**: `POST /api/<planner>` returns **HTTP 202** `{"job":"C6CBECF4-C61D-11F1-80DB-B431641780ED","status":"queued"}` [TESTED].
2. **Poll**: `GET /api/results/{job}`.
   - While running: **HTTP 202** and body `status:"queued"` [EDMC-SR checks 202; EDD checks `status=="queued"`]. `state` is `"queued"` and then `"started"`. While started, `updated_at` holds the start time; the site uses it, with `created_at`, for its progress bar [SITE-JS]. The in-progress body was not observed live, because the test job finished before the first poll (UNVERIFIED).
   - Done: **HTTP 200** [TESTED]:
     ```json
     {"created_at":"2026-10-12T09:17:41","updated_at":"2026-10-12T09:17:44",
      "job":"C6CB…","state":"completed","status":"ok",
      "parameters":{ …echo of the submitted parameters, values as strings… },
      "result":{ …planner-specific… }}
     ```
   - **404**: unknown or expired job. The site resubmits the original parameters (`send("calculate")`) [SITE-JS].
   - Other errors: the site rejects with `responseJSON.error` [SITE-JS].
3. **Polling cadence**:
   - Website [SITE-JS `routes/base/plotter/new-results`]: first poll after 250 ms, then the delay **doubles** each time, capped at **16 s**.
   - EDMC-SR: 1 s × 20 tries.
   - **Recommended for Elite Nexus**: 500 ms → ×2 → cap 8 s. Use an overall deadline of 60 s, or `max_time + 30 s` for the galaxy plotter. Keep the original parameters so you can resubmit once on 404.

```rust
// suggested shape
enum JobPoll { Queued { state: String }, Done(serde_json::Value), Expired /*404*/, Failed(String) }
```

---

## 3. Route planners

Every planner below uses the §2 mechanism. Parameter defaults are the website defaults.

### 3.1 Neutron plotter: `POST /api/route` [TESTED]

| Param | Type | Default | Meaning |
|---|---|---|---|
| `from` | string (system name) | required | Start system |
| `to` | string | required | Destination |
| `range` | float LY | required | Ship jump range (laden) |
| `efficiency` | int 1–100 | 60 | How closely to follow the straight line. At 100 the route never detours for neutrons and is split into ~20,000 LY blocks. Lower values detour more to reach neutron stars. |
| `supercharge_multiplier` | int | 4 | 4 = normal neutron supercharge. 6 = the site's "Overcharge Supercharge" option. |
| `via` | string, repeatable | – | Ordered intermediate waypoints |

The submit request that was tested (form body):
`efficiency=60&range=50&from=Sol&to=Colonia&supercharge_multiplier=4`

Result (`result` object) [TESTED, sample `neutron_route_sol_colonia.result.json`]:
```json
{"source_system":"Sol","destination_system":"Colonia","distance":22000.4740453411,
 "efficiency":"60","range":"50","job":"…","via":[],"total_jumps":128,
 "system_jumps":[{"system":"Sol","id64":10477373803,"x":0,"y":0,"z":0,
                  "distance_jumped":0,"distance_left":22000.47,"jumps":0,"neutron_star":false}, …]}
```

- `system_jumps[]` is one row per waypoint. Each row is a place where the player must open the galaxy map and plot again.
- `jumps` is the *estimated* number of jumps to reach that waypoint.
- Observation: `total_jumps` (128) equalled `len(system_jumps)-1`, while `Σ jumps` was 166. Use `Σ jumps` as the estimated total jump count.
- The test route had 129 waypoints over 22,000 LY at a 50 LY range.

### 3.2 Galaxy plotter (exact plotter): `POST /api/generic/route` [SITE-JS + EDD]

This plotter takes fuel, neutron, injection and refuelling into account. It needs FSD and ship constants.

| Param | Type | Default | Meaning |
|---|---|---|---|
| `source` | id64 (site) or name (EDD) | required | Start system |
| `destination` | id64 or name | required | End system |
| `is_supercharged` | 0/1 | 0 | Already supercharged at the start |
| `use_supercharge` | 0/1 | 1 | Use neutron and white-dwarf boosts |
| `use_injections` | 0/1 | 0 | Use FSD synthesis whenever no neutron is available |
| `use_injections_when_required` | 0/1 | 0 | Use synthesis only when needed (site only; EDD does not send it) |
| `exclude_secondary` | 0/1 | 0 | Do not use secondary neutron or scoopable stars |
| `refuel_every_scoopable` | 0/1 | 1 | Refuel at every scoopable star |
| `fuel_power` | float | from FSD | FSD fuel power constant |
| `fuel_multiplier` | float | from FSD | FSD fuel multiplier (linear constant) |
| `optimal_mass` | float t | from FSD (+engineering) | FSD optimal mass |
| `max_fuel_per_jump` | float t | from FSD (+engineering) | FSD maximum fuel per jump |
| `base_mass` | float t | `UnladenMass + Reserve` | Ship mass without fuel or cargo, plus the reservoir |
| `tank_size` | float t | `FuelCapacity.Main` | Main tank |
| `internal_tank_size` | float t | `FuelCapacity.Reserve` | Reservoir (internal) tank |
| `reserve_size` | float t | 0 | User fuel reserve that the router must not use |
| `range_boost` | float LY | Guardian FSD booster `jumpboost` | Omit if no booster is fitted |
| `cargo` | int t | 0 | Cargo carried |
| `max_time` | int s | 60 | Calculation time. The UI allows 60–120 s, or 240 s for Patreon patrons (client-side check). |
| `algorithm` | enum | `optimistic` | `fuel`, `fuel_jumps`, `guided`, `optimistic`, `pessimistic` (definitions below) |
| `supercharge_multiplier` | int | 4 | 6 for "overcharge" FSDs (see the FSD table below) |
| `injection_multiplier` | number | 2 | Synthesis boost multiplier |
| `ship_build` | string (JSON) | – | Raw build JSON: SLEF or Coriolis `ship-loadout/4`. The site and EDD always send it. Send SLEF. Whether the server requires it is UNVERIFIED. |

**Algorithms** (site help text, paraphrased):

- `optimistic`: favours neutron jumps and penalises sparse neutron regions. Usually the fastest route with the fewest jumps.
- `pessimistic`: favours calculation speed by over-estimating star spacing, which can give a less optimal route.
- `guided`: follows a neutron-plotter route as a guide and penalises routes more than 100 LY off it.
- `fuel`: minimises fuel use, with no scooping and no supercharging.
- `fuel_jumps`: like `fuel`, then reduces the jump count to use the whole tank.
- Logged-in patrons get `dijkstra` (exhaustive) on `/api/user/generic/route`. That endpoint is not for Elite Nexus.

**Deriving FSD parameters from the journal `Loadout` event.** This mirrors Spansh `objects/ship.js` `parseSLEF()`:

1. Find the module with `Slot == "FrameShiftDrive"`. Look up its `Item` symbol (lower-case) in the coriolis-data FSD table (`Modules.standard.fsd`, matched by `symbol`) to get `fuelpower`, `fuelmul`, `optmass`, `maxfuel`.
2. If `Engineering.Modifiers` is present:
   - `Label=="FSDOptimalMass"` replaces `optmass` with `Value`.
   - `Label=="MaxFuelPerJump"` replaces `maxfuel`.
3. `tank_size = FuelCapacity.Main`, `internal_tank_size = FuelCapacity.Reserve`, `base_mass = UnladenMass + FuelCapacity.Reserve`.
4. Guardian booster: find a module whose `Item` matches `^int_guardianfsdbooster`, look up `Modules.internal.gfsb[*].jumpboost` by `symbol`, and send it as `range_boost`.
5. Spansh overrides coriolis-data for these FSDs (`fsd_mapping`):

   | symbol | optmass | maxfuel | fuelmul | fuelpower | supercharge |
   |---|---|---|---|---|---|
   | `int_hyperdrive_overcharge_size8_class1` | 2800 | 13.6 | 0.008 | 2.9 | – |
   | `…_size8_class2` / `class3` / `class4` | 4200 | 20.4 | 0.012 | 2.9 | – |
   | `…_size8_class5` | 4670 | 20.7 | 0.013 | 2.9 | – |
   | `…_size8_class5_overchargebooster_mkii` | 4670 | 6.8 | 0.011 | 2.5025 | **6** |

   The defaults are `superchargeMultiplier=4` and `injectionMultiplier=2`.
6. For Coriolis builds, the reservoir size comes from a per-ship table in `calculateReservoirSize` (for example Anaconda 1.07, Python 0.83). A journal-based build has `FuelCapacity.Reserve` directly, so the table is not needed.

**SLEF wrapper** to send as `ship_build`. This is the format EDD sends, and the Spansh parser requires `header` plus `data`:
```json
[{"header":{"appName":"EliteNexus","appVersion":"0.1.0","appURL":"https://…"},
  "data":{"event":"Loadout","Ship":"anaconda","ShipID":12,"UnladenMass":…,"FuelCapacity":{"Main":32,"Reserve":1.07},"Modules":[…]}}]
```

**Result** [SITE-JS columns + EDD; not tested live]:

- `result.jumps[]` has `{name, id64, x, y, z, distance, distance_to_destination, fuel_in_tank, fuel_used, must_refuel, has_neutron, is_scoopable, must_inject}`.
- `result.incomplete` and `result.errors` flag a partial route.
- `parameters.source_system` and `parameters.destination_system` are echoed.

The site warns players to *target* the next system rather than plot to it, because the in-game galaxy map can refuse maximum-range plots when fuel is low.

### 3.3 Fleet carrier router: `POST /api/fleetcarrier/route` [SITE-JS + EDD]

| Param | Type | Meaning |
|---|---|---|
| `source` | id64 (site) or name (EDD) | Start system |
| `destinations` | repeatable id64 or name | Ordered waypoints. The last one is the final destination. The site can import an EDSM-style text file with one system per line, split on `\|`. |
| `capacity` | int | 25000 for a player carrier, 60000 for a squadron carrier |
| `mass` | int | 25000 for a player carrier, 15000 for a squadron carrier |
| `capacity_used` | int t | Market plus modules used capacity, as shown in carrier management |
| `calculate_starting_fuel` | 0/1 | 1 means "determine tritium requirements" (default 1) |
| `refuel_destinations` | repeatable id64 | Only sent with `calculate_starting_fuel=1`. Must be a subset of `destinations`. |
| `fuel_loaded` | int t | Only sent with `calculate_starting_fuel=0`: tritium in the tank (UI default 1000) |
| `tritium_stored` | int t | Only sent with `calculate_starting_fuel=0`: tritium in the carrier market |

**Result**:

- `result.jumps[]` has `{name, id64, x, y, z, distance, distance_to_destination, fuel_in_tank, fuel_used, tritium_in_market, has_icy_ring, is_system_pristine, must_restock, restock_amount, is_desired_destination}`.
- The site estimates 20 minutes per jump and 150 t/h of tritium mining.

### 3.4 Road to Riches and the specialised body routes: `POST /api/riches/route` [SITE-JS + EDD]

| Param | Type | Default | Meaning |
|---|---|---|---|
| `from` | name | required | Start system |
| `to` | name | – | Omit (or send `loop=1`) for a circular tour |
| `range` | float LY | required | Jump range |
| `radius` | int LY | 25 | Search radius around the route. Use 100–500 for circular tours. |
| `max_results` | int | 100 | Maximum systems |
| `max_distance` | int LS | 50000 | Maximum distance-to-arrival of bodies |
| `min_value` | int cr | 100000 | Minimum body value. The specialised routes below send 1. |
| `use_mapping_value` | 0/1 | 0 | Rank by DSS (mapping) value instead of FSS value |
| `avoid_thargoids` | 0/1 | 1 | Avoid Thargoid-war or Thargoid-controlled systems |
| `loop` | 0/1 | 1 | Return to the start |
| `body_types` | repeatable string | – | Restrict body subtypes |

**Specialised routes** use the same endpoint and only change `min_value=1` and `body_types` [SITE-JS]:

- Ammonia worlds: `body_types=Ammonia world`
- Earth-likes: `body_types=Earth-like world`
- Rocky/HMC: `body_types=Rocky body&body_types=High metal content world`

Any other value from the body `subType` enum is presumably accepted (UNVERIFIED).

**Result**:

- `result` is an **array** of systems: `{name, id64, x, y, z, jumps, bodies:[{name, subtype, type, is_terraformable, distance_to_arrival, estimated_scan_value, estimated_mapping_value}]}`.
- If fewer systems come back than `max_results`, raise `radius` or lower `min_value` (site hint).

### 3.5 Exobiology ("Expressway to Exomastery"): `POST /api/exobiology/route` [SITE-JS + EDD]

The parameters are the same as R2R, without `use_mapping_value` and `body_types`:

- `from`, `to?`, `range`, `radius` (25), `max_results` (100), `max_distance` (50000), `avoid_thargoids` (1), `loop` (1)
- `min_value`: default **10,000,000**

**Result**:

- `result[]` has `{name, id64, x, y, z, jumps, bodies:[{name, subtype, type, distance_to_arrival, estimated_scan_value, estimated_mapping_value, landmark_value, landmarks:[{type:"Bacterium", subtype:"Bacterium Aurasus", count?, value}]}]}`.
- Landmarks can repeat per subtype. The site sums `count` per subtype.

### 3.6 Trade planner: `POST /api/trade/route` [SITE-JS + EDD]

EDD records a real query captured in December 2025:
`max_hops=5&max_hop_distance=50&system=Sol&station=Abimbola+Metallurgic+Reserve&starting_capital=1000&max_cargo=7&max_system_distance=10000000&max_price_age=2592000&requires_large_pad=1&allow_prohibited=1&allow_planetary=1&allow_player_owned=1&allow_restricted_access=1&unique=1&permit=1`.

| Param | Default | Meaning |
|---|---|---|
| `system`, `station` | required | Starting system and station names |
| `max_hops` | 5 | Number of trade legs |
| `max_hop_distance` | 50 LY | Maximum distance per hop (can exceed ship range) |
| `starting_capital` | 1000 cr | |
| `max_cargo` | 7 t | Cargo capacity |
| `max_system_distance` | 10,000,000 LS | Maximum station distance-to-arrival |
| `max_price_age` | omitted | Seconds. The site sends `now − chosen_datetime`. |
| `requires_large_pad`, `allow_prohibited`, `allow_planetary`, `allow_player_owned` (fleet carriers), `allow_restricted_access`, `unique` (avoid loops), `permit` | 0 | 0/1 flags |

**Result**: `result[]`, one entry per hop:
```
{source:{system, station, market_id, distance_to_arrival, market_updated_at, x, y, z, system_id64},
 destination:{…same…}, distance,
 commodities:[{name, amount, profit, total_profit, source_commodity:{buy_price,…}, destination_commodity:{sell_price,…}}],
 cumulative_profit, total_profit}
```

### 3.7 Tourist router: `POST /api/tourist/route` [SITE-JS]

**Parameters**:

- `range`
- `source` (name)
- `destination` (repeatable names, to visit in an optimised order)
- `final_destination` (optional)
- `loop` (0/1, default 1)

The site can import an EDSM waypoint text file.

**Result**: `result.system_jumps[]` has `{system, jumps, x, y, z, …}` and `result.source_system`.

**Search-based tourist route**: `POST /api/search/tourist/route/{search_guid}` takes `range`, `source`, `final_destination` and `loop`, and visits every result of a saved search (§4.1).

### 3.8 Colonisation plotter: `POST /api/colonisation/route` [SITE-JS]

**Parameters**:

- `source_system`: name. Leave blank for "shortest route to the target, regardless of controlling faction" (site help).
- `destination_system`: name.

**Result**:

- `result.jumps[]` has `{name, x, y, z, distance, distance_to_destination, must_refuel, body_count, estimated_scan_value, landmark_value}`.
- `parameters.source_system` and `parameters.destination_system` are echoed.

The exact semantics (a chain of colonisation-range hops) are UNVERIFIED.

### 3.9 Engineer plotter: `POST /api/engineer/route` (JSON body) [SITE-JS]

```json
{"source":"Sol",
 "blueprints":[{"slot":"Frame Shift Drive","name":"Increased Range","grade":5,"effect":"Mass Manager"}],
 "engineers":["Felicity Farseer"]}
```

- Omit `engineers` to allow all engineers.
- Vocabularies come from these lookups, each returning `{"values":[…]}` [SITE-JS]: `GET /api/engineers`, `GET /api/modules`, `GET /api/blueprints`, `GET /api/effects`.
- The result structure is UNVERIFIED. The template references `result.jumps[].{system, distance, engineer, blueprints[].to.{slot,name,grade,effect}}`.

### 3.10 "Trade to system" commodity buying: `POST /api/commodity/buy/{system}/{commodity}/{amount}` [SITE-JS]

- The request has no body.
- The response lists nearby stations that sell `amount` of `commodity`. Rows have `{system_name, name, body_name, market_id, distance, distance_to_arrival, has_large_pad, market_updated_at, buy_price, supply}`.
- The envelope is UNVERIFIED.

### 3.11 Other site-only endpoints (do not use)

| Endpoint | Purpose |
|---|---|
| `/api/user/generic/route`, `/api/user/results/{job}`, `/api/user/routes`, `/api/user/scan/…`, `/api/user/scanned/…`, `/api/user/profile` | Logged-in features (Frontier OAuth cookie) |
| `/api/ship/{guid}` | Shared ship build for the exact plotter |
| `/api/bad_data/{id}` | Data error reports |
| `/api/error` | JS error logging |

---

## 4. Search APIs (systems, bodies, stations)

### 4.1 Endpoints

`{type}` is one of `systems`, `bodies`, `stations`.

| Method | URL | Notes |
|---|---|---|
| POST (JSON) | `/api/{type}/search` | Returns results directly [TESTED] |
| POST (JSON) | `/api/{type}/search/save` | Returns `{"search_reference":"<GUID>", …}`. This is what the website does before navigating [SITE-JS]. |
| GET | `/api/{type}/search/recall/{guid}` and `/api/{type}/search/recall/{guid}/{page}` | Re-runs a saved search. The path `page` is **0-based**; the site omits it for page 0 [SITE-JS]. |
| GET | `/api/{type}/searchable_fields` | Field catalogue: name, type, subfields [TESTED, samples saved] |
| GET | `/api/{type}/sortable_fields` | Sortable fields [SITE-JS] |
| GET | `/api/{type}/field_values/{field}` and `?q=` | Enum values for a group field (`{"values":[…]}`, possibly grouped as `{"values":{group:[…]}}`). For numeric fields, min/max for range sliders. Some fields name their own URL, such as `/api/stations/field_values/autocomplete_controlling_minor_faction`. |

A direct `/search` response also returns a `search_reference`, so results can be shared as `https://spansh.co.uk/stations/search/{guid}/1`.

### 4.2 Request envelope [SITE-JS `createStationSearch` etc., TESTED]

```json
{
  "filters": { … },
  "sort":    [ { "<field>": { "direction": "asc|desc" } } ],
  "size": 10,
  "page": 0,
  "reference_system": "Sol"
}
```

- **`size`**: rows per page. The site offers 5, 10, 25, 50, 100 and 500. EDD notes that **500 is the maximum**.
- **`page`**: 0-based.
- **Reference point**: choose exactly one.
  - `"reference_system": "<name>"`
  - `"reference_coords": {"x":0,"y":0,"z":0}`
  - `"reference_route": {"source":"Sol","destination":"Colonia"}` (distance measured to a route). [TESTED 2026-11-04] The route is the straight **segment** between the two systems, not a plotted route and not an infinite line: Sol→Alpha Centauri within 3 ly returned only those two systems' bodies. The response's `reference` is then an **array** of both ends (`[{id64,name,x,y,z},{…}]`) instead of one object.

  The `distance` filter and sort need a reference point.

### 4.3 Filter syntax per field type [SITE-JS `create*Filters`]

| Field type (from `searchable_fields`) | JSON shape |
|---|---|
| `string` | `"name": {"value": "Sol"}`. Wildcard `*` is supported (EDD uses `"value":"Col 285*"`). |
| `group` / `autocomplete` (enum) | `"primary_economy": {"value": ["High Tech","Industrial"]}`. Add `"logic":"AND"` (must have all) or `"logic":"NOT"` (none of). Omit `logic` for "any" [SITE-JS `autocomplete-search`]. |
| `numeric` (`real`/`integer`) range | `"population": {"comparison": "<=>", "value": [1000000, 1000000000]}`. The read-back code also handles `{"comparison":">=","value":x}` and `{"comparison":"<=","value":x}`. |
| `boolean` | `"has_large_pad": {"value": true}`. The site sends `{}` for its third UI state (`-1`); that probably means "field present, any value" (UNVERIFIED). |
| distance | `"distance": {"min": "0", "max": "100"}`. Strings work [TESTED] and are what EDD sends. Plain numbers are what the site's slider would produce, but that is UNVERIFIED. |
| `timestamp` | `"market_updated_at": {"comparison":"<=>","value":["now-1d","now"]}`. Values can be absolute ISO-8601 (`2026-10-01T00:00:00.000Z`), relative `now-<n><unit>` with unit `s m h d w M y` (Elasticsearch date-math style), or `now` [SITE-JS `date-input-field`]. |
| `value_struct` (name plus number) | A list of `{"name": <subfield value>, "<valuefield>": [min,max], "comparison":"<=>"}`, for example `"ring_signals": [{"name":"Painite","count":[2,999],"comparison":"<=>"}]` [TESTED]. Also `signals` (count), `materials`, `atmosphere_composition` and `solid_composition` (share %), and stations `economies` (share %). |
| `combined` (row of subfields) | A list of objects keyed by subfield. Group and string subfields are `{"value":[…]}` (plus optional `logic`). Numeric subfields are `{"comparison":"<=>","value":[a,b]}`. Boolean subfields are `{"value":true}`. Example: `"minor_faction_presences":[{"name":{"value":["Mother Gaia"]},"influence":{"comparison":"<=>","value":[0.1,1]}}]`. |
| `special` `market` (stations) | `"market":[{"name":"Tritium","supply":{"value":[1000,999999999],"comparison":"<=>"},"buy_price":{"value":[0,60000],"comparison":"<=>"}}]`. Subfields are `buy_price`, `sell_price`, `supply`, `demand` [TESTED]. |
| `special` `rings` / `belts` (bodies) | `"rings":[{"type":["Icy"],"inner_radius":{"comparison":"<=>","value":[a,b]},"outer_radius":{…}}]`. Whether `type` is an array or a string is UNVERIFIED. |

EDDiscovery uses **simpler object forms** for some combined fields. Its code is dated 2026, so they presumably still work (UNVERIFIED by us):

- `"services":{"value":["Material Trader"]}`
- `"ships":{"value":["Anaconda"]}`
- `"modules":{"name":["Frame Shift Drive"],"class":["5"],"rating":["A"]}`

The site's combined forms are:

- `"services":[{"name":{"value":[…]}}]`
- `"ships":[{"name":{"value":[…]}}]`
- `"modules":[{"name":{"value":[…]},"class":{"value":["5"]},"rating":{"value":["A"]}}]`

EDD omits `class`/`rating` when "all" is selected, because some modules (for example military composites) otherwise return nothing.

### 4.4 Sort syntax [SITE-JS `createGenericSort`]

- Normal field: `[{"distance":{"direction":"asc"}}]`
- `value_struct`: `[{"ring_signals":[{"name":"Painite","direction":"desc"}]}]`
- `combined`: `[{"marketplace":[{"name":"supply","direction":"desc"}]}]`
- Station market column: `[{"market_buy_price":[{"name":"Tritium","direction":"asc"}]}]`. The key is `market_` plus `buy_price`, `sell_price`, `supply` or `demand`.

### 4.5 Response envelope [TESTED]

```json
{"count":1720,"from":0,"size":3,
 "reference":{"id64":10477373803,"name":"Sol","x":0.0,"y":0.0,"z":0.0},
 "search":{ …echo of the request…, "reference_system_details":{"id64":…,"name":"Sol","x":0,"y":0,"z":0}},
 "search_reference":"D89314F8-C61D-11F1-8DFD-D2F019E66765",
 "results":[ … ]}
```

- `count` is the total number of hits.
- Every result has `distance` (LY to the reference point).

**Station result** fields [TESTED]:

- Identity and location: `id`(string), `market_id`, `name`, `type`, `state`, `is_planetary`, `distance`, `distance_to_arrival`, `updated_at`.
- Faction and economy: `allegiance`, `government`, `controlling_minor_faction`, `controlling_minor_faction_influence`, `primary_economy`, `economies[{name,share}]`.
- Pads and services: `has_large_pad`, `large_pads`, `medium_pads`, `small_pads`, `has_market`, `has_outfitting`, `has_shipyard`, `services[{name}]`.
- Market: `market[{commodity,category,buy_price,sell_price,supply,demand}]`, `market_updated_at`, `export_commodities[{name}]`, `import_commodities[{name}]`.
- Outfitting: `modules[{name,category,class,rating,ed_symbol,price?,ship?}]`, `outfitting_updated_at`.
- Shipyard: `ships[{name,price,symbol}]`, `shipyard_updated_at`.
- System: `system_name`, `system_id64`, `system_x/y/z`, `system_population`, `system_primary_economy`, `system_secondary_economy`, `system_controlling_power`, `system_power[]`, `system_power_state`, `system_is_colonised`, `system_is_being_colonised`.
- Also present per the OpenAPI station record: `material_trader` (None/Encoded/Manufactured/Raw), `technology_broker` (None/Guardian/Human), `carrier_name`, `carrier_docking_access`, `body_name`, `prohibited_commodities`.

Results are heavy: 3 stations came to 209 KB. Ask for a small `size` and page through.

**Body result** fields [TESTED]:

- Identity: `id`(string), `id64`, `body_id`, `name`, `type`, `subtype`, `updated_at`.
- Location: `distance`, `distance_to_arrival`.
- Physical: `gravity`, `radius`, `earth_masses`, `surface_temperature`, `surface_pressure`, `atmosphere`, `atmosphere_composition[{name,share}]`, `is_landable`, `terraforming_state`, `reserve_level`, `is_main_star`.
- Orbit and rotation: `orbital_*`, `semi_major_axis`, `arg_of_periapsis`, `axis_tilt`, `rotational_period`, `is_rotational_period_tidally_locked`, `parents[{id64,subtype,type}]`.
- Values: `estimated_scan_value`, `estimated_mapping_value`.
- Signals: `signals[{name:"Human",count}]`, `signal_count`, `signals_updated_at`.
- Rings: `rings[{name,type,mass,inner_radius,outer_radius,signal_count,signals[{name:"Painite",count}],signals_updated_at}]`.
- System: `system_name`, `system_id64`, `system_x/y/z`, `system_region`, `system_controlling_power`, `system_power_state`.

**System result**: see the record in §5 (`/api/system`). EDD reads `name`, `id64`, `x`, `y`, `z`, `distance` and `bodies[{is_main_star,subtype}]`.

### 4.6 Field catalogue (live `searchable_fields`, 2026-10-12)

The full JSON is in `spansh-samples/searchable_fields_{systems,bodies,stations}.json`. Types: G = group, N = numeric, B = boolean, S = string, T = timestamp, K = keyword, VS = value_struct, C = combined, SP = special.

**systems**:

| Type | Fields |
|---|---|
| S | `name` |
| K | `id64` |
| N | `x`, `y`, `z`, `distance`, `population`, `body_count`, `nearest_populated_distance`, `estimated_scan_value`, `estimated_mapping_value`, `landmark_value`, `power_state_control_progress`, `power_state_reinforcement`, `power_state_undermining`, `thargoid_war_progress`, `thargoid_war_days_remaining`, `thargoid_war_ports_remaining` |
| G | `allegiance`, `government`, `security`, `state`, `primary_economy`, `secondary_economy`, `region`, `power`, `controlling_power`, `power_state`, `controlling_minor_faction` (autocomplete), `controlling_minor_faction_state`, `thargoid_war_state`, `thargoid_war_success_state`, `thargoid_war_failure_state` |
| B | `needs_permit`, `known_permit`, `is_colonised`, `is_being_colonised`, `thargoid_war_success_reached` |
| T | `updated_at` |
| C | `bodies{type,subtype,distance_to_arrival}`, `stations{name,type,controlling_minor_faction,controlling_minor_faction_state,services,distance_to_arrival,small/medium/large_pads,market_id}`, `minor_faction_presences{name,allegiance,government,influence,state}`, `landmarks{type,subtype,count,value}` (path `bodies.landmarks`), `synthesis_recipes{name,level}` |

**bodies**:

| Type | Fields |
|---|---|
| S | `name`, `system_name` |
| K | `id64`, `system_id64` |
| N | `distance`, `distance_to_arrival`, `system_x/y/z`, `gravity`, `radius`, `earth_masses`, `solar_masses`, `solar_radius`, `age`, `surface_temperature`, `surface_pressure`, `orbital_period`, `semi_major_axis`, `orbital_eccentricity`, `orbital_inclination`, `arg_of_periapsis`, `ascending_node`, `mean_anomaly`, `axis_tilt`, `rotational_period`, `orbital_synchronicity`, `estimated_scan_value`, `estimated_mapping_value`, `landmark_value`, `signal_count` |
| G | `type`, `subtype`, `spectral_class`, `luminosity_class`, `atmosphere`, `volcanism_type`, `terraforming_state`, `reserve_level`, `system_region`, `system_controlling_power`, `system_power_state`, `parent_type`, `parent_subtype`, `genuses` (multiple) |
| B | `is_landable`, `is_main_star`, `is_rotational_period_tidally_locked` |
| T | `updated_at`, `signals_updated_at` |
| VS | `signals{name,count}`, `ring_signals{name,count}` (path `rings.signals`), `materials{name,share}`, `atmosphere_composition{name,share}`, `solid_composition{name,share}` |
| C | `landmarks{type,subtype,variant,value}`, `stations{…}`, `parents{type,subtype}`, `synthesis_recipes{name,level}` |
| SP | `rings`, `belts` |

**stations**:

| Type | Fields |
|---|---|
| S | `name`, `body_name`, `system_name`, `carrier_name`, `real_name` |
| K | `market_id`, `system_id64`, `body_id64` |
| N | `distance`, `distance_to_arrival`, `small_pads`, `medium_pads`, `large_pads`, `system_population`, `controlling_minor_faction_influence`, `body_gravity` |
| G | `type`, `state`, `allegiance`, `government`, `primary_economy`, `secondary_economy`, `system_primary_economy`, `system_secondary_economy`, `system_power`, `system_controlling_power`, `system_power_state`, `controlling_minor_faction` (autocomplete), `controlling_minor_faction_state`, `material_trader`, `technology_broker`, `carrier_docking_access`, `body_type`, `body_subtype` |
| B | `has_market`, `has_shipyard`, `has_outfitting`, `has_large_pad`, `is_planetary`, `is_dssa_carrier`, `body_has_atmosphere`, `body_has_rings`, `system_is_colonised`, `system_is_being_colonised` |
| T | `updated_at`, `market_updated_at`, `outfitting_updated_at`, `shipyard_updated_at` |
| VS | `economies{name,share}` |
| C | `services{name}`, `ships{name,price}`, `modules{name,class,rating,price}`, `marketplace{commodity,supply,buy_price,demand,sell_price,is_home_station,is_rare}` (path `market`), `export_commodities{name}`, `import_commodities{name}`, `prohibited_commodities{name}` |
| SP | `market` |

### 4.7 Worked examples

**A. Tested.** Nearest stations with Tritium supply ≥ 1000 and a large pad, within 100 LY of Sol. Files: `stations_search_tritium_near_sol.request.json` and `.response.trimmed.json`.
```json
{"filters":{"market":[{"name":"Tritium","supply":{"value":["1000","999999999"],"comparison":"<=>"}}],
            "has_large_pad":{"value":true},"distance":{"min":"0","max":"100"}},
 "sort":[{"distance":{"direction":"asc"}}],"size":3,"page":0,"reference_system":"Sol"}
```
The response was `count` 1720. The first results were Galileo, Daedalus and Columbus in Sol, for example Tritium at Galileo with buy 50137, sell 49107, supply 355413.

**B. Tested.** Bodies with ≥ 2 Painite hotspots in a ring, within 200 LY of Sol, nearest first. Files: `bodies_search_painite_rings.*`.
```json
{"filters":{"ring_signals":[{"comparison":"<=>","count":[2,999],"name":"Painite"}],"distance":{"min":"0","max":"200"}},
 "sort":[{"distance":{"direction":"asc"}}],"size":3,"page":0,"reference_system":"Sol"}
```
The response was `count` 3029: Barnard's Star 5 (A ring Painite ×2), WISE 0855-0714 6 and Kokary 5. The filter matches when **any** ring of the body satisfies it.

**C. Tested 2026-11-04.** Rhino surface mining: landable bodies with planetary mining locations (19 requests).

- The signal's search name is `Planetary Mining Location`; `GET /api/bodies/field_values/signals` lists it with counts 1 to 36. Search results carry it in `signals[]` under that name; dumps keep the journal symbol `$PlanetaryMiningLocation_Name;` (§7.3).
  ```json
  {"filters":{"is_landable":{"value":true},
              "signals":[{"name":"Planetary Mining Location","count":[1,999],"comparison":"<=>"}],
              "subtype":{"value":["Rocky body"]},
              "volcanism_type":{"value":["Major Rocky Magma","Rocky Magma","Minor Rocky Magma"]},
              "distance":{"min":"0","max":"100"}},
   "sort":[{"distance":{"direction":"asc"}}],"size":5,"page":0,"reference_system":"Sol"}
  ```
  That found 30 rocky bodies with rocky magma and mining locations within 100 ly of Sol; without `subtype` and `volcanism_type` the count hits the 10,000 cap. The filters are ANDed.
- `GET /api/bodies/field_values/volcanism_type` has 22 values: `No volcanism`, metallic magma, rocky magma and silicate vapour geysers (each plain, `Major` and `Minor`), water geysers and water magma (each three ways), carbon dioxide geysers (three ways), and `Minor Ammonia Magma`, `Minor Methane Magma`, `Minor Nitrogen Magma`. There is **no silicate magma**, although the journal manual lists it.
- A body without volcanism usually has **no `volcanism_type` field** in a search result (Mercury, Ganymede). Filtering on `"No volcanism"` matched 7 rocky bodies with mining locations within 20 ly of Sol; `{"value":[the 21 volcanic values],"logic":"NOT"}` matched 52, Ganymede and Callisto among them. Use the `NOT` form.
- Coverage: only scans since the Rhino update (4.4.1, 2026-09-02) report the signal. Within 20 ly of Sol, 559 of 703 landable bodies have a count; within 25 ly of Colonia, 3,161 of 4,089 (and of the 3,347 updated since 2026-09-02, 3,161). A body without a count may still have locations.

**D. Constructed, not executed.** These are built from the site's builders and the field catalogue:

- **Nearest raw material trader**:
  ```json
  {"filters":{"material_trader":{"value":["Raw"]},"distance":{"min":0,"max":200}},"sort":[{"distance":{"direction":"asc"}}],"size":5,"reference_system":"Sol"}
  ```
- **Nearest Guardian technology broker**: replace the filter with `"technology_broker":{"value":["Guardian"]}`.
- **Interstellar Factors**: `"services":[{"name":{"value":["Interstellar Factors Contact"]}}]`.
- **Outfitting, 5A FSD**: `"modules":[{"name":{"value":["Frame Shift Drive"]},"class":{"value":["5"]},"rating":{"value":["A"]}}]` (since tested, with several modules at once: §4.8).
- **Ship for sale**: `"ships":[{"name":{"value":["Mandalay"]}}]` (tested with `Anaconda` and `Anaconda Mk II`, §4.8).
- **Exclude fleet carriers**: `"type":{"value":["Coriolis Starport","Orbis Starport","Ocellus Starport","Outpost","Planetary Port","Planetary Outpost","Asteroid base","Mega ship","Dodec Starport","Dockable Planet Station","Settlement"]}`, or `{"value":["Drake-Class Carrier"],"logic":"NOT"}` (the second form tested, §4.8).
- **Fresh markets only**: `"market_updated_at":{"comparison":"<=>","value":["now-2d","now"]}`.
- **Powerplay system search**: `{"filters":{"controlling_power":{"value":["Jerome Archer"]},"power_state":{"value":["Fortified","Stronghold"]}}}` on `/api/systems/search`.
- **Biological signals ≥ 3**: `"signals":[{"name":"Biological","count":[3,99],"comparison":"<=>"}]` on bodies. Search results name signals `Human`, `Biological` and so on.

**Enum strings** for station `type` and `services` are listed in §7.3, and the EDD mapping is in `SpanshClassConvertEnums.cs`. Older dumps include `"Asteroid base"` and newer data includes `"Dodec Starport"`, `"Space Construction Depot"`, `"Planetary Construction Depot"` and `"Dockable Planet Station"`.

### 4.8 Outfitting: stations that sell a whole build [TESTED 2026-11-02 and 2026-11-04]

What Elite Nexus's outfitting search (Fleet › Where to buy, `nexus-app/src/shopping.rs`) relies on (`nexus_spansh::OutfittingQuery`, with names from `nexus_data::outfitting::fdev_names` and `ships::Ship::fdev_name`):

- **Several `modules` items must all match** (AND). Within 25 ly of Sol: 6A FSD 130 stations, 7A shield generator 108, both in one array 56. A 15-module Anaconda-style list plus `"ships":[{"name":{"value":["Anaconda"]}}]` answers in about 1.2–1.7 s; the nearest station selling all 15 was 32.8 ly from Sol.
- **Within one item, several `name` values match any of them.** That matters because **MercGear rows share their base module's symbol, and Spansh lists the module under one name, mostly the MercGear one**: within 50 ly of Sol, `Power Distributor` 5A matched 113 stations, `Balanced Power Distributor` 5A 1,082, either name 1,195. Galileo lists `Int_PowerDistributor_Size5_Class5` only as "Balanced Power Distributor" (category `mercgear`), with no regular row. So send every FDevIDs name of the symbol (`nexus_data::outfitting::fdev_names`). The affected symbols are the 25 MercGear rows of FDevIDs `outfitting.csv` (power distributors, module reinforcements, cargo racks, seeker racks, fragment cannons, a medium fixed multi-cannon, a huge fixed beam laser and cannon, …).
- **Names, classes and ratings are FDevIDs' own**: over 1,190 distinct modules at Jameson Memorial, Galileo, Ray Gateway and Abraham Lincoln, Spansh's `name`/`class`/`rating` equalled FDevIDs `outfitting.csv` for every symbol it knew (spelling `Multi-Cannon`, not the app's display `Multi-cannon`; utilities class 0). Ship names equal FDevIDs `shipyard.csv` (`Krait MkII`, `Python MkII`).
- **The filter can't say the mount or whose bulkheads**: `Multi-Cannon` 4A matches `Hpt_MultiCannon_Fixed_Huge` and `_Gimbal_Huge`; every hull's `Military Grade Composite` is class 1 rating I. Every result carries its whole `modules[]` with `ed_symbol` (mixed case, e.g. `Anaconda_Armour_Grade3`) and `price` (sometimes missing), and `ships[]` with `symbol` and `price`, so check the exact symbols on the results.
- **`carrier_docking_access`** values (`/api/stations/field_values/carrier_docking_access`): `All`, `Friends`, `None`, `Squadron`, `Squadron Friends`; often empty. Many carriers with `has_outfitting` list no modules at all, or data years old.
- **Data age**: `outfitting_updated_at` on the nearest full matches was 7–12 months old (Understrand 2026-03-24, Kyriazi Survey 2025-11-04); big stations are refreshed daily. Show it.
- `"type":{"value":["Drake-Class Carrier"],"logic":"NOT"}` excludes carriers (a 15-module search went from 4,970 to 4,969 hits).
- The Anaconda Mk II (`AnacondaMkII`, ARX early access) was sold at 2,319 stations on 2026-11-04, all without a credit price.

---

## 5. Lookup and autocomplete endpoints

| Endpoint | Status | Response |
|---|---|---|
| `GET /api/systems/field_values/system_names?q=Colon` | TESTED | `{"min_max":[{"id64":3238296097059,"name":"Colonia","x":-9530.5,"y":-910.28125,"z":19808.125}],"values":["Colonia"]}`. **Use this for autocomplete**, because it also gives id64 and coordinates. The site debounces by 200 ms and cancels stale requests. |
| `GET /api/systems?q=Colon` | TESTED | `["Colonia"]`. Legacy, names only. Used by the site's planner forms and EDMC-SR (3+ chars). |
| `GET /api/stations?q=…` | SITE-JS (UNVERIFIED shape) | Station autocomplete for the trade planner. Items carry `name` and `system`. |
| `GET /api/search?q=Shinrarta%20Dezhra` | TESTED | `{"count":43,"query":"…","results":[{"type":"system"\|"body"\|"station","record":{…}}]}`. 10 results are returned; the records match the `/api/system`, `/api/body` and `/api/station` record shapes. |
| `GET /api/search/systems?q=` | EDD | `{"results":[{name,id64,x,y,z,…}]}`. EDD takes the first case-insensitive exact name match. |
| `GET /api/system/{id64}` | OPENAPI | `{"record":{id64,name,x,y,z,population,nearest_populated_distance,nearest_populated_id64,updated_at,region,government,allegiance,state,security,primary_economy,secondary_economy,controlling_minor_faction,controlling_minor_faction_state,is_colonised,is_being_colonised,controlling_power,power[],power_state,power_state_control_progress,power_state_reinforcement,power_state_undermining,power_conflict_progress[{name,progress}],needs_permit,known_permit,minor_faction_presences[{name,allegiance,government,state,influence,active_states,pending_states,recovering_states}],synthesis_recipes[{name,level}],body_count,bodies[{id,id64,name,type,subtype,terraforming_state,is_main_star,distance_to_arrival,estimated_scan_value,estimated_mapping_value,landmarks[{type,subtype,count,value}],landmark_value}],stations[{market_id,name,type,body_id64,body_name,distance_to_arrival,has_large_pad,has_market,has_shipyard,has_outfitting,is_dssa_carrier,large/medium/small_pads,services,controlling_minor_faction,controlling_minor_faction_state}],thargoid_war_*,estimated_scan_value,estimated_mapping_value,landmark_value}}`. Returns 404 `{"error"}` when unknown. |
| `GET /api/body/{id64}` | OPENAPI | `{"record":{…}}`, the same fields as a body search result. |
| `GET /api/station/{market_id}` | OPENAPI | `{"record":{…}}`, the same fields as a station search result (§4.5), including `material_trader` and `technology_broker`. |
| `GET /api/dump/{id64}` | TESTED | `{"system":{…}}`. **Exactly the per-system record of the galaxy dumps** (§7.3). Sol is 2.0 MB decoded. EDD uses this as its main body and station source and caches it on disk for 7 days. |
| `GET /api/nearest?x=-9530&y=-910&z=19808` | TESTED | `{"system":{"distance":0.587134194626748,"id64":3238296097059,"name":"Colonia","x":-9530.5,"y":-910.28125,"z":19808.125}}` |
| `GET /api/station/stock/{market_id}/{commodity}/{start}/{end}` and `GET /api/station/stock/field_values` | SITE-JS (UNVERIFIED) | Commodity price and stock history. `field_values` gives `{min,max}` dates. |
| `GET /api/engineers`, `/api/modules`, `/api/blueprints`, `/api/effects` | SITE-JS | `{"values":[…]}`, used by the engineer plotter |
| `GET https://api.orerve.net/2.0/website/initiatives/list?lang=en` | SITE-JS (Frontier, not Spansh) | Community Goals feed used by the Spansh CG page. Untested. |

---

## 6. Rate limits, etiquette, terms

- **No published rate limits, API terms or API keys** were found. The site, the OpenAPI document (`info` holds only title, description and version) and the response headers carry none, and no `RateLimit-*` or `Retry-After` header was seen. That absence is UNVERIFIED beyond what was checked.
- The site is donation-funded (https://www.patreon.com/spansh). The contact is `plotter@spansh.co.uk`, plus the `#spansh` channel on the Elite Dangerous community Discord linked from the dumps page.
- Etiquette Elite Nexus should follow:
  - Send a descriptive `User-Agent`, such as `EliteNexus/<ver> (+https://…)`.
  - Allow at most 1–2 concurrent requests per client.
  - Debounce autocomplete by 200 ms or more and cancel stale requests.
  - Cache lookups. EDD uses 7 days for dumps; for markets, prefer EDDN or the hourly dump.
  - Use exponential backoff on job polling (§2), and back off hard on 429 or 5xx.
  - Do not bulk-crawl the API. Use the dumps (§7) for bulk data.
  - Show attribution such as "Route data © Spansh" with a link.
- The site's own Google Analytics and Patreon link are irrelevant to API use.

---

## 7. Data dumps: `https://downloads.spansh.co.uk/`

The directory listing is **forbidden (403)**. The website's dumps page `HEAD`s a fixed list of files [SITE-JS `routes/dumps`]. The values below are from `HEAD` on 2026-10-12. Range requests are supported (`Accept-Ranges: bytes`, observed `206`). ETag and Last-Modified are present, so conditional and resumable downloads work.

### 7.1 Files

| File | Size (bytes) | ≈ | Last-Modified (UTC) | Content (site description) | Schema |
|---|---:|---:|---|---|---|
| `galaxy.json.gz` | 117,719,058,532 | 110 GiB | 2026-10-12 06:25:39 | The entire galaxy including bodies and stations | `galaxy.schema.json` |
| `galaxy_populated.json.gz` | 4,549,588,213 | 4.2 GiB | 2026-10-12 06:25:39 | Only systems with population > 0 | galaxy |
| `galaxy_stations.json.gz` | 4,518,820,279 | 4.2 GiB | 2026-10-12 **09:15:47** | Only systems with a station, including fleet carriers. "Updated at least every hour." | galaxy |
| `galaxy_1day.json.gz` | 1,468,893,108 | 1.4 GiB | 2026-10-12 06:25:39 | Systems where *any* information changed in the last 24 h | galaxy |
| `galaxy_7days.json.gz` | 2,997,388,745 | 2.8 GiB | 2026-10-12 06:25:39 | …changed in the last 7 days | galaxy |
| `galaxy_1month.json.gz` | 5,458,782,327 | 5.1 GiB | 2026-10-12 06:25:39 | …changed in the last 32 days | galaxy |
| `systems.json.gz` | 6,363,326,812 | 5.9 GiB | 2026-10-12 08:47:53 | Just system details, with no bodies or stations | `systems.schema.json` |
| `systems_neutron.json.gz` | 181,347,590 | 173 MiB | 2026-10-12 08:47:53 | Systems whose **main star is a neutron star**. This is enough for a local neutron plotter. | systems |
| `systems_1day.json.gz` | 4,031,578 | 3.8 MiB | 2026-10-12 08:47:53 | System details updated in the last 24 h | systems |
| `systems_1week.json.gz` | 24,401,795 | 23 MiB | 2026-10-12 08:47:53 | …last week | systems |
| `systems_2weeks.json.gz` | 46,283,735 | 44 MiB | 2026-10-12 08:47:53 | …last 2 weeks | systems |
| `systems_1month.json.gz` | 93,001,885 | 89 MiB | 2026-10-12 08:47:53 | …last month | systems |
| `systems_6months.json.gz` | 638,451,314 | 609 MiB | 2026-10-12 08:47:53 | …last 6 months | systems |
| `factions.json.gz` | 17,261,370 | 16 MiB | 2026-10-12 08:47:58 | Factions with every system they are present in, and whether they control it | `factions.schema.json` |

The legacy `galaxy_populated.json.bz2`, `galaxy_stations.json.bz2` and `galaxy.json.bz2` now return **404** (the translations say they are "no longer updated").

**Cadence**:

- The site calls them "Nightly dumps". The observed nightly times were ~06:25 UTC for `galaxy*` and ~08:48 UTC for `systems*` and `factions`.
- `galaxy_stations` is refreshed at least hourly.

**Schemas** (draft-04, which the site calls "early versions"):

- https://docs.spansh.co.uk/galaxy.schema.json
- https://docs.spansh.co.uk/systems.schema.json
- https://docs.spansh.co.uk/factions.schema.json

The source repo is https://github.com/spansh/elite_dangerous_schemas (branch `main`). Its README carries an **MIT licence** (© 2025 Gareth Harper). `package.json` is `elite_dangerous_schemas` 3.1.1 but declares `"license": "ISC"`. Both are permissive, so attribute either way [VERIFIED via raw files].

The repo also ships:

- `api.openapi.json`, which has the same 4 paths as the docs site. Its `info.version` reads 0.0.1, while the docs site serves 3.1.1.
- `model.openapi.json`, `dumps.openapi.json`, `galaxy.downloads.openapi.json`, `systems.downloads.openapi.json`, `factions.downloads.openapi.json`, `merged.downloads.openapi.json` and `galaxy.new.schema.json`.

The README links canonical copies at `https://docs.spansh.co.uk/{api,model}.openapi.json` and versioned copies at `https://docs.spansh.co.uk/[version]/[file]` (e.g. `/2.0.1/galaxy.schema.json`).

Versioning policy: a patch adds enum values only, a minor version adds fields, and a major version breaks. Because of the MIT licence, Elite Nexus can **vendor `model.openapi.json` and generate serde types from it** (for example with `typify`). The **OpenAPI `/dump/{id64}` schema is the most complete description** of the galaxy record.

**Decompression ratio**: the first 200 KB of `galaxy_populated` and `galaxy_stations` expanded about 6.7×. A rough estimate is ~30 GB uncompressed for the populated dump and roughly 0.8 TB for `galaxy.json` (estimate only).

### 7.2 File framing [TESTED on 4 files]

```
[
\t{…one complete JSON record…},
\t{…},
\t{…}
]
```

- Each record is on one line, prefixed by a tab, with a trailing comma on every line except the last.
- Stream the file line by line: strip whitespace, skip `[` and `]`, drop the trailing `,`, then call `serde_json::from_str`. There is no need for a streaming array parser.
- Lines can be very large. A populated system with full markets can reach several hundred KB; only 2 records fit in the first 1.37 MB of `galaxy_populated`.
- Use a buffered reader with a growable line buffer, and `flate2::read::MultiGzDecoder`. Whether the files are multi-member gzip is unknown, so the multi-member decoder is the safe choice.

### 7.3 Galaxy record (`galaxy*.json.gz` lines == `/api/dump/{id64}`.`system`) [TESTED + OPENAPI]

`*` marks a required field. The record below is the union of fields observed in the Sol dump and the galaxy dump heads.

```
id64*: u64                name*: str              coords*: {x,y,z: f64}
allegiance: str|null      government: str         primaryEconomy, secondaryEconomy: str
security: "Anarchy|High|Low|Medium"               population: u64          bodyCount: u32
date*: datetime (last system update)              timestamps: {attr -> datetime}  e.g. controllingPower, factions, powerState, powers
controllingFaction: {name*, allegiance, government, state?, influence?, activeStates[{state,trend?}], pendingStates[{state,trend}], recoveringStates[{state,trend}]}
factions: [{name, allegiance, government, influence (0..1), state, activeStates?, pendingStates?, recoveringStates?}]
controllingPower: str     powers: [str]           powerState: "Exploited|Fortified|Stronghold|Unoccupied|None"
powerStateControlProgress: f64   powerStateReinforcement: int   powerStateUndermining: int
powerConflictProgress: [{power, progress}]
thargoidWar: {currentState, successState, failureState, progress, daysRemaining, portsRemaining, successReached}
stations*: [Station]      (orbital, not body-bound)
bodies*: [Body]
```

**Body**:
```
id64*, bodyId*, name*, type ("Planet|Star|Barycentre"), subType, updateTime*, distanceToArrival (LS)
mainStar (bool, stars), age (Myr), spectralClass, luminosity, absoluteMagnitude, solarMasses, solarRadius (solar radii)
isLandable, gravity (g), earthMasses, radius (km), surfacePressure (atm), surfaceTemperature (K)
volcanismType, atmosphereType, atmosphereComposition {gas -> %}, solidComposition {Ice|Rock|Metal -> %}
terraformingState, materials {material -> %}, reserveLevel ("Pristine|Major|Common|Low|Depleted")
orbitalPeriod (days), semiMajorAxis (AU), orbitalEccentricity, orbitalInclination (deg), argOfPeriapsis (deg),
meanAnomaly (deg), ascendingNode (deg), axialTilt (rad), rotationalPeriod (days), rotationalPeriodTidallyLocked
parents: [{"Star":0}|{"Planet":3}|{"Null":31}]       (journal-style parent chain)
timestamps {distanceToArrival|meanAnomaly -> datetime}
signals: {signals {"$SAA_SignalType_Biological;":1, "$SAA_SignalType_Geological;":2, "$SAA_SignalType_Human;", "$SAA_SignalType_Other;", "$PlanetaryMiningLocation_Name;": n},
          genuses ["$Codex_Ent_Sphere_Name;", …], updateTime}
rings / belts: [{name*, type* ("Icy|Metal Rich|Metallic|Rocky"), mass* (Mt), innerRadius*, outerRadius* (metres), id64?,
                 signals {signals {"Benitoite":1, "Painite":2, …}, updateTime}}]
stations: [Station]       (planetary ports and settlements on this body)
```

**Units differ from the journal.** Verified on Sol:

| Field | Spansh unit | Journal unit |
|---|---|---|
| `semiMajorAxis` | AU (Earth 1.0000007) | m |
| `orbitalPeriod`, `rotationalPeriod` | days (Earth 365.256) | s |
| `radius` | km (Earth 6378.1) | m |
| `gravity` | g (Earth 0.999) | m/s² |
| `surfacePressure` | atm | Pa |
| ring radii | metres (Jupiter halo 9.2e7) | metres |

The schema text says "Kilometres" for distances and ring radii, but the values are LS and metres respectively.

**Signal keys differ by location**:

- Body signals keep the journal `$…;` identifiers.
- Ring signals use commodity display names, such as `"Benitoite"` and `"Painite"`. The search API shows `"Low Temperature Diamonds"`; the dump spelling for that one is UNVERIFIED.

**Station** (orbital or on a body):
```
name*, id* (marketId u64), updateTime*, type ("Coriolis Starport|Orbis Starport|Ocellus Starport|Outpost|Asteroid base|
  Mega ship|Drake-Class Carrier|Planetary Port|Planetary Outpost|Settlement|Surface Settlement|Dodec Starport|
  Space Construction Depot|Planetary Construction Depot|Dockable Planet Station"), state ("Construction|Damaged|
  DamagedHuman|UnderAttack|UnderRepairs"), realName (colonisation), carrierName, carrierDockingAccess (carriers),
controllingFaction, controllingFactionState|null, allegiance, government, primaryEconomy, secondaryEconomy,
economies {economy -> %}, distanceToArrival (LS), latitude, longitude (surface), landingPads {large, medium, small},
services: [str]   (42 values: "Market","Shipyard","Outfitting","Refuel","Repair","Restock","Black Market",
                   "Interstellar Factors Contact","Material Trader","Technology Broker","Universal Cartographics",
                   "Vista Genomics","Frontline Solutions","Pioneer Supplies","Apex Interstellar","Bartender",
                   "Redemption Office","Search and Rescue","Powerplay","System Colonisation","Construction Services",
                   "Fleet Carrier Fuel","Fleet Carrier Management","Fleet Carrier Administration","Fleet Carrier Vendor",
                   "Squadron Bank","Refinery Contact","Social Space","Crew Lounge","Livery","Shop","Tuning","Workshop",
                   "Missions","Missions Generated","On Dock Mission","Contacts","Dock","Autodock","Flight Controller",
                   "Station Menu","Station Operations")
market: {commodities [{name, symbol, category, commodityId, demand, supply, buyPrice, sellPrice}], prohibitedCommodities [str], updateTime}
shipyard: {ships [{name, symbol, shipId}], updateTime}
outfitting: {modules [{name, symbol, moduleId, class, rating, category ("hardpoint|internal|utility|standard|mercgear"), ship?}], updateTime}
```

**Where INARA-style features come from in this record**:

| Feature | Source |
|---|---|
| Material traders and technology brokers | `services[]` contains `"Material Trader"` / `"Technology Broker"`. The *type* (Raw/Encoded/Manufactured, Guardian/Human) is **not** in the dump. It is in the API station record (`material_trader`, `technology_broker`). Otherwise derive it from station and system economy rules, or from journal `MaterialTrade` events (UNVERIFIED rule set). |
| Interstellar Factors | `"Interstellar Factors Contact"` in `services[]` |
| Commodity search | `market.commodities[]` |
| Ship and module search | `shipyard.ships[]` / `outfitting.modules[]` |
| BGS | `factions[]`, `controllingFaction` |
| Powerplay | `powers`, `controllingPower`, `powerState`, `powerState*`, `powerConflictProgress` |

**Quirks seen**:

- Fleet-carrier stations can lack `controllingFaction`, `economies` and `landingPads`.
- `allegiance` can be `null`.
- Some body stations have no `type`. EDD treats a missing type with a latitude as "Settlement".
- `controllingFactionState` can be `null`.

### 7.4 `systems*.json.gz` record [TESTED + schema]

```json
{"id64":1802559,"name":"Muena AA-A h0","mainStar":"Wolf-Rayet NC Star",
 "coords":{"x":-15399.875,"y":600.125,"z":26977.4375},"updateTime":"2026-10-11T15:51:43Z"}
```

- `mainStar` is optional, and is absent for about 0.7% of records.
- `needsPermit` (bool) is optional and rare.
- `mainStar` can also be a planet subtype, per the schema enum.

### 7.5 `factions.json.gz` record [TESTED + schema]

```json
{"name":"\"Fearless\" Bounty Hunters Corp.","allegiance":"Independent","government":"Corporate",
 "systems":[{"systemName":"HIP 104256","systemId64":2175157635443,"isControllingFaction":true,"coords":{"x":45.875,"y":-115.90625,"z":123.59375}}, …]}
```

**Quirk**: every *controlled* system appears **twice** in `systems[]`, once with `isControllingFaction:true` and once without it. This held for 1005 of 1005 cases across 584 factions. Deduplicate by `systemId64` and OR the flag together.

The dump has no influence, state or power data; those are in the galaxy record's `factions[]`.

### 7.6 Delta strategy

There are **no true diff files**. The `*_1day`, `*_7days`/`*_1week` and `*_1month` files are full records of systems touched in the window. A suggested local mirror:

1. **Bootstrap**: download `galaxy_stations.json.gz` (markets, outfitting, shipyards, factions and powers for every system with a station, 4.2 GiB). Add `systems.json.gz` (5.9 GiB) or `systems_neutron` for routing and coordinates only.
2. **Daily**: download `galaxy_1day.json.gz` (1.4 GiB) and **upsert whole system records by `id64`**, replacing child arrays. This also handles removed stations and departed carriers, because any system whose station list changed is itself "updated". That point is an inference, not documented.
3. **Hourly**: optionally re-fetch `galaxy_stations` with `If-None-Match`, or use EDDN for live market and outfitting.
4. **Missed days**: use `galaxy_7days`/`galaxy_1month`, or `systems_1week`/`2weeks`/`1month`/`6months` for system-only tables.
5. Keep the `Last-Modified` and `ETag` per file. Resume downloads with `Range: bytes=<n>-` (206 is supported).

---

## 8. Rust implementation notes

- **HTTP**: `reqwest` (rustls, gzip). Set `User-Agent`. Use one `SpanshClient` with a semaphore of 2.
- **Forms**: `serde_urlencoded` cannot repeat keys from a `Vec` directly. Build the body as `Vec<(&str, String)>`, with one tuple per array element, and pass it to `.form(&pairs)`.
- **IDs**: `type Id64 = u64; type MarketId = u64;`. Accept both a string and a number for `id` in search results with a custom deserializer, or ignore `id` and use `market_id`/`id64`.
- **Timestamps**: parse both `…Z` and the offset-less job timestamps as UTC (`chrono::NaiveDateTime` → `Utc`).
- **Data model**: keep Spansh camelCase dump models separate from snake_case search and record models; they differ in naming and nesting.
- **Dumps**: use `flate2::read::MultiGzDecoder` + `BufReader::with_capacity(1<<20)` + `read_line` + `serde_json::from_str::<DumpSystem>`. Use `#[serde(default)]` everywhere, because nearly every field is optional. Ingest into SQLite with `PRAGMA journal_mode=WAL` and batched transactions.
- **Optional data**: treat everything as optional except `id64`, `name` and `coords`.

## 9. Samples (`docs/research/spansh-samples/`)

| File | What |
|---|---|
| `neutron_route.submit.response.json` | `POST /api/route` → 202 body |
| `neutron_route_sol_colonia.result.json` | Full `GET /api/results/{job}` (Sol→Colonia, 50 LY, eff 60) |
| `stations_search_tritium_near_sol.request.json` / `.response.trimmed.json` | Stations search (arrays truncated to 4 items) |
| `bodies_search_painite_rings.request.json` / `.response.json` | Bodies search, ring hotspots (complete) |
| `systems_field_values_system_names_q_Colon.json` | Autocomplete with id64 and coordinates |
| `systems_q_Colon.legacy_autocomplete.json` | Legacy autocomplete |
| `smart_search_q_shinrarta.response.trimmed.json` | `/api/search` (arrays truncated to 3) |
| `nearest_colonia_coords.json` | `/api/nearest` |
| `dump_sol.response.trimmed.json` | `/api/dump/10477373803` (arrays truncated to 3) |
| `searchable_fields_{systems,bodies,stations}.json` | Full field catalogues |
| `galaxy_populated.first_record.trimmed.json`, `galaxy_stations.first_record.trimmed.json` | First record of each dump (arrays truncated to 3) |
| `systems_1day.first_records.json`, `factions.first_record.trimmed.json` | Dump record examples |

## 10. Sources

**Spansh**:

- Front-end (Ember app): https://spansh.co.uk/, bundle `https://spansh.co.uk/assets/elite-dangerous-gui-4ce93861bd384bc982212db7e24f463c.js`. The modules used were `services/api`, `controllers/*`, `routes/base/*`, `objects/ship` and `components/date-input-field`. Translations (help texts) are in `https://spansh.co.uk/assets/vendor-e211a712d23dc02baf30b3b446e25890.js`. The hashed file names change on every deploy.
- OpenAPI: https://docs.spansh.co.uk/ (spec embedded as `window.apiDocs`). Dump schemas: https://docs.spansh.co.uk/galaxy.schema.json, …/systems.schema.json, …/factions.schema.json. Schema repo (MIT): https://github.com/spansh/elite_dangerous_schemas (README, `package.json` and `api.openapi.json` read via raw.githubusercontent.com).
- Dumps: https://downloads.spansh.co.uk/<file> (HEAD and ranged GET).

**Third-party clients**:

- EDDiscovery core, Apache-2.0: https://github.com/EDDiscovery/EliteDangerousCore/tree/master/EliteDangerous/3rdPartyInterfaces/Spansh. Files used: `SpanshClass.cs`, `SpanshClassRouters.cs`, `SpanshClassStation.cs`, `SpanshClassSystems.cs`, `SpanshClassDump.cs`, `SpanshClassConvertToStationInfo.cs`, `SpanshClassConvertEnums.cs`. The SLEF builder is in `EliteDangerous/History/Ships/Ship.cs`.
- EDMC_SpanshRouter (archived 2023, no licence file): https://github.com/CMDR-Kiel42/EDMC_SpanshRouter (`SpanshRouter/SpanshRouter.py`, `AutoCompleter.py`).
- EDMC core Spansh link provider: https://github.com/EDCD/EDMarketConnector/blob/main/plugins/spansh_core.py.
- EDCoPilot docs were not consulted.
