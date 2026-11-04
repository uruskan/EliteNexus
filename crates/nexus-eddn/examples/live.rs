//! Listens to the live EDDN relay for N seconds, then prints subscriber stats, database stats and
//! sample query timings.
//!
//! ```text
//! cargo run -p nexus-eddn --example live -- <db path> <seconds>
//! ```

use nexus_eddn::EddnSubscriber;
use nexus_galaxy::*;
use std::time::{Duration, Instant};

fn timed<T>(label: &str, f: impl FnOnce() -> Result<T>, describe: impl Fn(&T) -> String) {
    let t = Instant::now();
    match f() {
        Ok(v) => println!(
            "  {label:<52} {:>8.2} ms  {}",
            t.elapsed().as_secs_f64() * 1000.0,
            describe(&v)
        ),
        Err(e) => println!("  {label:<52} error: {e}"),
    }
}

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .try_init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(path), Some(secs)) = (
        args.first(),
        args.get(1).and_then(|s| s.parse::<u64>().ok()),
    ) else {
        eprintln!("usage: live <db path> <seconds>");
        std::process::exit(2);
    };
    let db = GalaxyDb::open(path).expect("open database");
    let before = db.stats().expect("stats");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let updates = db.subscribe_updates();
    let mut eddn = EddnSubscriber::spawn(rt.handle(), db.clone());

    let started = Instant::now();
    let mut market_updates = 0usize;
    let mut last_report = Instant::now();
    while started.elapsed() < Duration::from_secs(secs) {
        market_updates += updates.try_iter().count();
        if last_report.elapsed() >= Duration::from_secs(10) {
            last_report = Instant::now();
            let s = eddn.stats().snapshot();
            println!(
                "[{:>4.0}s] connected={} msgs={} ({}/min) dup={} markets committed={} pending writes={}",
                started.elapsed().as_secs_f64(),
                s.connected,
                s.total_messages,
                s.messages_per_minute,
                s.duplicates,
                market_updates,
                db.pending()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    eddn.stop();
    db.flush().expect("flush");
    market_updates += updates.try_iter().count();
    let elapsed = started.elapsed().as_secs_f64();

    let s = eddn.stats().snapshot();
    println!("\n=== EDDN ({elapsed:.0} s) ===");
    println!(
        "messages {} ({:.0}/min average, {} in the last minute), duplicates {}, /test dropped {}, decode errors {}, \
         backlog drops {}, connects {}",
        s.total_messages,
        s.total_messages as f64 * 60.0 / elapsed,
        s.messages_per_minute,
        s.duplicates,
        s.test_dropped,
        s.decode_errors,
        s.dropped_backlog,
        s.connects
    );
    println!(
        "database items queued {}, stored {}, market snapshots committed {market_updates}",
        s.ingested, s.stored
    );
    println!("per schema:");
    for (schema, n) in &s.per_schema {
        println!(
            "  {schema:<26} {n:>6}  ({:.1}/min)",
            *n as f64 * 60.0 / elapsed
        );
    }
    println!("latest headlines:");
    for h in s.recent_headlines.iter().take(12) {
        println!("  [{}] {}", h.schema, h.text);
    }

    let st = db.stats().expect("stats");
    println!("\n=== database {} ===", db.path().display());
    println!(
        "systems {} (+{}), stations {} (+{}), markets {}, market rows {}, factions {}, ring signals {}, price history {}, \
         stale snapshots skipped {}, commits {}, size {:.2} MB",
        st.systems,
        st.systems - before.systems,
        st.stations,
        st.stations - before.stations,
        st.stations_with_market,
        st.market_rows,
        st.faction_rows,
        st.ring_signals,
        st.price_history_rows.map_or("not counted yet".to_string(), |n| n.to_string()),
        st.stale_markets,
        st.commits,
        st.db_bytes as f64 / 1e6
    );

    println!("\n=== sample queries ===");
    let recent = db.recent_markets(50).unwrap_or_default();
    let anchor = recent.iter().find(|s| s.pos.is_some()).cloned();
    let origin = anchor
        .as_ref()
        .and_then(|s| s.pos)
        .unwrap_or([0.0, 0.0, 0.0]);
    println!(
        "  origin: {} ({:?})",
        anchor
            .as_ref()
            .map_or("Sol".to_string(), |a| a.system.clone()),
        origin
    );
    for c in ["tritium", "gold", "painite"] {
        timed(
            &format!("search_commodity {c} buy, global"),
            || {
                db.search_commodity(&CommodityQuery {
                    commodity: c.into(),
                    ..Default::default()
                })
            },
            |v| {
                format!(
                    "{} hits{}",
                    v.len(),
                    v.first()
                        .map(|h| format!(", best {} cr at {}", h.price, h.station))
                        .unwrap_or_default()
                )
            },
        );
        timed(
            &format!("search_commodity {c} sell, 100 ly, pad L"),
            || {
                db.search_commodity(&CommodityQuery {
                    commodity: c.into(),
                    mode: TradeMode::SellToStation,
                    origin: Some(origin),
                    max_distance_ly: Some(100.0),
                    min_pad: Some('L'),
                    ..Default::default()
                })
            },
            |v| format!("{} hits", v.len()),
        );
    }
    if let Some(a) = &anchor {
        timed(
            "station_market (latest update)",
            || db.station_market(a.market_id),
            |m| format!("{} rows", m.as_ref().map_or(0, |m| m.items.len())),
        );
        timed(
            "system_by_name",
            || db.system_by_name(&a.system),
            |s| format!("found: {}", s.is_some()),
        );
    }
    timed(
        "systems_near 50 ly",
        || db.systems_near(origin, 50.0, 100),
        |v| format!("{} systems", v.len()),
    );
    timed(
        "stations_near interstellar factors",
        || {
            db.stations_near(&StationQuery {
                origin,
                services: Services::INTERSTELLAR_FACTORS,
                limit: 5,
                ..Default::default()
            })
        },
        |v| format!("{} hits", v.len()),
    );
    timed(
        "trade_routes 50 ly, hop 30 ly",
        || {
            db.trade_routes(&TradeQuery {
                origin,
                radius_ly: 50.0,
                max_hop_ly: 30.0,
                cargo_capacity: 500,
                ..Default::default()
            })
        },
        |v| {
            v.first().map_or("no routes".to_string(), |r| {
                format!(
                    "{} routes, best: {} {} -> {} ({} cr/t)",
                    v.len(),
                    r.commodity_name,
                    r.from.station,
                    r.to.station,
                    r.profit_per_ton
                )
            })
        },
    );
    timed(
        "stats",
        || db.stats(),
        |s| format!("{} market rows", s.market_rows),
    );
    drop(eddn);
    drop(rt);
}
