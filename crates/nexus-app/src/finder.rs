//! Market lookups used by other pages: suppliers for colonisation depots, buyers for the cargo
//! hold, a shopping trip for several goods. Plain functions over the galaxy database; pages run them
//! in background jobs. And, from the journal alone, whether a pinned fleet carrier is in this system
//! (the Market page and the overlay).

use crate::pages::navigation::{PlannedRoute, Waypoint};
use nexus_galaxy::{CommodityHit, CommodityQuery, GalaxyDb, Sort, TradeMode};
use nexus_model::location::distance;
use rustc_hash::{FxHashMap, FxHashSet};

/// One thing to buy or sell: (symbol, display name, tonnes).
pub type Line = (String, String, i64);

pub struct Supplier {
    pub name: String,
    /// Nearest stations with enough stock for a full hold (or for what is left), nearest first.
    pub hits: Vec<CommodityHit>,
}

/// Nearest sellers for each commodity a construction site still needs.
pub fn suppliers(db: &GalaxyDb, origin: [f64; 3], needs: &[Line], min_pad: Option<char>, hold: i64, max_age_hours: f64) -> Result<Vec<Supplier>, String> {
    needs
        .iter()
        .map(|(symbol, name, needed)| {
            let q = CommodityQuery {
                commodity: symbol.clone(),
                mode: TradeMode::BuyFromStation,
                origin: Some(origin),
                min_pad,
                max_age_hours: Some(max_age_hours),
                min_quantity: (*needed).min(hold.max(1)),
                sort: Sort::Nearest,
                limit: 3,
                ..Default::default()
            };
            let hits = db.search_commodity(&q).map_err(|e| e.to_string())?;
            Ok(Supplier { name: name.clone(), hits })
        })
        .collect()
}

/// A station that buys some or all of the hold.
pub struct SellStop {
    pub hit: CommodityHit,
    /// Credits for everything it buys (limited by its demand).
    pub total: i64,
    /// (name, tonnes it takes, price).
    pub lines: Vec<(String, i64, i64)>,
}

pub struct SellPlan {
    /// Best single stations, most credits first.
    pub stops: Vec<SellStop>,
    /// Best buyers per commodity.
    pub per_item: Vec<(String, i64, Vec<CommodityHit>)>,
}

/// Where to sell the hold: the best buyers of each commodity within `radius_ly`, and the stations
/// that pay the most for everything together.
pub fn sell_plan(db: &GalaxyDb, origin: [f64; 3], hold: &[Line], min_pad: Option<char>, radius_ly: f64, max_age_hours: f64) -> Result<SellPlan, String> {
    let mut per_item = Vec::with_capacity(hold.len());
    let mut stops: FxHashMap<u64, SellStop> = FxHashMap::default();
    for (symbol, name, count) in hold {
        let q = CommodityQuery {
            commodity: symbol.clone(),
            mode: TradeMode::SellToStation,
            origin: Some(origin),
            max_distance_ly: Some(radius_ly),
            min_pad,
            max_age_hours: Some(max_age_hours),
            min_quantity: 1,
            sort: Sort::BestPrice,
            limit: 60,
            ..Default::default()
        };
        let hits = db.search_commodity(&q).map_err(|e| e.to_string())?;
        for h in &hits {
            let tons = (*count).min(h.quantity.max(0));
            if tons <= 0 {
                continue;
            }
            let stop = stops.entry(h.market_id).or_insert_with(|| SellStop { hit: h.clone(), total: 0, lines: Vec::new() });
            stop.total += tons * h.price;
            stop.lines.push((name.clone(), tons, h.price));
        }
        per_item.push((name.clone(), *count, hits.into_iter().take(3).collect()));
    }
    let mut stops: Vec<SellStop> = stops.into_values().collect();
    stops.sort_by_key(|s| std::cmp::Reverse(s.total));
    stops.truncate(15);
    Ok(SellPlan { stops, per_item })
}

// ---------------------------------------------------------------------------------------------
// Shopping trip: several goods, and the shortest trip among their sellers that buys them all

/// Goods a shopping trip plans for at most: it weighs every set of them at every station it considers.
pub const TRIP_GOODS: usize = 8;
/// Each good's sellers a trip weighs: the nearest the start, found in one search per good; the rest of the plan works
/// from these, so a seller farther out isn't seen. A limit that keeps planning quick: a nearest-first search reads
/// every seller of a good.
pub const TRIP_SELLERS: usize = 200;
/// Around each place the trip may go on from (the start, a stop it picked), each good's nearest sellers it tries.
const NEAR_EACH: usize = 6;
/// Stations the plan weighs at most: it works out every set of goods bought at each, so this keeps it quick.
const MAX_STATIONS: usize = 160;
/// Rounds of looking around the stops picked; a round that finds no shorter trip ends it.
const REFINE_ROUNDS: usize = 4;
/// What each stop adds to a trip's length while planning, so that of two trips equally long the one with fewer stops
/// wins (stations in one system are 0 ly apart), then the one nearer the stars: far below what tells real trips apart.
const STOP_TIE_LY: f64 = 1e-3;
const LS_TIE_LY: f64 = 1e-9;

/// What a shopping trip asks of the stations it stops at: Find commodity's filters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TripFilters {
    pub min_pad: Option<char>,
    /// `None`: prices of any age.
    pub max_age_hours: Option<f64>,
    pub include_carriers: bool,
    pub include_planetary: bool,
}

impl Default for TripFilters {
    /// Every market the database knows.
    fn default() -> Self {
        TripFilters { min_pad: None, max_age_hours: None, include_carriers: true, include_planetary: true }
    }
}

/// One good bought at a stop of a shopping trip.
#[derive(Debug, Clone, PartialEq)]
pub struct TripBuy {
    pub symbol: String,
    pub name: String,
    /// Tonnes to buy: the station had at least this many in stock at its latest report. 1 when any amount does.
    pub tonnes: i64,
    pub price: i64,
    /// The station's stock at its latest report.
    pub stock: i64,
}

/// A stop of a shopping trip: the station (`hit.distance_ly` is from the start; its price and quantity are one of the
/// goods', `buys` has each good's), what to buy there, and the straight line from the stop before it (or from the start).
#[derive(Debug, Clone)]
pub struct TripStop {
    pub hit: CommodityHit,
    pub buys: Vec<TripBuy>,
    pub leg_ly: f64,
}

/// A good the trip found no seller for, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct TripMissing {
    pub name: String,
    pub why: String,
}

#[derive(Debug, Clone, Default)]
pub struct Trip {
    pub stops: Vec<TripStop>,
    /// Straight-line light years from the start through every stop.
    pub total_ly: f64,
    pub missing: Vec<TripMissing>,
    /// The shortest trip there is among the sellers weighed: every one near enough to be on a shorter trip was. False
    /// when there were more than the plan weighs at once ([`MAX_STATIONS`]): then it's the shortest it found.
    pub exact: bool,
}

/// The shortest trip from `origin` that buys every good of `goods` (symbol, name, tonnes: a stop has at least that
/// many in stock, 1 being any amount; a good listed twice needs both amounts), among each good's [`TRIP_SELLERS`]
/// sellers nearest the start. Every nearest-seller chain is weighed (from the start, and from each good's nearest
/// seller, on to the nearest seller of a good still to buy), then every seller near enough to be on a shorter trip, so
/// the trip is never longer than such a chain, and it's the shortest there is among those sellers unless more were
/// near enough than the plan weighs at once (`exact` says which). A station that sells several of the goods is one
/// stop for all (the plan reads the markets of the stations it weighs), and of two trips equally long the one with
/// fewer stops wins. Distances are straight lines between systems (Navigation plots the jumps), and only markets the
/// database knows count. At most [`TRIP_GOODS`] goods; one left over, or one no market sells under the filters, is
/// left out, and `missing` says why. Any page can call it (Missions, Colonisation): run it in a background job, as it
/// reads the database.
pub fn shopping_trip(db: &GalaxyDb, origin: [f64; 3], goods: &[Line], f: &TripFilters) -> Result<Trip, String> {
    plan_trip(db, origin, goods, f, TRIP_SELLERS)
}

/// [`shopping_trip`] weighing `per_good` sellers of each good (tests weigh few).
fn plan_trip(db: &GalaxyDb, origin: [f64; 3], goods: &[Line], f: &TripFilters, per_good: usize) -> Result<Trip, String> {
    let mut lines: Vec<Line> = Vec::new();
    for (symbol, name, tonnes) in goods {
        let t = (*tonnes).max(1);
        match lines.iter_mut().find(|l| l.0.eq_ignore_ascii_case(symbol)) {
            // 1 is any amount: it adds nothing to an amount.
            Some(l) => l.2 = if l.2 <= 1 { t } else if t <= 1 { l.2 } else { l.2.saturating_add(t) },
            None => lines.push((symbol.clone(), name.clone(), t)),
        }
    }
    let over: Vec<TripMissing> = lines.iter().skip(TRIP_GOODS).map(|l| TripMissing { name: l.1.clone(), why: format!("a trip plans for {TRIP_GOODS} goods at most") }).collect();
    lines.truncate(TRIP_GOODS);
    // Each good's sellers, nearest the start first: the one search per good.
    let mut missing = Vec::new();
    let mut pools = Vec::with_capacity(lines.len());
    let mut wanted = 0u16;
    for (g, line) in lines.iter().enumerate() {
        let pool: Vec<CommodityHit> = sellers(db, origin, line, f, per_good)?.into_iter().filter(|h| h.pos.is_some()).collect();
        if pool.is_empty() {
            missing.push(TripMissing { name: line.1.clone(), why: why_missing(db, origin, line, f)? });
        } else {
            wanted |= 1 << g;
        }
        pools.push(pool);
    }
    missing.extend(over);
    let mut plan = Candidates::new(&pools);
    for g in goods_in(wanted) {
        plan.add_nearest(g, origin);
    }
    plan.add_chain(origin, None, wanted);
    for g in goods_in(wanted) {
        plan.add_chain(origin, pools[g].first().map(|h| h.market_id), wanted);
    }
    plan.complete(db, &lines, f, wanted)?;
    let mut best = plan.shortest(origin, wanted);
    // A station on a shorter trip is nearer the start than this one is long: every such seller goes in, nearest first.
    // When they all fit, the trip is the shortest there is among the sellers weighed.
    let mut exact = false;
    if let Some((_, cost)) = &best {
        let mut near: Vec<(f64, u64)> = goods_in(wanted).flat_map(|g| pools[g].iter()).filter_map(|h| h.pos.map(|p| (distance(origin, p), h.market_id))).filter(|(d, _)| d < cost).collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        exact = near.iter().all(|(_, m)| plan.add(*m).is_some());
        plan.complete(db, &lines, f, wanted)?;
        best = plan.shortest(origin, wanted).or(best);
    }
    // Too many for that: around the stops picked, each good bought elsewhere's nearest sellers, until no shorter trip.
    let rounds = if exact { 0 } else { REFINE_ROUNDS };
    for _ in 0..rounds {
        let Some((path, cost)) = &best else { break };
        let (cost, before) = (*cost, plan.stations.len());
        let stops: Vec<([f64; 3], u16)> = path.iter().map(|(s, adds)| (plan.stations[*s].pos, *adds)).collect();
        for (at, adds) in stops {
            for g in goods_in(wanted & !adds) {
                plan.add_nearest(g, at);
            }
        }
        if plan.stations.len() == before {
            break;
        }
        plan.complete(db, &lines, f, wanted)?;
        match plan.shortest(origin, wanted) {
            Some(next) if next.1 < cost - 1e-9 => best = Some(next),
            _ => break,
        }
    }
    let Some((path, _)) = best else { return Ok(Trip { missing, ..Default::default() }) };
    let (mut here, mut total_ly) = (origin, 0.0);
    let mut stops = Vec::with_capacity(path.len());
    for (s, adds) in path {
        let st = &plan.stations[s];
        let buys: Vec<TripBuy> = goods_in(adds)
            .filter_map(|g| {
                let (symbol, name, tonnes) = &lines[g];
                plan.offer(st.market, g).map(|h| TripBuy { symbol: symbol.clone(), name: name.clone(), tonnes: *tonnes, price: h.price, stock: h.quantity })
            })
            .collect();
        let Some(hit) = goods_in(adds).find_map(|g| plan.offer(st.market, g)) else { continue };
        let leg_ly = distance(here, st.pos);
        (here, total_ly) = (st.pos, total_ly + leg_ly);
        stops.push(TripStop { hit: CommodityHit { distance_ly: Some(distance(origin, st.pos)), ..hit.clone() }, buys, leg_ly });
    }
    Ok(Trip { stops, total_ly, missing, exact })
}

/// The goods (their places in the list) a set holds.
fn goods_in(set: u16) -> impl Iterator<Item = usize> {
    (0..TRIP_GOODS).filter(move |g| set & (1 << g) != 0)
}

/// A good's sellers nearest `at`, nearest first, that pass the filters and have the tonnes asked for in stock.
fn sellers(db: &GalaxyDb, at: [f64; 3], (symbol, _, tonnes): &Line, f: &TripFilters, limit: usize) -> Result<Vec<CommodityHit>, String> {
    let q = CommodityQuery {
        commodity: symbol.clone(),
        mode: TradeMode::BuyFromStation,
        origin: Some(at),
        min_pad: f.min_pad,
        max_age_hours: f.max_age_hours,
        min_quantity: (*tonnes).max(1),
        include_carriers: f.include_carriers,
        include_planetary: f.include_planetary,
        sort: Sort::Nearest,
        limit,
        ..Default::default()
    };
    db.search_commodity(&q).map_err(|e| e.to_string())
}

/// Why a good has no seller on the trip, as Find commodity explains an empty list: its nearest seller with no filters,
/// and every one of them that seller fails (stock included); or that no market the database knows sells it, or that
/// the database can't place its sellers.
fn why_missing(db: &GalaxyDb, origin: [f64; 3], (symbol, name, tonnes): &Line, f: &TripFilters) -> Result<String, String> {
    // Nearest first: a seller the database can place comes before one it can't.
    let nearest = sellers(db, origin, &(symbol.clone(), name.clone(), 1), &TripFilters::default(), 1)?;
    let Some(h) = nearest.first() else { return Ok("no market in the database sells it".into()) };
    let Some(pos) = h.pos else { return Ok("the database doesn't know where its sellers are".into()) };
    let mut fails = Vec::new();
    if *tonnes > 1 && h.quantity < *tonnes {
        fails.push(format!("only {} t in stock", nexus_data::names::group_digits(h.quantity)));
    }
    let hours = (nexus_journal::Timestamp::now().0 - h.updated_unix).max(0) as f64 / 3600.0;
    if let Some(max) = f.max_age_hours
        && hours > max
    {
        fails.push(format!("prices {hours:.0} h old"));
    }
    // S leaves nothing out (every station has one), as in the database's searches.
    if f.min_pad.is_some_and(|m| m != 'S') && pad_rank(h.max_pad) < pad_rank(f.min_pad) {
        fails.push(match h.max_pad {
            Some(p) => format!("an {p} pad"),
            None => "its pads never reported".into(),
        });
    }
    if h.is_carrier && !f.include_carriers {
        fails.push("a fleet carrier, and they're left out".into());
    }
    if h.is_planetary && !f.include_planetary {
        fails.push("planetary, and those are left out".into());
    }
    // None failed: only at the edge of the age limit, as the search rounds it.
    if fails.is_empty() {
        return Ok("no seller passes the filters".into());
    }
    Ok(format!("{} ({}, {:.0} ly), the nearest seller, is left out: {}", h.station, h.system, distance(origin, pos), fails.join(", ")))
}

/// A landing pad's size, for comparing: L 3, M 2, S 1, none known 0.
fn pad_rank(p: Option<char>) -> u8 {
    match p {
        Some('L') => 3,
        Some('M') => 2,
        Some('S') => 1,
        _ => 0,
    }
}

/// The sellers a trip weighs (each good's, nearest the start first) and the stations picked from them to plan over.
struct Candidates<'a> {
    pools: &'a [Vec<CommodityHit>],
    /// Every market in the pools: for each good, where its offer is in that good's pool.
    offers: FxHashMap<u64, [Option<usize>; TRIP_GOODS]>,
    /// Offers found by reading a station's market (it sells a good whose nearest sellers it isn't among), and the
    /// markets read.
    extra: FxHashMap<(u64, usize), CommodityHit>,
    read: FxHashSet<u64>,
    stations: Vec<Candidate>,
    by_market: FxHashMap<u64, usize>,
}

/// A station the plan weighs: where it is, the goods it sells (a bit per good), and what a stop there adds while
/// planning ([`STOP_TIE_LY`]).
#[derive(Clone, Copy)]
struct Candidate {
    market: u64,
    pos: [f64; 3],
    goods: u16,
    tie: f64,
}

impl<'a> Candidates<'a> {
    fn new(pools: &'a [Vec<CommodityHit>]) -> Self {
        let mut offers: FxHashMap<u64, [Option<usize>; TRIP_GOODS]> = FxHashMap::default();
        for (g, pool) in pools.iter().enumerate().take(TRIP_GOODS) {
            for (i, h) in pool.iter().enumerate() {
                offers.entry(h.market_id).or_insert([None; TRIP_GOODS])[g].get_or_insert(i);
            }
        }
        Candidates { pools, offers, extra: FxHashMap::default(), read: FxHashSet::default(), stations: Vec::new(), by_market: FxHashMap::default() }
    }

    /// A market's offer of a good.
    fn offer(&self, market: u64, g: usize) -> Option<&CommodityHit> {
        self.offers.get(&market).and_then(|o| o[g]).and_then(|i| self.pools.get(g)?.get(i)).or_else(|| self.extra.get(&(market, g)))
    }

    /// Puts a market among the stations weighed, with every good it's among the sellers of, unless they're full: its
    /// place.
    fn add(&mut self, market: u64) -> Option<usize> {
        if let Some(&i) = self.by_market.get(&market) {
            return Some(i);
        }
        if self.stations.len() >= MAX_STATIONS {
            return None;
        }
        let offers = *self.offers.get(&market)?;
        let goods = offers.iter().enumerate().filter(|(_, o)| o.is_some()).fold(0u16, |set, (g, _)| set | (1 << g));
        let (pos, ls) = {
            let h = (0..TRIP_GOODS).find_map(|g| self.offer(market, g))?;
            (h.pos?, h.dist_ls)
        };
        // A station far from its star (Hutton Orbital is 6.8 M ls out) weighs no more than a second stop.
        let tie = STOP_TIE_LY + ls.unwrap_or(0.0).clamp(0.0, 100_000.0) * LS_TIE_LY;
        self.stations.push(Candidate { market, pos, goods, tie });
        self.by_market.insert(market, self.stations.len() - 1);
        Some(self.stations.len() - 1)
    }

    /// Reads the market of each station weighed that lacks some of the goods, so a station that sells several of them
    /// counts for all: a good's sellers are only its nearest the start, and a farther one may sell it too. A good counts
    /// where it's sold (a buy price) with the tonnes asked for in stock, at a price no older than the filters allow; the
    /// station passed the filters for the good that brought it in.
    fn complete(&mut self, db: &GalaxyDb, lines: &[Line], f: &TripFilters, wanted: u16) -> Result<(), String> {
        let cutoff = f.max_age_hours.map(|h| nexus_journal::Timestamp::now().0 - (h * 3600.0) as i64);
        for s in 0..self.stations.len() {
            let Candidate { market, goods, .. } = self.stations[s];
            if goods & wanted == wanted || !self.read.insert(market) {
                continue;
            }
            let Some(m) = db.station_market(market).map_err(|e| e.to_string())? else { continue };
            let Some(base) = (0..TRIP_GOODS).find_map(|g| self.offer(market, g)).cloned() else { continue };
            for g in goods_in(wanted & !goods) {
                let Some((symbol, _, tonnes)) = lines.get(g) else { continue };
                let Some(row) = m.items.iter().find(|r| r.symbol.eq_ignore_ascii_case(symbol)) else { continue };
                if row.buy <= 0 || row.stock < (*tonnes).max(1) || cutoff.is_some_and(|c| row.updated_unix < c) {
                    continue;
                }
                self.extra.insert((market, g), CommodityHit { price: row.buy, quantity: row.stock, updated_unix: row.updated_unix, ..base.clone() });
                self.stations[s].goods |= 1 << g;
            }
        }
        Ok(())
    }

    /// A good's [`NEAR_EACH`] sellers nearest `at` go among the stations weighed.
    fn add_nearest(&mut self, g: usize, at: [f64; 3]) {
        let mut near: Vec<(f64, u64)> = self.pools[g].iter().filter_map(|h| h.pos.map(|p| (distance(at, p), h.market_id))).collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, market) in near.into_iter().take(NEAR_EACH) {
            self.add(market);
        }
    }

    /// A nearest-seller chain goes among the stations weighed: from `first` (a market) or else from the start, the
    /// nearest seller of a good still to buy, then from there the next, until every good in `wanted` is bought.
    fn add_chain(&mut self, origin: [f64; 3], first: Option<u64>, wanted: u16) {
        let (mut at, mut left, mut next) = (origin, wanted, first);
        while left != 0 {
            let market = match next.take() {
                Some(m) => m,
                None => {
                    let nearest = goods_in(left).flat_map(|g| self.pools[g].iter()).filter_map(|h| h.pos.map(|p| (distance(at, p), h.market_id))).min_by(|a, b| a.0.total_cmp(&b.0));
                    match nearest {
                        Some((_, m)) => m,
                        None => break,
                    }
                }
            };
            let Some(s) = self.add(market) else { break };
            let st = &self.stations[s];
            if st.goods & left == 0 {
                break;
            }
            (at, left) = (st.pos, left & !st.goods);
        }
    }

    /// The shortest path from `origin` through stations that together sell every good in `wanted`: (station, goods
    /// bought there) in order, and its length as planned (with each stop's tie). For every set of goods bought and
    /// every station, the shortest way to have bought them and stand there; a station buys every wanted good it sells
    /// that isn't bought yet (buying it later can't make the trip shorter).
    fn shortest(&self, origin: [f64; 3], wanted: u16) -> Option<(Vec<(usize, u16)>, f64)> {
        let m = self.stations.len();
        if wanted == 0 || m == 0 {
            return None;
        }
        let between: Vec<f64> = (0..m * m).map(|k| distance(self.stations[k / m].pos, self.stations[k % m].pos)).collect();
        let at = |mask: u16, s: usize| usize::from(mask) * m + s;
        let size = (usize::from(wanted) + 1) * m;
        let mut best = vec![f64::INFINITY; size];
        // How each state was reached: the goods bought before and the station before (`usize::MAX`: the start).
        let mut came = vec![(0u16, usize::MAX); size];
        for (s, c) in self.stations.iter().enumerate() {
            let mask = c.goods & wanted;
            let d = distance(origin, c.pos) + c.tie;
            if mask != 0 && d < best[at(mask, s)] {
                best[at(mask, s)] = d;
            }
        }
        // A stop only adds goods, so a set is reached from smaller ones: in counting order, each is final when its
        // turn comes.
        for mask in (1..=wanted).filter(|m| m & !wanted == 0) {
            for s in 0..m {
                let here = best[at(mask, s)];
                if !here.is_finite() {
                    continue;
                }
                for (t, c) in self.stations.iter().enumerate() {
                    let adds = c.goods & wanted & !mask;
                    if adds == 0 {
                        continue;
                    }
                    let next = at(mask | adds, t);
                    let v = here + between[s * m + t] + c.tie;
                    if v < best[next] {
                        best[next] = v;
                        came[next] = (mask, s);
                    }
                }
            }
        }
        let (end, total) = (0..m).map(|s| (s, best[at(wanted, s)])).filter(|(_, v)| v.is_finite()).min_by(|a, b| a.1.total_cmp(&b.1))?;
        let mut path = Vec::new();
        let (mut mask, mut s) = (wanted, end);
        loop {
            let (before, prev) = came[at(mask, s)];
            path.push((s, mask & !before));
            if prev == usize::MAX {
                break;
            }
            (mask, s) = (before, prev);
        }
        path.reverse();
        Some((path, total))
    }
}

/// A shopping trip as a route for Navigation: one waypoint per system (two stops in one system share it), with what
/// to buy where.
pub fn trip_route(trip: &Trip) -> PlannedRoute {
    let mut points: Vec<Waypoint> = Vec::new();
    for s in &trip.stops {
        let goods: Vec<String> = s.buys.iter().map(|b| if b.tonnes > 1 { format!("{} t {}", nexus_data::names::group_digits(b.tonnes), b.name) } else { b.name.clone() }).collect();
        let note = format!("{}: buy {}", s.hit.station, goods.join(", "));
        match points.last_mut() {
            Some(w) if w.system.eq_ignore_ascii_case(&s.hit.system) => w.note = format!("{}; {note}", w.note),
            _ => points.push(Waypoint { system: s.hit.system.clone(), pos: s.hit.pos, star_class: None, jumps: 0, neutron: false, refuel: false, scoopable: None, value: None, note }),
        }
    }
    let n = trip.stops.len();
    let summary = format!("{n} stop{}, about {:.0} ly", if n == 1 { "" } else { "s" }, trip.total_ly);
    PlannedRoute::new("Shopping trip", summary, String::new(), points)
}

/// The ship's landing pad requirement ('S', 'M' or 'L').
pub fn ship_pad(gs: &nexus_model::GameState) -> Option<char> {
    gs.fleet.current().map(|s| nexus_data::ships::pad_size(&s.ship_type))
}

/// Sellable cargo (not mission cargo, not stolen, no limpets).
pub fn sellable_hold(gs: &nexus_model::GameState) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for c in gs.inventory.cargo.items.iter().filter(|c| c.mission_id.is_none() && c.symbol != "drones") {
        let n = c.count - c.stolen;
        if n <= 0 {
            continue;
        }
        match lines.iter_mut().find(|l| l.0 == c.symbol) {
            Some(l) => l.2 += n,
            None => lines.push((c.symbol.clone(), c.name.clone(), n)),
        }
    }
    lines
}

/// Fleet carriers the game has listed in the current system since you arrived: (signal name, callsign).
/// The game logs a carrier's signal when it shows up in your navigation panel: on arrival, or
/// when you drop out of supercruise near it.
pub fn carriers_here(gs: &nexus_model::GameState) -> Vec<(String, String)> {
    let Some(sys) = gs.exploration.systems.get(&gs.location.system_address) else { return Vec::new() };
    sys.signals
        .iter()
        .filter(|s| s.kind.as_deref() == Some("FleetCarrier") && s.ts >= gs.location.last_arrival)
        .map(|s| (s.name.clone(), callsign(&s.name)))
        .collect()
}

/// A carrier signal's callsign: its last word (`WEDDY BLIRTNAUT H1X-L0L` → `H1X-L0L`).
pub fn callsign(signal: &str) -> String {
    signal.split_whitespace().last().unwrap_or_default().to_ascii_uppercase()
}

/// Whether a carrier is in the current system, from what the game listed since you arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Here,
    /// The game listed this many carriers here, and not this one.
    NotHere(usize),
    /// No carrier listed yet this visit.
    Unknown,
}

/// Whether the carrier with the callsign `station` is among those `listed` (see [`carriers_here`]).
pub fn presence(listed: &[(String, String)], station: &str) -> Presence {
    let cs = station.trim().to_ascii_uppercase();
    if listed.iter().any(|(_, c)| *c == cs) {
        Presence::Here
    } else if listed.is_empty() {
        Presence::Unknown
    } else {
        Presence::NotHere(listed.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDb, good, station, station_at};
    use serde_json::json;

    #[test]
    fn sell_plan_and_suppliers() {
        let t = TempDb::new("finder");
        let db = t.db();
        // Alpha pays most for gold but only wants 80 t; Beta takes everything.
        station(db, ("Alpha", 1001, 0.0), "Alpha Port", 5001, json!([good("gold", 0, 0, 60_000, 80), good("steel", 300, 100, 0, 0)]));
        station(db, ("Beta", 1002, 10.0), "Beta Hub", 5002, json!([good("gold", 0, 0, 55_000, 1000), good("silver", 0, 0, 5_000, 1000), good("steel", 320, 10_000, 0, 0)]));
        db.flush().unwrap();

        let hold = vec![("gold".to_string(), "Gold".to_string(), 100), ("silver".to_string(), "Silver".to_string(), 50)];
        let plan = sell_plan(db, [0.0; 3], &hold, None, 50.0, 24.0).unwrap();
        assert_eq!(plan.stops.len(), 2);
        assert_eq!(plan.stops[0].hit.station, "Beta Hub");
        assert_eq!(plan.stops[0].total, 100 * 55_000 + 50 * 5_000);
        assert_eq!(plan.stops[1].total, 80 * 60_000, "limited by Alpha's demand");
        assert_eq!(plan.per_item[0].2[0].station, "Alpha Port", "best gold price");

        // 500 t of steel with a 200 t hold: Alpha's 100 t is not enough, Beta is the nearest real supplier.
        let s = suppliers(db, [0.0; 3], &[("steel".to_string(), "Steel".to_string(), 500)], None, 200, 24.0).unwrap();
        assert_eq!(s[0].hits[0].station, "Beta Hub");
    }

    fn line(symbol: &str, name: &str, tonnes: i64) -> Line {
        (symbol.to_string(), name.to_string(), tonnes)
    }

    fn stops(trip: &Trip) -> Vec<&str> {
        trip.stops.iter().map(|s| s.hit.station.as_str()).collect()
    }

    /// nhok0169's case: Cobalt and Micro Controllers, never sold at one station. The trip buys Cobalt at its nearest
    /// seller, then Micro Controllers at the seller nearest that one, though twelve others are nearer the start.
    #[test]
    fn a_shopping_trip_chains_the_nearest_sellers() {
        let t = TempDb::new("trip-chain");
        let db = t.db();
        station(db, ("Cobalt Sys", 2001, 10.0), "Cobalt Port", 6001, json!([good("cobalt", 400, 500, 0, 0)]));
        // Twelve sellers nearer the start, the other way: through any of them, Cobalt Port is 12 ly or more.
        for k in 0..12u64 {
            let x = -1.0 - k as f64 * 0.5;
            station(db, (&format!("West {k}"), 2100 + k, x), &format!("Chip Shop {k}"), 6100 + k, json!([good("microcontrollers", 3_000, 200, 0, 0)]));
        }
        // A light year past Cobalt Port, and only the 13th nearest the start.
        station(db, ("East", 2003, 11.0), "Chip Forge", 6003, json!([good("microcontrollers", 3_100, 200, 0, 0)]));
        db.flush().unwrap();

        let goods = [line("cobalt", "Cobalt", 1), line("microcontrollers", "Micro Controllers", 1)];
        let trip = shopping_trip(db, [0.0; 3], &goods, &TripFilters::default()).unwrap();
        assert_eq!(stops(&trip), ["Cobalt Port", "Chip Forge"]);
        assert!((trip.total_ly - 11.0).abs() < 1e-9, "{}", trip.total_ly);
        assert!((trip.stops[1].leg_ly - 1.0).abs() < 1e-9);
        assert_eq!(trip.stops[1].hit.distance_ly, Some(11.0), "how far from the start, not from the stop before");
        assert_eq!(trip.stops[0].buys, [TripBuy { symbol: "cobalt".into(), name: "Cobalt".into(), tonnes: 1, price: 400, stock: 500 }]);
        assert!(trip.missing.is_empty());
    }

    /// A station that sells several of the goods is one stop for all of them, unless it hasn't the tonnes asked for.
    #[test]
    fn a_shopping_trip_buys_several_goods_at_one_stop() {
        let t = TempDb::new("trip-one-stop");
        let db = t.db();
        station(db, ("Cobalt Sys", 2001, 10.0), "Cobalt Port", 6001, json!([good("cobalt", 400, 500, 0, 0)]));
        station(db, ("East", 2003, 14.0), "Chip Forge", 6003, json!([good("microcontrollers", 3_100, 200, 0, 0)]));
        station(db, ("Corner", 2005, 6.0), "Corner Shop", 6005, json!([good("cobalt", 420, 50, 0, 0), good("microcontrollers", 3_200, 50, 0, 0)]));
        db.flush().unwrap();

        let any = [line("cobalt", "Cobalt", 1), line("microcontrollers", "Micro Controllers", 1)];
        let trip = shopping_trip(db, [0.0; 3], &any, &TripFilters::default()).unwrap();
        assert_eq!(stops(&trip), ["Corner Shop"], "both goods at one stop");
        assert_eq!(trip.stops[0].buys.len(), 2);

        // 200 t of Cobalt: Corner Shop has 50, so the Cobalt comes from Cobalt Port, after the Micro Controllers.
        let more = [line("cobalt", "Cobalt", 200), line("microcontrollers", "Micro Controllers", 1)];
        let trip = shopping_trip(db, [0.0; 3], &more, &TripFilters::default()).unwrap();
        assert_eq!(stops(&trip), ["Corner Shop", "Cobalt Port"]);
        assert_eq!((trip.stops[0].buys[0].name.as_str(), trip.stops[1].buys[0].tonnes), ("Micro Controllers", 200));
        assert!((trip.total_ly - 10.0).abs() < 1e-9, "{}", trip.total_ly);
    }

    /// A good no market sells, or none with the tonnes asked for, is left out of the trip, and it says why.
    #[test]
    fn a_shopping_trip_says_why_a_good_is_missing() {
        let t = TempDb::new("trip-missing");
        let db = t.db();
        station(db, ("Cobalt Sys", 2001, 10.0), "Cobalt Port", 6001, json!([good("cobalt", 400, 500, 0, 0)]));
        db.flush().unwrap();

        let goods = [line("cobalt", "Cobalt", 1), line("painite", "Painite", 1)];
        let trip = shopping_trip(db, [0.0; 3], &goods, &TripFilters::default()).unwrap();
        assert_eq!(stops(&trip), ["Cobalt Port"]);
        assert_eq!(trip.missing, [TripMissing { name: "Painite".into(), why: "no market in the database sells it".into() }]);

        // Every filter its nearest seller fails, as Find commodity says: here the stock, then the pad as well.
        let too_much = [line("cobalt", "Cobalt", 600)];
        let trip = shopping_trip(db, [0.0; 3], &too_much, &TripFilters::default()).unwrap();
        assert!(trip.stops.is_empty());
        assert_eq!(trip.missing[0].why, "Cobalt Port (Cobalt Sys, 10 ly), the nearest seller, is left out: only 500 t in stock");
        let no_l = TripFilters { min_pad: Some('L'), include_planetary: false, ..TripFilters::default() };
        let trip = shopping_trip(db, [0.0; 3], &too_much, &no_l).unwrap();
        assert_eq!(trip.missing[0].why, "Cobalt Port (Cobalt Sys, 10 ly), the nearest seller, is left out: only 500 t in stock", "it has an L pad and isn't planetary");

        // A good listed twice needs both amounts; past eight goods, the rest are left out and say so.
        let twice = [line("cobalt", "Cobalt", 300), line("Cobalt", "Cobalt", 300)];
        assert_eq!(shopping_trip(db, [0.0; 3], &twice, &TripFilters::default()).unwrap().missing[0].why, "Cobalt Port (Cobalt Sys, 10 ly), the nearest seller, is left out: only 500 t in stock");
        let symbols = ["cobalt", "painite", "gold", "silver", "tritium", "water", "bauxite", "bertrandite", "gallite", "indite"];
        let many: Vec<Line> = symbols.iter().map(|s| line(s, s, 1)).collect();
        let trip = shopping_trip(db, [0.0; 3], &many, &TripFilters::default()).unwrap();
        assert_eq!(stops(&trip), ["Cobalt Port"]);
        let whys: Vec<(&str, &str)> = trip.missing.iter().map(|m| (m.name.as_str(), m.why.as_str())).collect();
        assert_eq!(whys.len(), 9, "{whys:?}");
        assert_eq!(whys[6], ("bertrandite", "no market in the database sells it"));
        assert_eq!(&whys[7..], [("gallite", "a trip plans for 8 goods at most"), ("indite", "a trip plans for 8 goods at most")]);
    }

    /// The review's case: the first plan goes through a cluster of sellers near the start, and nothing around its stops
    /// leads elsewhere; the nearest-seller chain (Cobalt Port, then the Micro Controllers seller nearest it) is far
    /// shorter, and the trip is never longer than it.
    #[test]
    fn a_shopping_trip_is_never_longer_than_the_nearest_seller_chain() {
        let t = TempDb::new("trip-chain-2d");
        let db = t.db();
        station_at(db, ("Cobalt Sys", 2001, [10.0, 0.0, 0.0]), "Cobalt Port", 6001, json!([good("cobalt", 400, 500, 0, 0)]));
        station_at(db, ("South", 2002, [0.0, -20.5, 0.0]), "Cobalt Mine", 6002, json!([good("cobalt", 380, 500, 0, 0)]));
        for k in 0..12u64 {
            let y = -10.5 - k as f64 * 0.01;
            station_at(db, (&format!("Cluster {k}"), 2100 + k, [0.0, y, 0.0]), &format!("Chip Shop {k}"), 6100 + k, json!([good("microcontrollers", 3_000, 200, 0, 0)]));
        }
        station_at(db, ("East", 2003, [11.0, 0.0, 0.0]), "Chip Forge", 6003, json!([good("microcontrollers", 3_100, 200, 0, 0)]));
        db.flush().unwrap();

        let goods = [line("cobalt", "Cobalt", 1), line("microcontrollers", "Micro Controllers", 1)];
        let trip = shopping_trip(db, [0.0; 3], &goods, &TripFilters::default()).unwrap();
        assert_eq!(stops(&trip), ["Cobalt Port", "Chip Forge"], "{:.1} ly", trip.total_ly);
        assert!((trip.total_ly - 11.0).abs() < 1e-9, "{}", trip.total_ly);
    }

    /// A station that sells two of the goods counts for both, though it isn't among one good's nearest sellers: the plan
    /// reads its market. Here Cobalt is sold only far out, at a mall that has Micro Controllers too, and the three
    /// Micro Controllers sellers it weighs (of each good, the nearest only) are near the start: one stop, not two.
    #[test]
    fn a_far_station_counts_for_every_good_it_sells() {
        let t = TempDb::new("trip-complete");
        let db = t.db();
        station(db, ("Far Sys", 2001, 50.0), "Far Mall", 6001, json!([good("cobalt", 400, 500, 0, 0), good("microcontrollers", 3_000, 200, 0, 0)]));
        for k in 0..3u64 {
            station(db, (&format!("Near {k}"), 2100 + k, -1.0 - k as f64), &format!("Chip Shop {k}"), 6100 + k, json!([good("microcontrollers", 3_000, 200, 0, 0)]));
        }
        db.flush().unwrap();

        let goods = [line("cobalt", "Cobalt", 1), line("microcontrollers", "Micro Controllers", 1)];
        let trip = plan_trip(db, [0.0; 3], &goods, &TripFilters::default(), 3).unwrap();
        assert_eq!(stops(&trip), ["Far Mall"], "{:.1} ly", trip.total_ly);
        assert_eq!(trip.stops[0].buys.iter().map(|b| (b.name.as_str(), b.price)).collect::<Vec<_>>(), [("Cobalt", 400), ("Micro Controllers", 3_000)]);
        // Not with too little of it in stock for the tonnes asked.
        let more = [line("cobalt", "Cobalt", 1), line("microcontrollers", "Micro Controllers", 300)];
        assert_eq!(plan_trip(db, [0.0; 3], &more, &TripFilters::default(), 3).unwrap().stops.len(), 1, "no seller of 300 t: Micro Controllers left out");
    }

    /// The review's second case: the cheaper chains all run 30 ly, through sellers 12 ly out on either side; each good's
    /// seventh-nearest seller, about 20 ly out one way, makes a 20.1 ly trip. Every seller nearer than the best trip so
    /// far is weighed, so the trip is the shortest there is among them.
    #[test]
    fn a_shopping_trip_is_the_shortest_among_the_sellers_weighed() {
        let t = TempDb::new("trip-exact");
        let db = t.db();
        let mut id = 0u64;
        let mut put = |pos: [f64; 3], name: &str, symbol: &str| {
            id += 1;
            station_at(db, (&format!("Sys {id}"), 3000 + id, pos), &format!("{name} {id}"), 8000 + id, json!([good(symbol, 1_000, 200, 0, 0)]));
        };
        for _ in 0..6 {
            put([0.0, 12.0, 0.0], "Cobalt North", "cobalt");
            put([0.0, -30.0, 0.0], "Cobalt South", "cobalt");
            put([0.0, -12.0, 0.0], "Chips South", "microcontrollers");
            put([0.0, 30.0, 0.0], "Chips North", "microcontrollers");
        }
        put([20.0, 0.0, 0.0], "Cobalt East", "cobalt");
        put([20.1, 0.0, 0.0], "Chips East", "microcontrollers");
        db.flush().unwrap();

        let goods = [line("cobalt", "Cobalt", 1), line("microcontrollers", "Micro Controllers", 1)];
        let trip = shopping_trip(db, [0.0; 3], &goods, &TripFilters::default()).unwrap();
        assert!((trip.total_ly - 20.1).abs() < 1e-9, "{:.2} ly: {:?}", trip.total_ly, stops(&trip));
        assert!(trip.exact);
    }

    /// 1 t is any amount: merged with another line of the same good it adds nothing.
    #[test]
    fn any_amount_adds_nothing_to_an_amount() {
        let t = TempDb::new("trip-merge");
        let db = t.db();
        station(db, ("Cobalt Sys", 2001, 10.0), "Cobalt Port", 6001, json!([good("cobalt", 400, 500, 0, 0)]));
        db.flush().unwrap();
        let tonnes = |a: i64, b: i64| shopping_trip(db, [0.0; 3], &[line("cobalt", "Cobalt", a), line("cobalt", "Cobalt", b)], &TripFilters::default()).unwrap().stops[0].buys[0].tonnes;
        assert_eq!((tonnes(1, 1), tonnes(20, 1), tonnes(1, 20), tonnes(20, 30)), (1, 20, 20, 50));
    }

    /// Of two trips equally long, the one with fewer stops: one system holds a seller of each good and a station that
    /// sells both, and the trip docks once.
    #[test]
    fn a_shopping_trip_docks_once_where_one_station_sells_everything() {
        let t = TempDb::new("trip-tie");
        let db = t.db();
        station(db, ("Mall Sys", 3001, 5.0), "Chip Shop", 7001, json!([good("microcontrollers", 2_900, 200, 0, 0)]));
        station(db, ("Mall Sys", 3001, 5.0), "Everything Mall", 7002, json!([good("microcontrollers", 3_000, 200, 0, 0), good("cobalt", 400, 500, 0, 0)]));
        station(db, ("Mall Sys", 3001, 5.0), "Cobalt Shop", 7003, json!([good("cobalt", 380, 500, 0, 0)]));
        db.flush().unwrap();

        let goods = [line("microcontrollers", "Micro Controllers", 1), line("cobalt", "Cobalt", 1)];
        let trip = shopping_trip(db, [0.0; 3], &goods, &TripFilters::default()).unwrap();
        assert_eq!(stops(&trip), ["Everything Mall"]);
        assert_eq!(trip.stops[0].buys.len(), 2);
        assert!((trip.total_ly - 5.0).abs() < 1e-9, "the ties aren't in the distance: {}", trip.total_ly);
    }

    /// Two stops in one system share its waypoint on Navigation, each with what to buy there.
    #[test]
    fn a_shopping_trip_follows_as_one_waypoint_per_system() {
        let t = TempDb::new("trip-route");
        let db = t.db();
        station(db, ("Twin", 3001, 5.0), "Twin A", 7001, json!([good("cobalt", 400, 500, 0, 0)]));
        station(db, ("Twin", 3001, 5.0), "Twin B", 7002, json!([good("microcontrollers", 3_000, 200, 0, 0)]));
        db.flush().unwrap();

        let goods = [line("cobalt", "Cobalt", 20), line("microcontrollers", "Micro Controllers", 1)];
        let trip = shopping_trip(db, [0.0; 3], &goods, &TripFilters::default()).unwrap();
        assert_eq!(trip.stops.len(), 2);
        assert_eq!(trip.stops[1].leg_ly, 0.0, "the same system");
        let route = trip_route(&trip);
        assert_eq!(route.waypoints.len(), 1);
        assert_eq!(route.waypoints[0].system, "Twin");
        let note = &route.waypoints[0].note;
        assert!(note.contains("Twin A: buy 20 t Cobalt") && note.contains("Twin B: buy Micro Controllers"), "{note}");
        assert_eq!(route.summary, "2 stops, about 5 ly");
    }
}
