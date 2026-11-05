//! What a build does in a fight and in flight: pips and the power distributor's capacitors, each weapon's damage,
//! draw, heat and ammunition, power priority groups, and heat.
//!
//! The pips' curves are the game's: systems pips harden the shields by pips^0.85 × 60 % / 4^0.85 (a third at two
//! pips, 60 % at four), and a capacitor refills at its full rate × (pips / 4)^1.1. Shields regenerate from the systems
//! capacitor: 0.6 MW of it for each MJ a second, so a drained capacitor slows them to what the pips put back.

use super::{DamageDist, Live};

/// Pips to systems, engines and weapons, 0 to 4 each in half pips: the game's six pips in all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pips {
    pub sys: f64,
    pub eng: f64,
    pub wep: f64,
}

impl Default for Pips {
    fn default() -> Self {
        Pips { sys: 2.0, eng: 2.0, wep: 2.0 }
    }
}

impl Pips {
    /// A press of one of the game's pip buttons (0 systems, 1 engines, 2 weapons): a whole pip goes there, half a pip
    /// from each of the others, or all of it from one when the other has none, up to four.
    pub fn press(&mut self, which: usize) {
        let mut halves = [self.sys, self.eng, self.wep].map(|p| (p * 2.0).round().clamp(0.0, 8.0) as u8);
        let Some(&mine) = halves.get(which) else { return };
        let want = (8 - mine).min(2);
        let mut taken = 0;
        let others = [(which + 1) % 3, (which + 2) % 3];
        for o in others {
            if taken < want && halves[o] > 0 {
                halves[o] -= 1;
                taken += 1;
            }
        }
        for o in others {
            while taken < want && halves[o] > 0 {
                halves[o] -= 1;
                taken += 1;
            }
        }
        halves[which] += taken;
        [self.sys, self.eng, self.wep] = halves.map(|h| f64::from(h) / 2.0);
    }

    /// The share of shield damage the systems pips stop, on top of the resistances.
    pub fn sys_resistance(&self) -> f64 {
        self.sys.clamp(0.0, 4.0).powf(0.85) * 0.6 / 4f64.powf(0.85)
    }

    /// How fast a capacitor refills at `pips`, as a share of its rate at four.
    pub fn recharge_share(pips: f64) -> f64 {
        (pips.clamp(0.0, 4.0) / 4.0).powf(1.1)
    }
}

/// What the commander sets: pips and what's in the hold and the tank while flying, and what a station takes off its
/// prices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    pub pips: Pips,
    /// Cargo carried, t.
    pub cargo: f64,
    /// Fuel carried, t; None: a full tank.
    pub fuel: Option<f64>,
    /// A station's discount on its list prices, 0 to 1 (0.15: 15 % off): the hull's and modules' prices come down by
    /// it, and with them the rebuy.
    pub discount: f64,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { pips: Pips::default(), cargo: 0.0, fuel: None, discount: 0.0 }
    }
}

/// One of the power distributor's capacitors, and what draws on it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Capacitor {
    /// MJ.
    pub capacity: f64,
    /// MJ a second at the pips set.
    pub recharge: f64,
    /// MJ a second drawn when everything on it runs.
    pub drain: f64,
    /// Seconds until it's empty; None: it keeps up.
    pub time_to_empty: Option<f64>,
}

/// A capacitor of `capacity` refilling at `recharge` while `drain` comes off it.
pub(super) fn capacitor(capacity: f64, recharge: f64, drain: f64) -> Capacitor {
    let net = drain - recharge;
    Capacitor { capacity, recharge, drain, time_to_empty: (net > 1e-9).then(|| capacity / net) }
}

/// A weapon's figures, as fitted (engineering included).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Weapon {
    pub slot: String,
    pub item: String,
    /// Damage a shot (every round of a fragment cannon's shot), or a second for a beam.
    pub damage: f64,
    pub dist: DamageDist,
    /// Shots a second, bursts and charging counted; a beam fires all the time (1).
    pub rate: f64,
    /// Damage a second while firing, and over time with reloads.
    pub dps: f64,
    pub sustained_dps: f64,
    /// MJ a second from the weapons capacitor while firing, and over time.
    pub eps: f64,
    pub sustained_eps: f64,
    /// Heat a second while firing.
    pub hps: f64,
    /// Metres: how far it reaches, and from where its damage falls off.
    pub range: f64,
    pub falloff: f64,
    pub piercing: f64,
    /// Rounds in the clip and in reserve, and the seconds a reload takes; 0: it needs none.
    pub clip: f64,
    pub ammo: f64,
    pub reload: f64,
    /// Seconds of fire until clip and reserve are empty; None: it doesn't run out.
    pub time_to_empty: Option<f64>,
}

impl Weapon {
    /// The share of its damage that gets through armour of `hardness`: all of it when its piercing is as high.
    pub fn against_hardness(&self, hardness: f64) -> f64 {
        if hardness <= 0.0 || self.piercing >= hardness { 1.0 } else { (self.piercing / hardness).max(0.0) }
    }

    /// The share of its damage left at `distance` metres: all of it to the falloff, none at its range.
    pub fn at_distance(&self, distance: f64) -> f64 {
        if self.falloff <= 0.0 || distance <= self.falloff || self.range <= self.falloff {
            return if self.range > 0.0 && distance > self.range { 0.0 } else { 1.0 };
        }
        (1.0 - (distance - self.falloff) / (self.range - self.falloff)).clamp(0.0, 1.0)
    }
}

/// A hardpoint's weapon figures, engineering included; None for what isn't a weapon (point defence shoots down
/// missiles, not ships).
pub fn weapon(l: &Live) -> Option<Weapon> {
    let damage = l.get("damage");
    if damage <= 0.0 || l.m.grp == "po" {
        return None;
    }
    let rounds = l.m.base("roundspershot").filter(|r| *r >= 1.0).unwrap_or(1.0);
    let burst = l.opt("burst").filter(|b| *b >= 1.0).unwrap_or(1.0);
    let burst_rate = l.opt("burstrof").filter(|b| *b > 0.0).unwrap_or(1.0);
    let charge = l.opt("charge").filter(|c| *c > 0.0).unwrap_or(0.0);
    // A beam has no interval between shots: its damage and draw are a second's.
    let rate = match l.opt("rof").filter(|r| *r > 0.0) {
        Some(rof) => burst / ((burst - 1.0) / burst_rate + 1.0 / rof + charge),
        None => 1.0,
    };
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    let clip = l.opt("clip").filter(|c| *c > 0.0).map(f64::ceil).unwrap_or(0.0);
    let ammo = l.get("ammo").max(0.0);
    let reload = l.get("reload").max(0.0);
    // With a clip, a reload breaks the fire every clip; Auto Loader reloads a round for every two fired.
    let sustained = if clip > 0.0 {
        let clip = if l.experimental().is_some_and(|x| x.eq_ignore_ascii_case("special_auto_loader")) { clip * 2.0 - 1.0 } else { clip };
        let per_clip = (clip - burst).max(0.0) / rate + (burst - 1.0) / burst_rate + reload;
        if per_clip > 0.0 { (clip / per_clip / rate).min(1.0) } else { 1.0 }
    } else {
        1.0
    };
    let dps = damage * rounds * rate;
    let eps = l.get("distdraw") * rate;
    let range = l.get("range");
    let falloff = l.opt("falloff").map_or(range, |f| if range > 0.0 { f.min(range) } else { f });
    // A clip with no reserve (an abrasion blaster's) reloads without end.
    let shots = if ammo > 0.0 { clip + ammo } else { 0.0 };
    Some(Weapon {
        slot: l.f.slot.clone(),
        item: l.f.item.clone(),
        damage: damage * rounds,
        dist: l.damage(),
        rate,
        dps,
        sustained_dps: dps * sustained,
        eps,
        sustained_eps: eps * sustained,
        hps: l.get("thermload") * rate,
        range,
        falloff,
        piercing: l.get("piercing"),
        clip,
        ammo,
        reload,
        time_to_empty: (shots > 0.0).then(|| shots / (rate * sustained)),
    })
}

/// Heat: what the ship holds and what makes it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Heat {
    /// What the hull holds before it overheats.
    pub capacity: f64,
    /// The power plant's heat a second: the power drawn × its heat efficiency, retracted and deployed.
    pub plant_retracted: f64,
    pub plant_deployed: f64,
    /// Every weapon firing, a second.
    pub weapons: f64,
    /// One shield cell's heat, from the hottest bank.
    pub cell: f64,
    /// Heat sinks carried, every launcher's clip and reserve.
    pub sinks: u32,
}

/// A power priority group: what it and the groups before it draw, MW.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PowerGroup {
    pub retracted: f64,
    pub deployed: f64,
}

/// What a ship's weapons do to another ship: damage a second on its shields (through their resistances and its
/// systems pips' share) and on its hull (through its hardness and resistances), and the seconds each takes. Every weapon
/// firing and hitting at full damage, over time with reloads; the shields don't regenerate meanwhile.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Against {
    pub shield_dps: f64,
    pub hull_dps: f64,
    /// Seconds to break the shields (None: it has none, or nothing gets through), then to destroy the hull.
    pub shield_secs: Option<f64>,
    pub hull_secs: Option<f64>,
}

/// `weapons` against a ship of `shield` (None: no shields), `armour` hull points, `hull_res` and `hardness`.
pub fn against(weapons: &[Weapon], shield: Option<&super::Shield>, armour: f64, hull_res: &super::Resistances, hardness: f64) -> Against {
    let through = |d: &DamageDist, r: &super::Resistances, caustic: bool| d.kinetic * (1.0 - r.kinetic) + d.thermal * (1.0 - r.thermal) + d.explosive * (1.0 - r.explosive) + d.absolute + d.caustic * if caustic { 1.0 - r.caustic } else { 1.0 };
    let shield_dps = shield.map_or(0.0, |s| weapons.iter().map(|w| w.sustained_dps * through(&w.dist, &s.res, false)).sum::<f64>() * (1.0 - s.sys_res));
    let hull_dps: f64 = weapons.iter().map(|w| w.sustained_dps * w.against_hardness(hardness) * through(&w.dist, hull_res, true)).sum();
    let secs = |amount: f64, dps: f64| (dps > 1e-9 && amount.is_finite()).then(|| amount.max(0.0) / dps);
    Against { shield_dps, hull_dps, shield_secs: shield.and_then(|s| secs(s.strength, shield_dps)), hull_secs: secs(armour, hull_dps) }
}

/// Seconds to put back `amount` MJ of shield at `rate` MJ/s while each MJ/s takes `draw` MW from a systems capacitor of
/// `capacity` MJ refilling at `recharge`: from the capacitor while it lasts, then as fast as the pips refill it. None:
/// never (no regeneration, or no pips to refill a drained capacitor).
pub(super) fn regen_time(amount: f64, rate: f64, draw: f64, capacity: f64, recharge: f64) -> Option<f64> {
    if amount <= 0.0 {
        return Some(0.0);
    }
    if rate <= 0.0 {
        return None;
    }
    let drain = rate * draw - recharge;
    if drain <= 0.0 {
        return Some(amount / rate);
    }
    let lasts = capacity / drain;
    if amount <= lasts * rate {
        return Some(amount / rate);
    }
    if recharge <= 0.0 || draw <= 0.0 {
        return None;
    }
    Some(lasts + (amount - lasts * rate) / (recharge / draw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pips_move_as_the_game_moves_them() {
        let mut p = Pips::default();
        p.press(0);
        assert_eq!((p.sys, p.eng, p.wep), (3.0, 1.5, 1.5));
        p.press(0);
        p.press(0);
        assert_eq!((p.sys, p.eng, p.wep), (4.0, 1.0, 1.0), "four is the most");
        p.press(1);
        p.press(1);
        assert_eq!((p.sys, p.eng, p.wep), (3.0, 3.0, 0.0));
        p.press(1);
        assert_eq!((p.sys, p.eng, p.wep), (2.0, 4.0, 0.0), "with one empty, the whole pip comes from the other");
        assert!((p.sys + p.eng + p.wep - 6.0).abs() < 1e-9);
        // Four pips harden the shields 60 %, none not at all; capacitors refill fully at four.
        assert!((Pips { sys: 4.0, eng: 1.0, wep: 1.0 }.sys_resistance() - 0.6).abs() < 1e-9);
        assert_eq!(Pips { sys: 0.0, eng: 3.0, wep: 3.0 }.sys_resistance(), 0.0);
        assert!((Pips::recharge_share(4.0) - 1.0).abs() < 1e-12 && Pips::recharge_share(0.0) == 0.0);
    }

    #[test]
    fn shields_come_back_from_the_capacitor_then_the_pips() {
        // Plenty of capacitor: at the regeneration rate.
        assert_eq!(regen_time(100.0, 2.0, 0.6, 1000.0, 1.0), Some(50.0));
        // The pips keep up with the draw: at the regeneration rate too.
        assert_eq!(regen_time(100.0, 1.0, 0.6, 0.0, 1.0), Some(100.0));
        // A small capacitor runs dry: slower after that, and never with no pips to refill it.
        let slow = regen_time(100.0, 2.0, 0.6, 6.0, 0.6).expect("time");
        assert!(slow > 50.0, "{slow}");
        assert_eq!(regen_time(100.0, 2.0, 0.6, 6.0, 0.0), None);
        assert_eq!(regen_time(0.0, 0.0, 0.6, 6.0, 0.0), Some(0.0));
    }

    #[test]
    fn a_capacitor_lasts_while_the_pips_keep_up() {
        assert_eq!(capacitor(40.0, 4.0, 3.0).time_to_empty, None);
        assert_eq!(capacitor(40.0, 2.0, 6.0).time_to_empty, Some(10.0));
    }

    #[test]
    fn damage_falls_off_with_distance_and_hardness() {
        let w = Weapon { range: 3000.0, falloff: 500.0, piercing: 50.0, ..Weapon::default() };
        assert_eq!(w.at_distance(400.0), 1.0);
        assert!((w.at_distance(1750.0) - 0.5).abs() < 1e-9);
        assert_eq!(w.at_distance(3500.0), 0.0);
        assert_eq!(w.against_hardness(40.0), 1.0);
        assert!((w.against_hardness(100.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn weapons_against_a_target() {
        use super::super::{Resistances, Shield};
        // 10 DPS, half thermal half kinetic, piercing 50, against 100 MJ of shields that stop 50 % of thermal and none
        // of kinetic with a third more for two systems pips, and 200 hull points of hardness 100 that stop 20 % kinetic.
        let w = Weapon { sustained_dps: 10.0, dist: DamageDist { kinetic: 0.5, thermal: 0.5, ..DamageDist::default() }, piercing: 50.0, ..Weapon::default() };
        let shield = Shield { strength: 100.0, res: Resistances { thermal: 0.5, ..Resistances::default() }, sys_res: 1.0 / 3.0, ..Shield::default() };
        let hull = Resistances { kinetic: 0.2, ..Resistances::default() };
        let a = against(std::slice::from_ref(&w), Some(&shield), 200.0, &hull, 100.0);
        // Shields: 10 × (0.5 + 0.25) × 2/3 = 5 a second, 20 s. Hull: 10 × 0.5 (hardness) × (0.4 + 0.5) = 4.5, 44.4 s.
        assert!((a.shield_dps - 5.0).abs() < 1e-9 && (a.shield_secs.unwrap_or_default() - 20.0).abs() < 1e-9, "{a:?}");
        assert!((a.hull_dps - 4.5).abs() < 1e-9 && (a.hull_secs.unwrap_or_default() - 200.0 / 4.5).abs() < 1e-9, "{a:?}");
        // No shields: none to break. No weapons: never.
        assert_eq!(against(&[w], None, 200.0, &hull, 100.0).shield_secs, None);
        assert_eq!(against(&[], Some(&shield), 200.0, &hull, 100.0).hull_secs, None);
    }
}
