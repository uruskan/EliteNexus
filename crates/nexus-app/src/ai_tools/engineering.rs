//! The assistant's engineering tool: ship blueprints, what each grade takes, experimental effects and engineers, from
//! the data the Engineering page plans with (`nexus_data::engineering`). A plan is set against what the commander holds,
//! with the material-trader swaps that cover what's short (`nexus_data::trader`), as the Blueprint planner works it out.

use super::{NexusTools, arg_f64, arg_str, fit, r1, strip_nulls};
use nexus_data::commodities::name_key;
use nexus_data::engineering::{self as eng, Blueprint, Effect, Engineer, EngineerKind, Experimental, Ingredient, Region};
use nexus_data::outfitting::{display_name, slot_display};
use nexus_data::trader::{ShoppingOptions, UnresolvedReason, blueprint_merc_coins, blueprint_shopping, plan_shopping_with};
use nexus_model::GameState;
use nexus_model::fleet::ShipModule;
use nexus_model::inventory::MaterialCategory;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Rolls planned at each grade unless asked otherwise, as on the Engineering page: most grades take 2 to 4.
const ROLLS: u32 = 3;
/// Engineers listed when the whole list doesn't fit.
const FEW_ENGINEERS: usize = 3;

fn effects(list: &[Effect]) -> Vec<String> {
    list.iter().map(|e| format!("{} {}", e.property, e.value)).collect()
}

/// "Arsenic", "3 × Arsenic".
fn ingredient(i: &Ingredient) -> String {
    if i.count == 1 { i.name() } else { format!("{} × {}", i.count, i.name()) }
}

fn item_name(symbol: &str) -> String {
    Ingredient { symbol: symbol.to_string(), count: 0 }.name()
}

/// The words asked, each as a name key: "Increased range" is `["increased", "range"]`.
fn words(asked: &str) -> Vec<String> {
    asked.split_whitespace().map(name_key).filter(|w| !w.is_empty()).collect()
}

/// Whether every word asked is part of `name`.
fn has_words(name: &str, words: &[String]) -> bool {
    let k = name_key(name);
    !words.is_empty() && words.iter().all(|w| k.contains(w.as_str()))
}

/// The type blueprints are filed under: seeker racks share the missile racks'.
fn filed(t: &str) -> &str {
    if t == "Seeker Missile Rack" { "Missile Rack" } else { t }
}

/// The module type asked for: a type ("Frame Shift Drive", "FSD", "shield boosters"), an item symbol, or words only one
/// type's name has ("wake scanner").
fn module_type(asked: &str) -> Result<&'static str, String> {
    let k = name_key(asked);
    let alias = match k.as_str() {
        "armor" | "bulkhead" | "bulkheads" => Some("Armour"),
        "shields" => Some("Shield Generator"),
        _ => None,
    };
    if let Some(t) = eng::module_type(asked).or(alias).or_else(|| k.strip_suffix('s').and_then(eng::module_type)) {
        return Ok(t);
    }
    let w = words(asked);
    let hits: Vec<&str> = eng::module_types().iter().copied().filter(|t| has_words(t, &w)).collect();
    match hits.as_slice() {
        [t] => Ok(*t),
        [] => Err(format!("Engineers don't work on \"{asked}\". The module types they do: {}.", eng::module_types().join(", "))),
        many => Err(format!("\"{asked}\" could be {}: say which.", many.join(", "))),
    }
}

/// Whether `b` is the blueprint asked for: exactly (its name or journal symbol), or loosely (every word asked is part of
/// its name, or the words run together are part of its symbol: "long range" is `FSD_LongRange`).
fn is_blueprint(b: &Blueprint, asked: &str, exact: bool) -> bool {
    let k = name_key(asked);
    if exact {
        return b.is(asked) || name_key(&b.name) == k;
    }
    has_words(&b.name, &words(asked)) || (!k.is_empty() && name_key(&b.symbol).contains(&k))
}

fn names(list: &[&Blueprint]) -> String {
    let mut v: Vec<&str> = list.iter().map(|b| b.name.as_str()).collect();
    v.dedup();
    v.join(", ")
}

/// The blueprint asked for, among the module's (or every module's, when no module was given). Generic blueprints
/// (Overcharged, Lightweight, Reinforced…) exist once for each module type that takes them: those need the module, and
/// so does a name that is exactly one module's blueprint but part of others' ("Overcharged" is a power plant's, and
/// "Overcharged Weapon" every gun's).
fn find_blueprint(module: Option<&'static str>, asked: &str) -> Result<&'static Blueprint, String> {
    let pool: Vec<&'static Blueprint> = match module {
        Some(t) => eng::blueprints_for_module(t),
        None => eng::blueprints().iter().collect(),
    };
    let exact: Vec<&'static Blueprint> = pool.iter().copied().filter(|b| is_blueprint(b, asked, true)).collect();
    let loose: Vec<&'static Blueprint> = pool.iter().copied().filter(|b| is_blueprint(b, asked, false)).collect();
    let found = if exact.is_empty() { &loose } else { &exact };
    // The modules it could be for: with no module given, every module with a blueprint that matches even loosely.
    let mut modules: Vec<&str> = found.iter().chain(if module.is_none() { &loose } else { found }).map(|b| b.module.as_str()).collect();
    modules.sort_unstable();
    modules.dedup();
    match (found.first(), modules.len()) {
        (None, _) => Err(match module {
            Some(t) => format!("{t} has no blueprint \"{asked}\". Its blueprints: {}.", names(&pool)),
            None => format!("No blueprint is called \"{asked}\": give module= to list a module's blueprints."),
        }),
        (Some(b), 1) if found.iter().all(|x| x.name == b.name) => Ok(*b),
        (Some(_), 1) => Err(format!("\"{asked}\" could be {}: say which.", names(found))),
        _ => Err(format!("\"{asked}\" is a blueprint for {}: say which with module=.", modules.join(", "))),
    }
}

/// The experimental effect asked for, among the module type's.
fn find_experimental(t: &str, asked: &str) -> Result<&'static Experimental, String> {
    let list = eng::experimentals_for(t);
    let w = words(asked);
    let loose: Vec<&'static Experimental> = list.iter().copied().filter(|e| has_words(&e.name, &w)).collect();
    list.iter()
        .copied()
        .find(|e| name_key(&e.name) == name_key(asked) || e.symbol.eq_ignore_ascii_case(asked.trim()))
        .or(if loose.len() == 1 { loose.first().copied() } else { None })
        .ok_or_else(|| {
            if list.is_empty() {
                format!("{t} takes no experimental effect.")
            } else {
                format!("{t} has no experimental effect \"{asked}\". Its effects: {}.", list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", "))
            }
        })
}

/// The engineer asked for: by name, without a title or nickname ("Tod McQuinn", "Palin"), or a part of the name only one
/// engineer has ("Farseer").
fn find_engineer(asked: &str) -> Result<&'static Engineer, String> {
    if let Some(e) = eng::engineer_by_name(asked) {
        return Ok(e);
    }
    let w = words(asked);
    let hits: Vec<&'static Engineer> = eng::engineers().iter().filter(|e| has_words(&e.name, &w)).collect();
    let list = |v: &mut dyn Iterator<Item = &'static Engineer>| v.map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ");
    match hits.as_slice() {
        [e] => Ok(*e),
        [] => Err(format!("No engineer is called \"{asked}\". The engineers: {}.", list(&mut eng::engineers().iter()))),
        many => Err(format!("\"{asked}\" could be {}: say which.", list(&mut many.iter().copied()))),
    }
}

/// A grade asked for: 3, 3.0, "3" or "G3".
fn arg_grade(a: &Value, k: &str) -> Option<u8> {
    let v = a.get(k)?;
    let n = v.as_f64().or_else(|| v.as_str().map(|s| s.trim().trim_start_matches(['G', 'g'])).and_then(|s| s.parse().ok()))?;
    (n.is_finite() && n >= 1.0).then(|| n.round().min(5.0) as u8)
}

/// Everything the commander holds that engineering takes, by lower-case journal symbol: materials, the ship locker's
/// goods and data, and cargo, as the Engineering page counts them.
fn holdings(gs: &GameState) -> HashMap<String, i64> {
    let mut inv: HashMap<String, i64> = HashMap::new();
    let m = &gs.inventory.materials;
    for cat in [MaterialCategory::Raw, MaterialCategory::Manufactured, MaterialCategory::Encoded] {
        for (k, v) in m.map(cat) {
            *inv.entry(k.to_ascii_lowercase()).or_default() += *v;
        }
    }
    let l = &gs.inventory.ship_locker;
    for map in [&l.items, &l.components, &l.data, &l.consumables] {
        for (k, v) in map {
            *inv.entry(k.to_ascii_lowercase()).or_default() += v.count;
        }
    }
    for c in &gs.inventory.cargo.items {
        *inv.entry(c.symbol.to_ascii_lowercase()).or_default() += c.count;
    }
    inv
}

/// Merc Coins on hand, from the statistics the game writes at login.
fn merc_coins(gs: &GameState) -> Option<u64> {
    gs.cmdr.statistics.as_ref()?.bank_account.get("MercCoins_Current")?.as_u64()
}

/// What is left to do before an engineer works for the commander, from where they stand (`None`: the journal doesn't
/// list the engineer, so the commander hasn't heard of them yet).
fn to_unlock(e: &Engineer, status: Option<&str>) -> Option<String> {
    let (learn, invite, unlock) = (("Learn of them", e.discovery.as_str()), ("Invitation", e.meeting.as_str()), ("Unlock", e.unlock.as_str()));
    let steps: &[(&str, &str)] = match status {
        None => &[learn, invite, unlock],
        Some("Known") => &[invite, unlock],
        Some("Invited" | "Acquainted") => &[unlock],
        Some(_) => &[],
    };
    let text: Vec<String> = steps
        .iter()
        .filter_map(|(label, s)| {
            let s = s.trim().trim_end_matches('.');
            (!s.is_empty()).then(|| format!("{label}: {s}."))
        })
        .collect();
    (!text.is_empty()).then(|| text.join(" "))
}

fn unlocked(gs: &GameState, e: &Engineer) -> bool {
    gs.cmdr.engineers.get(&e.id).is_some_and(|s| s.progress == "Unlocked")
}

/// An engineer in a list: where, how far, the top grade they offer for the module, the commander's standing, and for
/// one not unlocked yet (when `unlock`), what unlocking them takes.
fn engineer_row(gs: &GameState, e: &Engineer, up_to: u8, unlock: bool) -> Value {
    let status = gs.cmdr.engineers.get(&e.id).map(|s| s.progress.as_str());
    let mut v = json!({
        "name": e.name, "up_to_grade": up_to, "status": status,
        "system": e.system, "distance_ly": gs.location.star_pos.map(|p| r1(e.distance_to(p))),
        "permit_needed": e.permit.then_some(true), "in_colonia": (e.region == Region::Colonia).then_some(true),
        "to_unlock": if unlock { to_unlock(e, status) } else { None },
    });
    strip_nulls(&mut v);
    v
}

/// Engineers with the top grade they offer, the unlocked ones first, then the nearest.
fn by_reach<'a>(gs: &GameState, list: impl Iterator<Item = (&'a Engineer, u8)>) -> Vec<(&'a Engineer, u8)> {
    let dist = |e: &Engineer| gs.location.star_pos.map_or(0.0, |p| e.distance_to(p));
    let mut v: Vec<(&Engineer, u8)> = list.collect();
    v.sort_by(|a, b| unlocked(gs, b.0).cmp(&unlocked(gs, a.0)).then_with(|| dist(a.0).total_cmp(&dist(b.0))));
    v
}

/// The current ship's modules of a type, engineered or not.
fn fitted_modules<'a>(gs: &'a GameState, t: &str) -> Vec<&'a ShipModule> {
    let Some(ship) = gs.fleet.current() else { return Vec::new() };
    ship.modules.iter().filter(|m| eng::module_type(&m.item).is_some_and(|mt| filed(mt) == filed(t))).collect()
}

fn fitted_row(m: &ShipModule) -> Value {
    let mut v = json!({ "slot": slot_display(&m.slot), "module": display_name(&m.item) });
    match &m.engineering {
        Some(e) => {
            v["blueprint"] = json!(eng::blueprint_for(&m.item, &e.blueprint).map_or_else(|| e.blueprint.clone(), |b| b.name.clone()));
            v["grade"] = json!(e.level);
            v["quality_pct"] = json!((e.quality * 100.0).round() as i64);
            v["experimental"] = json!(e.experimental);
        }
        None => v["engineered"] = json!(false),
    }
    strip_nulls(&mut v);
    v
}

/// Why a shortfall can't be traded for.
fn why(reason: UnresolvedReason) -> &'static str {
    match reason {
        UnresolvedReason::NotAMaterial => "not an engineering material: a commodity or an on-foot item, which traders don't exchange",
        UnresolvedReason::NotTradeable => "a Guardian or Thargoid material: traders don't take it, it has to be collected",
        UnresolvedReason::InsufficientSurplus => "not enough spare materials of its kind to trade for it",
        UnresolvedReason::ExceedsCap => "more than its storage cap holds at once: collect and use it in batches",
    }
}

/// A material's kind ("raw", "manufactured", "encoded") and grade; a commodity or an on-foot item has neither.
fn material_kind(symbol: &str) -> (Option<String>, Option<u8>) {
    match nexus_data::materials::material(symbol) {
        Some(m) => (Some(m.kind.label().to_lowercase()), Some(m.grade)),
        None => (None, None),
    }
}

impl NexusTools {
    pub(super) fn engineering(&self, a: &Value) -> Result<Value, String> {
        let module = arg_str(a, "module").map(|m| module_type(&m)).transpose()?;
        let blueprint = arg_str(a, "blueprint").map(|b| find_blueprint(module, &b)).transpose()?;
        let engineer = arg_str(a, "engineer").map(|e| find_engineer(&e)).transpose()?;
        let model = self.model.read();
        let empty;
        let gs = match model.active() {
            Some(gs) => gs,
            None => {
                empty = GameState::default();
                &empty
            }
        };
        match (blueprint, module, engineer) {
            (Some(bp), _, _) => self.blueprint_plan(gs, bp, module.unwrap_or(bp.module.as_str()), engineer, a),
            (None, Some(t), _) => Ok(self.module_overview(gs, t, engineer)),
            (None, None, Some(e)) => Ok(engineer_detail(gs, e)),
            (None, None, None) => Err("Give module (its blueprints and engineers), module and blueprint (what each grade takes, against what the commander holds) or engineer.".to_string()),
        }
    }

    /// A module type's blueprints, experimental effects and engineers, and the current ship's modules of that type.
    fn module_overview(&self, gs: &GameState, t: &'static str, engineer: Option<&Engineer>) -> Value {
        let blueprints = eng::blueprints_for_module(t);
        let experimentals = eng::experimentals_for(t);
        let engineers = by_reach(gs, eng::engineers_for(t).into_iter());
        let fitted: Vec<Value> = fitted_modules(gs, t).into_iter().map(fitted_row).collect();
        // What gives way when the reply is too long for the caller: what each experimental effect does, what each
        // blueprint does, all but the first few engineers, the current ship's modules.
        const CUT: [&str; 4] = [
            "What each experimental effect does left out to fit.",
            "What each blueprint does left out to fit: ask with blueprint= for one.",
            "Only the first few engineers listed to fit.",
            "The current ship's modules of this type left out to fit: the fleet tool has them.",
        ];
        let (out, _) = fit(self.budget, CUT.len() + 1, |level| {
            let bps: Vec<Value> = blueprints
                .iter()
                .map(|b| {
                    let top = b.effects.get(usize::from(b.max_grade()).saturating_sub(1)).filter(|e| level < 2 && !e.is_empty());
                    let mut v = json!({
                        "name": b.name, "max_grade": b.max_grade(), "from_grade": (b.min_grade() > 1).then_some(b.min_grade()),
                        "merc_coins": b.is_merc().then_some(true), "for_merc_module": b.merc_module,
                        "effects_at_max_grade": top.map(Vec::as_slice).map(effects),
                    });
                    strip_nulls(&mut v);
                    v
                })
                .collect();
            let exps: Vec<Value> = experimentals.iter().map(|x| if level < 1 { json!({ "name": x.name, "effects": effects(&x.effects) }) } else { json!(x.name) }).collect();
            let shown = if level >= 3 { FEW_ENGINEERS.min(engineers.len()) } else { engineers.len() };
            let mut out = json!({
                "module": t, "blueprints": bps, "experimentals": exps,
                "engineers": engineers[..shown].iter().map(|(e, g)| engineer_row(gs, e, *g, false)).collect::<Vec<_>>(),
            });
            if shown < engineers.len() {
                out["engineers_omitted"] = json!(engineers.len() - shown);
            }
            if level < 4 && !fitted.is_empty() {
                out["current_ship"] = json!(fitted);
            }
            if let Some(e) = engineer {
                out["engineer"] = engineer_detail(gs, e);
            }
            let mut notes = vec!["Ask with blueprint= for what each grade takes against what the commander holds; with engineer= for how to unlock one."];
            notes.extend(&CUT[..level]);
            out["notes"] = json!(notes);
            out
        });
        out
    }

    /// What rolling a blueprint from one grade to another takes, against what the commander holds; the trades that
    /// cover what's short; who offers the grade.
    fn blueprint_plan(&self, gs: &GameState, bp: &'static Blueprint, t: &'static str, engineer: Option<&Engineer>, a: &Value) -> Result<Value, String> {
        let (lo, hi) = (bp.min_grade(), bp.max_grade());
        let mut notes: Vec<String> = Vec::new();
        let asked_to = arg_grade(a, "grade");
        let to = asked_to.unwrap_or(hi).clamp(lo, hi);
        if asked_to.is_some_and(|g| g > hi) {
            notes.push(format!("{} goes up to G{hi}.", bp.name));
        }
        let rolls = arg_f64(a, "rolls_per_grade").map_or(ROLLS, |r| r.round().clamp(1.0, 10.0) as u32);
        let x = arg_str(a, "experimental").map(|e| find_experimental(t, &e)).transpose()?;

        // Where to start: as asked, or where the current ship's module of this type already is with this blueprint.
        let fitted = fitted_modules(gs, t);
        let progress = fitted
            .iter()
            .filter_map(|m| {
                let e = m.engineering.as_ref()?;
                let same = eng::blueprint_for(&m.item, &e.blueprint).is_some_and(|b| b.symbol == bp.symbol && b.module == bp.module);
                same.then(|| (e.level, e.quality, slot_display(&m.slot)))
            })
            .min_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap_or(std::cmp::Ordering::Equal));
        let from = match (arg_grade(a, "from_grade"), progress) {
            (Some(f), _) => f.clamp(lo, to),
            (None, Some((grade, quality, slot))) => {
                let maxed = quality >= 0.999;
                let next = u8::try_from(if maxed { grade + 1 } else { grade }).unwrap_or(u8::MAX).max(lo);
                if next > to {
                    notes.push(format!("The current ship's {slot} already has it at G{grade}{}: nothing left to roll up to G{to}.", if maxed { ", fully rolled" } else { "" }));
                } else {
                    notes.push(format!("Planned from G{next}: the current ship's {slot} has it at G{grade} ({}% of the way). from_grade=1 plans it from scratch.", (quality * 100.0).round() as i64));
                }
                next
            }
            (None, None) => lo,
        };

        // What it takes, against what the commander holds.
        let mut need = if from <= to { blueprint_shopping(bp, from, to, rolls) } else { Vec::new() };
        for i in x.map(|x| x.cost.as_slice()).unwrap_or_default() {
            match need.iter_mut().find(|(s, _)| *s == i.symbol) {
                Some((_, n)) => *n = n.saturating_add(i.count),
                None => need.push((i.symbol.clone(), i.count)),
            }
        }
        let inv = holdings(gs);
        let have = |s: &str| u32::try_from(inv.get(&s.to_ascii_lowercase()).copied().unwrap_or(0).max(0)).unwrap_or(u32::MAX);
        let (short, enough): (Vec<_>, Vec<_>) = need.iter().partition(|(s, n)| have(s) < *n);
        let plan = plan_shopping_with(&need, &inv, &ShoppingOptions::default());
        let coins = if from <= to { blueprint_merc_coins(bp, from, to, rolls) } else { 0 };
        let balance = merc_coins(gs);
        let coins_short = balance.map_or(0, |b| u64::from(coins).saturating_sub(b));
        let total: u64 = need.iter().map(|(_, n)| u64::from(*n)).sum();
        let in_hand: u64 = need.iter().map(|(s, n)| u64::from(have(s).min(*n))).sum();
        let mut gaps = Vec::new();
        if !short.is_empty() {
            gaps.push(format!("{} of the {} materials", short.len(), need.len()));
        }
        if coins_short > 0 {
            gaps.push(format!("{coins_short} Merc Coins"));
        }
        let verdict = if need.is_empty() && coins == 0 {
            "Nothing to roll.".to_string()
        } else if gaps.is_empty() && coins > 0 && balance.is_none() {
            format!("The commander has the materials; it also takes {coins} Merc Coins (their balance shows after the next login).")
        } else if gaps.is_empty() {
            "The commander has everything it takes.".to_string()
        } else {
            let trades = match (short.is_empty(), plan.unresolved.is_empty(), plan.trades.is_empty()) {
                (true, _, _) => "",
                (false, true, _) => "; the trades cover the materials",
                (false, false, true) => "; still_short says why no trade covers them",
                (false, false, false) => "; the trades cover some, still_short lists the rest",
            };
            format!("Short of {}{trades}.", gaps.join(" and "))
        };
        let trades: Vec<Value> = plan
            .trades
            .iter()
            .map(|tr| json!({ "trader": tr.trader.label().to_lowercase(), "give": item_name(&tr.give), "give_qty": tr.give_qty, "get": item_name(&tr.receive), "get_qty": tr.receive_qty }))
            .collect();
        // What no trade covers, by reason: with nothing spare of a kind, that's every material of it.
        let mut still_short: Vec<(UnresolvedReason, Vec<String>)> = Vec::new();
        for u in &plan.unresolved {
            let item = ingredient(&Ingredient { symbol: u.symbol.clone(), count: u.qty });
            match still_short.iter_mut().find(|(r, _)| *r == u.reason) {
                Some((_, items)) => items.push(item),
                None => still_short.push((u.reason, vec![item])),
            }
        }
        let still_short: Vec<Value> = still_short.into_iter().map(|(r, items)| json!({ "why": why(r), "materials": items })).collect();
        let engineers = by_reach(gs, bp.engineers_for_grade(to).filter_map(|g| eng::engineer_by_name(&g.engineer).map(|e| (e, g.max_grade))));
        let fitted: Vec<Value> = fitted.into_iter().map(fitted_row).collect();
        notes.push(format!("A roll takes one set of its grade's materials and fills part of the grade; needed counts {rolls} roll{} at each grade.", if rolls == 1 { "" } else { "s" }));
        if !plan.trades.is_empty() {
            notes.push("Trades are made at material traders; the plan keeps a tenth of each material's storage cap and never gives away what it needs.".into());
        }
        if let Some(m) = &bp.merc_module {
            notes.push(format!("For the {m} bought with Merc Coins: it comes at G1."));
        }
        if engineers.is_empty() {
            notes.push(format!("No engineer offers G{to} of it."));
        }
        if gs.cmdr.name.is_empty() {
            notes.push("No commander loaded yet: have is 0 for everything.".into());
        }

        // What gives way when the reply is too long for the caller: how to unlock each engineer, the recipe for one roll
        // at each grade, the materials the commander has enough of, all but the first few engineers, what the blueprint
        // and the experimental do, the current ship's modules.
        const CUT: [&str; 6] = [
            "How to unlock each engineer left out to fit: ask with engineer= for one.",
            "The recipe for one roll at each grade left out to fit: needed has the totals.",
            "The materials the commander has enough of left out to fit.",
            "Only the first few engineers listed to fit.",
            "What the blueprint and the experimental effect do left out to fit.",
            "The current ship's modules of this type left out to fit.",
        ];
        let row = |(s, n): &&(String, u32)| {
            let h = have(s);
            let (kind, grade) = material_kind(s);
            let mut v = json!({ "name": item_name(s), "kind": kind, "grade": grade, "need": n, "have": h, "short": (h < *n).then(|| n - h) });
            strip_nulls(&mut v);
            v
        };
        let (out, _) = fit(self.budget, CUT.len() + 1, |level| {
            let mut out = json!({
                "module": t, "blueprint": bp.name, "from_grade": from, "to_grade": to, "rolls_per_grade": rolls,
                "verdict": verdict, "materials_in_hand": format!("{in_hand} of {total}"),
            });
            if level < 5 {
                out["effects_at_grade"] = json!(bp.effects.get(usize::from(to).saturating_sub(1)).map(Vec::as_slice).map(effects).unwrap_or_default());
            }
            if let Some(x) = x {
                out["experimental"] = json!({ "name": x.name, "cost": x.cost.iter().map(ingredient).collect::<Vec<_>>() });
                if level < 5 {
                    out["experimental"]["effects"] = json!(effects(&x.effects));
                }
            }
            if level < 2 && from <= to {
                out["per_roll"] = (from..=to).filter_map(|g| bp.grade(g).filter(|r| !r.is_empty()).map(|r| json!({ "grade": g, "materials": r.iter().map(ingredient).collect::<Vec<_>>() }))).collect();
            }
            let mut needed: Vec<Value> = short.iter().map(row).collect();
            if level < 3 {
                needed.extend(enough.iter().map(row));
            } else if !enough.is_empty() {
                out["enough_of"] = json!(enough.len());
            }
            out["needed"] = json!(needed);
            if coins > 0 {
                out["merc_coins"] = json!({ "need": coins, "have": balance });
                strip_nulls(&mut out["merc_coins"]);
            }
            if !trades.is_empty() {
                out["trades"] = json!(trades);
            }
            if !still_short.is_empty() {
                out["still_short"] = json!(still_short);
            }
            let shown = if level >= 4 { FEW_ENGINEERS.min(engineers.len()) } else { engineers.len() };
            out["engineers"] = engineers[..shown].iter().map(|(e, g)| engineer_row(gs, e, *g, level < 1)).collect();
            if shown < engineers.len() {
                out["engineers_omitted"] = json!(engineers.len() - shown);
            }
            if level < 6 && !fitted.is_empty() {
                out["current_ship"] = json!(fitted);
            }
            if let Some(e) = engineer {
                out["engineer"] = engineer_detail(gs, e);
            }
            let mut all: Vec<&str> = notes.iter().map(String::as_str).collect();
            all.extend(&CUT[..level]);
            out["notes"] = json!(all);
            out
        });
        Ok(out)
    }
}

/// One engineer: where, what they offer, the commander's standing and what unlocking them takes.
fn engineer_detail(gs: &GameState, e: &Engineer) -> Value {
    let status = gs.cmdr.engineers.get(&e.id);
    let text = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    let mut v = json!({
        "name": e.name,
        "works_on": if e.kind == EngineerKind::OnFoot { "suits and hand weapons" } else { "ship modules" },
        "system": e.system, "workshop": text(&e.base), "body": text(&e.body),
        "distance_ly": gs.location.star_pos.map(|p| r1(e.distance_to(p))),
        "permit_needed": e.permit.then_some(true), "in_colonia": (e.region == Region::Colonia).then_some(true),
        "status": status.map(|s| s.progress.as_str()),
        "engineer_rank": status.and_then(|s| s.rank),
        "rank_progress_pct": status.and_then(|s| s.rank_progress),
        "to_unlock": to_unlock(e, status.map(|s| s.progress.as_str())),
        "unlock_cost": (!e.unlock_cost.is_empty()).then(|| e.unlock_cost.iter().map(ingredient).collect::<Vec<_>>()),
        "reputation": text(&e.reputation),
        "referral": text(&e.referral),
    });
    if e.kind == EngineerKind::OnFoot {
        v["modifications"] = e.mods.iter().map(|s| json!(eng::odyssey_mod(s).map_or_else(|| s.clone(), |m| m.name.clone()))).collect();
    } else {
        v["modules"] = e.modules.iter().map(|m| json!(format!("{} up to G{}", m.module, m.max_grade))).collect();
    }
    strip_nulls(&mut v);
    if status.is_none() {
        v["notes"] = json!(["No status: the commander's journal doesn't list this engineer yet, so they haven't heard of them."]);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_types_are_found_the_way_commanders_name_them() {
        assert_eq!(module_type("Frame Shift Drive"), Ok("Frame Shift Drive"));
        assert_eq!(module_type("fsd"), Ok("Frame Shift Drive"));
        assert_eq!(module_type("int_hyperdrive_size5_class5"), Ok("Frame Shift Drive"));
        assert_eq!(module_type("shield boosters"), Ok("Shield Booster"));
        assert_eq!(module_type("armor"), Ok("Armour"));
        assert_eq!(module_type("wake scanner"), Ok("Frame Shift Wake Scanner"));
        assert!(module_type("frame shift").unwrap_err().contains("could be"), "three types share the words");
        assert!(module_type("toaster").unwrap_err().contains("Thrusters"), "the error lists the types");
    }

    #[test]
    fn blueprints_are_found_by_name_symbol_or_words() {
        let fsd = Some("Frame Shift Drive");
        let symbol = |r: Result<&Blueprint, String>| r.map(|b| b.symbol.clone());
        assert_eq!(symbol(find_blueprint(fsd, "Increased FSD Range")), Ok("FSD_LongRange".into()));
        assert_eq!(symbol(find_blueprint(fsd, "fsd_longrange")), Ok("FSD_LongRange".into()));
        assert_eq!(symbol(find_blueprint(fsd, "increased range")), Ok("FSD_LongRange".into()));
        // Unique names need no module; generic ones do.
        assert_eq!(find_blueprint(None, "Dirty Drive Tuning").map(|b| b.module.clone()), Ok("Thrusters".into()));
        let generic = find_blueprint(None, "Overcharged").unwrap_err();
        assert!(generic.contains("say which with module=") && generic.contains("Multi-cannon"), "{generic}");
        assert!(find_blueprint(fsd, "Dirty Drive Tuning").unwrap_err().contains("Its blueprints"));
        assert_eq!(find_experimental("Frame Shift Drive", "mass manager").map(|x| x.name.clone()), Ok("Mass Manager".into()));
        assert!(find_experimental("Frame Shift Drive", "incendiary rounds").unwrap_err().contains("Mass Manager"), "the error lists the module's effects");
        assert_eq!(find_engineer("Farseer").map(|e| e.name.clone()), Ok("Felicity Farseer".into()));
        assert!(find_engineer("Tod McQuinn").is_ok());
        assert!(find_engineer("Nobody Atall").unwrap_err().contains("Felicity Farseer"), "the error lists the engineers");
    }

    #[test]
    fn grades_are_read_as_written() {
        let g = |v: Value| arg_grade(&json!({ "grade": v }), "grade");
        assert_eq!((g(json!(3)), g(json!("G4")), g(json!("5")), g(json!(9)), g(json!(0)), g(json!("x"))), (Some(3), Some(4), Some(5), Some(5), None, None));
    }

    #[test]
    fn unlocking_starts_where_the_commander_stands() {
        let e = eng::engineer_by_name("Felicity Farseer").unwrap();
        let unknown = to_unlock(e, None).unwrap();
        assert!(unknown.starts_with("Learn of them: ") && unknown.contains(e.meeting.trim()) && unknown.contains(e.unlock.trim()), "{unknown}");
        assert_eq!(to_unlock(e, Some("Invited")), Some(format!("Unlock: {}", e.unlock.trim())));
        assert_eq!(to_unlock(e, Some("Unlocked")), None);
        // Every step ends its sentence, whether the data has a full stop or not.
        let elvira = eng::engineer_by_name("Elvira Martuuk").unwrap();
        assert!(!to_unlock(elvira, None).unwrap().contains(". .") && !to_unlock(elvira, None).unwrap().contains("knowledge Attain"));
    }
}
