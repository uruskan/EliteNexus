//! Test helpers: a throw-away galaxy database filled through the public EDDN ingest path.

use nexus_galaxy::GalaxyDb;
use serde_json::{Value, json};
use std::path::PathBuf;

/// A galaxy database in the temp folder, deleted on drop.
pub struct TempDb {
    dir: PathBuf,
    pub db: Option<GalaxyDb>,
}

impl TempDb {
    pub fn new(name: &str) -> TempDb {
        let dir = std::env::temp_dir().join(format!("nexus-app-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = GalaxyDb::open(dir.join("galaxy.db")).expect("open test db");
        TempDb { dir, db: Some(db) }
    }

    pub fn db(&self) -> &GalaxyDb {
        self.db.as_ref().expect("db")
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        self.db = None;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn eddn(schema: &str, message: Value) -> Value {
    json!({ "$schemaRef": format!("https://eddn.edcd.io/schemas/{schema}"), "header": { "uploaderID": "t", "softwareName": "t", "softwareVersion": "1" }, "message": message })
}

/// A populated system at (x, 0, 0) with one large-pad station and its market.
pub fn station(db: &GalaxyDb, sys: (&str, u64, f64), name: &str, market_id: u64, goods: Value) {
    let (system, id, x) = sys;
    station_at(db, (system, id, [x, 0.0, 0.0]), name, market_id, goods);
}

/// [`station`] anywhere: a populated system at `pos`.
pub fn station_at(db: &GalaxyDb, sys: (&str, u64, [f64; 3]), name: &str, market_id: u64, goods: Value) {
    let ts = nexus_journal::Timestamp::now().to_iso();
    let (system, id, pos) = sys;
    db.ingest_eddn(&eddn("journal/1", json!({ "timestamp": ts, "event": "FSDJump", "StarSystem": system, "SystemAddress": id, "StarPos": pos, "Population": 1000 })));
    db.ingest_eddn(&eddn(
        "journal/1",
        json!({ "timestamp": ts, "event": "Docked", "StationName": name, "StationType": "Coriolis", "StarSystem": system, "SystemAddress": id,
                "MarketID": market_id, "StarPos": pos, "StationServices": ["dock", "commodities"], "DistFromStarLS": 100.0,
                "LandingPads": { "Small": 4, "Medium": 4, "Large": 2 } }),
    ));
    db.ingest_eddn(&eddn("commodity/3", json!({ "systemName": system, "stationName": name, "marketId": market_id, "timestamp": ts, "commodities": goods })));
}

/// One market row for [`station`].
pub fn good(name: &str, buy: i64, stock: i64, sell: i64, demand: i64) -> Value {
    json!({ "name": name, "meanPrice": sell.max(buy), "buyPrice": buy, "stock": stock, "stockBracket": if stock > 0 { 2 } else { 0 },
            "sellPrice": sell, "demand": demand, "demandBracket": if demand > 0 { 2 } else { 0 } })
}
