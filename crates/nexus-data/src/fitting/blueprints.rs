//! Planned engineering: what a blueprint at a grade, rolled part or all of the way, and an experimental effect make of
//! a module's figures. Data: coriolis-data's blueprints (each grade's range for every figure it changes), how each
//! figure changes (multiplied, added to or replaced) and what each experimental effect does (`data/coriolis`).
//!
//! The order is the game's: the blueprint's changes first, each from the start to the end of its grade's range by the
//! roll; resistances scale with what's left of the module's own (a +10 % roll on a 40 % resistance makes 46 %); then
//! the experimental effect on top. Moving the optimal mass moves the minimum and maximum with it, and moving the
//! optimal multiplier moves all three multipliers, as the game moves the whole curve.

use super::DamageDist;
use rustc_hash::FxHashMap;
use serde_json::Value;
use std::sync::LazyLock;

const BLUEPRINTS_JSON: &str = include_str!("../../../../data/coriolis/blueprints.json");
const MODIFICATIONS_JSON: &str = include_str!("../../../../data/coriolis/modifications.json");
const ACTIONS_JSON: &str = include_str!("../../../../data/coriolis/modifier-actions.json");

/// How engineering changes a figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Multiply,
    Add,
    Replace,
}

#[derive(Debug, Clone, Copy)]
struct Stat {
    method: Method,
    higher_better: bool,
}

/// One thing a blueprint's grade or an experimental effect changes.
#[derive(Debug, Clone, PartialEq)]
enum Change {
    /// A grade's range, from its start to its end: a share (0.55 = +55 %) for a multiplied figure, a number for one
    /// added to or replaced.
    Range(f64, f64),
    /// An experimental effect's one value (resistances in percent, as coriolis-data has them).
    Value(f64),
    /// The damage types the weapon deals now.
    Damage(DamageDist),
}

struct Data {
    stats: FxHashMap<String, Stat>,
    /// (blueprint, grade) → its changes; blueprints by coriolis-data's key in lower case (the journal's symbol, except
    /// `mc_overcharged`: the multi-cannon's Overcharged).
    grades: FxHashMap<(String, u8), Vec<(String, Change)>>,
    /// Experimental effect (lower-case journal symbol) → its changes.
    effects: FxHashMap<String, Vec<(String, Change)>>,
}

static DATA: LazyLock<Data> = LazyLock::new(load);

fn load() -> Data {
    let parse = |text: &str| -> serde_json::Map<String, Value> { serde_json::from_str(text).unwrap_or_default() };
    let stats = parse(MODIFICATIONS_JSON)
        .into_iter()
        .map(|(k, v)| {
            let method = match v.get("method").and_then(Value::as_str) {
                Some("additive") => Method::Add,
                Some("overwrite") => Method::Replace,
                _ => Method::Multiply,
            };
            (k, Stat { method, higher_better: v.get("higherbetter").and_then(Value::as_bool).unwrap_or(true) })
        })
        .collect();
    let change = |v: &Value| -> Option<Change> {
        match v {
            Value::Array(a) => Some(Change::Range(a.first()?.as_f64()?, a.get(1)?.as_f64()?)),
            Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(Change::Value),
            Value::Object(_) => Some(Change::Damage(DamageDist::from_json(v))),
            _ => None,
        }
    };
    let mut grades = FxHashMap::default();
    for (key, bp) in parse(BLUEPRINTS_JSON) {
        for (g, grade) in bp.get("grades").and_then(Value::as_object).into_iter().flatten() {
            let Ok(g) = g.trim().parse::<u8>() else { continue };
            let features = grade.get("features").and_then(Value::as_object).into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), change(v)?))).collect();
            grades.insert((key.to_ascii_lowercase(), g), features);
        }
    }
    let effects = parse(ACTIONS_JSON)
        .into_iter()
        .filter(|(k, _)| k.starts_with("special_"))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.as_object().into_iter().flatten().filter_map(|(s, x)| Some((s.clone(), change(x)?))).collect()))
        .collect();
    Data { stats, grades, effects }
}

fn is_resistance(stat: &str) -> bool {
    matches!(stat, "kinres" | "thermres" | "explres" | "causres")
}

/// The data's key for a blueprint on a module: the journal's symbol, but a multi-cannon's Overcharged is its own.
fn key(item: &str, blueprint: &str) -> String {
    let b = blueprint.trim().to_ascii_lowercase();
    if b == "weapon_overcharged" && item.to_ascii_lowercase().contains("multicannon") { "mc_overcharged".into() } else { b }
}

/// The grades this data has a blueprint's figures for, on a module.
pub fn has_figures(item: &str, blueprint: &str, grade: u8) -> bool {
    DATA.grades.contains_key(&(key(item, blueprint), grade))
}

/// An experimental effect that changes figures the planner shows (Auto Loader's reloads count in sustained damage);
/// false for one this data doesn't know or that only changes how a weapon behaves (Drag Munitions, Thermal Vent).
pub fn effect_has_figures(experimental: &str) -> bool {
    let x = experimental.trim().to_ascii_lowercase();
    x == "special_auto_loader" || DATA.effects.get(&x).is_some_and(|changes| !changes.is_empty())
}

/// A module's figures after engineering: by the figures' own keys, only those the engineering changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Engineered {
    pub stats: FxHashMap<String, f64>,
    /// The damage types it deals now, when the engineering changes them.
    pub damage: Option<DamageDist>,
}

/// What `blueprint` at `grade`, rolled `roll` of the way through the grade (0: its start, 1: its end), and
/// `experimental` make of a module whose own figures `base` gives (None: it has no such figure). `item` is the module's
/// symbol (a multi-cannon's Overcharged has figures of its own). None when this data has no figures for the blueprint
/// at that grade; an experimental effect it doesn't know adds nothing.
pub fn planned(item: &str, base: &dyn Fn(&str) -> Option<f64>, blueprint: &str, grade: u8, roll: f64, experimental: Option<&str>) -> Option<Engineered> {
    let d = &*DATA;
    let features = d.grades.get(&(key(item, blueprint), grade))?;
    let roll = if roll.is_finite() { roll.clamp(0.0, 1.0) } else { 1.0 };
    let stat = |s: &str| d.stats.get(s).copied().unwrap_or(Stat { method: Method::Multiply, higher_better: true });
    let own = |s: &str| base(s).unwrap_or(0.0);
    // The changes as the game keeps them: shares for multiplied figures, amounts for added or replaced ones.
    let mut mods: FxHashMap<String, f64> = FxHashMap::default();
    let mut damage = None;
    for (s, change) in features {
        match change {
            Change::Damage(x) => damage = Some(*x),
            Change::Value(v) => {
                mods.insert(s.clone(), *v);
            }
            Change::Range(a, b) => {
                // From the start of the grade's range to its end; a range of drawbacks (below zero) runs the other way.
                let negative = *a < 0.0 || (*a == 0.0 && *b < 0.0);
                let v = if stat(s).higher_better != negative { a + (b - a) * roll } else { b + (a - b) * roll };
                // A resistance gains its share of what's left of the module's own.
                let v = if is_resistance(s) { (1.0 - own(s)) * v } else { v };
                mods.insert(s.clone(), v);
            }
        }
    }
    if let Some(effects) = experimental.and_then(|x| d.effects.get(&x.trim().to_ascii_lowercase())) {
        for (s, change) in effects {
            let a = match change {
                Change::Damage(x) => {
                    damage = Some(*x);
                    continue;
                }
                Change::Value(a) | Change::Range(a, _) => *a,
            };
            let r = mods.get(s).copied().unwrap_or(0.0);
            let next = if is_resistance(s) {
                // In percent, on what the module and its blueprint leave.
                r + a / 100.0 * (1.0 - (own(s) + r))
            } else {
                match stat(s).method {
                    Method::Add => r + a,
                    Method::Replace => a,
                    Method::Multiply => (1.0 + r) * (1.0 + a) - 1.0,
                }
            };
            mods.insert(s.clone(), next);
        }
    }
    // The figures they come to.
    let mut stats = FxHashMap::default();
    for (s, r) in &mods {
        let value = match (s.as_str(), stat(s).method) {
            (_, Method::Add) => own(s) + r,
            (_, Method::Replace) => *r,
            ("shieldboost" | "hullboost", _) => (1.0 + own(s)) * (1.0 + r) - 1.0,
            // The rate of fire changes as its interval does: +10 % on the interval is a tenth slower.
            ("rof", _) => match base("rof") {
                Some(rof) => rof / (1.0 + r),
                None => continue,
            },
            _ => match base(s) {
                Some(b) => b * (1.0 + r),
                None => continue,
            },
        };
        if value.is_finite() {
            stats.insert(s.clone(), value);
        }
    }
    // The curve moves as a whole.
    if let Some(r) = mods.get("optmass") {
        for k in ["minmass", "maxmass"] {
            if !mods.contains_key(k)
                && let Some(b) = base(k)
            {
                stats.insert(k.into(), b * (1.0 + r));
            }
        }
    }
    if let Some(r) = mods.get("optmul") {
        for k in ["minmul", "maxmul", "minmulspeed", "optmulspeed", "maxmulspeed", "minmulrotation", "optmulrotation", "maxmulrotation"] {
            if let Some(b) = base(k) {
                stats.insert(k.into(), b * (1.0 + r));
            }
        }
    }
    // Some long-range blueprints make the whole range full damage.
    if mods.get("fallofffromrange").is_some_and(|v| *v != 0.0) {
        let range = stats.get("range").copied().or_else(|| base("range"));
        if let Some(range) = range {
            stats.insert("falloff".into(), range);
        }
    }
    Some(Engineered { stats, damage })
}

/// A curve's ends and its speed and turning shares: they move with its middle, so they aren't typed on their own.
pub fn is_curve_end(key: &str) -> bool {
    matches!(key, "minmass" | "maxmass" | "minmul" | "maxmul" | "minmulspeed" | "optmulspeed" | "maxmulspeed" | "minmulrotation" | "optmulrotation" | "maxmulrotation")
}

/// What `goal` makes of a module: [`planned`] at its roll, then the figures typed in exactly. An exact optimal mass or
/// multiplier moves the curve's ends with it.
pub fn planned_goal(item: &str, base: &dyn Fn(&str) -> Option<f64>, goal: &super::Goal) -> Option<Engineered> {
    let mut e = planned(item, base, &goal.blueprint, goal.grade, goal.roll, goal.experimental.as_deref())?;
    for (k, v) in &goal.exact {
        if !v.is_finite() {
            continue;
        }
        let was = e.stats.get(k).copied().or_else(|| base(k));
        let ends: &[&str] = match k.as_str() {
            "optmass" => &["minmass", "maxmass"],
            "optmul" => &["minmul", "maxmul", "minmulspeed", "optmulspeed", "maxmulspeed", "minmulrotation", "optmulrotation", "maxmulrotation"],
            _ => &[],
        };
        if let Some(was) = was.filter(|w| *w != 0.0) {
            for end in ends {
                if let Some(x) = e.stats.get(*end).copied().or_else(|| base(end)) {
                    e.stats.insert((*end).to_string(), x * v / was);
                }
            }
        }
        e.stats.insert(k.clone(), *v);
    }
    Some(e)
}

/// The damage types an experimental effect makes a weapon deal, if it changes them (Incendiary Rounds: mostly
/// thermal).
pub fn effect_damage(experimental: &str) -> Option<DamageDist> {
    DATA.effects.get(&experimental.trim().to_ascii_lowercase())?.iter().find_map(|(_, c)| match c {
        Change::Damage(d) => Some(*d),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fsd_base(k: &str) -> Option<f64> {
        // A 5A frame shift drive's own figures.
        match k {
            "optmass" => Some(1050.0),
            "mass" => Some(20.0),
            "power" => Some(0.6),
            "maxfuel" => Some(5.0),
            "integrity" => Some(120.0),
            _ => None,
        }
    }

    #[test]
    fn a_grade_runs_from_its_start_to_its_end() {
        // Increased FSD Range at grade 5: the optimal mass +45 % at the start of the grade, +55 % at its end; the
        // drawbacks the same all the way.
        let best = planned("int_hyperdrive_size5_class5", &fsd_base, "FSD_LongRange", 5, 1.0, None).expect("figures");
        let worst = planned("int_hyperdrive_size5_class5", &fsd_base, "FSD_LongRange", 5, 0.0, None).expect("figures");
        assert!((best.stats["optmass"] - 1050.0 * 1.55).abs() < 1e-9, "{best:?}");
        assert!((worst.stats["optmass"] - 1050.0 * 1.45).abs() < 1e-9, "{worst:?}");
        assert!((best.stats["mass"] - 26.0).abs() < 1e-9 && best.stats["mass"] == worst.stats["mass"]);
        // Mass Manager adds 4 % on top of what the blueprint makes.
        let heavy = planned("int_hyperdrive_size5_class5", &fsd_base, "FSD_LongRange", 5, 1.0, Some("special_fsd_heavy")).expect("figures");
        assert!((heavy.stats["optmass"] - 1050.0 * 1.55 * 1.04).abs() < 1e-6, "{heavy:?}");
        // Grades and blueprints this data doesn't have give nothing, not a guess.
        assert!(planned("int_hyperdrive_size5_class5", &fsd_base, "FSD_LongRange", 9, 1.0, None).is_none());
        assert!(planned("int_hyperdrive_size5_class5", &fsd_base, "No_Such_Blueprint", 5, 1.0, None).is_none());
    }

    #[test]
    fn resistances_scale_with_what_is_left() {
        // A shield generator's thermal resistance (-20 %) and kinetic (40 %): Thermal Resistant adds a share of what's
        // left, and an experimental's percent applies to what the blueprint leaves.
        let base = |k: &str| match k {
            "thermres" => Some(-0.2),
            "kinres" => Some(0.4),
            "explres" => Some(0.5),
            "optmul" => Some(1.2),
            "minmul" => Some(0.7),
            "maxmul" => Some(1.7),
            "mass" => Some(20.0),
            "power" => Some(3.64),
            _ => None,
        };
        let e = planned("int_shieldgenerator_size5_class5", &base, "ShieldGenerator_Thermic", 5, 1.0, None).expect("figures");
        let thermal = e.stats["thermres"];
        assert!(thermal > -0.2 && thermal < 1.0, "{e:?}");
        // The multipliers move together.
        if let (Some(opt), Some(min)) = (e.stats.get("optmul"), e.stats.get("minmul")) {
            assert!((opt / 1.2 - min / 0.7).abs() < 1e-9);
        }
        let x = planned("int_shieldgenerator_size5_class5", &base, "ShieldGenerator_Thermic", 5, 1.0, Some("special_shield_thermic")).expect("figures");
        assert!((x.stats["thermres"] - (thermal + 0.08 * (1.0 - thermal))).abs() < 1e-9, "{x:?}");
    }

    #[test]
    fn a_figure_typed_exactly_replaces_the_roll() {
        let base = |k: &str| match k {
            "optmass" => Some(1000.0),
            "minmass" => Some(500.0),
            "maxmass" => Some(1500.0),
            "optmul" => Some(1.0),
            "minmul" => Some(0.9),
            "maxmul" => Some(1.1),
            "mass" => Some(10.0),
            "power" => Some(5.0),
            "integrity" => Some(100.0),
            "thermload" => Some(1.3),
            _ => None,
        };
        let plain = super::super::Goal::new("Engine_Dirty", 5);
        let rolled = planned_goal("int_engine_size5_class5", &base, &plain).expect("figures");
        // An optimal mass typed in replaces the roll's and moves the curve's ends with it; the rest stays as rolled.
        let goal = super::super::Goal { exact: vec![("optmass".into(), 1200.0), ("power".into(), 6.0)], ..plain };
        let e = planned_goal("int_engine_size5_class5", &base, &goal).expect("figures");
        assert_eq!((e.stats["optmass"], e.stats["power"]), (1200.0, 6.0));
        let was = rolled.stats.get("optmass").copied().unwrap_or(1000.0);
        let min = rolled.stats.get("minmass").copied().unwrap_or(500.0);
        assert!((e.stats["minmass"] - min * 1200.0 / was).abs() < 1e-9, "{e:?}");
        assert_eq!(e.stats.get("optmul"), rolled.stats.get("optmul"));
        assert!(is_curve_end("maxmul") && !is_curve_end("optmul"));
    }

    #[test]
    fn a_multicannons_overcharged_is_its_own() {
        assert!(has_figures("hpt_multicannon_gimbal_large", "Weapon_Overcharged", 5));
        assert!(has_figures("hpt_pulselaser_gimbal_large", "Weapon_Overcharged", 5));
        // Incendiary Rounds turn a weapon thermal.
        assert!(effect_damage("special_incendiary_rounds").is_some_and(|d| d.thermal > 0.5));
    }
}
