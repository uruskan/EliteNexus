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
| Pre-engineered modules (V1 drives, Guardian and Merc gear) to pick | missing |
| A table of every hull to compare (speed, shields, armour, mass, slots, price) | missing |

### Engineering

| Feature | Status |
|---|---|
| Engineering already on your ship, with the game's own values | done |
| A blueprint and grade to plan per module, and for the bulkheads | done |
| Planned engineering changes the figures | done |
| How far a grade is rolled (any percentage, 100 % for a full roll) | done |
| Experimental effects change the figures (damage types too) | done |
| Materials the plan needs, against what you hold | done |
| Engineers who offer each grade | missing |
| Typing an exact value for one engineered figure | missing |

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
| Damage against a target build's shields and resistances (Coriolis' opponent) | missing |

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
| Neutron and white dwarf supercharge | missing |

### Mass, cargo, fuel, cost

| Feature | Status |
|---|---|
| Hull, unladen and laden mass, cargo, passengers, fuel | done |
| Hull and module prices, rebuy | done |
| Discounts | missing |
| Ammunition restock cost | done |
| What to buy against your ship or a new hull | done (beyond both tools: Where to buy finds the stations) |

### Builds

| Feature | Status |
|---|---|
| Start from a new hull, one of your ships or a pasted build | done |
| Save builds, open, rename and delete them | missing |
| Compare two builds | missing |
| Import SLEF, EDSY and Coriolis import links, the game's Loadout | done |
| Export SLEF, EDSY and Coriolis links, the game's Loadout | partial: SLEF export missing |
| Undo and start over | done |

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
