//! Long-term history: jumps, visited systems, docks, deaths, the credit ledger, time in the game and what the
//! commander did (the statistics count them over any period).

use nexus_journal::Timestamp;
use rustc_hash::FxHashMap;
use std::collections::BTreeMap;

/// Time in the game: from a login (`LoadGame`) until the commander went back to the main menu or closed the game, as the
/// game's own time played counts it. A crash writes nothing, so a session that ended in one runs to its journal's last
/// line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaySpan {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl PlaySpan {
    pub fn secs(&self) -> i64 {
        (self.end.0 - self.start.0).max(0)
    }
}

/// Something the commander did that the statistics count over a period: the model's other records keep what is (a
/// body's scan, a species, the cargo), not when it was done.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Deed {
    /// A ship destroyed for a bounty (`Bounty`) or a combat bond (`FactionKillBond`, `CapShipBond`), and the reward.
    Kill { reward: i64, bond: bool },
    /// A star or planet scanned in detail for the first time; `first`: nobody had discovered it before.
    Scan { first: bool },
    /// A planet mapped with probes; `first`: nobody had mapped it before; `efficient`: within the probe target.
    Map { first: bool, efficient: bool },
    /// A species analysed (its third sample), what Vista Genomics should pay for it (with the first-logged bonus when
    /// that's likely), and whether it is.
    Organic { value: i64, first: bool },
    /// Cargo sold at a market: tonnes, what it had cost and the profit over that, as the game works them out. `mined`:
    /// it cost nothing (mined, salvaged, stolen or given for a mission), so all of the sale is profit.
    Sale { tonnes: i64, cost: i64, profit: i64, mined: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeedRecord {
    pub ts: Timestamp,
    pub deed: Deed,
}

#[derive(Debug, Clone)]
pub struct JumpRecord {
    pub ts: Timestamp,
    pub system: String,
    pub address: u64,
    pub pos: Option<[f64; 3]>,
    pub dist: f64,
    pub fuel_used: f64,
    pub carrier: bool,
}

#[derive(Debug, Clone)]
pub struct VisitedSystem {
    pub name: String,
    pub pos: Option<[f64; 3]>,
    pub first: Timestamp,
    pub last: Timestamp,
    pub visits: u32,
}

#[derive(Debug, Clone)]
pub struct DockRecord {
    pub ts: Timestamp,
    pub station: String,
    pub station_type: String,
    pub system: String,
    pub market_id: u64,
}

#[derive(Debug, Clone)]
pub struct DeathRecord {
    pub ts: Timestamp,
    pub system: String,
    pub killers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LedgerCategory {
    Trade,
    Mining,
    Exploration,
    Exobiology,
    Bounties,
    CombatBonds,
    Missions,
    Ships,
    Outfitting,
    Repairs,
    Fuel,
    Ammo,
    Fines,
    Rebuy,
    Carrier,
    Colonisation,
    Materials,
    Taxi,
    /// Moving money between the commander and their own carrier: not income or spending.
    Transfer,
    Other,
}

impl LedgerCategory {
    pub fn label(self) -> &'static str {
        match self {
            LedgerCategory::Trade => "Trade",
            LedgerCategory::Mining => "Mining",
            LedgerCategory::Exploration => "Exploration",
            LedgerCategory::Exobiology => "Exobiology",
            LedgerCategory::Bounties => "Bounties",
            LedgerCategory::CombatBonds => "Combat bonds",
            LedgerCategory::Missions => "Missions",
            LedgerCategory::Ships => "Ships",
            LedgerCategory::Outfitting => "Outfitting",
            LedgerCategory::Repairs => "Repairs",
            LedgerCategory::Fuel => "Fuel",
            LedgerCategory::Ammo => "Ammo & restock",
            LedgerCategory::Fines => "Fines",
            LedgerCategory::Rebuy => "Rebuy",
            LedgerCategory::Carrier => "Carrier",
            LedgerCategory::Colonisation => "Colonisation",
            LedgerCategory::Materials => "Materials",
            LedgerCategory::Taxi => "Taxi",
            LedgerCategory::Transfer => "Carrier transfers",
            LedgerCategory::Other => "Other",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LedgerEntry {
    pub ts: Timestamp,
    pub category: LedgerCategory,
    /// Positive = income, negative = expense.
    pub amount: i64,
    pub note: String,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    pub jumps: Vec<JumpRecord>,
    pub visited: FxHashMap<u64, VisitedSystem>,
    pub total_distance_ly: f64,
    pub docks: Vec<DockRecord>,
    pub deaths: Vec<DeathRecord>,
    pub ledger: Vec<LedgerEntry>,
    /// Absolute credit balance samples (LoadGame + live Status balance changes).
    pub credit_samples: Vec<(Timestamp, i64)>,
    /// Time in the game, one span per session, oldest first.
    pub play: Vec<PlaySpan>,
    /// In the game now: journal lines extend the last span.
    in_game: bool,
    /// What the commander did, oldest first.
    pub deeds: Vec<DeedRecord>,
    /// Which of the commander's ships they were in and what they did counted for, from each moment that changed,
    /// oldest first. Whatever else the history holds goes to a ship through it.
    pub aboard: Vec<Aboard>,
    /// Every ship of the commander's the journals have named, by its ship ID, sold ones too.
    pub ships: BTreeMap<u64, ShipRecord>,
    /// A ship bought, waiting for its ID: the game says what it cost (`ShipyardBuy`) before naming it (`ShipyardNew`).
    pending_purchase: Option<(Timestamp, i64)>,
}

/// From `ts` on: the commander's ship they're in (None on foot, in a taxi or a dropship, or crewing another
/// commander's ship), and the one what they do counts for: that one, or the one they stepped out of and are on foot
/// beside (sampling on its planet, at Vista Genomics or a mission board where it's docked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aboard {
    pub ts: Timestamp,
    pub ship: Option<u64>,
    pub credit: Option<u64>,
}

/// A ship of the commander's that the journals have named: what it is and is called, and when it was bought and sold,
/// as far as the journals go.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShipRecord {
    /// Lower-case journal symbol (`empire_courier`).
    pub ship_type: String,
    pub type_name: String,
    pub name: String,
    pub ident: String,
    /// When it was bought and what it cost; None when that was before the journals Elite Nexus read.
    pub bought: Option<(Timestamp, i64)>,
    /// When it was sold, and for what when the game said (selling it in part exchange for another doesn't).
    pub sold: Option<(Timestamp, Option<i64>)>,
}

impl History {
    /// From `ts`: the ship the commander is in, and the one what they do counts for (see [`Aboard`]).
    pub fn set_aboard(&mut self, ts: Timestamp, ship: Option<u64>, credit: Option<u64>) {
        if self.aboard.last().is_none_or(|a| a.ship != ship || a.credit != credit) {
            self.aboard.push(Aboard { ts, ship, credit });
        }
    }

    /// What held at `t`: the last change up to then (a line in the same second as a change goes with it).
    fn aboard_entry(&self, t: Timestamp) -> Option<&Aboard> {
        let i = self.aboard.partition_point(|a| a.ts <= t);
        i.checked_sub(1).map(|i| &self.aboard[i])
    }

    /// The ship the commander was in at `t`.
    pub fn aboard_at(&self, t: Timestamp) -> Option<u64> {
        self.aboard_entry(t).and_then(|a| a.ship)
    }

    /// The ship what the commander did at `t` counts for.
    pub fn credit_at(&self, t: Timestamp) -> Option<u64> {
        self.aboard_entry(t).and_then(|a| a.credit)
    }

    /// What a ship is and is called (the journals say it again whenever it changes).
    pub fn name_ship(&mut self, id: u64, ship_type: &str, type_name: &str, name: &str, ident: &str) {
        let r = self.ships.entry(id).or_default();
        if !ship_type.is_empty() && r.ship_type != ship_type {
            r.ship_type = ship_type.to_string();
        }
        if !type_name.is_empty() && r.type_name != type_name {
            r.type_name = type_name.to_string();
        }
        if r.name != name {
            r.name = name.to_string();
        }
        if r.ident != ident {
            r.ident = ident.to_string();
        }
    }

    /// A ship bought for `price`: its ID comes with the next [`History::new_ship`].
    pub fn purchase(&mut self, ts: Timestamp, price: i64) {
        self.pending_purchase = Some((ts, price));
    }

    /// The ship just bought has its ID.
    pub fn new_ship(&mut self, id: u64, ship_type: &str) {
        let r = self.ships.entry(id).or_default();
        if r.ship_type.is_empty() {
            r.ship_type = ship_type.to_string();
        }
        if let Some(p) = self.pending_purchase.take() {
            r.bought = Some(p);
        }
    }

    /// A ship sold, for `price` when the game said.
    pub fn ship_sold(&mut self, id: u64, ship_type: &str, ts: Timestamp, price: Option<i64>) {
        let r = self.ships.entry(id).or_default();
        if r.ship_type.is_empty() {
            r.ship_type = ship_type.to_string();
        }
        r.sold = Some((ts, price));
    }

    /// A login: a session starts.
    pub fn login(&mut self, ts: Timestamp) {
        self.play.push(PlaySpan { start: ts, end: ts });
        self.in_game = true;
    }

    /// A journal line written at `ts`: the session, if one is on, lasts until then at least.
    pub fn played(&mut self, ts: Timestamp) {
        if self.in_game
            && let Some(s) = self.play.last_mut()
            && ts > s.end
        {
            s.end = ts;
        }
    }

    /// Back at the main menu, the game closed, or a new game started (a journal's first part): the session is over.
    pub fn logout(&mut self) {
        self.in_game = false;
    }

    pub fn deed(&mut self, ts: Timestamp, deed: Deed) {
        self.deeds.push(DeedRecord { ts, deed });
    }

    /// The oldest moment the history holds: where the journals Elite Nexus read begin.
    pub fn first(&self) -> Option<Timestamp> {
        [
            self.play.first().map(|p| p.start),
            self.credit_samples.first().map(|(t, _)| *t),
            self.ledger.first().map(|e| e.ts),
            self.jumps.first().map(|j| j.ts),
            self.docks.first().map(|d| d.ts),
            self.deeds.first().map(|d| d.ts),
        ]
        .into_iter()
        .flatten()
        .filter(|t| !t.is_zero())
        .min()
    }

    pub fn visit(&mut self, address: u64, name: &str, pos: Option<[f64; 3]>, ts: Timestamp) {
        let v = self.visited.entry(address).or_insert_with(|| VisitedSystem {
            name: name.to_string(),
            pos,
            first: ts,
            last: ts,
            visits: 0,
        });
        v.visits += 1;
        v.last = ts;
        if v.pos.is_none() {
            v.pos = pos;
        }
    }

    pub fn jump(&mut self, rec: JumpRecord) {
        self.total_distance_ly += rec.dist;
        self.visit(rec.address, &rec.system, rec.pos, rec.ts);
        self.jumps.push(rec);
    }

    pub fn ledger(&mut self, ts: Timestamp, category: LedgerCategory, amount: i64, note: impl Into<String>) {
        if amount != 0 {
            self.ledger.push(LedgerEntry { ts, category, amount, note: note.into() });
        }
    }

    pub fn credit_sample(&mut self, ts: Timestamp, credits: i64) {
        if self.credit_samples.last().is_none_or(|(_, c)| *c != credits) {
            self.credit_samples.push((ts, credits));
        }
    }

    /// Income/expense totals per category since `since`.
    pub fn totals_since(&self, since: Timestamp) -> Vec<(LedgerCategory, i64, i64)> {
        let mut map: std::collections::BTreeMap<LedgerCategory, (i64, i64)> = Default::default();
        for e in self.ledger.iter().filter(|e| e.ts >= since) {
            let t = map.entry(e.category).or_default();
            if e.amount >= 0 {
                t.0 += e.amount;
            } else {
                t.1 += e.amount;
            }
        }
        map.into_iter().map(|(k, (i, x))| (k, i, x)).collect()
    }
}
