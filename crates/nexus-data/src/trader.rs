//! Material-trader maths and a shopping planner (engineering.md §3).
//!
//! * Same category: up Δ grades costs `6^Δ : 1`; down Δ grades gives `1 : 3^Δ`.
//! * Different category of the same kind: the input side is multiplied by 6
//!   (same grade 6:1, one grade down 2:1, one grade up 36:1).
//! * Trades happen in whole multiples of the (reduced) ratio. Raw ↔ Manufactured ↔ Encoded is
//!   impossible, and Guardian/Thargoid materials are not tradeable.
//! * Storage caps by grade: 300 / 250 / 200 / 150 / 100.

use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::engineering::Blueprint;
use crate::materials::{self, Material, MaterialKind};

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// Give:receive per trade unit between two materials, reduced (`(6, 1)` = give 6, receive 1).
fn ratio_between(from: &Material, to: &Material) -> Option<(u32, u32)> {
    if from.symbol == to.symbol || from.kind != to.kind || !from.tradeable || !to.tradeable {
        return None;
    }
    let valid = |m: &Material| (1..=m.kind.max_grade()).contains(&m.grade);
    if !valid(from) || !valid(to) {
        return None;
    }
    let (mut give, receive) = if to.grade > from.grade {
        (6u32.pow(u32::from(to.grade - from.grade)), 1)
    } else {
        (1, 3u32.pow(u32::from(from.grade - to.grade)))
    };
    if from.category != to.category {
        give *= 6;
    }
    let g = gcd(give, receive);
    Some((give / g, receive / g))
}

/// Give:receive per trade unit (`trade_ratio("iron", "zinc") == Some((6, 1))`), or `None` when the
/// two materials cannot be exchanged.
pub fn trade_ratio(from: &str, to: &str) -> Option<(u32, u32)> {
    ratio_between(materials::material(from)?, materials::material(to)?)
}

/// How many `from` you must give to receive at least `qty` of `to` (whole trade units; caps are
/// not considered). `None` when the trade is impossible.
pub fn trade_cost(from: &str, to: &str, qty: u32) -> Option<u32> {
    let (give, receive) = trade_ratio(from, to)?;
    Some(qty.div_ceil(receive).saturating_mul(give))
}

/// How many `to` you receive for giving `give` of `from` (whole trade units only).
pub fn trade_yield(from: &str, to: &str, give: u32) -> Option<u32> {
    let (g, r) = trade_ratio(from, to)?;
    Some((give / g).saturating_mul(r))
}

/// How much of each material to keep back when it is used as trade input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reserve {
    None,
    /// Keep this many units of every material.
    Absolute(u32),
    /// Keep this percentage of the material's storage cap.
    PercentOfCap(u32),
}

impl Reserve {
    fn amount(self, m: &Material) -> u32 {
        match self {
            Reserve::None => 0,
            Reserve::Absolute(n) => n,
            Reserve::PercentOfCap(p) => m.cap() * p.min(100) / 100,
        }
    }
}

/// Planner knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShoppingOptions {
    /// What to keep of every material offered to a trader (default: 10 % of its cap).
    pub reserve: Reserve,
    /// Allow cross-category trades (×6 input). Same-category trades are always preferred.
    pub cross_category: bool,
}

impl Default for ShoppingOptions {
    fn default() -> Self {
        ShoppingOptions { reserve: Reserve::PercentOfCap(10), cross_category: true }
    }
}

/// One suggested material-trader exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeSuggestion {
    pub give: String,
    pub give_qty: u32,
    pub receive: String,
    pub receive_qty: u32,
    /// Which trader (Raw, Manufactured, Encoded).
    pub trader: MaterialKind,
}

/// Why a shortfall could not be covered by trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedReason {
    /// Not an engineering material (commodity, micro-resource, unknown symbol).
    NotAMaterial,
    /// Guardian/Thargoid material: must be collected.
    NotTradeable,
    /// Not enough spare materials of the same kind.
    InsufficientSurplus,
    /// More is required than the storage cap allows holding at once.
    ExceedsCap,
}

/// A shortfall left after trading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub symbol: String,
    pub qty: u32,
    pub reason: UnresolvedReason,
}

/// Result of [`plan_shopping`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShoppingPlan {
    /// Shortfall per required material before trading (required − held).
    pub missing: Vec<(String, u32)>,
    /// Suggested trades covering (part of) the shortfall.
    pub trades: Vec<TradeSuggestion>,
    /// What is still missing after the trades, with the reason.
    pub unresolved: Vec<Unresolved>,
}

impl ShoppingPlan {
    /// Everything required is either held or obtainable through the suggested trades.
    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty()
    }

    /// Total quantity of `symbol` the trades give away.
    pub fn given(&self, symbol: &str) -> u32 {
        self.trades.iter().filter(|t| t.give == symbol).map(|t| t.give_qty).sum()
    }

    /// Total quantity of `symbol` the trades receive.
    pub fn received(&self, symbol: &str) -> u32 {
        self.trades.iter().filter(|t| t.receive == symbol).map(|t| t.receive_qty).sum()
    }
}

/// One possible source for a shortfall while planning.
struct Candidate {
    /// (cross-category, only partially covers, share of the source's surplus spent in ppm, units
    /// given) — lower is better.
    score: (bool, bool, u64, u32),
    pool_index: usize,
    units: u32,
    give: u32,
    receive: u32,
}

/// Canonical key: the material symbol when known, otherwise the normalised journal symbol.
fn canonical(symbol: &str) -> String {
    materials::material(symbol).map(|m| m.symbol.clone()).unwrap_or_else(|| crate::commodities::symbol(symbol))
}

/// [`plan_shopping_with`] using [`ShoppingOptions::default`].
pub fn plan_shopping<S: BuildHasher>(required: &[(String, u32)], inventory: &HashMap<String, i64, S>) -> ShoppingPlan {
    plan_shopping_with(required, inventory, &ShoppingOptions::default())
}

/// Plans material-trader exchanges to cover `required` from the surplus in `inventory`
/// (symbol → count, any journal spelling).
///
/// * Materials that are themselves required are never traded away.
/// * Every material offered keeps `opts.reserve`.
/// * Same-category trades are preferred over cross-category ones; among candidates the planner
///   prefers one source that covers the whole shortfall and spends the smallest share of that
///   source's surplus (i.e. it uses what you have most of).
/// * Received quantities never push a material over its storage cap.
pub fn plan_shopping_with<S: BuildHasher>(
    required: &[(String, u32)],
    inventory: &HashMap<String, i64, S>,
    opts: &ShoppingOptions,
) -> ShoppingPlan {
    // Merge requirements and holdings by canonical symbol.
    let mut req: Vec<(String, u32)> = Vec::new();
    for (sym, n) in required {
        let key = canonical(sym);
        match req.iter_mut().find(|(k, _)| *k == key) {
            Some((_, c)) => *c = c.saturating_add(*n),
            None => req.push((key, *n)),
        }
    }
    let mut have: HashMap<String, u32> = HashMap::new();
    for (sym, &n) in inventory {
        let n = u32::try_from(n.max(0)).unwrap_or(u32::MAX);
        let c = have.entry(canonical(sym)).or_default();
        *c = c.saturating_add(n);
    }
    let held = |s: &str| have.get(s).copied().unwrap_or(0);

    let mut plan = ShoppingPlan::default();
    let mut deficits: Vec<(String, u32)> = Vec::new();
    for (sym, need) in &req {
        let short = need.saturating_sub(held(sym));
        if short > 0 {
            plan.missing.push((sym.clone(), short));
            deficits.push((sym.clone(), short));
        }
    }

    // Spare material per symbol (deterministic order).
    let mut pool: Vec<(&'static Material, u32)> = have
        .iter()
        .filter(|(s, _)| !req.iter().any(|(r, _)| r == *s))
        .filter_map(|(s, &n)| {
            let m = materials::material(s).filter(|m| m.tradeable)?;
            let spare = n.saturating_sub(opts.reserve.amount(m));
            (spare > 0).then_some((m, spare))
        })
        .collect();
    pool.sort_by(|a, b| a.0.symbol.cmp(&b.0.symbol));

    // Hardest first: highest grade, then the order given.
    let mut order: Vec<usize> = (0..deficits.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(materials::material(&deficits[i].0).map_or(0, |m| m.grade)));

    for i in order {
        let (sym, short) = &deficits[i];
        let Some(target) = materials::material(sym) else {
            plan.unresolved.push(Unresolved {
                symbol: sym.clone(),
                qty: *short,
                reason: UnresolvedReason::NotAMaterial,
            });
            continue;
        };
        if !target.tradeable {
            plan.unresolved.push(Unresolved {
                symbol: sym.clone(),
                qty: *short,
                reason: UnresolvedReason::NotTradeable,
            });
            continue;
        }
        let mut room = target.cap().saturating_sub(held(sym));
        let mut remaining = (*short).min(room);
        let over_cap = short - remaining;
        while remaining > 0 {
            let mut best: Option<Candidate> = None;
            for (pool_index, (src, avail)) in pool.iter().enumerate() {
                let Some((give, receive)) = ratio_between(src, target) else { continue };
                let cross = src.category != target.category;
                if cross && !opts.cross_category {
                    continue;
                }
                let wanted = remaining.div_ceil(receive);
                let units = wanted.min(avail / give).min(room / receive);
                if units == 0 {
                    continue;
                }
                // Share of this source's surplus, in parts per million (integer, deterministic).
                let share = u64::from(units * give) * 1_000_000 / u64::from(*avail);
                let c =
                    Candidate { score: (cross, units < wanted, share, units * give), pool_index, units, give, receive };
                if best.as_ref().is_none_or(|b| c.score < b.score) {
                    best = Some(c);
                }
            }
            let Some(c) = best else { break };
            let (src, avail) = &mut pool[c.pool_index];
            *avail -= c.units * c.give;
            plan.trades.push(TradeSuggestion {
                give: src.symbol.clone(),
                give_qty: c.units * c.give,
                receive: target.symbol.clone(),
                receive_qty: c.units * c.receive,
                trader: target.kind,
            });
            remaining = remaining.saturating_sub(c.units * c.receive);
            room = room.saturating_sub(c.units * c.receive);
        }
        if remaining > 0 {
            plan.unresolved.push(Unresolved {
                symbol: sym.clone(),
                qty: remaining,
                reason: UnresolvedReason::InsufficientSurplus,
            });
        }
        if over_cap > 0 {
            plan.unresolved.push(Unresolved {
                symbol: sym.clone(),
                qty: over_cap,
                reason: UnresolvedReason::ExceedsCap,
            });
        }
    }

    // Merge identical give/receive pairs.
    let mut merged: Vec<TradeSuggestion> = Vec::with_capacity(plan.trades.len());
    for t in plan.trades {
        match merged.iter_mut().find(|m| m.give == t.give && m.receive == t.receive) {
            Some(m) => {
                m.give_qty += t.give_qty;
                m.receive_qty += t.receive_qty;
            }
            None => merged.push(t),
        }
    }
    plan.trades = merged;
    plan
}

/// Materials needed to roll `blueprint` `rolls_per_grade` times at every grade from `from_grade`
/// to `to_grade` (inclusive, clamped to the blueprint's grades), summed per material.
pub fn blueprint_shopping(
    blueprint: &Blueprint,
    from_grade: u8,
    to_grade: u8,
    rolls_per_grade: u32,
) -> Vec<(String, u32)> {
    let rolls: Vec<(u8, u32)> =
        (from_grade.max(1)..=to_grade.min(blueprint.max_grade())).map(|g| (g, rolls_per_grade)).collect();
    blueprint_shopping_rolls(blueprint, &rolls)
}

/// Like [`blueprint_shopping`] with an explicit roll count per grade, e.g. `&[(4, 2), (5, 4)]`.
pub fn blueprint_shopping_rolls(blueprint: &Blueprint, rolls: &[(u8, u32)]) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for &(grade, n) in rolls {
        for ing in blueprint.grade(grade).unwrap_or_default() {
            let qty = ing.count.saturating_mul(n);
            match out.iter_mut().find(|(s, _)| *s == ing.symbol) {
                Some((_, c)) => *c = c.saturating_add(qty),
                None => out.push((ing.symbol.clone(), qty)),
            }
        }
    }
    out.retain(|(_, n)| *n > 0);
    out
}

/// Merc Coins for the same rolls as [`blueprint_shopping`] (0 for ordinary blueprints).
pub fn blueprint_merc_coins(blueprint: &Blueprint, from_grade: u8, to_grade: u8, rolls_per_grade: u32) -> u32 {
    (from_grade.max(1)..=to_grade.min(blueprint.max_grade()))
        .map(|g| blueprint.merc_coins_for(g).saturating_mul(rolls_per_grade))
        .fold(0, u32::saturating_add)
}

/// The rolls a plan makes at each grade from `from_grade` to `to_grade` (clamped to the blueprint's grades): `rolls[g -
/// 1]` at grade `g` (as [`crate::engineering::GRADE_ROLLS`]) on each of `modules` modules, as every module is rolled
/// on its own.
pub fn planned_rolls(blueprint: &Blueprint, from_grade: u8, to_grade: u8, rolls: &[u32; 5], modules: u32) -> Vec<(u8, u32)> {
    (from_grade.max(1)..=to_grade.min(blueprint.max_grade()))
        .map(|g| (g, rolls.get(usize::from(g) - 1).copied().unwrap_or(0).saturating_mul(modules)))
        .collect()
}

/// Materials needed to roll `blueprint` from `from_grade` to `to_grade` with `rolls[g - 1]` rolls at grade `g`, on
/// `modules` modules (four hull reinforcement packages with the same blueprint take four times the materials), summed
/// per material.
pub fn blueprint_shopping_by_grade(blueprint: &Blueprint, from_grade: u8, to_grade: u8, rolls: &[u32; 5], modules: u32) -> Vec<(String, u32)> {
    blueprint_shopping_rolls(blueprint, &planned_rolls(blueprint, from_grade, to_grade, rolls, modules))
}

/// Merc Coins for the same rolls as [`blueprint_shopping_by_grade`] (0 for ordinary blueprints).
pub fn blueprint_merc_coins_by_grade(blueprint: &Blueprint, from_grade: u8, to_grade: u8, rolls: &[u32; 5], modules: u32) -> u32 {
    planned_rolls(blueprint, from_grade, to_grade, rolls, modules)
        .into_iter()
        .map(|(g, n)| blueprint.merc_coins_for(g).saturating_mul(n))
        .fold(0, u32::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engineering::blueprint;

    fn inv(items: &[(&str, i64)]) -> HashMap<String, i64> {
        items.iter().map(|(s, n)| ((*s).to_string(), *n)).collect()
    }

    fn req(items: &[(&str, u32)]) -> Vec<(String, u32)> {
        items.iter().map(|(s, n)| ((*s).to_string(), *n)).collect()
    }

    #[test]
    fn same_category_ratios() {
        // Raw category 4: iron G1, zinc G2, tin G3, selenium G4.
        assert_eq!(trade_ratio("iron", "zinc"), Some((6, 1)));
        assert_eq!(trade_ratio("zinc", "iron"), Some((1, 3)));
        assert_eq!(trade_ratio("iron", "selenium"), Some((216, 1)));
        assert_eq!(trade_ratio("selenium", "iron"), Some((1, 27)));
        // Encoded wake scans G1 → G5.
        assert_eq!(trade_ratio("disruptedwakeechoes", "dataminedwake"), Some((1296, 1)));
        assert_eq!(trade_cost("disruptedwakeechoes", "fsdtelemetry", 10), Some(60));
        assert_eq!(trade_cost("hyperspacetrajectories", "wakesolutions", 4), Some(2));
        assert_eq!(trade_yield("hyperspacetrajectories", "wakesolutions", 2), Some(6));
        assert_eq!(trade_yield("iron", "zinc", 11), Some(1));
    }

    #[test]
    fn cross_category_ratios() {
        // Carbon (Raw cat 1, G1) → Iron (Raw cat 4, G1): 6:1.
        assert_eq!(trade_ratio("carbon", "iron"), Some((6, 1)));
        // Vanadium (cat 1, G2) → Iron (cat 4, G1): one grade down across categories = 2:1.
        assert_eq!(trade_ratio("vanadium", "iron"), Some((2, 1)));
        // Niobium (cat 1, G3) → Iron (cat 4, G1): 2:3.
        assert_eq!(trade_ratio("niobium", "iron"), Some((2, 3)));
        // Iron (G1) → Vanadium (other cat, G2): 36:1.
        assert_eq!(trade_ratio("iron", "vanadium"), Some((36, 1)));
        // Manufactured G5 → G1 cross: 2:27.
        assert_eq!(trade_ratio("pharmaceuticalisolators", "gridresistors"), Some((2, 27)));
        assert_eq!(trade_cost("carbon", "iron", 7), Some(42));
    }

    #[test]
    fn impossible_trades() {
        assert_eq!(trade_ratio("iron", "gridresistors"), None); // raw ↔ manufactured
        assert_eq!(trade_ratio("iron", "iron"), None);
        assert_eq!(trade_ratio("unknownenergysource", "imperialshielding"), None); // Sensor Fragment
        assert_eq!(trade_ratio("guardian_powercell", "gridresistors"), None);
        assert_eq!(trade_ratio("iron", "gold"), None); // commodity
        assert_eq!(trade_cost("iron", "nonsense", 1), None);
    }

    #[test]
    fn plan_prefers_same_category_and_protects_required() {
        // Need 2 Datamined Wake Exceptions (G5 wake scans). Hold plenty of G4 wake scans (same
        // category) and lots of G5 emission data (cross-category).
        let plan = plan_shopping_with(
            &req(&[("dataminedwake", 2), ("hyperspacetrajectories", 1)]),
            &inv(&[("hyperspacetrajectories", 60), ("compactemissionsdata", 100), ("adaptiveencryptors", 50)]),
            &ShoppingOptions { reserve: Reserve::None, cross_category: true },
        );
        assert_eq!(plan.missing, vec![("dataminedwake".to_string(), 2)]);
        // G4 wake scans are required themselves, so they are not touched; the planner falls back to
        // a cross-category trade (G5 → G5: 6:1) from the larger pile.
        assert_eq!(plan.given("hyperspacetrajectories"), 0);
        assert_eq!(
            plan.trades,
            vec![TradeSuggestion {
                give: "compactemissionsdata".into(),
                give_qty: 12,
                receive: "dataminedwake".into(),
                receive_qty: 2,
                trader: MaterialKind::Encoded,
            }]
        );
        assert!(plan.is_complete());
    }

    #[test]
    fn plan_same_category_first() {
        // Need 3 Tin (Raw cat 4, G3). Zinc (cat 4, G2) is same-category; Niobium (cat 1, G3) is not.
        let plan = plan_shopping_with(
            &req(&[("tin", 3)]),
            &inv(&[("zinc", 100), ("niobium", 200)]),
            &ShoppingOptions { reserve: Reserve::Absolute(10), cross_category: true },
        );
        assert_eq!(plan.trades.len(), 1);
        let t = &plan.trades[0];
        assert_eq!(
            (t.give.as_str(), t.give_qty, t.receive.as_str(), t.receive_qty, t.trader),
            ("zinc", 18, "tin", 3, MaterialKind::Raw)
        );
        assert!(plan.is_complete());
    }

    #[test]
    fn plan_reserve_and_unresolved() {
        // 30 Nickel available, reserve 10 % of cap (30) → nothing spare: insufficient.
        let plan = plan_shopping(&req(&[("germanium", 2)]), &inv(&[("nickel", 30)]));
        assert!(plan.trades.is_empty());
        assert_eq!(
            plan.unresolved,
            vec![Unresolved { symbol: "germanium".into(), qty: 2, reason: UnresolvedReason::InsufficientSurplus }]
        );
        // Non-tradeable and non-material requirements.
        let plan = plan_shopping(
            &req(&[("guardian_powercell", 5), ("$gold_name;", 1), ("Iron", 3)]),
            &inv(&[("iron", 5), ("carbon", 300)]),
        );
        assert!(plan.missing.is_empty() || plan.missing.iter().all(|(s, _)| s != "iron"));
        assert!(plan.unresolved.contains(&Unresolved {
            symbol: "guardian_powercell".into(),
            qty: 5,
            reason: UnresolvedReason::NotTradeable
        }));
        assert!(plan.unresolved.contains(&Unresolved {
            symbol: "gold".into(),
            qty: 1,
            reason: UnresolvedReason::NotAMaterial
        }));
    }

    #[test]
    fn plan_partial_and_caps() {
        // Need 10 Polonium (G4 raw cat 6); have 150 Arsenic (cat 6, G2): 36:1 → 4 units at most
        // with the default 10 % reserve (25) → 125 spare → 3 trades of 36 → 3 polonium.
        let plan = plan_shopping(&req(&[("polonium", 10)]), &inv(&[("arsenic", 150)]));
        assert_eq!(plan.received("polonium"), 3);
        assert_eq!(plan.given("arsenic"), 108);
        assert_eq!(
            plan.unresolved,
            vec![Unresolved { symbol: "polonium".into(), qty: 7, reason: UnresolvedReason::InsufficientSurplus }]
        );
        // Requirement above the cap (G5 cap 100).
        let plan = plan_shopping_with(
            &req(&[("dataminedwake", 120)]),
            &inv(&[("dataminedwake", 90), ("compactemissionsdata", 100)]),
            &ShoppingOptions { reserve: Reserve::None, cross_category: true },
        );
        assert_eq!(plan.received("dataminedwake"), 10);
        assert!(plan.unresolved.contains(&Unresolved {
            symbol: "dataminedwake".into(),
            qty: 20,
            reason: UnresolvedReason::ExceedsCap
        }));
        // Cross-category disabled.
        let plan = plan_shopping_with(
            &req(&[("iron", 5)]),
            &inv(&[("carbon", 300)]),
            &ShoppingOptions { reserve: Reserve::None, cross_category: false },
        );
        assert!(plan.trades.is_empty());
    }

    #[test]
    fn plan_down_trade_and_overshoot() {
        // Need 4 Iron: 1 Zinc gives 3 Iron, so 2 Zinc → 6 Iron.
        let plan = plan_shopping_with(
            &req(&[("iron", 4)]),
            &inv(&[("zinc", 50)]),
            &ShoppingOptions { reserve: Reserve::None, cross_category: true },
        );
        assert_eq!(plan.trades[0].give_qty, 2);
        assert_eq!(plan.trades[0].receive_qty, 6);
    }

    #[test]
    fn shopping_lists() {
        let lr = blueprint("FSD_LongRange").unwrap();
        let g5 = blueprint_shopping(lr, 5, 5, 4);
        let mut g5s: Vec<(&str, u32)> = g5.iter().map(|(s, n)| (s.as_str(), *n)).collect();
        g5s.sort();
        assert_eq!(g5s, vec![("arsenic", 4), ("chemicalmanipulators", 4), ("dataminedwake", 4)]);
        // G1..G5 once each: wake echoes appear in G1 and G2.
        let all = blueprint_shopping(lr, 1, 5, 1);
        assert_eq!(all.iter().find(|(s, _)| s == "disruptedwakeechoes").map(|(_, n)| *n), Some(2));
        assert_eq!(all.iter().map(|(_, n)| n).sum::<u32>(), 1 + 2 + 3 + 3 + 3);
        // Clamped grades and explicit rolls.
        assert_eq!(blueprint_shopping(lr, 6, 9, 3), Vec::<(String, u32)>::new());
        let r = blueprint_shopping_rolls(lr, &[(4, 2), (5, 1)]);
        assert_eq!(r.iter().find(|(s, _)| s == "manganese").map(|(_, n)| *n), Some(2));
        // Merc Coins: the 5E Extended cargo rack goes from G2 to G4 at 10, 20 and 40 coins a roll.
        assert_eq!(blueprint_merc_coins(lr, 1, 5, 3), 0);
        let rack = blueprint("cargoracks5c1_extended").unwrap();
        assert_eq!(blueprint_merc_coins(rack, 1, 5, 3), (10 + 20 + 40) * 3);
        assert_eq!(blueprint_shopping(rack, 1, 5, 1).iter().map(|(_, n)| n).sum::<u32>(), 3 + 3 + 4);
        // Plan the G5 list against an inventory.
        let plan = plan_shopping_with(
            &g5,
            &inv(&[
                ("arsenic", 4),
                ("chemicalmanipulators", 1),
                ("chemicalprocessors", 200),
                ("hyperspacetrajectories", 40),
            ]),
            &ShoppingOptions { reserve: Reserve::None, cross_category: true },
        );
        let mut missing = plan.missing.clone();
        missing.sort();
        assert_eq!(missing, vec![("chemicalmanipulators".to_string(), 3), ("dataminedwake".to_string(), 4)]);
        // CM (G4 chemical) from Chemical Processors (G2 chemical): 36:1 → 108.
        assert_eq!(plan.given("chemicalprocessors"), 108);
        // DWE (G5 wake) from Eccentric Hyperspace Trajectories (G4 wake): 6:1 → 24.
        assert_eq!(plan.given("hyperspacetrajectories"), 24);
        assert!(plan.is_complete());
    }

    /// Each grade its own rolls, times the modules that get the blueprint.
    #[test]
    fn rolls_per_grade_and_modules() {
        use crate::engineering::GRADE_ROLLS;
        let count = |list: &[(String, u32)], s: &str| list.iter().find(|(k, _)| k == s).map_or(0, |(_, n)| *n);
        let lr = blueprint("FSD_LongRange").unwrap();
        // At an engineer's top rank: G1 once (1 wake echo), G2 twice (wake echoes again, and more), up to G5 five times.
        let one = blueprint_shopping_by_grade(lr, 1, 5, &GRADE_ROLLS, 1);
        assert_eq!(count(&one, "disruptedwakeechoes"), 1 + 2, "G1's single roll and G2's two");
        assert_eq!(count(&one, "dataminedwake"), 5, "G5's five rolls");
        assert_eq!(count(&one, "manganese"), 4, "G4's four rolls");
        assert_eq!(one.iter().map(|(_, n)| n).sum::<u32>(), 1 + 2 * 2 + 3 * 3 + 4 * 3 + 5 * 3);
        // Four modules take four times everything.
        let four = blueprint_shopping_by_grade(lr, 1, 5, &GRADE_ROLLS, 4);
        for (s, n) in &one {
            assert_eq!(count(&four, s), n * 4, "{s}");
        }
        // Grades outside the plan or the blueprint count nothing; the same rolls everywhere match the old sum.
        assert_eq!(blueprint_shopping_by_grade(lr, 4, 5, &GRADE_ROLLS, 1).iter().map(|(_, n)| n).sum::<u32>(), 4 * 3 + 5 * 3);
        assert!(blueprint_shopping_by_grade(lr, 6, 9, &GRADE_ROLLS, 3).is_empty());
        assert!(blueprint_shopping_by_grade(lr, 1, 5, &GRADE_ROLLS, 0).is_empty(), "no modules, nothing to roll");
        assert_eq!(blueprint_shopping_by_grade(lr, 1, 5, &[3; 5], 1), blueprint_shopping(lr, 1, 5, 3));
        assert_eq!(planned_rolls(lr, 2, 4, &GRADE_ROLLS, 2), vec![(2, 4), (3, 6), (4, 8)]);
        // Merc Coins: the 5E Extended cargo rack's G2 to G4 at 10, 20 and 40 coins a roll, 2, 3 and 4 rolls, on 3 racks.
        let rack = blueprint("cargoracks5c1_extended").unwrap();
        assert_eq!(blueprint_merc_coins_by_grade(rack, 1, 5, &GRADE_ROLLS, 3), (10 * 2 + 20 * 3 + 40 * 4) * 3);
        assert_eq!(blueprint_merc_coins_by_grade(lr, 1, 5, &GRADE_ROLLS, 3), 0);
        // Huge counts saturate instead of wrapping.
        assert_eq!(blueprint_merc_coins_by_grade(rack, 2, 4, &[u32::MAX; 5], u32::MAX), u32::MAX);
    }
}
