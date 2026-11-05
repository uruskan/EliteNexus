//! What each of the commander's ships did over a period, from the journals Elite Nexus has read, through
//! [`History::aboard`](crate::history::History::aboard). Time, jumps, kills and deaths go to the ship the commander
//! was in; everything else to the ship it counts for, which is also the one they stepped out of while they're on foot
//! beside it (sampling on its planet, selling at Vista Genomics or handing in a mission where it's docked). What has no
//! ship (time on foot, in a taxi or a dropship, crewing another commander's ship, a kill on foot) is summed apart.
//!
//! Earnings go to the ship they were made with: a sale's profit to the ship that sold the cargo, a bounty or a combat
//! bond to the ship that made the kill, when it was made (a voucher can still be lost before it's handed in), data and
//! mission rewards to the ship the commander was paid with.

use crate::history::{Deed, LedgerCategory};
use crate::period::Window;
use crate::state::GameState;
use nexus_journal::Timestamp;
use std::collections::BTreeMap;

/// What one ship did over a period; with `ship: None`, what the commander did out of their ships.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShipSummary {
    pub ship: Option<u64>,
    /// Time in the game in it, in seconds.
    pub play_secs: i64,
    pub jumps: usize,
    pub ly: f64,
    /// Profit on the cargo it sold (below zero for sales that lost money overall).
    pub trade: i64,
    /// What its kills were worth when made: bounties and combat bonds.
    pub combat: i64,
    /// Exploration and organic data sold while in it (the scans may have been made in another ship).
    pub exploration: i64,
    pub exobiology: i64,
    /// Cargo it sold that cost nothing: mined, but also salvaged, stolen or given for a mission.
    pub mining: i64,
    /// Mission rewards handed in while in it.
    pub missions: i64,
    pub kills: usize,
    pub deaths: usize,
    pub rebuys: i64,
    pub fuel: i64,
    pub repairs: i64,
    /// Ammunition, limpets and restocking.
    pub ammo: i64,
    /// Stars and planets scanned, planets mapped, species analysed.
    pub scanned: usize,
    pub mapped: usize,
    pub organics: usize,
    /// Tonnes refined in mining sessions started in it.
    pub refined: i64,
}

impl ShipSummary {
    /// Everything it earned (trade when it made a profit).
    pub fn earned(&self) -> i64 {
        self.trade.max(0) + self.combat + self.exploration + self.exobiology + self.mining + self.missions
    }

    /// What it cost: fuel, repairs, ammunition and restocking, rebuys, and trade that lost money.
    pub fn costs(&self) -> i64 {
        self.fuel + self.repairs + self.ammo + self.rebuys + (-self.trade).max(0)
    }

    /// Nothing to show for the period.
    pub fn is_empty(&self) -> bool {
        self.play_secs == 0
            && self.jumps == 0
            && self.earned() == 0
            && self.costs() == 0
            && self.kills == 0
            && self.deaths == 0
            && self.scanned == 0
            && self.mapped == 0
            && self.organics == 0
            && self.refined == 0
    }
}

fn slot(by: &mut BTreeMap<Option<u64>, ShipSummary>, ship: Option<u64>) -> &mut ShipSummary {
    by.entry(ship).or_insert_with(|| ShipSummary { ship, ..Default::default() })
}

/// What each ship did over `w`, and what was done out of them (`ship: None`), leaving out what has nothing to show:
/// most time in the game first.
pub fn per_ship(gs: &GameState, w: Window) -> Vec<ShipSummary> {
    let h = &gs.history;
    let mut by: BTreeMap<Option<u64>, ShipSummary> = BTreeMap::new();
    // Time: each session in the period, split where the commander changed ships or got out.
    for p in &h.play {
        let (from, to) = (p.start.0.max(w.start.0), p.end.0.min(w.end.0));
        if from >= to {
            continue;
        }
        let mut t = from;
        let mut ship = h.aboard_at(Timestamp(from));
        let next = h.aboard.partition_point(|a| a.ts.0 <= from);
        for a in &h.aboard[next..] {
            if a.ts.0 >= to {
                break;
            }
            slot(&mut by, ship).play_secs += a.ts.0 - t;
            t = a.ts.0;
            ship = a.ship;
        }
        slot(&mut by, ship).play_secs += to - t;
    }
    for j in h.jumps.iter().filter(|j| !j.carrier && w.contains(j.ts)) {
        let s = slot(&mut by, h.aboard_at(j.ts));
        s.jumps += 1;
        s.ly += j.dist;
    }
    for d in h.deaths.iter().filter(|d| w.contains(d.ts)) {
        slot(&mut by, h.aboard_at(d.ts)).deaths += 1;
    }
    for e in h.ledger.iter().filter(|e| w.contains(e.ts)) {
        let s = slot(&mut by, h.credit_at(e.ts));
        match (e.category, e.amount > 0) {
            (LedgerCategory::Exploration, true) => s.exploration += e.amount,
            (LedgerCategory::Exobiology, true) => s.exobiology += e.amount,
            (LedgerCategory::Mining, true) => s.mining += e.amount,
            (LedgerCategory::Missions, true) => s.missions += e.amount,
            (LedgerCategory::Rebuy, false) => s.rebuys -= e.amount,
            (LedgerCategory::Fuel, false) => s.fuel -= e.amount,
            (LedgerCategory::Repairs, false) => s.repairs -= e.amount,
            (LedgerCategory::Ammo, false) => s.ammo -= e.amount,
            _ => {}
        }
    }
    for d in h.deeds.iter().filter(|d| w.contains(d.ts)) {
        // A kill is the ship's only when it made it (on foot beside it, it isn't).
        let ship = if matches!(d.deed, Deed::Kill { .. }) { h.aboard_at(d.ts) } else { h.credit_at(d.ts) };
        let s = slot(&mut by, ship);
        match d.deed {
            Deed::Kill { reward, .. } => {
                s.kills += 1;
                s.combat += reward;
            }
            Deed::Scan { .. } => s.scanned += 1,
            Deed::Map { .. } => s.mapped += 1,
            Deed::Organic { .. } => s.organics += 1,
            Deed::Sale { profit, mined: false, .. } => s.trade += profit,
            Deed::Sale { .. } => {}
        }
    }
    for m in gs.mining.history.iter().chain(gs.mining.session.as_ref()).filter(|m| w.contains(m.start)) {
        slot(&mut by, h.credit_at(m.start)).refined += i64::from(m.total_refined());
    }
    let mut out: Vec<ShipSummary> = by.into_values().filter(|s| !s.is_empty()).collect();
    out.sort_by(|a, b| b.play_secs.cmp(&a.play_secs).then(b.earned().cmp(&a.earned())));
    out
}

/// What a ship is and is called: (type, name, ident), from the fleet as it is now or, for a ship since sold, from the
/// history. Empty strings for what nobody wrote down.
pub fn ship_names(gs: &GameState, id: u64) -> (String, String, String) {
    if let Some(s) = gs.fleet.ships.get(&id) {
        return (s.type_name.clone(), s.name.clone(), s.ident.clone());
    }
    match gs.history.ships.get(&id) {
        Some(r) => {
            let ty = if r.type_name.is_empty() { nexus_data::ships::ship_name(&r.ship_type) } else { r.type_name.clone() };
            (ty, r.name.clone(), r.ident.clone())
        }
        None => (String::new(), String::new(), String::new()),
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

    /// A session in a Cobra: a jump, a trade, a kill, a walk on the station where it's docked (organic data sold there
    /// counts for it, a kill on foot doesn't); then a Type-9 bought (the Cobra stored), a jump and a refuel in it, the
    /// Cobra sold remotely, and the Type-9 lost.
    const JOURNAL: &[&str] = &[
        r#"{ "timestamp":"2026-10-01T10:00:00Z", "event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r1 " }"#,
        r#"{ "timestamp":"2026-10-01T10:00:01Z", "event":"Commander", "FID":"F1", "Name":"Test" }"#,
        r#"{ "timestamp":"2026-10-01T10:00:02Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Ship":"cobramkiii", "ShipID":1, "ShipName":"Workhorse", "ShipIdent":"WH-01", "GameMode":"Solo", "Credits":5000000, "Loan":0 }"#,
        r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":15.0 }"#,
        r#"{ "timestamp":"2026-10-01T10:15:00Z", "event":"Bounty", "Rewards":[ { "Faction":"A", "Reward":40000 } ], "Target":"viper", "TotalReward":40000, "VictimFaction":"B" }"#,
        r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"Docked", "StationName":"Port", "StationType":"Coriolis", "StarSystem":"Beta", "SystemAddress":200, "MarketID":1 }"#,
        r#"{ "timestamp":"2026-10-01T10:21:00Z", "event":"MarketSell", "MarketID":1, "Type":"gold", "Type_Localised":"Gold", "Count":10, "SellPrice":12000, "TotalSale":120000, "AvgPricePaid":10000 }"#,
        r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"Disembark", "SRV":false, "Taxi":false, "Multicrew":false, "ID":1, "StarSystem":"Beta", "SystemAddress":200, "OnStation":true, "OnPlanet":false, "StationName":"Port", "MarketID":1 }"#,
        r#"{ "timestamp":"2026-10-01T10:35:00Z", "event":"Bounty", "Rewards":[ { "Faction":"A", "Reward":5000 } ], "Target":"scavenger", "TotalReward":5000, "VictimFaction":"B" }"#,
        r#"{ "timestamp":"2026-10-01T10:36:00Z", "event":"SellOrganicData", "MarketID":1, "BioData":[ { "Genus":"$Codex_Ent_Bacterial_Genus_Name;", "Species":"$Codex_Ent_Bacterial_01_Name;", "Species_Localised":"Bacterium Aurasus", "Value":1000000, "Bonus":0 } ] }"#,
        r#"{ "timestamp":"2026-10-01T10:40:00Z", "event":"Embark", "SRV":false, "Taxi":false, "Multicrew":false, "ID":1, "StarSystem":"Beta", "SystemAddress":200, "OnStation":true, "OnPlanet":false, "StationName":"Port", "MarketID":1 }"#,
        r#"{ "timestamp":"2026-10-01T10:50:00Z", "event":"ShipyardBuy", "ShipType":"type9", "ShipPrice":76000000, "StoreOldShip":"CobraMkIII", "StoreShipID":1, "MarketID":1 }"#,
        r#"{ "timestamp":"2026-10-01T10:50:05Z", "event":"ShipyardNew", "ShipType":"type9", "NewShipID":2 }"#,
        r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"FSDJump", "StarSystem":"Gamma", "SystemAddress":300, "StarPos":[20.0,0.0,0.0], "Population":0, "JumpDist":12.5, "FuelUsed":2.0, "FuelLevel":30.0 }"#,
        r#"{ "timestamp":"2026-10-01T11:05:00Z", "event":"RefuelAll", "Cost":800, "Amount":2.0 }"#,
        r#"{ "timestamp":"2026-10-01T11:10:00Z", "event":"ShipyardSell", "ShipType":"cobramkiii", "SellShipID":1, "ShipPrice":300000, "MarketID":1, "System":"Beta" }"#,
        r#"{ "timestamp":"2026-10-01T11:20:00Z", "event":"Died", "KillerName":"Pirate" }"#,
        r#"{ "timestamp":"2026-10-01T11:21:00Z", "event":"Resurrect", "Option":"rebuy", "Cost":3800000, "Bankrupt":false }"#,
        r#"{ "timestamp":"2026-10-01T11:30:00Z", "event":"Shutdown" }"#,
    ];

    #[test]
    fn each_ship_gets_what_was_done_in_it() {
        let mut m = Model::default();
        feed(&mut m, JOURNAL);
        let gs = m.active().expect("commander");
        let all = Window::until(at("2026-10-02T00:00:00Z"));
        let ships = per_ship(gs, all);
        let get = |s: Option<u64>| ships.iter().find(|x| x.ship == s).cloned().unwrap_or_default();
        let (cobra, type9, out) = (get(Some(1)), get(Some(2)), get(None));
        // The Cobra: 10:00:02 to 10:30 and 10:40 to 10:50:05; the Type-9 from then to the shutdown; on foot 10:30 to 10:40.
        assert_eq!((cobra.play_secs, type9.play_secs, out.play_secs), (1_798 + 605, 2_395, 600));
        assert_eq!((cobra.jumps, cobra.ly, type9.jumps, type9.ly), (1, 10.0, 1, 12.5));
        assert_eq!((cobra.trade, cobra.kills, cobra.combat), (20_000, 1, 40_000));
        assert_eq!((out.kills, out.combat), (1, 5_000), "a kill on foot isn't the ship's");
        assert_eq!((cobra.exobiology, out.exobiology), (1_000_000, 0), "organic data sold on foot beside the ship counts for it");
        assert_eq!((type9.fuel, type9.deaths, type9.rebuys), (800, 1, 3_800_000));
        assert_eq!((cobra.earned(), type9.costs()), (1_060_000, 3_800_800));
        // A trade at a loss is a cost, not negative earnings.
        let loss = ShipSummary { trade: -500, fuel: 100, ..Default::default() };
        assert_eq!((loss.earned(), loss.costs()), (0, 600));
        // Most time in the game first.
        assert_eq!(ships.first().and_then(|s| s.ship), Some(1));

        // Bought and sold, and the names kept after the sale.
        let cobra = gs.history.ships.get(&1).expect("the Cobra");
        assert_eq!((cobra.name.as_str(), cobra.ident.as_str()), ("Workhorse", "WH-01"));
        assert_eq!(cobra.sold, Some((at("2026-10-01T11:10:00Z"), Some(300_000))));
        assert_eq!(cobra.bought, None, "bought before the journals");
        assert_eq!(gs.history.ships.get(&2).and_then(|r| r.bought), Some((at("2026-10-01T10:50:00Z"), 76_000_000)));
        let (ty, name, ident) = ship_names(gs, 1);
        assert_eq!((name.as_str(), ident.as_str()), ("Workhorse", "WH-01"));
        assert!(!ty.is_empty(), "a sold ship still has its type");

        // A period that holds only the Type-9's jump.
        let late = Window { start: at("2026-10-01T10:55:00Z"), end: at("2026-10-01T11:01:00Z") };
        let ships = per_ship(gs, late);
        assert_eq!(ships.len(), 1);
        assert_eq!((ships[0].ship, ships[0].jumps, ships[0].play_secs), (Some(2), 1, 360));
    }

    #[test]
    fn the_ship_aboard_is_the_last_one_named_up_to_then() {
        let mut h = crate::history::History::default();
        h.set_aboard(Timestamp(10), Some(1), Some(1));
        h.set_aboard(Timestamp(10), Some(1), Some(1));
        h.set_aboard(Timestamp(20), None, Some(1));
        h.set_aboard(Timestamp(30), Some(2), Some(2));
        assert_eq!(h.aboard.len(), 3, "the same again isn't a change");
        assert_eq!([5, 10, 15, 20, 25, 30, 99].map(|t| h.aboard_at(Timestamp(t))), [None, Some(1), Some(1), None, None, Some(2), Some(2)]);
        assert_eq!([5, 15, 25, 99].map(|t| h.credit_at(Timestamp(t))), [None, Some(1), Some(1), Some(2)], "on foot beside it");
    }

    /// What the commander logged in as: a Federal Assault Ship is a ship (its symbol says "dropship"), a suit is on
    /// foot, an Apex taxi a ride; and a login is never into another commander's ship, after a crash while crewing too.
    #[test]
    fn a_login_says_whether_the_commander_is_aboard() {
        let login = |ship: &str, id: u64| {
            format!(r#"{{ "timestamp":"2026-10-01T10:00:02Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Ship":"{ship}", "ShipID":{id}, "GameMode":"Solo", "Credits":1, "Loan":0 }}"#)
        };
        let aboard = |lines: &[String]| {
            let mut m = Model::default();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            feed(&mut m, &refs);
            let gs = m.active().expect("commander");
            (gs.aboard(), gs.history.aboard.last().map(|a| a.ship).unwrap_or_default())
        };
        assert_eq!(aboard(&[login("Federation_Dropship_MkII", 5)]), (Some(5), Some(5)));
        assert_eq!(aboard(&[login("cobramkiii", 1), login("TacticalSuit_Class1", 7)]), (None, None));
        assert_eq!(aboard(&[login("cobramkiii", 1), login("adder_taxi", 9)]), (None, None));
        let crewing = r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"JoinACrew", "Captain":"Someone" }"#.to_string();
        assert_eq!(aboard(&[login("cobramkiii", 1), crewing.clone()]).0, None);
        assert_eq!(aboard(&[login("cobramkiii", 1), crewing, login("cobramkiii", 1)]).0, Some(1), "a crash while crewing doesn't linger");
    }
}
