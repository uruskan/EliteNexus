//! SQLite schema, pragmas and seeding.

use crate::error::Result;
use rusqlite::Connection;
use std::time::Duration;

/// Bump when the schema changes incompatibly. The database is a cache that can be rebuilt from
/// EDDN and dumps, so an older schema is dropped and recreated. New tables (`CREATE TABLE IF NOT
/// EXISTS`, e.g. `bodies`) are added to existing databases without a bump.
pub(crate) const SCHEMA_VERSION: i64 = 2;

const TABLES: &str = r#"
CREATE TABLE IF NOT EXISTS systems (
    id64                      INTEGER PRIMARY KEY,
    name                      TEXT NOT NULL,
    name_lower                TEXT NOT NULL,
    x                         REAL,
    y                         REAL,
    z                         REAL,
    population                INTEGER,
    allegiance                TEXT,
    government                TEXT,
    economy                   TEXT,
    second_economy            TEXT,
    security                  TEXT,
    controlling_faction       TEXT,
    controlling_faction_state TEXT,
    controlling_power         TEXT,
    powerplay_state           TEXT,
    powers                    TEXT,
    updated                   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS systems_name_lower ON systems(name_lower);
CREATE VIRTUAL TABLE IF NOT EXISTS systems_rtree USING rtree(id, min_x, max_x, min_y, max_y, min_z, max_z);

CREATE TABLE IF NOT EXISTS stations (
    market_id             INTEGER PRIMARY KEY,
    system_id64           INTEGER,
    system_name           TEXT NOT NULL,
    system_name_lower     TEXT NOT NULL,
    name                  TEXT NOT NULL,
    type                  TEXT,
    dist_ls               REAL,
    max_pad               INTEGER NOT NULL DEFAULT 0,
    is_planetary          INTEGER NOT NULL DEFAULT 0,
    is_carrier            INTEGER NOT NULL DEFAULT 0,
    is_odyssey_settlement INTEGER NOT NULL DEFAULT 0,
    services              INTEGER NOT NULL DEFAULT 0,
    mat_trader            INTEGER NOT NULL DEFAULT 0,
    tech_broker           INTEGER NOT NULL DEFAULT 0,
    economy               TEXT,
    second_economy        TEXT,
    economies             TEXT,
    faction               TEXT,
    faction_state         TEXT,
    government            TEXT,
    allegiance            TEXT,
    carrier_access        TEXT,
    x                     REAL,
    y                     REAL,
    z                     REAL,
    updated               INTEGER NOT NULL DEFAULT 0,
    market_updated        INTEGER NOT NULL DEFAULT 0,
    n_commodities         INTEGER NOT NULL DEFAULT 0,
    prohibited            TEXT,
    -- The market report since which every price change and every new commodity at this market is in the price
    -- history (unix seconds); NULL when its prices aren't followed (history off, or not reported since this column).
    history_since         INTEGER
);
CREATE INDEX IF NOT EXISTS stations_system ON stations(system_id64);
CREATE INDEX IF NOT EXISTS stations_unlinked ON stations(system_name_lower) WHERE system_id64 IS NULL;
CREATE INDEX IF NOT EXISTS stations_name ON stations(name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS stations_market_updated ON stations(market_updated);

CREATE TABLE IF NOT EXISTS commodities (
    id       INTEGER PRIMARY KEY,
    symbol   TEXT NOT NULL UNIQUE,
    name     TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT '',
    fdev_id  INTEGER,
    is_rare  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS market (
    market_id      INTEGER NOT NULL,
    commodity_id   INTEGER NOT NULL,
    buy            INTEGER NOT NULL,
    sell           INTEGER NOT NULL,
    mean           INTEGER NOT NULL,
    stock          INTEGER NOT NULL,
    stock_bracket  INTEGER,
    demand         INTEGER NOT NULL,
    demand_bracket INTEGER,
    updated        INTEGER NOT NULL,
    PRIMARY KEY (market_id, commodity_id)
) WITHOUT ROWID;
-- "where can I buy X": stations selling (player buys), cheapest first
CREATE INDEX IF NOT EXISTS market_sellers ON market(commodity_id, buy, stock, updated) WHERE buy > 0 AND stock > 0;
-- "where can I sell X": stations buying (player sells), best price first (scanned backwards)
CREATE INDEX IF NOT EXISTS market_buyers ON market(commodity_id, sell, demand, updated) WHERE sell > 0 AND demand > 0;

CREATE TABLE IF NOT EXISTS system_factions (
    system_id64       INTEGER NOT NULL,
    faction           TEXT NOT NULL,
    influence         REAL,
    state             TEXT,
    government        TEXT,
    allegiance        TEXT,
    happiness         TEXT,
    active_states     TEXT,
    pending_states    TEXT,
    recovering_states TEXT,
    updated           INTEGER NOT NULL,
    PRIMARY KEY (system_id64, faction)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS system_factions_faction ON system_factions(faction);

CREATE TABLE IF NOT EXISTS ring_signals (
    system_id64 INTEGER NOT NULL,
    body_name   TEXT NOT NULL,
    signal      TEXT NOT NULL,
    count       INTEGER NOT NULL,
    ring_type   TEXT,
    updated     INTEGER NOT NULL,
    PRIMARY KEY (system_id64, body_name, signal)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS ring_signals_signal ON ring_signals(signal, count);

CREATE TABLE IF NOT EXISTS price_history (
    market_id    INTEGER NOT NULL,
    commodity_id INTEGER NOT NULL,
    ts           INTEGER NOT NULL,
    buy          INTEGER NOT NULL,
    sell         INTEGER NOT NULL,
    PRIMARY KEY (market_id, commodity_id, ts)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS price_history_ts ON price_history(ts);

-- Planets and moons worth asking about (see BodyUpdate for what is kept and how it merges).
-- Flags: NULL unknown, 0/1; *_seen: unix seconds when true was first seen, or while false the
-- newest observation that said false. Sources: 0 EDDN, 1 journal, 2 Spansh, 3 EDDN archive.
-- checked_online: last on-demand footfall lookup (unix seconds), so it isn't repeated within a day.
CREATE TABLE IF NOT EXISTS bodies (
    system_id64     INTEGER NOT NULL,
    body_id         INTEGER NOT NULL,
    name            TEXT COLLATE NOCASE,
    planet_class    TEXT,
    atmosphere      TEXT,
    atmosphere_type TEXT,
    gravity_g       REAL,
    temperature_k   REAL,
    pressure_atm    REAL,
    volcanism       TEXT,
    terraform_state TEXT,
    distance_ls     REAL,
    landable        INTEGER,
    scan_updated    INTEGER,
    was_discovered  INTEGER,
    discovered_seen INTEGER,
    was_mapped      INTEGER,
    mapped_seen     INTEGER,
    was_footfalled  INTEGER,
    footfall_seen   INTEGER,
    footfall_source INTEGER,
    bio_signals     INTEGER,
    geo_signals     INTEGER,
    genera          TEXT,
    signals_updated INTEGER,
    updated         INTEGER NOT NULL,
    source          INTEGER NOT NULL,
    checked_online  INTEGER,
    PRIMARY KEY (system_id64, body_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS bodies_name ON bodies(name);
"#;

/// Columns added to existing tables after they were first created: (table, column, type).
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[("bodies", "checked_online", "INTEGER"), ("stations", "history_since", "INTEGER")];

/// What earlier versions created and nothing reads any more: `market_goods`, a view whose rare goods queries read
/// the whole market table.
const RETIRED: &str = "DROP VIEW IF EXISTS market_goods;";

const DROP_ALL: &str = r#"
DROP TABLE IF EXISTS systems;
DROP TABLE IF EXISTS systems_rtree;
DROP TABLE IF EXISTS stations;
DROP TABLE IF EXISTS station_rtree;
DROP TABLE IF EXISTS commodities;
DROP TABLE IF EXISTS market;
DROP TABLE IF EXISTS system_factions;
DROP TABLE IF EXISTS ring_signals;
DROP TABLE IF EXISTS price_history;
DROP TABLE IF EXISTS bodies;
DROP TABLE IF EXISTS fixes;
"#;

/// Pragmas shared by every connection.
fn common_pragmas(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.execute_batch(
        "PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = OFF;
         PRAGMA mmap_size = 268435456;",
    )?;
    Ok(())
}

/// Opens the writer connection, enables WAL and creates/migrates the schema.
pub(crate) fn open_writer(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    common_pragmas(&conn)?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        tracing::warn!(mode, "could not enable WAL journal mode");
    }
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -65536;
         PRAGMA wal_autocheckpoint = 4000;
         PRAGMA journal_size_limit = 67108864;",
    )?;
    conn.set_prepared_statement_cache_capacity(64);

    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != 0 && version != SCHEMA_VERSION {
        tracing::warn!(
            found = version,
            expected = SCHEMA_VERSION,
            "galaxy database schema changed; rebuilding it"
        );
        conn.execute_batch(DROP_ALL)?;
    }
    conn.execute_batch(TABLES)?;
    conn.execute_batch(RETIRED)?;
    // Abnormal-price scans (built once on an existing database, then maintained by SQLite).
    conn.execute_batch(crate::deals::ANOMALY_INDEXES)?;
    add_missing_columns(&conn)?;
    run_fixes(&conn)?;
    conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    seed_commodities(&conn)?;
    Ok(conn)
}

/// The demand to store. The game lists a good a station produces with demand 1 and demand bracket 0,
/// which EDDN defines as "no demand": the station isn't buying it, so it's stored as 0.
pub(crate) fn real_demand(demand: i64, bracket: Option<i8>) -> i64 {
    if bracket == Some(0) { 0 } else { demand }
}

/// One-off data repairs, each run once per database (remembered in `fixes`).
const FIXES: &[(&str, &str)] = &[(
    // Rows stored before `real_demand`: producers listed as buyers of their own goods.
    "producer-demand",
    "UPDATE market SET demand = 0 WHERE sell > 0 AND demand > 0 AND demand_bracket = 0",
)];

fn run_fixes(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS fixes (name TEXT PRIMARY KEY, applied INTEGER NOT NULL)")?;
    for (name, sql) in FIXES {
        let done: bool = conn.prepare("SELECT 1 FROM fixes WHERE name = ?1")?.exists([name])?;
        if done {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        let n = tx.execute(sql, [])?;
        tx.execute("INSERT INTO fixes (name, applied) VALUES (?1, strftime('%s','now'))", [name])?;
        tx.commit()?;
        tracing::info!(fix = name, rows = n, "galaxy database repaired");
    }
    Ok(())
}

/// `CREATE TABLE IF NOT EXISTS` leaves an existing table alone: add the columns it lacks.
fn add_missing_columns(conn: &Connection) -> Result<()> {
    for (table, column, kind) in ADDED_COLUMNS {
        let present: bool = conn
            .prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"))?
            .exists([column])?;
        if !present {
            tracing::info!(table, column, "adding a column to the galaxy database");
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
        }
    }
    Ok(())
}

/// Opens a read connection (`query_only`).
pub(crate) fn open_reader(path: &std::path::Path) -> Result<Connection> {
    use rusqlite::OpenFlags;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    common_pragmas(&conn)?;
    conn.execute_batch(
        "PRAGMA query_only = ON;
         PRAGMA cache_size = -32768;",
    )?;
    conn.set_prepared_statement_cache_capacity(64);
    Ok(conn)
}

fn seed_commodities(conn: &Connection) -> Result<()> {
    let have: i64 = conn.query_row("SELECT COUNT(*) FROM commodities", [], |r| r.get(0))?;
    if have > 0 {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO commodities (symbol, name, category, fdev_id, is_rare) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for c in crate::commodities::seed() {
            stmt.execute(rusqlite::params![
                c.symbol, c.name, c.category, c.fdev_id, c.rare
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// An empty database in memory with every table and index, for tests.
#[cfg(test)]
pub(crate) fn memory() -> Connection {
    let conn = Connection::open_in_memory().expect("open");
    conn.execute_batch(TABLES).expect("schema");
    conn.execute_batch(crate::deals::ANOMALY_INDEXES).expect("indexes");
    conn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtree_is_compiled_in() {
        let conn = Connection::open_in_memory().expect("open");
        let used: i64 = conn
            .query_row(
                "SELECT sqlite_compileoption_used('ENABLE_RTREE')",
                [],
                |r| r.get(0),
            )
            .expect("query");
        assert_eq!(used, 1);
        conn.execute_batch(TABLES).expect("schema");
    }

    #[test]
    fn producer_demand_is_repaired_once() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(TABLES).expect("schema");
        let row = |mid: i64, demand: i64, bracket: Option<i64>| {
            conn.execute(
                "INSERT INTO market (market_id, commodity_id, buy, sell, mean, stock, demand, demand_bracket, updated) VALUES (?1, 1, 100, 90, 95, 50, ?2, ?3, 1)",
                rusqlite::params![mid, demand, bracket],
            )
            .expect("insert");
        };
        row(1, 1, Some(0)); // a producer's own good
        row(2, 1, Some(2)); // a real (small) demand
        row(3, 7, None); // Spansh: no brackets
        run_fixes(&conn).expect("fix");
        let demand = |mid: i64| -> i64 { conn.query_row("SELECT demand FROM market WHERE market_id = ?1", [mid], |r| r.get(0)).expect("row") };
        assert_eq!((demand(1), demand(2), demand(3)), (0, 1, 7));
        row(4, 1, Some(0));
        run_fixes(&conn).expect("again");
        assert_eq!(demand(4), 1, "runs once; new rows are stored right by the writer");
        assert_eq!((real_demand(1, Some(0)), real_demand(1, Some(1)), real_demand(5, None)), (0, 1, 5));
    }

    #[test]
    fn existing_tables_get_new_columns() {
        let conn = Connection::open_in_memory().expect("open");
        // the first shape of `bodies`, before `checked_online`
        let old = TABLES
            .replace("    checked_online  INTEGER,\n", "")
            .replace("\r\n    checked_online  INTEGER,", "");
        assert!(!old.contains("checked_online  INTEGER"));
        conn.execute_batch(&old).expect("old schema");
        conn.execute_batch(TABLES).expect("schema");
        add_missing_columns(&conn).expect("migrate");
        add_missing_columns(&conn).expect("idempotent");
        conn.execute_batch("INSERT INTO bodies (system_id64, body_id, updated, source, checked_online) VALUES (5, 1, 0, 0, 7)")
            .expect("new column usable");
    }
}
