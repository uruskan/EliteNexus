//! What the commander did over a period, from the journals Elite Nexus has read (the game's own lifetime totals are
//! [`crate::career`]): money in and out by category, time in the game, travel, combat, exploration, exobiology,
//! mining and missions, in all and per day; and the records the journals hold.
//!
//! Trade counts by its profit (what a sale fetched over what the cargo cost, the game's own figure), not by the sale:
//! a trader who buys for 100 M and sells for 110 M earned 10 M, rather than 110 M while spending 100 M. Cargo bought
//! and not sold in the period (delivered to a construction or a mission, lost, or still aboard) is spending, at cost.

use crate::exploration::BodyKind;
use crate::history::{Deed, LedgerCategory};
use crate::missions::MissionOutcome;
use crate::state::GameState;
use nexus_journal::time::LocalDays;
use nexus_journal::Timestamp;
use std::collections::{BTreeMap, HashSet};

/// A stretch of time: from `start`, up to but not including `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Window {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl Window {
    /// The last `days` days, up to and including `now`.
    pub fn last_days(days: u32, now: Timestamp) -> Window {
        Window { start: Timestamp(now.0 - i64::from(days) * 86_400), end: Timestamp(now.0 + 1) }
    }

    /// Everything up to and including `now`.
    pub fn until(now: Timestamp) -> Window {
        Window { start: Timestamp(i64::MIN / 2), end: Timestamp(now.0 + 1) }
    }

    pub fn contains(&self, t: Timestamp) -> bool {
        self.start <= t && t < self.end
    }

    /// As long as this one, just before it.
    pub fn before(&self) -> Window {
        Window { start: Timestamp(self.start.0.saturating_sub(self.end.0.saturating_sub(self.start.0))), end: self.start }
    }

    /// How much of `start..end` falls in this window, in seconds.
    pub fn overlap(&self, start: Timestamp, end: Timestamp) -> i64 {
        (end.0.min(self.end.0) - start.0.max(self.start.0)).max(0)
    }
}

/// Credits in or out of one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flow {
    pub category: LedgerCategory,
    /// Above zero, in or out.
    pub amount: i64,
}

/// Where credits came from and went over a period.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Money {
    /// Credits in, by category, biggest first. Trade is the profit on what was sold.
    pub income: Vec<Flow>,
    /// Credits out, by category, biggest first. Trade is there only for sales that lost money overall.
    pub spending: Vec<Flow>,
    /// Cargo bought in the period and not sold in it, at what it cost: delivered to a construction or a mission, lost,
    /// or still aboard. Spending, apart from the categories.
    pub unsold_cargo: i64,
    /// All the cargo bought in the period.
    pub cargo_bought: i64,
    /// Credits moved to (below zero) or from your own fleet carrier: they stay yours, so neither in nor out.
    pub moved: i64,
}

impl Money {
    pub fn income_total(&self) -> i64 {
        self.income.iter().map(|f| f.amount).sum()
    }

    pub fn spending_total(&self) -> i64 {
        self.spending.iter().map(|f| f.amount).sum::<i64>() + self.unsold_cargo
    }

    pub fn net(&self) -> i64 {
        self.income_total() - self.spending_total()
    }

    /// What was earned: the income, less what selling ships and modules brought in (that turns what you own into
    /// credits rather than earning them).
    pub fn earned(&self) -> i64 {
        self.income.iter().filter(|f| earning(f.category)).map(|f| f.amount).sum()
    }
}

/// Whether income of this kind is earned, rather than what you own sold off.
pub fn earning(category: LedgerCategory) -> bool {
    !matches!(category, LedgerCategory::Ships | LedgerCategory::Outfitting)
}

/// Credits in and out at `ts` the way the statistics count them: the ledger, except trade, which counts by the profit
/// on each sale. Calls `add(ts, category, amount)` for each; amounts below zero are spending (and a trade loss).
fn flows(gs: &GameState, w: Window, mut add: impl FnMut(Timestamp, LedgerCategory, i64)) {
    let h = &gs.history;
    for e in h.ledger.iter().filter(|e| w.contains(e.ts)) {
        // Transfers stay yours; cargo bought counts through the profit on its sale.
        if e.category != LedgerCategory::Transfer && e.category != LedgerCategory::Trade {
            add(e.ts, e.category, e.amount);
        }
    }
    for d in h.deeds.iter().filter(|d| w.contains(d.ts)) {
        if let Deed::Sale { profit, mined: false, .. } = d.deed {
            add(d.ts, LedgerCategory::Trade, profit);
        }
    }
}

/// Where credits came from and went over `w`.
pub fn money(gs: &GameState, w: Window) -> Money {
    let mut by: BTreeMap<LedgerCategory, (i64, i64)> = BTreeMap::new();
    let mut trade = 0;
    flows(gs, w, |_, cat, amount| {
        if cat == LedgerCategory::Trade {
            trade += amount;
        } else {
            let e = by.entry(cat).or_default();
            if amount >= 0 {
                e.0 += amount;
            } else {
                e.1 -= amount;
            }
        }
    });
    // Trade nets its sales: a profit is earned, a loss spent.
    if trade != 0 {
        let e = by.entry(LedgerCategory::Trade).or_default();
        if trade > 0 {
            e.0 += trade;
        } else {
            e.1 -= trade;
        }
    }
    let list = |pick: fn(&(i64, i64)) -> i64| {
        let mut v: Vec<Flow> = by.iter().map(|(c, a)| Flow { category: *c, amount: pick(a) }).filter(|f| f.amount > 0).collect();
        v.sort_by_key(|f| std::cmp::Reverse(f.amount));
        v
    };
    let mut m = Money { income: list(|a| a.0), spending: list(|a| a.1), ..Default::default() };
    for e in gs.history.ledger.iter().filter(|e| w.contains(e.ts)) {
        match e.category {
            LedgerCategory::Transfer => m.moved += e.amount,
            LedgerCategory::Trade if e.amount < 0 => m.cargo_bought -= e.amount,
            _ => {}
        }
    }
    // What the cargo sold in the period had cost; the rest of what was bought didn't come back as a sale.
    let sold_cost: i64 = gs.history.deeds.iter().filter(|d| w.contains(d.ts)).map(|d| if let Deed::Sale { cost, mined: false, .. } = d.deed { cost } else { 0 }).sum();
    m.unsold_cargo = (m.cargo_bought - sold_cost).max(0);
    m
}

/// One calendar day's activity (the commander's own days, local time).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Day {
    pub jumps: u32,
    pub ly: f64,
    /// Earned that day, as [`Money::earned`] counts it (trade by its profit, so a day of losses can go below zero).
    pub earned: i64,
    pub play_secs: i64,
}

/// Activity per day over `w`, by day (days since 1970 on the commander's calendar, as [`LocalDays`] counts them). Days
/// with nothing to count aren't there.
pub fn days(gs: &GameState, w: Window) -> BTreeMap<i64, Day> {
    let h = &gs.history;
    let mut cal = LocalDays::default();
    let mut out: BTreeMap<i64, Day> = BTreeMap::new();
    for j in h.jumps.iter().filter(|j| !j.carrier && w.contains(j.ts)) {
        let d = out.entry(cal.day(j.ts)).or_default();
        d.jumps += 1;
        d.ly += j.dist;
    }
    flows(gs, w, |ts, cat, amount| {
        // A day's earnings: what it earned, and trade by its profit, so a sale at a loss counts against the day.
        // Spending doesn't, nor what selling ships and modules brought in.
        if (amount > 0 && earning(cat)) || cat == LedgerCategory::Trade {
            out.entry(cal.day(ts)).or_default().earned += amount;
        }
    });
    for p in &h.play {
        let (mut from, to) = (p.start.0.max(w.start.0), p.end.0.min(w.end.0));
        // A session over midnight counts on both days.
        while from < to {
            let day = cal.day(Timestamp(from));
            let midnight = LocalDays::instant((day + 1) as f64).0.max(from + 1);
            let upto = to.min(midnight);
            out.entry(day).or_default().play_secs += upto - from;
            from = upto;
        }
    }
    out.retain(|_, d| d.jumps > 0 || d.earned != 0 || d.play_secs > 0);
    out
}

/// What the commander did over a period.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub window: Window,
    pub money: Money,
    /// The period day by day (see [`days`]).
    pub days: BTreeMap<i64, Day>,
    /// Time in the game, in seconds; sessions that fall in the period, at least partly; calendar days with any.
    pub play_secs: i64,
    pub sessions: usize,
    pub days_played: usize,
    /// Hyperspace jumps in your own ship (not the carrier's), and their distance.
    pub jumps: usize,
    pub ly: f64,
    pub carrier_jumps: usize,
    /// Systems jumped to (your ship or your carrier), and of those, systems your journals had never seen before.
    pub systems: usize,
    pub first_visits: usize,
    pub docks: usize,
    pub stations: usize,
    /// Kills for bounties and for combat bonds (ships, skimmers, on foot), and what they were worth when made.
    pub kills: usize,
    pub bond_kills: usize,
    pub kill_rewards: i64,
    /// Deaths, in a ship or on foot.
    pub deaths: usize,
    /// Cargo sold that had cost something: sales, tonnes and the profit.
    pub sales: usize,
    pub tonnes_sold: i64,
    pub trade_profit: i64,
    /// Mining: sessions started in the period, tonnes refined, asteroids cracked and prospected; and tonnes of cargo
    /// that cost nothing sold (mined, but also salvaged, stolen or given for a mission: the journal can't tell them
    /// apart).
    pub mining_sessions: usize,
    pub refined: i64,
    pub cracked: u32,
    pub prospected: u32,
    pub mined_sold: i64,
    /// Stars and planets scanned in detail, of those first discoveries; planets mapped, first mapped, within target.
    pub scanned: usize,
    pub first_discoveries: usize,
    pub mapped: usize,
    pub first_mapped: usize,
    pub efficient: usize,
    /// New codex entries.
    pub codex_new: usize,
    /// Exploration data sold.
    pub data_sold: i64,
    /// Species analysed, what they should fetch, how many likely earn the first-logged bonus; organic data sold.
    pub organics: usize,
    pub organic_value: i64,
    pub organic_first: usize,
    pub organic_sold: i64,
    pub missions_completed: usize,
    pub missions_failed: usize,
    pub missions_abandoned: usize,
    pub mission_rewards: i64,
}

impl Summary {
    pub fn compute(gs: &GameState, w: Window) -> Summary {
        let h = &gs.history;
        let mut s = Summary { window: w, money: money(gs, w), days: days(gs, w), ..Default::default() };
        for p in &h.play {
            let secs = w.overlap(p.start, p.end);
            if secs > 0 || (w.contains(p.start) && p.secs() == 0) {
                s.play_secs += secs;
                s.sessions += 1;
            }
        }
        s.days_played = s.days.values().filter(|d| d.play_secs > 0).count();
        let mut systems: HashSet<u64> = HashSet::new();
        for j in h.jumps.iter().filter(|j| w.contains(j.ts)) {
            systems.insert(j.address);
            if j.carrier {
                s.carrier_jumps += 1;
            } else {
                s.jumps += 1;
                s.ly += j.dist;
            }
        }
        s.systems = systems.len();
        // New among those jumped to (a system first seen at a login isn't one you arrived in).
        s.first_visits = systems.iter().filter(|a| h.visited.get(a).is_some_and(|v| w.contains(v.first))).count();
        let docks: Vec<_> = h.docks.iter().filter(|d| w.contains(d.ts)).collect();
        s.docks = docks.len();
        s.stations = docks.iter().map(|d| d.market_id).collect::<HashSet<_>>().len();
        s.deaths = h.deaths.iter().filter(|d| w.contains(d.ts)).count();
        for d in h.deeds.iter().filter(|d| w.contains(d.ts)) {
            match d.deed {
                Deed::Kill { reward, bond } => {
                    s.kills += 1;
                    s.bond_kills += usize::from(bond);
                    s.kill_rewards += reward;
                }
                Deed::Scan { first } => {
                    s.scanned += 1;
                    s.first_discoveries += usize::from(first);
                }
                Deed::Map { first, efficient } => {
                    s.mapped += 1;
                    s.first_mapped += usize::from(first);
                    s.efficient += usize::from(efficient);
                }
                Deed::Organic { value, first } => {
                    s.organics += 1;
                    s.organic_value += value;
                    s.organic_first += usize::from(first);
                }
                Deed::Sale { tonnes, profit, mined, .. } => {
                    if mined {
                        s.mined_sold += tonnes;
                    } else {
                        s.sales += 1;
                        s.tonnes_sold += tonnes;
                        s.trade_profit += profit;
                    }
                }
            }
        }
        for m in gs.mining.history.iter().chain(gs.mining.session.as_ref()).filter(|m| w.contains(m.start)) {
            s.mining_sessions += 1;
            s.refined += i64::from(m.total_refined());
            s.cracked += m.cracked;
            s.prospected += m.prospected;
        }
        s.codex_new = gs.exploration.codex.iter().filter(|c| c.new_entry && w.contains(c.ts)).count();
        s.data_sold = gs.exploration.sales.iter().filter(|x| w.contains(x.ts)).map(|x| x.earnings).sum();
        s.organic_sold = gs.exobio.sales.iter().filter(|(t, _)| w.contains(*t)).map(|(_, v)| v).sum();
        for m in gs.missions.history.iter().filter(|m| w.contains(m.ts)) {
            match m.outcome {
                MissionOutcome::Completed => {
                    s.missions_completed += 1;
                    s.mission_rewards += m.reward;
                }
                MissionOutcome::Failed => s.missions_failed += 1,
                MissionOutcome::Abandoned => s.missions_abandoned += 1,
            }
        }
        s
    }

    /// Nothing at all happened in the period, as far as the journals go.
    pub fn is_empty(&self) -> bool {
        self.play_secs == 0
            && self.jumps == 0
            && self.carrier_jumps == 0
            && self.docks == 0
            && self.money.income.is_empty()
            && self.money.spending.is_empty()
            && self.money.cargo_bought == 0
            && self.kills == 0
            && self.scanned == 0
    }
}

/// A personal best from the journals: how much, what or where, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct Feat {
    pub value: f64,
    pub what: String,
    pub ts: Timestamp,
}

/// The records your journals hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Records {
    /// The most earned in one calendar day (trade by its profit).
    pub best_day: Option<Feat>,
    /// The biggest single payment: a mission, a data sale, bounties handed in (trade apart).
    pub biggest_payment: Option<Feat>,
    /// The best profit on one sale of cargo.
    pub best_trade: Option<Feat>,
    pub best_mission: Option<Feat>,
    pub longest_jump: Option<Feat>,
    pub farthest_from_sol: Option<Feat>,
    /// The most jumps in one calendar day.
    pub busiest_day: Option<Feat>,
    pub longest_session: Option<Feat>,
    /// The scanned body worth most once mapped.
    pub best_find: Option<Feat>,
}

fn best<T>(items: impl Iterator<Item = T>, value: impl Fn(&T) -> f64, feat: impl Fn(T, f64) -> Feat) -> Option<Feat> {
    let mut top: Option<(f64, T)> = None;
    for it in items {
        let v = value(&it);
        if v > 0.0 && top.as_ref().is_none_or(|(b, _)| v > *b) {
            top = Some((v, it));
        }
    }
    top.map(|(v, it)| feat(it, v))
}

/// A ledger note without the profit the ledger adds to a sale ("Sold 720 Gold (profit 1,234)" reads "Sold 720 Gold").
fn note(text: &str) -> &str {
    text.split(" (profit").next().unwrap_or(text)
}

/// What a sale at `ts` was, from the ledger.
fn sale_note(gs: &GameState, ts: Timestamp) -> String {
    gs.history.ledger.iter().rev().find(|e| e.ts == ts && e.category == LedgerCategory::Trade && e.amount > 0).map(|e| note(&e.note).to_string()).unwrap_or_default()
}

impl Records {
    pub fn compute(gs: &GameState, now: Timestamp) -> Records {
        let h = &gs.history;
        let all = days(gs, Window::until(now));
        let day_start = |day: i64| LocalDays::instant(day as f64);
        Records {
            best_day: best(all.iter(), |(_, d)| d.earned as f64, |(day, _), v| Feat { value: v, what: String::new(), ts: day_start(*day) }),
            busiest_day: best(all.iter(), |(_, d)| f64::from(d.jumps), |(day, d), v| Feat { value: v, what: format!("{:.0} ly", d.ly), ts: day_start(*day) }),
            biggest_payment: best(
                h.ledger.iter().filter(|e| !matches!(e.category, LedgerCategory::Trade | LedgerCategory::Transfer | LedgerCategory::Ships | LedgerCategory::Outfitting)),
                |e| e.amount as f64,
                |e, v| Feat { value: v, what: format!("{}: {}", e.category.label(), note(&e.note)), ts: e.ts },
            ),
            best_trade: best(
                h.deeds.iter().filter_map(|d| match d.deed {
                    Deed::Sale { profit, mined: false, .. } => Some((d.ts, profit)),
                    _ => None,
                }),
                |(_, p)| *p as f64,
                |(ts, _), v| Feat { value: v, what: sale_note(gs, ts), ts },
            ),
            best_mission: best(
                gs.missions.history.iter().filter(|m| m.outcome == MissionOutcome::Completed),
                |m| m.reward as f64,
                |m, v| Feat { value: v, what: m.title.clone(), ts: m.ts },
            ),
            longest_jump: best(h.jumps.iter().filter(|j| !j.carrier), |j| j.dist, |j, v| Feat { value: v, what: j.system.clone(), ts: j.ts }),
            farthest_from_sol: best(
                h.jumps.iter().filter_map(|j| j.pos.map(|p| (j, (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()))),
                |(_, d)| *d,
                |(j, _), v| Feat { value: v, what: j.system.clone(), ts: j.ts },
            ),
            longest_session: best(h.play.iter(), |p| p.secs() as f64, |p, v| Feat { value: v, what: String::new(), ts: p.start }),
            best_find: best(
                // Not a nav beacon's data: that's someone else's scan.
                gs.exploration.systems.values().flat_map(|s| s.bodies.values()).filter(|b| b.has_scan && b.scan_type != "NavBeaconDetail" && matches!(b.kind, BodyKind::Star | BodyKind::Planet)),
                |b| b.value.mapped.max(b.value.current) as f64,
                |b, v| Feat { value: v, what: b.name.clone(), ts: b.scanned },
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Alerts, Model};
    use nexus_journal::{raw::scan_line, Event, EventCtx};

    fn feed(model: &mut Model, lines: &[&str]) {
        let mut alerts = Alerts::default();
        for line in lines {
            let (ts, name) = scan_line(line).expect("valid journal line");
            let ev = Event::parse(name, line).expect("typed parse");
            model.apply(&EventCtx { ts, name, line, live: false }, &ev, &mut alerts);
        }
    }

    fn at(s: &str) -> Timestamp {
        Timestamp::parse(s).expect("timestamp")
    }

    /// Four sessions over four days (around midday UTC, so each falls on one calendar day in any time zone that's
    /// within eleven hours of UTC): a day of trading, mining, fighting and a mission that ends at the main menu; a short
    /// session that buys steel for a construction and closes the game; one that crashes (no last line); and one whose
    /// journal goes on in a second part.
    const JOURNAL: &[&str] = &[
        r#"{ "timestamp":"2026-10-01T10:00:00Z", "event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r1 " }"#,
        r#"{ "timestamp":"2026-10-01T10:00:01Z", "event":"Commander", "FID":"F1", "Name":"Test" }"#,
        r#"{ "timestamp":"2026-10-01T10:00:02Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Ship":"explorer_nx", "ShipID":22, "GameMode":"Solo", "Credits":1000000, "Loan":0 }"#,
        r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":63.0 }"#,
        r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"FSDJump", "StarSystem":"Gamma", "SystemAddress":300, "StarPos":[20.0,10.0,0.0], "Population":5000, "JumpDist":14.1, "FuelUsed":1.0, "FuelLevel":62.0 }"#,
        r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"Docked", "StationName":"Port", "StationType":"Coriolis", "StarSystem":"Gamma", "SystemAddress":300, "MarketID":1 }"#,
        r#"{ "timestamp":"2026-10-01T10:21:00Z", "event":"MarketBuy", "MarketID":1, "Type":"gold", "Type_Localised":"Gold", "Count":100, "BuyPrice":10000, "TotalCost":1000000 }"#,
        r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"MarketSell", "MarketID":1, "Type":"gold", "Type_Localised":"Gold", "Count":100, "SellPrice":12000, "TotalSale":1200000, "AvgPricePaid":10000 }"#,
        r#"{ "timestamp":"2026-10-01T10:31:00Z", "event":"MarketSell", "MarketID":1, "Type":"painite", "Type_Localised":"Painite", "Count":10, "SellPrice":300000, "TotalSale":3000000, "AvgPricePaid":0 }"#,
        r#"{ "timestamp":"2026-10-01T10:32:00Z", "event":"RepairAll", "Cost":50000 }"#,
        r#"{ "timestamp":"2026-10-01T10:33:00Z", "event":"CarrierBankTransfer", "CarrierID":7, "Deposit":1000000, "PlayerBalance":3650000, "CarrierBalance":1000000 }"#,
        r#"{ "timestamp":"2026-10-01T10:40:00Z", "event":"Bounty", "Rewards":[ { "Faction":"A", "Reward":50000 } ], "Target":"anaconda", "TotalReward":50000, "VictimFaction":"B" }"#,
        r#"{ "timestamp":"2026-10-01T10:41:00Z", "event":"FactionKillBond", "Reward":20000, "AwardingFaction":"A", "VictimFaction":"B" }"#,
        r#"{ "timestamp":"2026-10-01T10:50:00Z", "event":"MissionCompleted", "Faction":"A", "Name":"Mission_Delivery", "LocalisedName":"Deliver 12 units of Gold", "MissionID":9, "Reward":500000 }"#,
        r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"Music", "MusicTrack":"MainMenu" }"#,
        r#"{ "timestamp":"2026-10-02T12:00:00Z", "event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r1 " }"#,
        r#"{ "timestamp":"2026-10-02T12:00:01Z", "event":"Commander", "FID":"F1", "Name":"Test" }"#,
        r#"{ "timestamp":"2026-10-02T12:00:02Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Ship":"explorer_nx", "ShipID":22, "GameMode":"Solo", "Credits":3650000, "Loan":0 }"#,
        r#"{ "timestamp":"2026-10-02T12:10:00Z", "event":"MarketBuy", "MarketID":1, "Type":"steel", "Type_Localised":"Steel", "Count":300, "BuyPrice":10000, "TotalCost":3000000 }"#,
        r#"{ "timestamp":"2026-10-02T12:20:00Z", "event":"ColonisationContribution", "MarketID":5, "Contributions":[ { "Name":"$Steel_name;", "Name_Localised":"Steel", "Amount":300 } ] }"#,
        r#"{ "timestamp":"2026-10-02T12:30:02Z", "event":"Shutdown" }"#,
        r#"{ "timestamp":"2026-10-03T12:00:00Z", "event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r1 " }"#,
        r#"{ "timestamp":"2026-10-03T12:00:05Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Ship":"explorer_nx", "ShipID":22, "GameMode":"Solo", "Credits":3650000, "Loan":0 }"#,
        r#"{ "timestamp":"2026-10-03T12:10:05Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":20.5, "FuelUsed":1.0, "FuelLevel":63.0 }"#,
        r#"{ "timestamp":"2026-10-04T12:00:00Z", "event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r1 " }"#,
        r#"{ "timestamp":"2026-10-04T12:00:01Z", "event":"Commander", "FID":"F1", "Name":"Test" }"#,
        r#"{ "timestamp":"2026-10-04T12:00:10Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Ship":"explorer_nx", "ShipID":22, "GameMode":"Solo", "Credits":650000, "Loan":0 }"#,
        r#"{ "timestamp":"2026-10-04T12:10:10Z", "event":"Fileheader", "part":2, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r1 " }"#,
        r#"{ "timestamp":"2026-10-04T12:20:10Z", "event":"Shutdown" }"#,
    ];

    fn model() -> Model {
        let mut m = Model::default();
        feed(&mut m, JOURNAL);
        m
    }

    #[test]
    fn sessions_run_from_login_to_the_main_menu_or_the_last_line() {
        let m = model();
        let gs = m.active().expect("commander");
        let spans: Vec<(Timestamp, i64)> = gs.history.play.iter().map(|p| (p.start, p.secs())).collect();
        assert_eq!(
            spans,
            [
                // To the main menu.
                (at("2026-10-01T10:00:02Z"), 3_598),
                // To the game closing.
                (at("2026-10-02T12:00:02Z"), 1_800),
                // A crash: to its last line, not to the next day's start.
                (at("2026-10-03T12:00:05Z"), 600),
                // A journal's second part goes on with the same session.
                (at("2026-10-04T12:00:10Z"), 1_200),
            ]
        );
        assert_eq!(gs.history.first(), Some(at("2026-10-01T10:00:02Z")));
    }

    #[test]
    fn a_period_counts_trade_by_its_profit() {
        let m = model();
        let gs = m.active().expect("commander");
        let day1 = Window { start: at("2026-10-01T00:00:00Z"), end: at("2026-10-02T00:00:00Z") };
        let money = money(gs, day1);
        let income: Vec<(LedgerCategory, i64)> = money.income.iter().map(|f| (f.category, f.amount)).collect();
        // Painite mined (3 M), the mission (500 k), and gold's profit (200 k), not its 1.2 M sale.
        assert_eq!(income, [(LedgerCategory::Mining, 3_000_000), (LedgerCategory::Missions, 500_000), (LedgerCategory::Trade, 200_000)]);
        assert_eq!(money.spending.iter().map(|f| (f.category, f.amount)).collect::<Vec<_>>(), [(LedgerCategory::Repairs, 50_000)]);
        assert_eq!(money.cargo_bought, 1_000_000);
        assert_eq!(money.unsold_cargo, 0, "the gold was sold: its profit took its cost off");
        assert_eq!(money.moved, -1_000_000, "credits put on your own carrier stay yours");
        assert_eq!((money.net(), money.earned()), (3_650_000, 3_700_000));

        // Steel bought and delivered to a construction never comes back as a sale: it's spending, at cost.
        let day2 = Window { start: at("2026-10-02T00:00:00Z"), end: at("2026-10-03T00:00:00Z") };
        let money2 = super::money(gs, day2);
        assert_eq!((money2.cargo_bought, money2.unsold_cargo, money2.net(), money2.earned()), (3_000_000, 3_000_000, -3_000_000, 0));

        let s = Summary::compute(gs, day1);
        assert_eq!((s.jumps, s.systems, s.first_visits, s.docks, s.stations), (2, 2, 2, 1, 1));
        assert!((s.ly - 24.1).abs() < 1e-9);
        assert_eq!((s.kills, s.bond_kills, s.kill_rewards), (2, 1, 70_000));
        assert_eq!((s.sales, s.tonnes_sold, s.trade_profit, s.mined_sold), (1, 100, 200_000, 10));
        assert_eq!((s.missions_completed, s.mission_rewards), (1, 500_000));
        assert_eq!((s.sessions, s.play_secs, s.days_played), (1, 3_598, 1));
        assert!(!s.is_empty());

        // The four days: each session on its own day; the second visit to Beta isn't a first visit.
        let all = Window { start: at("2026-10-01T00:00:00Z"), end: at("2026-10-05T00:00:00Z") };
        let s = Summary::compute(gs, all);
        assert_eq!((s.sessions, s.play_secs, s.days_played), (4, 3_598 + 1_800 + 600 + 1_200, 4));
        assert_eq!((s.jumps, s.systems, s.first_visits), (3, 2, 2));
        assert_eq!(s.days.len(), 4);
        assert_eq!(s.days, days(gs, all));
        let first = s.days.values().next().expect("a day");
        assert_eq!((first.jumps, first.earned, first.play_secs), (2, 3_700_000, 3_598));
        assert_eq!(s.money.unsold_cargo, 3_000_000, "over the four days, only the steel wasn't sold");

        // Nothing before the journals.
        let before = all.before();
        assert!(Summary::compute(gs, before).is_empty());
        assert_eq!(before.end, all.start);
    }

    #[test]
    fn a_session_over_midnight_counts_on_both_days() {
        let mut gs = GameState::default();
        let mut cal = LocalDays::default();
        let day = cal.day(at("2026-10-10T12:00:00Z"));
        let midnight = LocalDays::instant((day + 1) as f64);
        gs.history.login(Timestamp(midnight.0 - 1_800));
        gs.history.played(Timestamp(midnight.0 + 600));
        gs.history.logout();
        // Lines after the session ended don't extend it.
        gs.history.played(Timestamp(midnight.0 + 7_200));
        let d = days(&gs, Window::until(Timestamp(midnight.0 + 86_400)));
        assert_eq!(d.get(&day).map(|x| x.play_secs), Some(1_800));
        assert_eq!(d.get(&(day + 1)).map(|x| x.play_secs), Some(600));
        // A window that cuts the session counts only its part.
        let w = Window { start: midnight, end: Timestamp(midnight.0 + 86_400) };
        assert_eq!(Summary::compute(&gs, w).play_secs, 600);
    }

    #[test]
    fn records_from_the_journals() {
        let m = model();
        let gs = m.active().expect("commander");
        let r = Records::compute(gs, at("2026-10-05T00:00:00Z"));
        let best_trade = r.best_trade.expect("a trade");
        assert_eq!((best_trade.value, best_trade.what.as_str()), (200_000.0, "Sold 100 Gold"));
        let payment = r.biggest_payment.expect("a payment");
        assert_eq!((payment.value, payment.what.as_str()), (3_000_000.0, "Mining: Sold 10 Painite"));
        assert_eq!(r.best_mission.map(|f| f.what), Some("Deliver 12 units of Gold".to_string()));
        let jump = r.longest_jump.expect("a jump");
        assert_eq!((jump.value, jump.what.as_str()), (20.5, "Beta"));
        let far = r.farthest_from_sol.expect("a position");
        assert_eq!(far.what, "Gamma");
        assert!((far.value - 500f64.sqrt()).abs() < 1e-9);
        assert_eq!(r.busiest_day.map(|f| f.value), Some(2.0));
        assert_eq!(r.best_day.map(|f| f.value), Some(3_700_000.0));
        assert_eq!(r.longest_session.map(|f| f.value), Some(3_598.0));
        assert!(r.best_find.is_none(), "nothing scanned");
    }
}
