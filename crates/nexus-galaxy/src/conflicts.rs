//! Wars, civil wars and elections near a point (the Combat page's nearest fights, Ask's `conflicts_near`).
//!
//! A system's conflicts come from the game's own list (`Conflicts[]`) when its last report carried it: who fights
//! whom, what each side stands to lose, the days each has won. A report without that list (a Spansh dump, or a sender
//! that dropped it) leaves only the faction states, so those systems' conflicts are worked out from them: the
//! factions in the same war, civil war or election state, with no stakes or days won ([`ConflictHit::detailed`]).

use crate::error::Result;
use crate::model::*;
use crate::query::{MAX_RADIUS, bbox, json_strings};
use crate::util::dist;
use rusqlite::{Connection, Row, params};
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

/// The system columns every hit starts from (`sy` is `systems`).
const SYSTEM: &str = "sy.id64, sy.name, sy.x, sy.y, sy.z, sy.government, sy.security, sy.allegiance, sy.population, sy.controlling_faction";
/// How many columns [`SYSTEM`] has.
const N_SYSTEM: usize = 10;

/// A row's [`SYSTEM`] columns as a hit with no conflict yet; `None` when the system has no position.
fn system(r: &Row) -> rusqlite::Result<Option<ConflictHit>> {
    let (x, y, z): (Option<f64>, Option<f64>, Option<f64>) = (r.get(2)?, r.get(3)?, r.get(4)?);
    let (Some(x), Some(y), Some(z)) = (x, y, z) else { return Ok(None) };
    Ok(Some(ConflictHit {
        system_id64: r.get::<_, i64>(0)? as u64,
        system: r.get(1)?,
        pos: [x, y, z],
        distance_ly: 0.0,
        kind: ConflictKind::War,
        status: ConflictStatus::Active,
        sides: Vec::new(),
        detailed: false,
        updated_unix: 0,
        government: r.get(5)?,
        security: r.get(6)?,
        allegiance: r.get(7)?,
        population: r.get::<_, Option<i64>>(8)?.map(|p| p.max(0) as u64),
        controlling_faction: r.get(9)?,
    }))
}

pub(crate) fn conflicts_near(conn: &Connection, q: &ConflictQuery) -> Result<ConflictScan> {
    let r = q.max_distance_ly.unwrap_or(MAX_RADIUS).clamp(0.0, MAX_RADIUS);
    let b = bbox(q.origin, r);
    let keep = |hit: &mut ConflictHit| {
        hit.distance_ly = dist(q.origin, hit.pos);
        hit.distance_ly <= r && (q.elections || hit.kind.is_fight())
    };
    let mut hits = Vec::new();

    // The game's own lists, without the conflicts that are over.
    let sql = format!(
        "SELECT {SYSTEM}, c.faction1, c.faction2, c.kind, c.status, c.stake1, c.stake2, c.won1, c.won2, c.updated \
         FROM system_conflicts AS c CROSS JOIN systems AS sy ON sy.id64 = c.system_id64 \
         WHERE c.updated >= ?1 AND c.status IN ('active', 'pending') \
           AND sy.x BETWEEN ?2 AND ?3 AND sy.y BETWEEN ?4 AND ?5 AND sy.z BETWEEN ?6 AND ?7"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![q.since_unix, b[0], b[1], b[2], b[3], b[4], b[5]], |row| {
        let Some(mut hit) = system(row)? else { return Ok(None) };
        let o = N_SYSTEM;
        let Some(kind) = ConflictKind::parse(&row.get::<_, String>(o + 2)?) else { return Ok(None) };
        let side = |faction: String, stake: Option<String>, won: i64| ConflictSide {
            faction,
            stake,
            won_days: Some(won.clamp(0, 99) as u32),
            ..ConflictSide::default()
        };
        hit.kind = kind;
        hit.status = ConflictStatus::parse(&row.get::<_, String>(o + 3)?);
        hit.sides = vec![side(row.get(o)?, row.get(o + 4)?, row.get(o + 6)?), side(row.get(o + 1)?, row.get(o + 5)?, row.get(o + 7)?)];
        hit.detailed = true;
        hit.updated_unix = row.get(o + 8)?;
        Ok(Some(hit))
    })?;
    for hit in rows {
        if let Some(mut hit) = hit?
            && keep(&mut hit)
        {
            hits.push(hit);
        }
    }

    // Systems whose last report came without the list: the factions in each war, civil war or election state (active
    // or pending; a faction's main state can still say War while the war is only recovering, so it isn't read).
    let sql = format!(
        "SELECT {SYSTEM}, f.faction, f.active_states, f.pending_states, f.updated \
         FROM system_factions AS f CROSS JOIN systems AS sy ON sy.id64 = f.system_id64 \
         WHERE sy.conflicts_updated IS NULL AND f.updated >= ?1 \
           AND (f.active_states LIKE '%War\"%' OR f.active_states LIKE '%Election\"%' \
                OR f.pending_states LIKE '%War\"%' OR f.pending_states LIKE '%Election\"%') \
           AND sy.x BETWEEN ?2 AND ?3 AND sy.y BETWEEN ?4 AND ?5 AND sy.z BETWEEN ?6 AND ?7"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut groups: BTreeMap<(u64, ConflictKind, ConflictStatus), ConflictHit> = BTreeMap::new();
    let mut rows = stmt.query(params![q.since_unix, b[0], b[1], b[2], b[3], b[4], b[5]])?;
    while let Some(row) = rows.next()? {
        let Some(base) = system(row)? else { continue };
        let o = N_SYSTEM;
        let faction: String = row.get(o)?;
        let (active, pending) = (json_strings(row.get(o + 1)?), json_strings(row.get(o + 2)?));
        let updated: i64 = row.get(o + 3)?;
        for (kind, status) in states_in_conflict(&active, &pending) {
            let hit = groups.entry((base.system_id64, kind, status)).or_insert_with(|| ConflictHit { kind, status, ..base.clone() });
            hit.updated_unix = hit.updated_unix.max(updated);
            hit.sides.push(ConflictSide { faction: faction.clone(), ..ConflictSide::default() });
        }
    }
    for mut hit in groups.into_values() {
        if keep(&mut hit) {
            hits.push(hit);
        }
    }

    // Nearest first; in one system, active before pending, wars before elections.
    hits.sort_by(|a, b| {
        a.distance_ly
            .total_cmp(&b.distance_ly)
            .then(a.system_id64.cmp(&b.system_id64))
            .then(a.status.cmp(&b.status))
            .then(a.kind.cmp(&b.kind))
    });
    let total = hits.len();
    hits.truncate(q.limit.max(1));
    fill_sides(conn, &mut hits)?;
    Ok(ConflictScan { systems_reported: reported(conn, q, r, b)?, hits, total })
}

/// The conflicts a faction's active and pending states put it in.
fn states_in_conflict(active: &[String], pending: &[String]) -> Vec<(ConflictKind, ConflictStatus)> {
    let mut out: Vec<(ConflictKind, ConflictStatus)> = Vec::new();
    let found = active
        .iter()
        .map(|s| (s.as_str(), ConflictStatus::Active))
        .chain(pending.iter().map(|s| (s.as_str(), ConflictStatus::Pending)));
    for (s, status) in found {
        if let Some(kind) = ConflictKind::parse(s)
            && !out.contains(&(kind, status))
        {
            out.push((kind, status));
        }
    }
    out
}

/// A stored faction's influence, allegiance and government.
type FactionFacts = (Option<f64>, Option<String>, Option<String>);

/// Each side's influence, allegiance and government from the system's stored factions; sides worked out from
/// states are put in order of influence.
fn fill_sides(conn: &Connection, hits: &mut [ConflictHit]) -> Result<()> {
    let mut stmt = conn.prepare_cached("SELECT faction, influence, allegiance, government FROM system_factions WHERE system_id64 = ?1")?;
    let mut cache: HashMap<u64, HashMap<String, FactionFacts>> = HashMap::new();
    for hit in hits {
        let factions = match cache.entry(hit.system_id64) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let rows = stmt.query_map([hit.system_id64 as i64], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
                e.insert(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
            }
        };
        for side in &mut hit.sides {
            if let Some((influence, allegiance, government)) = factions.get(&side.faction) {
                side.influence = *influence;
                side.allegiance.clone_from(allegiance);
                side.government.clone_from(government);
            }
        }
        if !hit.detailed {
            hit.sides.sort_by(|a, b| b.influence.unwrap_or(0.0).total_cmp(&a.influence.unwrap_or(0.0)).then_with(|| a.faction.cmp(&b.faction)));
        }
    }
    Ok(())
}

/// Populated systems in range reported since the query's cut-off: the ground the list stands on.
fn reported(conn: &Connection, q: &ConflictQuery, r: f64, b: [f64; 6]) -> Result<usize> {
    let mut stmt = conn.prepare_cached(
        "SELECT sy.x, sy.y, sy.z FROM system_factions AS f CROSS JOIN systems AS sy ON sy.id64 = f.system_id64 \
         WHERE f.updated >= ?1 AND sy.x BETWEEN ?2 AND ?3 AND sy.y BETWEEN ?4 AND ?5 AND sy.z BETWEEN ?6 AND ?7 \
         GROUP BY f.system_id64",
    )?;
    let rows = stmt.query_map(params![q.since_unix, b[0], b[1], b[2], b[3], b[4], b[5]], |r| Ok([r.get::<_, f64>(0)?, r.get(1)?, r.get(2)?]))?;
    let mut n = 0;
    for p in rows {
        if dist(q.origin, p?) <= r {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(conn: &Connection, id: i64, name: &str, pos: [f64; 3], government: &str, conflicts_updated: Option<i64>) {
        conn.execute(
            "INSERT INTO systems (id64, name, name_lower, x, y, z, population, government, security, updated, conflicts_updated) \
             VALUES (?1, ?2, lower(?2), ?3, ?4, ?5, 1000000, ?6, 'Low', 100, ?7)",
            params![id, name, pos[0], pos[1], pos[2], government, conflicts_updated],
        )
        .expect("system");
    }

    fn faction(conn: &Connection, system: i64, name: &str, influence: f64, active: &str, pending: &str, updated: i64) {
        conn.execute(
            "INSERT INTO system_factions (system_id64, faction, influence, allegiance, government, active_states, pending_states, updated) \
             VALUES (?1, ?2, ?3, 'Independent', 'Corporate', nullif(?4, ''), nullif(?5, ''), ?6)",
            params![system, name, influence, active, pending, updated],
        )
        .expect("faction");
    }

    fn query(origin: [f64; 3], max: Option<f64>, since: i64, elections: bool) -> ConflictQuery {
        ConflictQuery { origin, max_distance_ly: max, since_unix: since, elections, limit: 50 }
    }

    #[test]
    fn the_games_list_first_then_faction_states() {
        let conn = crate::schema::memory();
        // Leesti: the game's list (a civil war and a pending war).
        sys(&conn, 1, "Leesti", [10.0, 0.0, 0.0], "Cooperative", Some(500));
        faction(&conn, 1, "Steelworks", 0.30, r#"["Civil War"]"#, "", 500);
        faction(&conn, 1, "Alliance Union", 0.31, r#"["Civil War"]"#, "", 500);
        conn.execute_batch(
            "INSERT INTO system_conflicts VALUES (1, 'Steelworks', 'Alliance Union', 'civilwar', 'active', 'Villeneuve', 'Pandey''s Point', 0, 2, 500);
             INSERT INTO system_conflicts VALUES (1, 'Mother Gaia', 'Federal Congress', 'war', 'pending', NULL, 'Abraham Lincoln', 0, 0, 500);
             INSERT INTO system_conflicts VALUES (1, 'Old Winner', 'Old Loser', 'war', '', NULL, NULL, 4, 1, 500);",
        )
        .expect("conflicts");
        // Orrere: only faction states (a dump), an anarchy with a war between two factions and an election.
        sys(&conn, 2, "Orrere", [0.0, 5.0, 0.0], "Anarchy", None);
        faction(&conn, 2, "Energy Company", 0.40, r#"["War","Boom"]"#, "", 400);
        faction(&conn, 2, "Union of Orrere", 0.45, r#"["War"]"#, "", 400);
        faction(&conn, 2, "Voters A", 0.05, r#"["Election"]"#, "", 400);
        faction(&conn, 2, "Voters B", 0.06, "", r#"["Election"]"#, 400);
        faction(&conn, 2, "Bystanders", 0.04, r#"["Expansion"]"#, "", 400);
        conn.execute("UPDATE system_factions SET state = 'War' WHERE faction = 'Bystanders'", []).expect("a main state that lags");
        // Far away, and an old report.
        sys(&conn, 3, "Far", [500.0, 0.0, 0.0], "Democracy", None);
        faction(&conn, 3, "Far One", 0.5, r#"["War"]"#, "", 400);
        faction(&conn, 3, "Far Two", 0.5, r#"["War"]"#, "", 400);
        sys(&conn, 4, "Stale", [1.0, 0.0, 0.0], "Democracy", None);
        faction(&conn, 4, "Old One", 0.5, r#"["War"]"#, "", 10);
        faction(&conn, 4, "Old Two", 0.5, r#"["War"]"#, "", 10);
        // A system whose list said "none": its leftover war states don't count.
        sys(&conn, 5, "Peaceful", [2.0, 0.0, 0.0], "Democracy", Some(450));
        faction(&conn, 5, "Calm", 0.5, r#"["War"]"#, "", 450);

        let scan = conflicts_near(&conn, &query([0.0; 3], Some(100.0), 300, false)).expect("query");
        let got: Vec<(&str, ConflictKind, ConflictStatus, bool)> = scan.hits.iter().map(|h| (h.system.as_str(), h.kind, h.status, h.detailed)).collect();
        assert_eq!(
            got,
            vec![
                ("Orrere", ConflictKind::War, ConflictStatus::Active, false),
                ("Leesti", ConflictKind::CivilWar, ConflictStatus::Active, true),
                ("Leesti", ConflictKind::War, ConflictStatus::Pending, true),
            ],
            "nearest first, active first; no elections and nothing over; Far out of range, Stale too old, Peaceful's list says none"
        );
        let orrere = &scan.hits[0];
        assert_eq!((orrere.distance_ly, orrere.government.as_deref()), (5.0, Some("Anarchy")));
        assert_eq!(orrere.sides.iter().map(|s| s.faction.as_str()).collect::<Vec<_>>(), ["Union of Orrere", "Energy Company"], "by influence");
        assert_eq!(orrere.sides[0].won_days, None, "states don't say");
        let leesti = &scan.hits[1];
        assert_eq!(leesti.sides[1].stake.as_deref(), Some("Pandey's Point"));
        assert_eq!((leesti.sides[0].won_days, leesti.sides[1].won_days), (Some(0), Some(2)));
        assert_eq!((leesti.sides[1].influence, leesti.sides[1].allegiance.as_deref()), (Some(0.31), Some("Independent")));
        assert_eq!(scan.hits[2].sides[0].influence, None, "a side that isn't among the stored factions");
        assert_eq!(scan.systems_reported, 3, "Leesti, Orrere and Peaceful are in range and fresh");

        // Elections, an active one and a pending one, each with the one faction that reported it.
        let scan = conflicts_near(&conn, &query([0.0; 3], Some(100.0), 300, true)).expect("query");
        let elections: Vec<(ConflictStatus, Vec<&str>)> = scan
            .hits
            .iter()
            .filter(|h| h.kind == ConflictKind::Election)
            .map(|h| (h.status, h.sides.iter().map(|s| s.faction.as_str()).collect()))
            .collect();
        assert_eq!(elections, vec![(ConflictStatus::Active, vec!["Voters A"]), (ConflictStatus::Pending, vec!["Voters B"])]);

        // The whole galaxy, and an older cut-off.
        let scan = conflicts_near(&conn, &query([0.0; 3], None, 0, false)).expect("query");
        let names: Vec<&str> = scan.hits.iter().map(|h| h.system.as_str()).collect();
        assert_eq!(names, ["Stale", "Orrere", "Leesti", "Leesti", "Far"]);
        assert_eq!(scan.systems_reported, 5);
        assert_eq!(scan.total, 5);
        let scan = conflicts_near(&conn, &ConflictQuery { limit: 1, ..query([0.0; 3], None, 0, false) }).expect("query");
        assert_eq!((scan.hits.len(), scan.total), (1, 5), "the nearest one, and how many there were");
    }

    #[test]
    fn faction_states_in_conflict() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            states_in_conflict(&s(&["War", "Boom"]), &s(&["Civil War", "Expansion"])),
            vec![(ConflictKind::War, ConflictStatus::Active), (ConflictKind::CivilWar, ConflictStatus::Pending)]
        );
        assert_eq!(states_in_conflict(&s(&["Boom"]), &s(&["Expansion"])), vec![]);
    }
}
