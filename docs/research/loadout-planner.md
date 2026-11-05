# Loadout planner: what it does next to EDSY and Coriolis

Fleet › Plan a loadout aims to do everything EDSY and Coriolis do for a build, behind a simpler page. This is the
checklist of what those tools offer and where Elite Nexus stands on each. Status: **done**, **partial** (what's
missing is said), **missing**, or **not possible** (and why). Keep it current when the planner changes.

The sums and where they come from are under [Formulas](#formulas).

## Checklist

### Hulls and modules

| Feature | Status |
|---|---|
| Every hull with its slots, bulkheads and stock modules | done |
| Each slot lists only what the game lets you fit there (size, exact-size core modules, military and named slots, hull-only modules, fighter hangars and luxury cabins, mass limits, one per ship) | done |
| Search and filter the module list | done |
| Pre-engineered modules (V1 drives, Guardian and Merc gear) to pick | not possible from this data: coriolis-data lists most of them with the plain module's figures and only the names of their blueprints (a V1 drive has a 5A drive's optimal mass), and the rest inconsistently. One on your ship comes in with the game's own values; to plan one, take the plain module and type the real module's values in Its figures |
| A table of every hull to compare (speed, shields, armour, mass, slots, price) | done (the start screen's Every hull) |

### Engineering

| Feature | Status |
|---|---|
| Engineering already on your ship, with the game's own values | done |
| A blueprint and grade to plan per module, and for the bulkheads | done |
| Planned engineering changes the figures | done |
| How far a grade is rolled (any percentage, 100 % for a full roll) | done |
| Experimental effects change the figures (damage types too) | done |
| Materials the plan needs, against what you hold | done |
| Engineers who offer each grade | done (the ones you've unlocked first, with how far you are with them) |
| Typing an exact value for one engineered figure | done (Its figures: each changed figure, as rolled, to type over; kept in saved builds and in what's shared) |

### Power

| Feature | Status |
|---|---|
| Power made against power drawn, retracted and deployed | done |
| Priorities 1 to 5 per module, to change (the cargo hatch's too) | done |
| Modules switched on and off | done |
| Power by priority group: what switches off first | done |

### Heat

| Feature | Status |
|---|---|
| Heat capacity | done |
| Power plant heat (draw × heat efficiency) | done |
| Weapons' heat while firing, per weapon and in all | done |
| Shield cell banks' heat | done |
| Heat sinks carried | done |
| Heat level over time, time to overheat | not possible: how fast each hull sheds heat isn't in coriolis-data (EDSY has it, under a licence that doesn't allow bundling) |

### Weapons

| Feature | Status |
|---|---|
| Raw and sustained DPS per weapon and in all | done |
| Damage types (kinetic, thermal, explosive, absolute, caustic) | done |
| Distributor draw, time to drain the weapons capacitor by pips | done |
| Ammo: clip, reserve, reload, time to empty | done |
| Range and falloff | done |
| Armour piercing against a target's hardness | done |
| Damage against a target build's shields and resistances (Coriolis' opponent) | done (a hull as sold, a saved build or one of yours: seconds to break its shields at two systems pips, then its hull through hardness and resistances) |

### Shields and armour

| Feature | Status |
|---|---|
| Shield strength: generator, boosters, Guardian reinforcement, shield cells | done |
| Shield resistances with diminishing returns | done |
| Shields against each damage type (effective MJ) | done |
| Systems pips: their resistance, recharge and recovery times | done |
| Regeneration and broken regeneration | done |
| Armour with bulkheads and hull reinforcements, resistances stacked and capped | done |
| Armour against each damage type | done |
| Module protection and module reinforcement integrity | done |
| Hardness | done |

### Movement

| Feature | Status |
|---|---|
| Top speed and boost | done |
| A boost a power distributor can't give | done |
| Speed by engine pips | done |
| Boost interval | done |
| Pitch, yaw and roll | done |
| Mass lock factor, crew seats | done |

### Jump

| Feature | Status |
|---|---|
| The game's own maximum range | done (matches the game) |
| Unladen with a full tank, laden, one tank's total | done |
| Fuel per jump | done |
| Range with the cargo and fuel you choose | done |
| Guardian FSD booster | done |
| Neutron and white dwarf supercharge | done (×4, ×6 for the FSD Mk II, and ×1.5 of the drive's range with what's carried; a Guardian booster's light years added after. Whether the game boosts the booster's share too isn't settled: the lower reading is shown, and said) |
| FSD injection (synthesis) | done (+25 %, +50 %, +100 %) |

### Mass, cargo, fuel, cost

| Feature | Status |
|---|---|
| Hull, unladen and laden mass, cargo, passengers, fuel | done |
| Hull and module prices, rebuy | done |
| Discounts | done (one discount on the hull and modules, which the rebuy follows) |
| Ammunition restock cost | done |
| What to buy against your ship or a new hull | done (beyond both tools: Where to buy finds the stations) |

### Builds

| Feature | Status |
|---|---|
| Start from a new hull, one of your ships, a saved build or a pasted build | done |
| Save builds, open, rename and delete them | done (`builds.json` in the data folder, planned engineering kept) |
| Compare two builds | done (the plan beside your ship, the stock hull, a saved build or another ship of yours) |
| Import SLEF, EDSY and Coriolis import links, the game's Loadout | done |
| Export SLEF, EDSY and Coriolis links, the game's Loadout | done (with the engineering on the modules) |
| Planned engineering in what's exported | done (Share writes it as the game writes engineering: each changed figure with its label, value and original value; EDSY was checked to read the labels and units) |
| Undo and start over | done |

## Checked against the game

What a journal records for a ship (its `Loadout`), against the planner's sums for the same build: a commander's
unengineered Kestrel Mk II, 2026-11-04.

| Figure | The game | The planner |
|---|---|---|
| Unladen mass | 323.1 t | 323.1 t (a test) |
| Max jump range | 19.642 ly | 19.642 ly (a test) |
| Fuel, cargo | 16 t, 4 t | 16 t, 4 t (a test) |
| Modules' value | 18,369,465 Cr | 18,310,005 Cr at list prices (0.3 % less) |
| Hull's value | 17,128,581 Cr | 13,776,380 Cr, coriolis-data's list price: the journals have no purchase of this ship, and the game values a hull by what was paid for it. The Overview shows the game's value beside the sum while the plan is the ship as it is |
| Rebuy | 1,774,903 Cr: 5 % of hull and modules, rounded up | the same formula |

Speed, shields, armour, power and weapons have no figure in the journal to check against; engineering can't be
checked yet, as the commander's journals have no engineered module. Planned engineering was checked by hand against
the blueprints' ranges (tests in `fitting/blueprints.rs`) and, for how it's written out, against what EDSY reads
(the labels, units and the planned mark import with no error; a figure the export doesn't carry, EDSY works out
itself).

The same Kestrel opened in Coriolis (2026-11-05) comes to the same top speed, speed at two pips and boost, masses,
shields and armour against each damage type, shield recovery (113 s) and recharge (238 s), power retracted and
deployed, each weapon's damage and draw, cost and insurance. Its jump range is 0.03 ly lower than the game's (19.61
against 19.64 ly; it counts the reserve tank). It drains the weapons capacitor at the weapons' draw while firing
(6 s), where this planner first used the draw over time with reloads (7 s): a capacitor empties before a clip does,
so the planner now drains it at the draw while firing too. Its boost interval (6 s) is at four engine pips; the
planner's is at the pips set (13.7 s at two).

## Formulas

Data: EDCD coriolis-data (`data/coriolis`, see `SOURCE.txt`): hulls, slots, module figures, blueprints' feature
ranges per grade, how each figure changes (multiplied, added or replaced), and experimental effects.

* **Unladen mass** = hull + bulkheads + modules. **Max jump range** = (min(max fuel per jump, fuel) / fuel
  multiplier)^(1 / fuel power) × optimal mass / (unladen mass + one jump's fuel) + a Guardian FSD booster's range.
  The game's own figure leaves the reserve tank out: an unengineered Kestrel Mk II comes to its journal's
  UnladenMass 323.1 t and MaxJumpRange 19.64237 ly (a test).
* **Mass curve** (thrusters, shield generators): x = (max mass − mass) / (max mass − min mass), clamped to 0..1;
  exponent = ln((opt mul − min mul) / (max mul − min mul)) / ln((max mass − opt mass) / (max mass − min mass));
  multiplier = min mul + x^exponent × (max mul − min mul). Engineering that moves the optimal mass moves the minimum
  and maximum with it; engineering that moves the optimal multiplier moves all three multipliers.
* **Planned engineering** (`fitting/blueprints.rs`): each figure a grade changes runs from the start of its range to
  the end by the roll (a range of drawbacks runs the other way); a resistance gains that share of what's left of the
  module's own (1 − resistance); the experimental effect then multiplies, adds or replaces on top (its resistances in
  percent of what's left). Multiplied figures are base × (1 + change); shield and hull boosts (1 + base) × (1 + change)
  − 1; the rate of fire changes as its interval does (rate / (1 + change)).
* **Pips**: systems pips stop pips^0.85 × 60 % / 4^0.85 of shield damage (a third at two, 60 % at four); a capacitor
  refills at its full rate × (pips / 4)^1.1. Speed at a pip setting: top speed × (minimum thrust + (1 − minimum
  thrust) × pips / 4); turn rates lose the hull's per-pip share for each pip short of four.
* **Shields**: generator strength × (1 + boosters) + Guardian reinforcement; resistances with diminishing returns
  past 30 % below the generator's own damage. Regeneration draws 0.6 MW from the systems capacitor for each MJ/s: the
  capacitor carries it while it lasts, then the systems pips' refill does. Coming back from broken waits 16 seconds.
* **Weapons**: shots a second = burst / ((burst − 1) / burst rate + interval + charge); damage a second = damage ×
  rounds a shot × shots a second; with a clip, sustained = clip / ((clip − burst) / rate + (burst − 1) / burst rate
  + reload) shots a second (Auto Loader: a round back for every two). Draw and heat a second scale the same way. A
  hull's hardness passes piercing / hardness of the damage when the piercing is lower.
* **Power groups**: each group counts what it and the groups before it draw, retracted and deployed; the highest
  groups switch off first when the ship draws more than its power plant makes.
* **Heat**: the power plant makes power drawn × its heat efficiency a second; weapons their heat a shot × shots a
  second.
* **Against a target** (`combat::against`): on shields, each weapon's damage over time × Σ (its share of each type ×
  (1 − the shields' resistance to it)) × (1 − the target's systems pips' share, a third at two pips); on the hull,
  × its hardness share × Σ (share × (1 − the hull's resistance)). Absolute damage passes every resistance. Seconds =
  shields ÷ shield damage a second, then armour ÷ hull damage a second; no regeneration, no shield cells.
* **One jump boosted**: a neutron star's cone multiplies the drive's jump range by 4 (6 for the Caspian's FSD Mk II, as
  `nexus_spansh::ship` has it; its white dwarf factor isn't known), a white dwarf's by 1.5; FSD injection
  adds 25, 50 or 100 %. Each is shown for the range with what's carried, with a Guardian FSD booster's light years
  added after the multiplier: whether the game multiplies them too couldn't be settled from a source, so the lower
  reading is shown and the page says so.
* **Prices**: a discount takes its share off the hull with its bulkheads and every module; the rebuy is 5 % of what
  that comes to. Ammunition restock stays at list price.
