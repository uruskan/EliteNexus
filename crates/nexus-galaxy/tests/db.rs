//! End-to-end tests of the galaxy database through its public API.

use nexus_galaxy::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique temporary database under the cargo target dir (on the same drive as the build).
struct TempDb {
    dir: PathBuf,
    db: Option<GalaxyDb>,
}

impl TempDb {
    fn new(name: &str) -> TempDb {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "galaxy-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let db = GalaxyDb::open(dir.join("galaxy.db")).expect("open db");
        TempDb { dir, db: Some(db) }
    }

    fn db(&self) -> &GalaxyDb {
        self.db.as_ref().expect("db")
    }

    fn reopen(&mut self) {
        let path = self.db().path().to_path_buf();
        self.db = None;
        self.db = Some(GalaxyDb::open(path).expect("reopen"));
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        self.db = None;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

const SYS_A: u64 = 1001;
const SYS_B: u64 = 1002;
const SYS_C: u64 = 1003;

fn system(id: u64, name: &str, pos: [f64; 3]) -> Ingest {
    Ingest::System(SystemUpdate {
        id64: id,
        name: name.into(),
        pos: Some(pos),
        timestamp: now() - 60,
        details: Some(SystemDetails {
            population: Some(1_000_000),
            economy: Some("Industrial".into()),
            security: Some("High".into()),
            ..Default::default()
        }),
    })
}

fn station(
    mid: u64,
    name: &str,
    sys: u64,
    sys_name: &str,
    kind: &str,
    services: &[&str],
    economy: &str,
) -> Ingest {
    Ingest::Station(StationUpdate {
        market_id: mid,
        name: name.into(),
        system_id64: Some(sys),
        system_name: Some(sys_name.into()),
        station_type: Some(kind.into()),
        dist_ls: Some(500.0),
        services: Some(Services::from_names(services)),
        primary_economy: Some(economy.into()),
        economies: Some(vec![(economy.to_string(), 1.0)]),
        timestamp: now() - 60,
        ..Default::default()
    })
}

/// (symbol, buy, sell, stock, demand)
fn market(
    mid: u64,
    sys_name: &str,
    name: &str,
    ts: i64,
    items: &[(&str, i64, i64, i64, i64)],
) -> Ingest {
    Ingest::Market(MarketSnapshot {
        market_id: mid,
        system_name: sys_name.into(),
        station_name: name.into(),
        timestamp: ts,
        items: items
            .iter()
            .map(|&(s, buy, sell, stock, demand)| MarketItem {
                symbol: s.into(),
                buy,
                sell,
                mean: 0,
                stock,
                stock_bracket: Some(2),
                demand,
                demand_bracket: Some(2),
            })
            .collect(),
        ..Default::default()
    })
}

/// A at the origin, B 10 ly away, C 100 ly away; five stations selling gold.
fn populate(db: &GalaxyDb) {
    let t = now() - 30;
    db.ingest_many(vec![
        system(SYS_A, "Alpha", [0.0, 0.0, 0.0]),
        system(SYS_B, "Beta", [10.0, 0.0, 0.0]),
        system(SYS_C, "Gamma", [0.0, 100.0, 0.0]),
        station(
            1,
            "A Port",
            SYS_A,
            "Alpha",
            "Coriolis",
            &["commodities", "materialtrader", "facilitator"],
            "Industrial",
        ),
        station(
            2,
            "B Outpost",
            SYS_B,
            "Beta",
            "Outpost",
            &["commodities", "techBroker"],
            "High Tech",
        ),
        station(
            3,
            "C Orbis",
            SYS_C,
            "Gamma",
            "Orbis",
            &["commodities", "materialtrader"],
            "High Tech",
        ),
        station(
            4,
            "B-CARRIER",
            SYS_B,
            "Beta",
            "FleetCarrier",
            &["commodities"],
            "Private Enterprise",
        ),
        station(
            5,
            "B Crater",
            SYS_B,
            "Beta",
            "CraterPort",
            &["commodities"],
            "Extraction",
        ),
        market(
            1,
            "Alpha",
            "A Port",
            t,
            &[("gold", 9000, 8900, 100, 0), ("silver", 0, 5000, 0, 400)],
        ),
        market(
            2,
            "Beta",
            "B Outpost",
            t,
            &[("gold", 8500, 8400, 50, 0), ("silver", 4000, 3900, 900, 0)],
        ),
        market(
            3,
            "Gamma",
            "C Orbis",
            t,
            &[("gold", 8000, 7900, 1000, 0), ("silver", 0, 6000, 0, 50)],
        ),
        market(4, "Beta", "B-CARRIER", t, &[("gold", 7000, 0, 10, 0)]),
        market(
            5,
            "Beta",
            "B Crater",
            t,
            &[("gold", 8200, 8100, 20, 0), ("tritium", 0, 60000, 0, 10000)],
        ),
    ]);
    db.flush().expect("flush");
}

fn ids(hits: &[CommodityHit]) -> Vec<u64> {
    hits.iter().map(|h| h.market_id).collect()
}

#[test]
fn commodity_search_filters_and_sorts() {
    let t = TempDb::new("search");
    let db = t.db();
    populate(db);
    let q = CommodityQuery {
        commodity: "gold".into(),
        ..Default::default()
    };
    assert_eq!(
        ids(&db.search_commodity(&q).expect("q")),
        vec![4, 3, 5, 2, 1]
    );
    // other spellings of the commodity
    for name in ["Gold", "$gold_name;", " GOLD "] {
        let q = CommodityQuery {
            commodity: name.into(),
            ..Default::default()
        };
        assert_eq!(db.search_commodity(&q).expect("q").len(), 5, "{name}");
    }
    let q = CommodityQuery {
        commodity: "gold".into(),
        include_carriers: false,
        ..Default::default()
    };
    assert_eq!(ids(&db.search_commodity(&q).expect("q")), vec![3, 5, 2, 1]);
    let q = CommodityQuery {
        commodity: "gold".into(),
        include_planetary: false,
        ..Default::default()
    };
    assert_eq!(ids(&db.search_commodity(&q).expect("q")), vec![4, 3, 2, 1]);
    let q = CommodityQuery {
        commodity: "gold".into(),
        min_pad: Some('L'),
        ..Default::default()
    };
    assert_eq!(
        ids(&db.search_commodity(&q).expect("q")),
        vec![4, 3, 5, 1],
        "outposts have no large pad"
    );
    let q = CommodityQuery {
        commodity: "gold".into(),
        min_quantity: 60,
        ..Default::default()
    };
    assert_eq!(ids(&db.search_commodity(&q).expect("q")), vec![3, 1]);
    let q = CommodityQuery {
        commodity: "gold".into(),
        origin: Some([0.0, 0.0, 0.0]),
        max_distance_ly: Some(50.0),
        ..Default::default()
    };
    let hits = db.search_commodity(&q).expect("q");
    assert_eq!(ids(&hits), vec![4, 5, 2, 1]);
    assert_eq!(hits[0].distance_ly, Some(10.0));
    assert_eq!(hits[0].system, "Beta");
    assert_eq!(hits[0].max_pad, Some('L'));
    assert!(hits[0].is_carrier);
    assert_eq!(hits[1].station_type.as_deref(), Some("Planetary Port"));
    assert!(hits[1].is_planetary);
    let q = CommodityQuery {
        commodity: "gold".into(),
        origin: Some([0.0, 0.0, 0.0]),
        sort: Sort::Nearest,
        limit: 2,
        ..Default::default()
    };
    let hits = db.search_commodity(&q).expect("q");
    assert_eq!(hits[0].market_id, 1);
    assert_eq!(hits[0].distance_ly, Some(0.0));
    assert_eq!(hits.len(), 2);
    // sell mode: best price first, needs demand
    let q = CommodityQuery {
        commodity: "silver".into(),
        mode: TradeMode::SellToStation,
        ..Default::default()
    };
    let hits = db.search_commodity(&q).expect("q");
    assert_eq!(ids(&hits), vec![3, 1]);
    assert_eq!(hits[0].price, 6000);
    assert_eq!(hits[0].quantity, 50);
    let q = CommodityQuery {
        commodity: "no such commodity".into(),
        ..Default::default()
    };
    assert!(db.search_commodity(&q).expect("q").is_empty());
}

/// A carrier's docking access, as its market reported it, reaches the list rows (in the journal's spelling, whatever
/// the source wrote); stations, and carriers whose access was never reported, have none.
#[test]
fn carrier_access_reaches_the_lists() {
    let t = TempDb::new("access");
    let db = t.db();
    populate(db);
    // The Spansh dump writes "Squadron Friends", the journal and EDDN "squadronfriends".
    db.ingest(Ingest::Market(MarketSnapshot {
        carrier_docking_access: Some("Squadron Friends".into()),
        ..match market(4, "Beta", "B-CARRIER", now() - 20, &[("gold", 7000, 0, 10, 0)]) {
            Ingest::Market(m) => m,
            _ => unreachable!("a market"),
        }
    }));
    db.flush().expect("flush");
    let q = CommodityQuery { commodity: "gold".into(), ..Default::default() };
    let hits = db.search_commodity(&q).expect("q");
    let access = |id: u64| hits.iter().find(|h| h.market_id == id).and_then(|h| h.carrier_access.clone());
    assert_eq!(access(4).as_deref(), Some("squadronfriends"));
    assert_eq!(access(1), None, "a station has none");
    let latest = db.latest_prices(&q).expect("latest");
    assert_eq!(latest.iter().find(|l| l.market_id == 4).and_then(|l| l.carrier_access.as_deref()), Some("squadronfriends"));
    assert!(latest.iter().filter(|l| l.market_id != 4).all(|l| l.carrier_access.is_none()));
}

#[test]
fn age_filter_and_freshest_sort() {
    let t = TempDb::new("age");
    let db = t.db();
    populate(db);
    db.ingest(market(
        2,
        "Beta",
        "B Outpost",
        now() - 10 * 86_400 + 100_000,
        &[("gold", 8500, 8400, 50, 0)],
    ));
    // that snapshot is older than the stored one, so it is ignored
    db.flush().expect("flush");
    assert_eq!(db.stats().expect("stats").stale_markets, 1);
    db.ingest(market(
        6,
        "Beta",
        "Old Station",
        now() - 10 * 86_400,
        &[("gold", 100, 0, 5, 0)],
    ));
    db.flush().expect("flush");
    let q = CommodityQuery {
        commodity: "gold".into(),
        max_age_hours: Some(48.0),
        ..Default::default()
    };
    assert!(!ids(&db.search_commodity(&q).expect("q")).contains(&6));
    let q = CommodityQuery {
        commodity: "gold".into(),
        ..Default::default()
    };
    assert_eq!(ids(&db.search_commodity(&q).expect("q"))[0], 6);
    let q = CommodityQuery {
        commodity: "gold".into(),
        sort: Sort::Freshest,
        ..Default::default()
    };
    assert_eq!(
        *ids(&db.search_commodity(&q).expect("q"))
            .last()
            .expect("hits"),
        6
    );
}

#[test]
fn station_market_and_history() {
    let t = TempDb::new("market");
    let db = t.db();
    populate(db);
    let ts = now() - 30;
    db.ingest(market(
        1,
        "Alpha",
        "A Port",
        ts + 10,
        &[("gold", 9100, 8900, 100, 0), ("silver", 0, 5000, 0, 400)],
    ));
    db.ingest(market(
        1,
        "Alpha",
        "A Port",
        ts + 20,
        &[("gold", 9100, 8900, 90, 0), ("silver", 0, 5000, 0, 400)],
    ));
    db.flush().expect("flush");
    let m = db.station_market(1).expect("q").expect("station");
    assert_eq!(m.station.name, "A Port");
    assert_eq!(m.station.station_type.as_deref(), Some("Coriolis Starport"));
    assert_eq!(m.station.commodity_count, 2);
    assert_eq!(m.station.market_updated_unix, ts + 20);
    assert_eq!(m.system.as_ref().map(|s| s.name.as_str()), Some("Alpha"));
    let gold = m.items.iter().find(|i| i.symbol == "gold").expect("gold");
    assert_eq!(gold.name, "Gold");
    assert_eq!(gold.category, "Metals");
    assert_eq!(gold.stock, 90);
    let h = db.price_history(1, "Gold").expect("history");
    assert_eq!(
        h.iter().map(|p| p.buy).collect::<Vec<_>>(),
        vec![9000, 9100],
        "{h:?}"
    );
    assert!(
        db.price_history(1, "silver").expect("history").is_empty(),
        "unchanged prices add no history"
    );
    assert!(db.station_market(999).expect("q").is_none());
}

/// The price history count shown on Market › Database is kept by the writer instead of counted on every look
/// (counting a month of live prices took seconds): it must match the rows stored, also after reopening.
#[test]
fn price_history_count_is_kept() {
    let mut t = TempDb::new("history-count");
    let db = t.db();
    populate(db);
    let ts = now() - 30;
    for (i, gold) in [(10, 9100), (20, 9100), (30, 9200)] {
        db.ingest(market(1, "Alpha", "A Port", ts + i, &[("gold", gold, 8900, 100, 0), ("silver", 0, 5000, 0, 400)]));
    }
    db.flush().expect("flush");
    let stored = db.price_history(1, "gold").expect("history").len() as u64;
    assert_eq!(stored, 4, "two changes, each with the price before and after");
    assert_eq!(db.stats().expect("stats").price_history_rows, Some(stored));
    t.reopen();
    t.db().flush().expect("flush");
    assert_eq!(t.db().stats().expect("stats").price_history_rows, Some(stored));
}

/// History older than the 30 days kept is pruned (in batches) when the writer starts, and the kept count follows.
#[test]
fn old_price_history_is_pruned_and_uncounted() {
    let mut t = TempDb::new("history-prune");
    let db = t.db();
    populate(db);
    let old = now() - 40 * 86_400;
    for (i, gold) in [(0, 9000), (10, 9100), (20, 9200)] {
        db.ingest(market(9, "Alpha", "Old Port", old + i, &[("gold", gold, 8900, 100, 0)]));
    }
    db.flush().expect("flush");
    // 9000 → 9100 → 9200: three points (each change stores the price before it, unless already there).
    assert_eq!(db.price_history(9, "gold").expect("history").len(), 3);
    t.reopen();
    t.db().flush().expect("flush");
    assert!(t.db().price_history(9, "gold").expect("history").is_empty());
    assert_eq!(t.db().stats().expect("stats").price_history_rows, Some(0));
}

/// How long a carrier's buy order has stood, from the reports the database compared: since a change, since it
/// appeared, at least since the market was first reported (or since the 30 days kept), or not known.
#[test]
fn how_long_an_order_stood() {
    let mut t = TempDb::new("since");
    let db = t.db();
    populate(db);
    let n = now();
    const D: i64 = 86_400;
    let carrier = |mid: u64, ts: i64, items: &[(&str, i64, i64, i64, i64)]| market(mid, "Beta", "B-CARRIER", ts, items);
    // Changed from 50,000 to 55,000 twelve days ago, reported unchanged since.
    for (ts, p) in [(n - 20 * D, 50_000), (n - 12 * D, 55_000), (n - 3 * D, 55_000), (n - 3600, 55_000)] {
        db.ingest(carrier(21, ts, &[("tritium", 0, p, 0, 9_000)]));
    }
    // The same price on every report for 25 days.
    for ts in [n - 25 * D, n - 10 * D, n - 7200] {
        db.ingest(carrier(22, ts, &[("tritium", 0, 101_000, 0, 8_000)]));
    }
    // A buy order that appeared since the report six days ago (which had only gold).
    db.ingest(carrier(23, n - 6 * D, &[("gold", 8_000, 0, 400, 0)]));
    db.ingest(carrier(23, n - 3 * 3600, &[("gold", 8_000, 0, 380, 0), ("tritium", 0, 62_000, 0, 5_000)]));
    // One report only.
    db.ingest(carrier(24, n - 5 * 3600, &[("tritium", 0, 58_000, 0, 2_500)]));
    // Followed for 40 days, changed 35 days ago: those changes are pruned (on the next start).
    for (ts, p) in [(n - 40 * D, 40_000), (n - 35 * D, 45_000), (n - 3600, 45_000)] {
        db.ingest(carrier(25, ts, &[("tritium", 0, p, 0, 1_000)]));
    }
    db.flush().expect("flush");
    let sell = |db: &GalaxyDb, mid: u64| db.price_since(mid, "tritium").expect("since").and_then(|s| s.sell);
    assert_eq!(sell(db, 21), Some(Stood::Since { since: n - 12 * D, before: n - 20 * D, previous: 50_000 }));
    assert_eq!(sell(db, 22), Some(Stood::AtLeast { since: n - 25 * D }));
    assert_eq!(sell(db, 23), Some(Stood::Since { since: n - 3 * 3600, before: n - 6 * D, previous: 0 }), "a new order");
    assert_eq!(sell(db, 24), Some(Stood::AtLeast { since: n - 5 * 3600 }));
    assert_eq!(db.price_since(21, "tritium").expect("since").and_then(|s| s.buy), None, "it doesn't sell tritium");
    assert_eq!(db.price_since(21, "gold").expect("since"), None, "not on its market");
    // The new order's points don't show as price changes of their own: the report before it lists nothing.
    let h = db.price_history(23, "tritium").expect("history");
    assert_eq!(h.iter().map(|p| (p.ts_unix, p.sell)).collect::<Vec<_>>(), vec![(n - 6 * D, 0), (n - 3 * 3600, 62_000)]);

    t.reopen();
    let db = t.db();
    db.flush().expect("flush");
    match sell(db, 25) {
        // At least the 30 days kept: what came before is gone.
        Some(Stood::AtLeast { since }) => assert!((since - (n - 30 * D)).abs() < 600, "{since} vs {}", n - 30 * D),
        other => panic!("{other:?}"),
    }
    assert_eq!(sell(db, 21), Some(Stood::Since { since: n - 12 * D, before: n - 20 * D, previous: 50_000 }), "kept");
}

/// The live feed's "updates stored" counts what changed the database: not a bare rock it doesn't keep, a market
/// report older than the one stored, or a report that changes nothing.
#[test]
fn counted_ingest_counts_what_was_stored() {
    let t = TempDb::new("counted");
    let db = t.db();
    populate(db);
    let stored = std::sync::Arc::new(AtomicU64::new(0));
    let body = |id: u32, class: &str, bio: u32| {
        Ingest::Body(BodyUpdate {
            system_id64: SYS_A,
            body_id: id,
            name: Some(format!("Alpha {id}")),
            scan: Some(BodyScan { planet_class: class.into(), landable: true, ..Default::default() }),
            signals: (bio > 0).then(|| BodySignals { bio, ..Default::default() }),
            timestamp: now() - 100,
            source: Source::Eddn,
            ..Default::default()
        })
    };
    let rock = body(3, "Rocky body", 0);
    let bio = body(4, "Rocky body", 2);
    let fresh = market(1, "Alpha", "A Port", now() - 10, &[("gold", 9_200, 9_000, 100, 0)]);
    let stale = market(1, "Alpha", "A Port", now() - 20, &[("gold", 9_300, 9_100, 100, 0)]);
    db.ingest_counted(vec![rock.clone(), bio.clone(), fresh, stale], stored.clone());
    db.flush().expect("flush");
    assert_eq!(stored.load(Ordering::Relaxed), 2, "the biology body and the newer market");
    // The same bodies again: nothing changes.
    db.ingest_counted(vec![rock, bio], stored.clone());
    db.flush().expect("flush");
    assert_eq!(stored.load(Ordering::Relaxed), 2);
    // A new system counts; uncounted ingests leave the counter alone.
    db.ingest_counted(vec![system(SYS_C + 10, "Delta", [5.0, 5.0, 5.0])], stored.clone());
    db.ingest(system(SYS_C + 11, "Epsilon", [6.0, 6.0, 6.0]));
    db.flush().expect("flush");
    assert_eq!(stored.load(Ordering::Relaxed), 3);
}

/// Without price history, nothing can be said about how long a price stood.
#[test]
fn no_history_no_since() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("galaxy-nohistory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = GalaxyDb::open_with(dir.join("galaxy.db"), GalaxyOptions { price_history: false, ..Default::default() }).expect("open");
        populate(&db);
        db.ingest(market(1, "Alpha", "A Port", now() - 10, &[("gold", 9_100, 8_900, 100, 0)]));
        db.flush().expect("flush");
        assert_eq!(db.price_since(1, "gold").expect("since").and_then(|s| s.buy), Some(Stood::Unknown));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn market_before_system_is_linked_later() {
    let t = TempDb::new("link");
    let db = t.db();
    db.ingest(market(
        42,
        "Nowhere",
        "Lonely Hub",
        now() - 5,
        &[("gold", 9000, 0, 10, 0)],
    ));
    db.flush().expect("flush");
    let st = db.station(42).expect("q").expect("station");
    assert_eq!(st.system_id64, None);
    let near = CommodityQuery {
        commodity: "gold".into(),
        origin: Some([500.0, 0.0, 0.0]),
        max_distance_ly: Some(20.0),
        ..Default::default()
    };
    assert!(db.search_commodity(&near).expect("q").is_empty());
    db.ingest(system(77, "NOWHERE", [500.0, 1.0, 0.0]));
    db.flush().expect("flush");
    let st = db.station(42).expect("q").expect("station");
    assert_eq!(st.system_id64, Some(77));
    assert_eq!(st.pos, Some([500.0, 1.0, 0.0]));
    let hits = db.search_commodity(&near).expect("q");
    assert_eq!(ids(&hits), vec![42]);
    assert_eq!(db.stations_in_system(77).expect("q").len(), 1);
}

#[test]
fn carrier_moves_with_its_market() {
    let t = TempDb::new("carrier");
    let db = t.db();
    populate(db);
    db.ingest(market(
        4,
        "Gamma",
        "B-CARRIER",
        now() - 1,
        &[("gold", 7000, 0, 10, 0)],
    ));
    db.flush().expect("flush");
    let st = db.station(4).expect("q").expect("carrier");
    assert_eq!(st.system, "Gamma");
    assert_eq!(st.system_id64, Some(SYS_C));
    assert!(st.is_carrier);
    let near_c = CommodityQuery {
        commodity: "gold".into(),
        origin: Some([0.0, 100.0, 0.0]),
        max_distance_ly: Some(1.0),
        ..Default::default()
    };
    assert_eq!(ids(&db.search_commodity(&near_c).expect("q")), vec![4, 3]);
}

#[test]
fn stations_near_with_services() {
    let t = TempDb::new("services");
    let db = t.db();
    populate(db);
    let base = StationQuery {
        origin: [0.0, 0.0, 0.0],
        include_carriers: true,
        ..Default::default()
    };
    let all = db.stations_near(&base).expect("q");
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].station.market_id, 1);
    let q = StationQuery {
        material_trader: Some(MaterialTrader::Manufactured),
        ..base.clone()
    };
    let hits = db.stations_near(&q).expect("q");
    assert_eq!(
        hits.iter().map(|h| h.station.market_id).collect::<Vec<_>>(),
        vec![1]
    );
    let q = StationQuery {
        material_trader: Some(MaterialTrader::Encoded),
        ..base.clone()
    };
    let hits = db.stations_near(&q).expect("q");
    assert_eq!(
        hits.iter().map(|h| h.station.market_id).collect::<Vec<_>>(),
        vec![3]
    );
    assert_eq!(hits[0].distance_ly, 100.0);
    let q = StationQuery {
        tech_broker: Some(TechBroker::Guardian),
        ..base.clone()
    };
    assert_eq!(db.stations_near(&q).expect("q")[0].station.market_id, 2);
    let q = StationQuery {
        services: Services::INTERSTELLAR_FACTORS,
        ..base.clone()
    };
    assert_eq!(db.stations_near(&q).expect("q").len(), 1);
    let q = StationQuery {
        max_distance_ly: Some(50.0),
        min_pad: Some('L'),
        include_carriers: false,
        ..base.clone()
    };
    let ids: Vec<u64> = db
        .stations_near(&q)
        .expect("q")
        .iter()
        .map(|h| h.station.market_id)
        .collect();
    assert_eq!(ids, vec![1, 5]);
    let q = StationQuery {
        max_dist_ls: Some(100.0),
        ..base
    };
    assert!(db.stations_near(&q).expect("q").is_empty());
}

#[test]
fn trade_routes_find_the_best_hop() {
    let t = TempDb::new("trade");
    let db = t.db();
    let ts = now() - 60;
    db.ingest_many(vec![
        system(SYS_A, "Alpha", [0.0, 0.0, 0.0]),
        system(SYS_B, "Beta", [10.0, 0.0, 0.0]),
        system(SYS_C, "Gamma", [0.0, 100.0, 0.0]),
        station(
            1,
            "A Port",
            SYS_A,
            "Alpha",
            "Coriolis",
            &["commodities"],
            "Industrial",
        ),
        station(
            2,
            "B Port",
            SYS_B,
            "Beta",
            "Orbis",
            &["commodities"],
            "Refinery",
        ),
        station(
            3,
            "C Port",
            SYS_C,
            "Gamma",
            "Orbis",
            &["commodities"],
            "Refinery",
        ),
        market(
            1,
            "Alpha",
            "A Port",
            ts,
            &[
                ("gold", 9000, 8900, 1000, 0),
                ("silver", 4000, 3900, 1000, 0),
            ],
        ),
        market(
            2,
            "Beta",
            "B Port",
            ts,
            &[("gold", 0, 12000, 0, 1000), ("silver", 0, 4100, 0, 1000)],
        ),
        market(3, "Gamma", "C Port", ts, &[("gold", 0, 20000, 0, 1000)]),
    ]);
    db.flush().expect("flush");
    let q = TradeQuery {
        origin: [0.0, 0.0, 0.0],
        radius_ly: 50.0,
        max_hop_ly: 20.0,
        cargo_capacity: 100,
        ..Default::default()
    };
    let routes = db.trade_routes(&q).expect("routes");
    assert_eq!(routes.len(), 1, "one route per station pair: {routes:?}");
    let r = &routes[0];
    assert_eq!((r.from.market_id, r.to.market_id), (1, 2));
    assert_eq!(r.commodity, "gold");
    assert_eq!(r.commodity_name, "Gold");
    assert_eq!(r.profit_per_ton, 3000);
    assert_eq!(r.units, 100);
    assert_eq!(r.total_profit, 300_000);
    assert_eq!(r.hop_ly, 10.0);
    let q = TradeQuery {
        capital: Some(450_000),
        ..q.clone()
    };
    assert_eq!(db.trade_routes(&q).expect("routes")[0].units, 50);
    let q = TradeQuery {
        max_hop_ly: 5.0,
        ..q
    };
    assert!(db.trade_routes(&q).expect("routes").is_empty());
    // bigger radius: the far, better buyer is in reach of nothing within 20 ly, but within 150 ly
    let q = TradeQuery {
        origin: [0.0, 0.0, 0.0],
        radius_ly: 200.0,
        max_hop_ly: 150.0,
        cargo_capacity: 10,
        ..Default::default()
    };
    let routes = db.trade_routes(&q).expect("routes");
    assert_eq!((routes[0].from.market_id, routes[0].to.market_id), (1, 3));
    assert_eq!(routes[0].total_profit, 110_000);
}

#[test]
fn trade_routes_from_one_station() {
    let t = TempDb::new("trade-from");
    let db = t.db();
    let ts = now() - 60;
    let mut batch = vec![
        system(SYS_A, "Alpha", [0.0, 0.0, 0.0]),
        system(SYS_B, "Beta", [10.0, 0.0, 0.0]),
        station(1, "A Port", SYS_A, "Alpha", "Coriolis", &["commodities"], "Industrial"),
        station(2, "A Carrier", SYS_A, "Alpha", "FleetCarrier", &["commodities"], "Carrier"),
        // Two hours old.
        market(1, "Alpha", "A Port", ts - 7200, &[("gold", 9000, 8900, 1000, 0)]),
        market(2, "Alpha", "A Carrier", ts, &[("silver", 4000, 3900, 1000, 0)]),
    ];
    // Five buyers of gold (and silver) in Beta, the best price first.
    for i in 0..5u64 {
        let name = format!("B Port {i}");
        let sell = 12_000 - i as i64 * 100;
        batch.push(station(10 + i, &name, SYS_B, "Beta", "Orbis", &["commodities"], "Refinery"));
        batch.push(market(10 + i, "Beta", &name, ts, &[("gold", 0, sell, 0, 1000), ("silver", 0, 4500, 0, 1000)]));
    }
    db.ingest_many(batch);
    db.flush().expect("flush");
    let q = TradeQuery {
        origin: [0.0, 0.0, 0.0],
        radius_ly: 50.0,
        max_hop_ly: 20.0,
        cargo_capacity: 100,
        ..Default::default()
    };
    let ends = |q: &TradeQuery| -> Vec<(u64, u64, String)> {
        db.trade_routes(q)
            .expect("routes")
            .into_iter()
            .map(|r| (r.from.market_id, r.to.market_id, r.commodity))
            .collect()
    };
    let around = ends(&q);
    assert_eq!(around.len(), 3, "three buyers per seller: {around:?}");
    assert!(around.iter().all(|(from, _, c)| *from == 1 && c == "gold"), "carriers are left out: {around:?}");
    // From A Port, every buyer within reach.
    let here = ends(&TradeQuery { from_market: Some(1), ..q.clone() });
    assert_eq!(
        here.iter().map(|(_, to, _)| *to).collect::<Vec<_>>(),
        [10, 11, 12, 13, 14]
    );
    // Docked at the carrier: its routes, though the search leaves carriers out.
    let carrier = ends(&TradeQuery { from_market: Some(2), ..q.clone() });
    assert_eq!(carrier.len(), 5);
    assert!(carrier.iter().all(|(from, _, c)| *from == 2 && c == "silver"));
    // A station with no known market has none, nor one whose prices are older than the age limit.
    assert!(ends(&TradeQuery { from_market: Some(99), ..q.clone() }).is_empty());
    let hour = |from| TradeQuery {
        from_market: Some(from),
        max_age_hours: Some(1.0),
        ..q.clone()
    };
    assert!(ends(&hour(1)).is_empty(), "A Port's prices are two hours old");
    assert_eq!(ends(&hour(2)).len(), 5);
}

#[test]
fn systems_and_factions() {
    let t = TempDb::new("systems");
    let db = t.db();
    populate(db);
    let s = db.system_by_name("beta").expect("q").expect("beta");
    assert_eq!(s.id64, SYS_B);
    assert_eq!(s.population, Some(1_000_000));
    let near = db.systems_near([0.0, 0.0, 0.0], 50.0, 10).expect("q");
    assert_eq!(
        near.iter().map(|h| h.system.id64).collect::<Vec<_>>(),
        vec![SYS_A, SYS_B]
    );
    assert_eq!(db.find_systems("Ga", 10).expect("q")[0].name, "Gamma");
    // FSDJump from EDDN with factions, newer than populate()
    let ts = nexus_journal::Timestamp(now() - 5).to_iso();
    let v = json!({
        "$schemaRef": "https://eddn.edcd.io/schemas/journal/1",
        "header": {"uploaderID": "x", "softwareName": "test", "softwareVersion": "1"},
        "message": {"timestamp": ts, "event": "FSDJump", "StarSystem": "Beta", "SystemAddress": SYS_B,
            "StarPos": [10.0, 0.0, 0.0], "SystemAllegiance": "Federation", "SystemGovernment": "$government_Democracy;",
            "Population": 5000, "SystemFaction": {"Name": "Beta Party", "FactionState": "Boom"},
            "Factions": [{"Name": "Beta Party", "FactionState": "Boom", "Government": "Democracy", "Influence": 0.6,
                          "Allegiance": "Federation", "ActiveStates": [{"State": "Boom"}]},
                         {"Name": "Beta Pirates", "FactionState": "None", "Government": "Anarchy", "Influence": 0.4,
                          "Allegiance": "Independent"}]}
    });
    assert_eq!(db.ingest_eddn(&v), 1);
    db.flush().expect("flush");
    let s = db.system_by_id(SYS_B).expect("q").expect("beta");
    assert_eq!(s.controlling_faction.as_deref(), Some("Beta Party"));
    assert_eq!(s.controlling_faction_state.as_deref(), Some("Boom"));
    assert_eq!(s.population, Some(5000));
    assert_eq!(s.government.as_deref(), Some("Democracy"));
    let f = db.system_factions(SYS_B).expect("q");
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].name, "Beta Party");
    assert_eq!(f[0].influence, Some(0.6));
    assert_eq!(f[0].active_states, vec!["Boom".to_string()]);
    // an older FSDJump does not overwrite newer state
    let mut old = v.clone();
    old["message"]["timestamp"] = json!(nexus_journal::Timestamp(now() - 3600).to_iso());
    old["message"]["Population"] = json!(1);
    db.ingest_eddn(&old);
    db.flush().expect("flush");
    assert_eq!(
        db.system_by_id(SYS_B).expect("q").expect("beta").population,
        Some(5000)
    );
}

#[test]
fn factions_replace_on_newer_info() {
    let t = TempDb::new("factions");
    let db = t.db();
    let mk = |ts: i64, names: &[&str]| {
        Ingest::System(SystemUpdate {
            id64: 5,
            name: "Five".into(),
            pos: Some([1.0, 2.0, 3.0]),
            timestamp: ts,
            details: Some(SystemDetails {
                controlling_faction: Some(names[0].to_string()),
                factions: Some(
                    names
                        .iter()
                        .map(|n| FactionUpdate {
                            name: n.to_string(),
                            influence: Some(0.5),
                            ..Default::default()
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
        })
    };
    db.ingest(mk(1000, &["One", "Two"]));
    db.ingest(mk(2000, &["Three"]));
    db.ingest(mk(1500, &["Stale"]));
    db.flush().expect("flush");
    let f = db.system_factions(5).expect("q");
    assert_eq!(
        f.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        vec!["Three"]
    );
    assert_eq!(
        db.system_by_id(5)
            .expect("q")
            .expect("sys")
            .controlling_faction
            .as_deref(),
        Some("Three")
    );
}

#[test]
fn ring_hotspots() {
    let t = TempDb::new("rings");
    let db = t.db();
    db.ingest(system(9, "Ringworld", [3.0, 4.0, 0.0]));
    let saa = json!({"timestamp": "2026-10-12T09:00:00Z", "event": "SAASignalsFound", "BodyName": "Ringworld 1 A Ring",
        "SystemAddress": 9, "BodyID": 7,
        "Signals": [{"Type": "Painite", "Count": 2}, {"Type": "LowTemperatureDiamond", "Count": 1}]});
    assert_eq!(db.ingest_journal("SAASignalsFound", &saa), 1);
    db.flush().expect("flush");
    let q = HotspotQuery {
        origin: [0.0, 0.0, 0.0],
        signal: "painite".into(),
        ..Default::default()
    };
    let hits = db.hotspots_near(&q).expect("q");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].distance_ly, 5.0);
    assert_eq!(hits[0].count, 2);
    assert_eq!(hits[0].system, "Ringworld");
    let q = HotspotQuery {
        origin: [0.0, 0.0, 0.0],
        signal: "Low Temperature Diamonds".into(),
        ..Default::default()
    };
    assert_eq!(db.hotspots_near(&q).expect("q").len(), 1);
    let q = HotspotQuery {
        origin: [0.0, 0.0, 0.0],
        signal: "painite".into(),
        min_count: 3,
        ..Default::default()
    };
    assert!(db.hotspots_near(&q).expect("q").is_empty());
}

#[test]
fn updates_are_published_after_commit() {
    let t = TempDb::new("updates");
    let db = t.db();
    let rx = db.subscribe_updates();
    populate(db);
    let got: Vec<MarketUpdate> = rx.try_iter().collect();
    assert_eq!(got.len(), 5);
    let u = got.iter().find(|u| u.market_id == 3).expect("update for 3");
    assert_eq!(u.system, "Gamma");
    assert_eq!(u.pos, Some([0.0, 100.0, 0.0]));
    assert_eq!(u.commodities, 2);
    let st = db.stats().expect("stats");
    assert_eq!(st.stations, 5);
    assert_eq!(st.stations_with_market, 5);
    assert_eq!(st.market_rows, 9);
    assert_eq!(st.systems, 3);
    assert!(st.commodities > 350);
    assert!(st.db_bytes > 0);
    assert_eq!(db.recent_markets(2).expect("q").len(), 2);
    assert_eq!(db.find_stations("b", 10).expect("q").len(), 3);
    assert_eq!(db.find_stations("B cr", 10).expect("q")[0].market_id, 5);
    assert!(db.find_stations("100%", 10).expect("q").is_empty());
}

#[test]
fn eddn_messages_end_to_end() {
    let t = TempDb::new("eddn");
    let db = t.db();
    let fsd = json!({"$schemaRef": "https://eddn.edcd.io/schemas/journal/1", "header": {"uploaderID": "x", "softwareName": "t", "softwareVersion": "1"},
        "message": {"timestamp": "2026-10-12T09:00:00Z", "event": "FSDJump", "StarSystem": "Sol", "SystemAddress": 10477373803u64,
                    "StarPos": [0.0, 0.0, 0.0], "Population": 18320926115u64, "SystemEconomy": "$economy_Refinery;"}});
    let docked = json!({"$schemaRef": "https://eddn.edcd.io/schemas/journal/1", "header": {"uploaderID": "x", "softwareName": "t", "softwareVersion": "1"},
        "message": {"timestamp": "2026-10-12T09:01:00Z", "event": "Docked", "StationName": "Daedalus", "StationType": "Orbis",
                    "StarSystem": "Sol", "SystemAddress": 10477373803u64, "MarketID": 128016384u64, "StarPos": [0.0, 0.0, 0.0],
                    "StationServices": ["dock", "commodities", "shipyard", "outfitting", "blackmarket"],
                    "StationEconomy": "$economy_Refinery;", "DistFromStarLS": 230.4,
                    "LandingPads": {"Small": 10, "Medium": 13, "Large": 6}}});
    let commodity = json!({"$schemaRef": "https://eddn.edcd.io/schemas/commodity/3", "header": {"uploaderID": "x", "softwareName": "t", "softwareVersion": "1"},
        "message": {"systemName": "Sol", "stationName": "Daedalus", "marketId": 128016384u64, "timestamp": "2026-10-12T09:02:00Z",
                    "commodities": [{"name": "tritium", "meanPrice": 50000, "buyPrice": 0, "stock": 0, "stockBracket": 0,
                                     "sellPrice": 51000, "demand": 3000, "demandBracket": 2}]}});
    let mut test = commodity.clone();
    test["$schemaRef"] = json!("https://eddn.edcd.io/schemas/commodity/3/test");
    assert_eq!(db.ingest_eddn(&test), 0);
    db.ingest(Ingest::Raw(RawIngest::Eddn(fsd)));
    assert_eq!(db.ingest_eddn(&docked), 2);
    assert_eq!(db.ingest_eddn(&commodity), 1);
    db.flush().expect("flush");
    let m = db.station_market(128016384).expect("q").expect("Daedalus");
    assert_eq!(m.station.system_id64, Some(10477373803));
    assert_eq!(m.station.pos, Some([0.0, 0.0, 0.0]));
    assert_eq!(m.station.max_pad, Some('L'));
    assert_eq!(m.station.dist_ls, Some(230.4));
    assert!(
        m.station
            .services
            .contains(Services::SHIPYARD | Services::BLACK_MARKET)
    );
    assert_eq!(m.items.len(), 1);
    assert_eq!(
        m.system.as_ref().and_then(|s| s.economy.clone()).as_deref(),
        Some("Refinery")
    );
    let q = CommodityQuery {
        commodity: "Tritium".into(),
        mode: TradeMode::SellToStation,
        origin: Some([1.0, 1.0, 1.0]),
        max_distance_ly: Some(10.0),
        ..Default::default()
    };
    let hits = db.search_commodity(&q).expect("q");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].price, 51000);
}

// ---------------------------------------------------------------------------------------------
// Bodies (footfall, signals)
// ---------------------------------------------------------------------------------------------

const SWOILZ: u64 = 908552967066;

fn eddn_msg(schema: &str, message: serde_json::Value) -> serde_json::Value {
    json!({"$schemaRef": format!("https://eddn.edcd.io/schemas/{schema}"),
           "header": {"uploaderID": "x", "softwareName": "t", "softwareVersion": "1", "gameversion": "4.4.1.1"},
           "message": message})
}

/// A landable planet `Scan` as EDDN relays it.
fn planet_scan(ts: i64, body_id: u32, name: &str, footfalled: bool) -> serde_json::Value {
    json!({"timestamp": nexus_journal::Timestamp(ts).to_iso(), "event": "Scan", "ScanType": "Detailed", "BodyName": name,
        "BodyID": body_id, "StarSystem": "Swoilz MH-W c17-3", "SystemAddress": SWOILZ, "DistanceFromArrivalLS": 1209.4,
        "TerraformState": "", "PlanetClass": "High metal content body", "Atmosphere": "thin carbon dioxide atmosphere",
        "AtmosphereType": "CarbonDioxide", "Volcanism": "", "SurfaceGravity": 0.876, "SurfaceTemperature": 165.2,
        "SurfacePressure": 1107.5, "Landable": true, "WasDiscovered": true, "WasMapped": false, "WasFootfalled": footfalled,
        "StarPos": [-28.8, -112.4, -96.9]})
}

#[test]
fn bodies_merge_footfall_and_signals() {
    let mut t = TempDb::new("bodies");
    let db = t.db();
    let t0 = now() - 10 * 86_400;
    let a4 = "Swoilz MH-W c17-3 A 4";
    // FSS signals arrive first (as in the journal), then the scan: a biology body, kept from EDDN.
    let fss = eddn_msg(
        "fssbodysignals/1",
        json!({"timestamp": nexus_journal::Timestamp(t0).to_iso(), "event": "FSSBodySignals", "BodyName": a4, "BodyID": 6,
            "SystemAddress": SWOILZ, "StarSystem": "Swoilz MH-W c17-3", "StarPos": [-28.8, -112.4, -96.9],
            "Signals": [{"Type": "$SAA_SignalType_Biological;", "Count": 4}]}),
    );
    assert_eq!(db.ingest_eddn(&fss), 1);
    assert_eq!(db.ingest_eddn(&eddn_msg("journal/1", planet_scan(t0 + 1, 6, a4, false))), 1);
    // Bare rock from EDDN is not kept; our own scan of it is.
    db.ingest_eddn(&eddn_msg("journal/1", planet_scan(t0 + 2, 7, "Swoilz MH-W c17-3 A 5", false)));
    db.ingest_journal("Scan", &planet_scan(t0 + 3, 8, "Swoilz MH-W c17-3 A 6", false));
    db.flush().expect("flush");

    let bodies = db.bodies_in_system(SWOILZ).expect("q");
    assert_eq!(bodies.iter().map(|b| b.body_id).collect::<Vec<_>>(), vec![6, 8], "{bodies:#?}");
    let b = &bodies[0];
    assert_eq!(b.name.as_deref(), Some(a4));
    assert_eq!(b.bio_signals, Some(4));
    assert_eq!(b.planet_class.as_deref(), Some("High metal content body"));
    assert_eq!(b.atmosphere_type.as_deref(), Some("CarbonDioxide"));
    assert!((b.gravity_g.expect("g") - 0.0893).abs() < 1e-3);
    assert_eq!(b.landable, Some(true));
    assert_eq!(b.was_footfalled, Some(Seen { value: false, at_unix: t0 + 1 }));
    assert_eq!(b.footfall_source, Some(Source::Eddn));
    assert_eq!(b.was_discovered.map(|s| s.value), Some(true));
    assert_eq!(bodies[1].source, Source::Journal);
    assert!(db.system_by_id(SWOILZ).expect("q").is_none(), "Scan/FSSBodySignals add no system rows");

    // DSS genera, then another commander samples organics there: footfalled.
    let dss = eddn_msg(
        "journal/1",
        json!({"timestamp": nexus_journal::Timestamp(t0 + 100).to_iso(), "event": "SAASignalsFound", "BodyName": a4,
            "SystemAddress": SWOILZ, "BodyID": 6, "StarSystem": "Swoilz MH-W c17-3", "StarPos": [-28.8, -112.4, -96.9],
            "Signals": [{"Type": "$SAA_SignalType_Biological;", "Count": 4}],
            "Genuses": [{"Genus": "$Codex_Ent_Bacterial_Genus_Name;"}, {"Genus": "$Codex_Ent_Stratum_Genus_Name;"},
                        {"Genus": "$Codex_Ent_Tubus_Genus_Name;"}, {"Genus": "$Codex_Ent_Tussocks_Genus_Name;"}]}),
    );
    db.ingest_eddn(&dss);
    let organic = eddn_msg(
        "scanorganic/1",
        json!({"timestamp": nexus_journal::Timestamp(t0 + 5000).to_iso(), "event": "ScanOrganic", "ScanType": "Log",
            "Genus": "$Codex_Ent_Stratum_Genus_Name;", "Species": "$Codex_Ent_Stratum_07_Name;", "SystemAddress": SWOILZ,
            "BodyID": 6, "StarSystem": "Swoilz MH-W c17-3", "StarPos": [-28.8, -112.4, -96.9]}),
    );
    db.ingest_eddn(&organic);
    // A scan made before the landing but delivered late (and a newer FSS count) can't undo it.
    db.ingest_eddn(&eddn_msg("journal/1", planet_scan(t0 + 4000, 6, a4, false)));
    db.ingest_eddn(&fss);
    db.flush().expect("flush");
    let b = db.body_by_name("swoilz mh-w c17-3 a 4").expect("q").expect("A 4");
    assert_eq!(b.genera.len(), 4);
    assert_eq!(b.was_footfalled, Some(Seen { value: true, at_unix: t0 + 5000 }));
    assert_eq!(b.updated_unix, t0 + 5000);
    // An EDDN scan of a body we already have (bare rock we scanned ourselves) updates it.
    db.ingest_eddn(&eddn_msg("journal/1", planet_scan(t0 + 6000, 8, "Swoilz MH-W c17-3 A 6", true)));
    db.flush().expect("flush");
    let a6 = db.body_by_name("Swoilz MH-W c17-3 A 6").expect("q").expect("A 6");
    assert_eq!(a6.was_footfalled, Some(Seen { value: true, at_unix: t0 + 6000 }));
    assert_eq!(a6.footfall_source, Some(Source::Eddn));
    assert!(db.body_by_name("Swoilz MH-W c17-3 A 5").expect("q").is_none());
    // Prefix search finds a system's bodies (e.g. when the system itself isn't stored).
    let found = db.find_bodies("SWOILZ MH-W C17-3 ", 10).expect("q");
    assert_eq!(found.len(), 2);
    assert!(db.find_bodies("Swoilz MH-W c17-30 ", 10).expect("q").is_empty());

    // Replaying the same observations (the journal history at every start) writes nothing new.
    let before = db.bodies_in_system(SWOILZ).expect("q");
    db.ingest_journal("Scan", &planet_scan(t0 + 3, 8, "Swoilz MH-W c17-3 A 6", false));
    db.ingest_eddn(&dss);
    db.flush().expect("flush");
    assert_eq!(db.bodies_in_system(SWOILZ).expect("q"), before);
    t.reopen();
    assert_eq!(t.db().bodies_in_system(SWOILZ).expect("q"), before);
}

#[test]
fn all_eddn_bodies_option_keeps_bare_rock() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("galaxy-allbodies-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let opts = GalaxyOptions { all_eddn_bodies: true, ..GalaxyOptions::default() };
        let db = GalaxyDb::open_with(dir.join("galaxy.db"), opts).expect("open");
        db.ingest_eddn(&eddn_msg("journal/1", planet_scan(now() - 60, 7, "Swoilz MH-W c17-3 A 5", false)));
        db.flush().expect("flush");
        assert_eq!(db.bodies_in_system(SWOILZ).expect("q").len(), 1);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn handle_is_clone_send_sync() {
    fn check<T: Clone + Send + Sync + 'static>() {}
    check::<GalaxyDb>();
    check::<MarketUpdate>();
}

#[test]
fn data_survives_reopen() {
    let mut t = TempDb::new("reopen");
    populate(t.db());
    t.reopen();
    assert_eq!(t.db().stats().expect("stats").stations, 5);
    let q = CommodityQuery {
        commodity: "gold".into(),
        ..Default::default()
    };
    assert_eq!(t.db().search_commodity(&q).expect("q").len(), 5);
}

#[test]
fn reads_run_while_writing() {
    let t = TempDb::new("concurrent");
    let db = t.db().clone();
    populate(&db);
    let writer = {
        let db = db.clone();
        std::thread::spawn(move || {
            for i in 0..400u64 {
                db.ingest(market(
                    1000 + i,
                    "Alpha",
                    &format!("Station {i}"),
                    now() - 5,
                    &[("gold", 9000 + i as i64, 0, 10, 0)],
                ));
            }
            db.flush().expect("flush");
        })
    };
    let q = CommodityQuery {
        commodity: "gold".into(),
        origin: Some([0.0; 3]),
        max_distance_ly: Some(20.0),
        ..Default::default()
    };
    for _ in 0..200 {
        db.search_commodity(&q).expect("query during writes");
    }
    writer.join().expect("writer thread");
    let q = CommodityQuery { limit: 1000, ..q };
    assert_eq!(db.search_commodity(&q).expect("q").len(), 404);
}

// ---------------------------------------------------------------------------------------------
// Spansh import
// ---------------------------------------------------------------------------------------------

fn sample(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/research/spansh-samples")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("sample json")
}

/// Records in the dump framing: `[`, one tab-indented record per line with trailing commas, `]`.
fn dump_framing(records: &[serde_json::Value]) -> Vec<u8> {
    let mut s = String::from("[\n");
    for (i, r) in records.iter().enumerate() {
        s.push('\t');
        s.push_str(&r.to_string());
        if i + 1 < records.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("]\n");
    s.into_bytes()
}

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).expect("gzip");
    e.finish().expect("gzip")
}

#[test]
fn spansh_galaxy_records_import() {
    let t = TempDb::new("spansh");
    let db = t.db();
    let mut records = Vec::new();
    records.extend(
        sample("galaxy_populated.first_record.trimmed.json")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    records.extend(
        sample("galaxy_stations.first_record.trimmed.json")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    records.push(sample("dump_sol.response.trimmed.json")); // {"system": {...}} wrapper
    records.extend(
        sample("systems_1day.first_records.json")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    records.extend(
        sample("factions.first_record.trimmed.json")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let data = gzip(&dump_framing(&records));
    let mut calls = 0;
    let summary = db
        .import_spansh_dump(&data[..], |p| {
            calls += 1;
            assert!(p.bytes_read > 0);
            true
        })
        .expect("import");
    assert!(calls >= 1);
    assert!(!summary.cancelled);
    assert_eq!(summary.stream_error, None);
    let p = &summary.progress;
    assert_eq!(p.parse_errors, 0);
    assert_eq!(p.records, records.len() as u64);
    assert_eq!(p.bytes_read, data.len() as u64);
    assert_eq!(p.systems, 6); // populated + stations + Sol + 3 systems records
    assert_eq!(p.stations, 2 + 3 + 3); // HD 219286 + carrier + Sol orbital + Mercury surface
    assert_eq!(p.faction_presences, 2);

    let sol = db.system_by_name("Sol").expect("q").expect("Sol");
    assert_eq!(sol.pos, Some([0.0, 0.0, 0.0]));
    assert_eq!(sol.controlling_power.as_deref(), Some("Jerome Archer"));
    assert_eq!(sol.powerplay_state.as_deref(), Some("Stronghold"));
    assert_eq!(sol.security.as_deref(), Some("High"));
    assert_eq!(sol.controlling_faction.as_deref(), Some("Mother Gaia"));
    let factions = db.system_factions(sol.id64).expect("q");
    assert_eq!(factions[0].name, "Mother Gaia");
    assert_eq!(
        factions[0].recovering_states,
        vec!["Public Holiday".to_string()]
    );

    let daedalus = db.station_market(128016384).expect("q").expect("Daedalus");
    assert_eq!(
        daedalus.station.station_type.as_deref(),
        Some("Orbis Starport")
    );
    assert_eq!(daedalus.station.max_pad, Some('L'));
    assert_eq!(daedalus.items.len(), 3);
    assert!(
        daedalus
            .items
            .iter()
            .any(|i| i.symbol == "advancedcatalysers" && i.sell == 3501 && i.demand == 2855292)
    );
    assert_eq!(daedalus.prohibited.len(), 3);
    let walz = db.station(3534389760).expect("q").expect("Walz Depot");
    assert!(walz.is_planetary);
    assert_eq!(walz.station_type.as_deref(), Some("Planetary Outpost"));
    let carrier = db.station(3708716800).expect("q").expect("carrier");
    assert!(carrier.is_carrier);
    assert_eq!(carrier.pos, Some([-34608.78125, 513.34375, 26058.59375]));
    let q = CommodityQuery {
        commodity: "tritium".into(),
        ..Default::default()
    };
    assert_eq!(ids(&db.search_commodity(&q).expect("q")), vec![3708716800]);
    // systems dump: coordinates only
    let ga = db
        .system_by_name("Great Annihilator")
        .expect("q")
        .expect("GA");
    assert_eq!(ga.pos, Some([354.84375, -42.4375, 22997.21875]));
    assert_eq!(ga.updated_unix, 0, "no state from a systems dump");
    // factions dump: presence + controlling faction (the system is only known from that dump)
    let hip = db
        .system_by_name("HIP 104256")
        .expect("q")
        .expect("HIP 104256");
    assert_eq!(
        hip.controlling_faction.as_deref(),
        Some("\"Fearless\" Bounty Hunters Corp.")
    );

    // importing the same dump again changes nothing (markets are not newer)
    let again = db.import_spansh_dump(&data[..], |_| true).expect("import");
    assert_eq!(again.stale_markets, p.markets);
}

#[test]
fn spansh_import_plain_cancel_and_truncation() {
    let t = TempDb::new("spansh2");
    let db = t.db();
    let records = sample("systems_1day.first_records.json")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let plain = dump_framing(&records);
    let s = db
        .import_spansh_dump(&plain[..], |_| false)
        .expect("import");
    assert!(s.cancelled);
    assert_eq!(
        s.progress.systems, 3,
        "the first chunk is imported before the callback"
    );
    // truncated gzip: what was read is imported, the error is reported
    let gz = gzip(&dump_framing(&records));
    let s = db
        .import_spansh_dump(&gz[..gz.len() - 10], |_| true)
        .expect("import");
    assert!(s.stream_error.is_some());
    // garbage lines are counted, not fatal
    let s = db.import_spansh_dump(&b"[\n\t{\"id64\": 5, \"name\": \"X\", \"coords\": {\"x\": 1, \"y\": 2, \"z\": 3}},\n\tnot json,\n]\n"[..], |_| true)
        .expect("import");
    assert_eq!(s.progress.parse_errors, 1);
    assert_eq!(s.progress.systems, 1);
}

/// (symbol, buy, sell, mean, stock, demand)
fn priced(mid: u64, sys_name: &str, name: &str, kind: Option<&str>, items: &[(&str, i64, i64, i64, i64, i64)]) -> Ingest {
    Ingest::Market(MarketSnapshot {
        market_id: mid,
        system_name: sys_name.into(),
        station_name: name.into(),
        station_type: kind.map(Into::into),
        timestamp: now() - 30,
        items: items
            .iter()
            .map(|&(s, buy, sell, mean, stock, demand)| MarketItem {
                symbol: s.into(),
                buy,
                sell,
                mean,
                stock,
                stock_bracket: Some(2),
                demand,
                demand_bracket: Some(2),
            })
            .collect(),
        ..Default::default()
    })
}

#[test]
fn price_deals_rank_by_value_and_fill_carrier_averages() {
    let t = TempDb::new("deals");
    let db = t.db();
    let carrier = 3_700_000_001u64;
    db.ingest_many(vec![
        system(SYS_A, "Alpha", [0.0, 0.0, 0.0]),
        system(SYS_B, "Beta", [10.0, 0.0, 0.0]),
        system(SYS_C, "Gamma", [100.0, 0.0, 0.0]),
        station(11, "Alpha Port", SYS_A, "Alpha", "Coriolis", &["dock", "commodities"], "Industrial"),
        station(12, "Beta Hub", SYS_B, "Beta", "Coriolis", &["dock", "commodities"], "Industrial"),
        station(13, "Far Out", SYS_C, "Gamma", "Coriolis", &["dock", "commodities"], "Industrial"),
        station(carrier, "X9X-99X", SYS_B, "Beta", "FleetCarrier", &["dock", "commodities"], "Carrier"),
        // Alpha: gold only 20 % over average (below the threshold); painite 50 % over.
        priced(11, "Alpha", "Alpha Port", Some("Coriolis"), &[("gold", 0, 60_000, 50_000, 0, 1000), ("painite", 0, 600_000, 400_000, 0, 60)]),
        // Beta: gold 40 % over; silver sold at a fifth of the average; tritium sold slightly cheap.
        priced(
            12,
            "Beta",
            "Beta Hub",
            Some("Coriolis"),
            &[("gold", 0, 70_000, 50_000, 0, 2000), ("silver", 1_000, 0, 5_000, 10_000, 0), ("tritium", 40_000, 0, 50_000, 100, 0)],
        ),
        // The carrier pays double for tritium but reports no average (as carriers do).
        priced(carrier, "Beta", "X9X-99X", Some("FleetCarrier"), &[("tritium", 0, 100_000, 0, 0, 1000)]),
        // Far away: the best gold price, outside the radius.
        priced(13, "Gamma", "Far Out", Some("Coriolis"), &[("gold", 0, 200_000, 50_000, 0, 1000)]),
    ]);
    db.flush().expect("flush");

    let q = DealQuery { origin: Some([0.0; 3]), max_distance_ly: Some(50.0), cargo_capacity: 500, ..Default::default() };
    let sell = db.price_deals(&q).expect("deals");
    let names: Vec<(&str, &str)> = sell.iter().map(|d| (d.commodity.as_str(), d.station.as_str())).collect();
    assert_eq!(names, vec![("tritium", "X9X-99X"), ("painite", "Alpha Port"), ("gold", "Beta Hub")]);
    assert_eq!(sell[0].average, 50_000, "carrier row uses the average other stations report");
    assert_eq!(sell[0].extra_value, 50_000 * 500, "capped by the hold");
    assert_eq!(sell[1].extra_value, 200_000 * 60, "capped by demand");
    assert!((sell[2].ratio - 1.4).abs() < 1e-9);

    let no_carriers = db.price_deals(&DealQuery { include_carriers: false, ..q.clone() }).expect("deals");
    assert!(no_carriers.iter().all(|d| !d.is_carrier));

    let buy = db.price_deals(&DealQuery { mode: TradeMode::BuyFromStation, ..q.clone() }).expect("deals");
    let names: Vec<&str> = buy.iter().map(|d| d.commodity.as_str()).collect();
    assert_eq!(names, vec!["silver", "tritium"], "silver at a fifth of the average beats tritium at 80 %");
    assert_eq!(buy[0].extra_value, 4_000 * 500);

    let anywhere = db.price_deals(&DealQuery { max_distance_ly: None, ..q }).expect("deals");
    assert_eq!(anywhere[0].station, "Far Out", "150 000 Cr over average beats the carrier when distance does not matter");
}

#[test]
fn commodity_summary_and_latest_prices() {
    let t = TempDb::new("summary");
    let db = t.db();
    let carrier = 3_700_000_002u64;
    let ts = now() - 600;
    db.ingest_many(vec![
        system(SYS_A, "Alpha", [0.0, 0.0, 0.0]),
        system(SYS_B, "Beta", [10.0, 0.0, 0.0]),
        system(SYS_C, "Gamma", [100.0, 0.0, 0.0]),
        station(21, "Alpha Port", SYS_A, "Alpha", "Coriolis", &["dock", "commodities"], "Industrial"),
        station(22, "Beta Hub", SYS_B, "Beta", "Outpost", &["dock", "commodities"], "Industrial"),
        station(23, "Far Out", SYS_C, "Gamma", "Coriolis", &["dock", "commodities"], "Industrial"),
        station(carrier, "Q2Q-22Q", SYS_B, "Beta", "FleetCarrier", &["dock", "commodities"], "Carrier"),
        // Alpha sells gold, Beta and Far Out buy it, the carrier buys it too (no average reported).
        market(21, "Alpha", "Alpha Port", ts - 300, &[("gold", 9_000, 8_800, 500, 0)]),
        market(22, "Beta", "Beta Hub", ts - 200, &[("gold", 0, 9_500, 0, 40)]),
        market(23, "Gamma", "Far Out", ts - 100, &[("gold", 0, 9_900, 0, 2_000)]),
        market(carrier, "Beta", "Q2Q-22Q", ts, &[("gold", 0, 12_000, 0, 10)]),
    ]);
    // Alpha re-priced, listing its own gold the way the game does: demand 1, demand bracket 0 ("no demand").
    let mut producer = priced(21, "Alpha", "Alpha Port", Some("Coriolis"), &[("gold", 9_000, 8_800, 9_400, 500, 1)]);
    if let Ingest::Market(m) = &mut producer {
        m.items[0].demand_bracket = Some(0);
    }
    db.ingest(producer);
    db.flush().expect("flush");

    let s = db.commodity_summary("Gold", Some(72.0)).expect("summary").expect("known commodity");
    assert_eq!((s.symbol.as_str(), s.name.as_str(), s.category.as_str()), ("gold", "Gold", "Metals"));
    assert_eq!((s.sellers, s.carrier_sellers, s.buyers, s.carrier_buyers), (1, 0, 3, 1), "the producer's demand 1 is no demand");
    let sell = CommodityQuery { commodity: "gold".into(), mode: TradeMode::SellToStation, ..Default::default() };
    assert!(db.search_commodity(&sell).expect("sell").iter().all(|h| h.station != "Alpha Port"));
    assert_eq!((s.lowest_buy, s.highest_sell), (Some(9_000), Some(12_000)));
    assert_eq!(s.average, Some(9_400));
    assert!(db.commodity_summary("no such thing", None).expect("summary").is_none());
    // Thargoid salvage resolves by its display name although its symbol is UnknownArtifact2.
    let probe = db.commodity_summary("thargoid probe", None).expect("summary").expect("known");
    assert_eq!((probe.symbol.as_str(), probe.sellers, probe.buyers), ("unknownartifact2", 0, 0));

    // Newest first, one row per market with both prices.
    let q = CommodityQuery { commodity: "gold".into(), origin: Some([0.0; 3]), ..Default::default() };
    let latest = db.latest_prices(&q).expect("latest");
    let names: Vec<&str> = latest.iter().map(|l| l.station.as_str()).collect();
    assert_eq!(names, vec!["Alpha Port", "Q2Q-22Q", "Far Out", "Beta Hub"], "the re-priced Alpha Port is newest");
    assert_eq!((latest[0].buy, latest[0].sell, latest[0].stock, latest[0].demand, latest[0].mean), (9_000, 8_800, 500, 0, 9_400));
    assert_eq!(latest[2].distance_ly, Some(100.0));
    assert!(latest[1].is_carrier);
    // The commodity search filters apply.
    let near = db
        .latest_prices(&CommodityQuery { max_distance_ly: Some(50.0), include_carriers: false, min_pad: Some('L'), ..q.clone() })
        .expect("latest");
    let names: Vec<&str> = near.iter().map(|l| l.station.as_str()).collect();
    assert_eq!(names, vec!["Alpha Port"], "Far Out is too far, the carrier is excluded, the outpost has no large pad");
    let busy = db.latest_prices(&CommodityQuery { min_quantity: 100, ..q }).expect("latest");
    assert_eq!(busy.len(), 2, "stock or demand of at least 100: Alpha Port and Far Out");
}

/// Rare goods taken as one commodity: George Lucas sells two of them (Azure Milk is Foods, Leestian
/// Evil Juice Legal Drugs), each a row of its own that says which it is; a category narrows them;
/// normal goods stay out; the one-commodity search is unchanged.
#[test]
fn rare_goods_as_one_commodity() {
    let t = TempDb::new("rares");
    let db = t.db();
    db.ingest_many(vec![
        system(SYS_A, "Leesti", [0.0, 0.0, 0.0]),
        system(SYS_B, "Rusani", [10.0, 0.0, 0.0]),
        station(31, "George Lucas", SYS_A, "Leesti", "Coriolis", &["dock", "commodities"], "Industrial"),
        station(32, "Fernandes Market", SYS_B, "Rusani", "Coriolis", &["dock", "commodities"], "Industrial"),
    ]);
    db.ingest(priced(31, "Leesti", "George Lucas", Some("Coriolis"), &[("bluemilk", 6_000, 0, 9_000, 20, 0), ("leestianeviljuice", 400, 0, 1_600, 40, 0), ("gold", 9_000, 0, 9_400, 500, 0)]));
    db.ingest(priced(32, "Rusani", "Fernandes Market", Some("Coriolis"), &[("rusanioldsmokey", 1_000, 0, 3_000, 10, 0), ("bluemilk", 0, 12_000, 9_000, 0, 50)]));
    db.flush().expect("flush");
    let rows = |hits: &[CommodityHit]| hits.iter().map(|h| (h.station.clone(), h.commodity.as_ref().map(|c| c.name.clone()).unwrap_or_default())).collect::<Vec<_>>();
    let row = |station: &str, name: &str| (station.to_string(), name.to_string());

    let q = CommodityQuery { origin: Some([0.0; 3]), mode: TradeMode::BuyFromStation, ..Default::default() };
    let buy = db.search_rare_goods(None, &q).expect("rares");
    assert_eq!(rows(&buy), vec![row("George Lucas", "Leestian Evil Juice"), row("Fernandes Market", "Rusani Old Smokey"), row("George Lucas", "Azure Milk")], "cheapest first, gold left out");
    assert_eq!(buy[0].commodity.as_ref().map(|c| (c.symbol.as_str(), c.mean)), Some(("leestianeviljuice", 1_600)), "with its own average");
    let legal = db.search_rare_goods(Some("Legal Drugs"), &q).expect("rares");
    assert_eq!(rows(&legal), vec![row("George Lucas", "Leestian Evil Juice"), row("Fernandes Market", "Rusani Old Smokey")]);
    let sell = db.search_rare_goods(None, &CommodityQuery { mode: TradeMode::SellToStation, ..q.clone() }).expect("rares");
    assert_eq!(rows(&sell), vec![row("Fernandes Market", "Azure Milk")]);

    // Latest prices: George Lucas twice (one row per market and commodity), Fernandes Market twice.
    let latest = db.latest_rare_prices(None, &q).expect("latest");
    let mut pairs: Vec<(String, String)> = latest.iter().map(|l| (l.station.clone(), l.commodity.as_ref().map(|c| c.name.clone()).unwrap_or_default())).collect();
    pairs.sort();
    assert_eq!(pairs, vec![row("Fernandes Market", "Azure Milk"), row("Fernandes Market", "Rusani Old Smokey"), row("George Lucas", "Azure Milk"), row("George Lucas", "Leestian Evil Juice")]);

    let s = db.rare_goods_summary(Some("Legal Drugs"), Some(72.0)).expect("summary").expect("rare goods");
    assert_eq!((s.category.as_str(), s.is_rare, s.sellers, s.buyers), ("Legal Drugs", true, 2, 0));
    assert_eq!((s.lowest_buy, s.average), (Some(400), Some(2_300)), "the average of 1 600 and 3 000");
    let all = db.rare_goods_summary(None, Some(72.0)).expect("summary").expect("rare goods");
    assert_eq!((all.sellers, all.buyers, all.highest_sell), (3, 1, Some(12_000)));

    // One commodity: no row commodity, as before.
    let gold = db.search_commodity(&CommodityQuery { commodity: "gold".into(), ..q }).expect("gold");
    assert_eq!(gold.len(), 1);
    assert!(gold[0].commodity.is_none());
}
