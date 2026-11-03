//! Mining sessions: prospector results, refined tonnage, rates. And Rhino surface mining: the planetary mining
//! locations the commander refined at, with the number the game's navigation panel shows, so they can go back.

use crate::location::surface_distance;
use nexus_journal::events as ev;
use nexus_journal::Timestamp;
use std::collections::BTreeMap;

/// Refines this close to a deposit already on record count as that deposit: the rigs on one deposit stand a little
/// apart, and the Rhino collects from each.
pub const SPOT_RADIUS_M: f64 = 200.0;

#[derive(Debug, Clone, Default)]
pub struct ProspectResult {
    pub ts: Timestamp,
    pub content: String,
    pub motherlode: Option<String>,
    pub materials: Vec<(String, f64)>,
    pub remaining: f64,
}

#[derive(Debug, Clone, Default)]
pub struct MiningSession {
    pub start: Timestamp,
    pub last_activity: Timestamp,
    pub system: String,
    pub body: Option<String>,
    pub prospected: u32,
    pub prospects: Vec<ProspectResult>,
    /// Refined tonnes by commodity display name.
    pub refined: BTreeMap<String, u32>,
    pub cracked: u32,
    pub limpets: BTreeMap<String, u32>,
    pub motherlodes: BTreeMap<String, u32>,
    /// Best proportion seen per material (percent).
    pub best_proportion: BTreeMap<String, f64>,
    /// The planetary mining locations the Rhino refined at during the session, in order.
    pub sites: Vec<SiteKey>,
}

impl MiningSession {
    pub fn total_refined(&self) -> u32 {
        self.refined.values().sum()
    }

    pub fn duration_secs(&self) -> i64 {
        (self.last_activity.0 - self.start.0).max(1)
    }

    pub fn tons_per_hour(&self) -> f64 {
        self.total_refined() as f64 * 3600.0 / self.duration_secs() as f64
    }

    pub fn content_counts(&self) -> (u32, u32, u32) {
        let mut h = (0, 0, 0);
        for p in &self.prospects {
            match p.content.as_str() {
                c if c.contains("High") => h.0 += 1,
                c if c.contains("Medium") => h.1 += 1,
                _ => h.2 += 1,
            }
        }
        h
    }
}

/// A planetary mining location: the body, and the number the game's navigation panel shows for it (Planetary Mining
/// Location 4). `location` is None when the game named none: the ship set down far from every location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SiteKey {
    pub system_address: u64,
    pub body_id: u32,
    pub location: Option<u32>,
}

/// A deposit the Rhino refined at: where the first tonne came out, and everything that came out near it.
#[derive(Debug, Clone, Default)]
pub struct Spot {
    pub lat: f64,
    pub lon: f64,
    /// Tonnes refined here while Elite Nexus was running, by commodity.
    pub refined: BTreeMap<String, u32>,
    pub found: Timestamp,
}

/// A planetary mining location the Rhino refined at. Everything but the deposits comes from the journal, so past
/// runs are here too; the deposits' positions come from Status.json, which only a running Elite Nexus reads.
#[derive(Debug, Clone, Default)]
pub struct SurfaceSite {
    pub key: SiteKey,
    pub system: String,
    pub body: String,
    /// Tonnes refined by commodity: the deposit's mineral and its by-products, as the refinery named them.
    pub refined: BTreeMap<String, u32>,
    pub first: Timestamp,
    pub last: Timestamp,
    /// Mining sessions that refined here.
    pub visits: u32,
    /// Where the ship set down by it (Touchdown), oldest first.
    pub landings: Vec<(f64, f64)>,
    /// Deposits, recorded live (the journal has no positions).
    pub spots: Vec<Spot>,
    /// The body's radius in metres when the deposits were recorded.
    pub radius_m: Option<f64>,
    /// The session that refined here last, so each counts as one visit.
    session: Timestamp,
}

impl SurfaceSite {
    pub fn new(key: SiteKey, system: &str, body: &str) -> Self {
        SurfaceSite { key, system: system.to_string(), body: body.to_string(), ..Default::default() }
    }

    pub fn total(&self) -> u32 {
        self.refined.values().sum()
    }

    /// What came out, most tonnes first.
    pub fn by_tonnes(&self) -> Vec<(&str, u32)> {
        let mut v: Vec<(&str, u32)> = self.refined.iter().map(|(k, n)| (k.as_str(), *n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v
    }

    /// The body's name without the system's: "1" for "Hyades Sector NC-V c2-15 1".
    pub fn short_body(&self) -> &str {
        self.body.strip_prefix(&self.system).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&self.body)
    }
}

/// The mining location the last landing or lift-off named, on the body the commander is at.
#[derive(Debug, Clone, Default)]
pub struct Here {
    pub key: SiteKey,
    /// Where the ship set down (the last Touchdown), until it lifts off.
    pub ship: Option<(f64, f64)>,
    /// The location is the one targeted when the Rhino was dropped from a hovering ship. It lasts until the Rhino is
    /// back aboard: the hovering ship can then fly anywhere without a word in the journal.
    pub dropped: bool,
}

/// Where the commander is when the Rhino refines.
pub struct Place<'a> {
    pub system: &'a str,
    pub system_address: u64,
    pub body: &'a str,
    pub body_id: u32,
}

/// A live position on a body (Status.json): latitude, longitude and the body's radius in metres.
#[derive(Debug, Clone, Copy)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
}

/// The number of a planetary mining location in a `NearestDestination` (Touchdown, Liftoff) or a Status.json
/// destination: `$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=4;` is location 4. A settlement, a
/// geological or biological signal, or nothing at all is None. The symbol is the same in every game language.
pub fn mining_location(name: &str) -> Option<u32> {
    let name = name.to_ascii_lowercase();
    if !name.contains("planetarymininglocation") {
        return None;
    }
    let rest = &name[name.find("#index=")? + "#index=".len()..];
    let digits = &rest[..rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len())];
    digits.parse().ok()
}

/// The number of a planetary mining location targeted in Status.json: written as in a landing ([`mining_location`]),
/// or bare, `$SAA_Unknown_Signal:#index=15;`, as other tools have seen it there. The bare form names no kind of
/// signal, so it counts only on a body known to have mining locations (`bare_ok`).
pub fn targeted_location(name: &str, bare_ok: bool) -> Option<u32> {
    mining_location(name).or_else(|| {
        let lower = name.to_ascii_lowercase();
        let n = lower.strip_prefix("$saa_unknown_signal:#index=")?.strip_suffix(';')?.parse().ok()?;
        bare_ok.then_some(n)
    })
}

fn refined_name(r: &ev::MiningRefined) -> String {
    r.kind_localised.clone().unwrap_or_else(|| nexus_data::names::humanize_symbol(&r.kind))
}

#[derive(Debug, Clone, Default)]
pub struct Mining {
    pub session: Option<MiningSession>,
    pub history: Vec<MiningSession>,
    /// The planetary mining locations the Rhino refined at.
    pub sites: BTreeMap<SiteKey, SurfaceSite>,
    pub here: Option<Here>,
    /// Goes up when a deposit is added or gives a new commodity: the app saves the deposits then.
    pub spots_rev: u64,
    /// The planetary mining location last targeted on the body you're at (Status.json, so live only).
    pub targeted: Option<SiteKey>,
    /// Rhino launches no landing named a location for (dropped from a hovering ship), by the launch's journal time:
    /// the location targeted then. Recorded live; the app keeps them and gives them back before the journal is read
    /// again, so those runs keep their number.
    pub drops: BTreeMap<Timestamp, SiteKey>,
    /// Goes up when a drop is recorded: the app saves the drops then.
    pub drops_rev: u64,
}

impl Mining {
    fn session_mut(&mut self, ts: Timestamp, system: &str, body: Option<&str>) -> &mut MiningSession {
        let s = self.session.get_or_insert_with(|| MiningSession {
            start: ts,
            last_activity: ts,
            system: system.to_string(),
            body: body.map(str::to_string),
            ..Default::default()
        });
        s.last_activity = ts;
        s
    }

    pub fn on_prospected(&mut self, p: &ev::ProspectedAsteroid, ts: Timestamp, system: &str, body: Option<&str>) {
        let s = self.session_mut(ts, system, body);
        s.prospected += 1;
        let materials: Vec<(String, f64)> = p
            .materials
            .iter()
            .map(|m| (m.name_localised.clone().unwrap_or_else(|| nexus_data::names::split_camel(&m.name)), m.proportion))
            .collect();
        for (name, prop) in &materials {
            let best = s.best_proportion.entry(name.clone()).or_insert(0.0);
            if *prop > *best {
                *best = *prop;
            }
        }
        let motherlode = p.motherlode_material_localised.clone().or_else(|| p.motherlode_material.clone());
        if let Some(m) = &motherlode {
            *s.motherlodes.entry(m.clone()).or_default() += 1;
        }
        s.prospects.push(ProspectResult {
            ts,
            content: p.content_localised.clone().unwrap_or_else(|| p.content.clone()),
            motherlode,
            materials,
            remaining: p.remaining,
        });
    }

    pub fn on_refined(&mut self, r: &ev::MiningRefined, ts: Timestamp, system: &str, body: Option<&str>) {
        let s = self.session_mut(ts, system, body);
        *s.refined.entry(refined_name(r)).or_default() += 1;
    }

    /// One tonne the Rhino refined (MiningRefined while in an SRV), credited to the mining location the last landing
    /// named on this body. `fix` is where it happened, live only. Call after [`Mining::on_refined`].
    pub fn on_surface_refined(&mut self, r: &ev::MiningRefined, ts: Timestamp, place: Place<'_>, fix: Option<Fix>) {
        let name = refined_name(r);
        let here = self.here.as_ref().filter(|h| h.key.system_address == place.system_address && h.key.body_id == place.body_id);
        let key = here.map_or(SiteKey { system_address: place.system_address, body_id: place.body_id, location: None }, |h| h.key);
        let ship = here.and_then(|h| h.ship);
        let session = self.session.as_ref().map_or(ts, |s| s.start);
        let site = self.sites.entry(key).or_insert_with(|| SurfaceSite { first: ts, ..SurfaceSite::new(key, place.system, place.body) });
        *site.refined.entry(name.clone()).or_default() += 1;
        site.last = ts;
        if site.session != session {
            site.session = session;
            site.visits += 1;
        }
        if let Some(s) = ship && site.landings.last() != Some(&s) {
            site.landings.push(s);
        }
        if let Some(f) = fix {
            site.radius_m = Some(f.radius_m);
            let near = site.spots.iter_mut().find(|s| surface_distance((s.lat, s.lon), (f.lat, f.lon), f.radius_m) <= SPOT_RADIUS_M);
            let added = match near {
                Some(s) => {
                    let new = !s.refined.contains_key(&name);
                    *s.refined.entry(name.clone()).or_default() += 1;
                    new
                }
                None => {
                    site.spots.push(Spot { lat: f.lat, lon: f.lon, refined: BTreeMap::from([(name.clone(), 1)]), found: ts });
                    true
                }
            };
            if added {
                self.spots_rev += 1;
            }
        }
        if let Some(s) = self.session.as_mut() && !s.sites.contains(&key) {
            s.sites.push(key);
        }
    }

    /// Touchdown or Liftoff: the game names the mining location nearest the ship (within 50 km). A landing the
    /// commander flies names the location, or none, rather than keeping one from before; lifting off leaves it, so a
    /// Rhino dropped later from a hovering ship isn't credited to the location left behind. The ship sent away from
    /// the Rhino (an unpiloted lift-off) or called back to it (an unpiloted landing) comes with no position, and the
    /// commander is still there, so those keep the location already named on this body unless they name one.
    pub fn on_landing(&mut self, l: &ev::Landing, touchdown: bool) {
        let named = l.nearest_destination.as_deref().and_then(mining_location);
        let before = self.here.as_ref().filter(|h| h.key.system_address == l.system_address && h.key.body_id == l.body_id);
        let location = match (touchdown, l.player_controlled != Some(false)) {
            (true, true) => named,
            (false, true) => None,
            (_, false) => named.or(before.and_then(|h| h.key.location)),
        };
        let dropped = named.is_none() && location.is_some() && before.is_some_and(|h| h.dropped);
        let key = SiteKey { system_address: l.system_address, body_id: l.body_id, location };
        let ship = if touchdown { l.latitude.zip(l.longitude) } else { None };
        self.here = Some(Here { key, ship, dropped });
    }

    /// The location the ship just set down by, when the Rhino refined there in an earlier session.
    pub fn returning(&self) -> Option<&SurfaceSite> {
        let h = self.here.as_ref()?;
        h.key.location?;
        let site = self.sites.get(&h.key)?;
        let now = self.session.as_ref().map(|s| s.start);
        (Some(site.session) != now).then_some(site)
    }

    /// Status.json's destination: a planetary mining location targeted on the body you're at, or anything else (None).
    /// With no destination at all it isn't called, so the location stays targeted when deselected (the game may do
    /// that on arrival), until you target something else or leave the body.
    pub fn on_target(&mut self, key: Option<SiteKey>) {
        self.targeted = key;
    }

    /// LaunchSRV of the Rhino. A landing that named a location says where, and so does one flown down far from every
    /// location (none). Otherwise the ship hovered, or landed by itself without a word: the launch takes the location
    /// recorded for it before (a journal read again), or else the one targeted on this body, and records it.
    pub fn on_launch(&mut self, ts: Timestamp, system_address: u64, body_id: u32) {
        let on_body = |k: &SiteKey| k.system_address == system_address && k.body_id == body_id;
        if self.here.as_ref().is_some_and(|h| on_body(&h.key) && (h.key.location.is_some() || h.ship.is_some())) {
            return;
        }
        let key = match self.drops.get(&ts).copied().filter(|k| on_body(k)) {
            Some(k) => k,
            None => {
                let Some(k) = self.targeted.filter(|k| on_body(k)) else { return };
                self.drops.insert(ts, k);
                self.drops_rev += 1;
                k
            }
        };
        self.here = Some(Here { key, ship: None, dropped: true });
    }

    /// DockSRV, or the SRV destroyed: a location taken from a target at a hover drop ends with the run, since the ship
    /// still hovering can take the Rhino anywhere unseen. One a landing named holds while the ship stays down.
    pub fn on_srv_gone(&mut self) {
        if self.here.as_ref().is_some_and(|h| h.dropped) {
            self.here = None;
        }
    }

    /// Supercruise, a jump, leaving the body: the next landing names the location again.
    pub fn leave_body(&mut self) {
        self.here = None;
        self.targeted = None;
    }

    /// Location (login, respawn) and ApproachBody: the named location holds only while still at the same body.
    pub fn at_body(&mut self, system_address: u64, body_id: Option<u32>) {
        let elsewhere = |k: &SiteKey| k.system_address != system_address || Some(k.body_id) != body_id;
        if self.here.as_ref().is_some_and(|h| elsewhere(&h.key)) {
            self.here = None;
        }
        if self.targeted.as_ref().is_some_and(elsewhere) {
            self.targeted = None;
        }
    }

    pub fn on_cracked(&mut self, ts: Timestamp, system: &str, body: Option<&str>) {
        self.session_mut(ts, system, body).cracked += 1;
    }

    pub fn on_drone(&mut self, d: &ev::LaunchDrone, ts: Timestamp) {
        if let Some(s) = self.session.as_mut() {
            *s.limpets.entry(d.kind.clone()).or_default() += 1;
            s.last_activity = ts;
        } else if d.kind == "Prospector" || d.kind == "Collection" {
            // first limpet of a session: start tracking right away
            let s = self.session.get_or_insert_with(|| MiningSession { start: ts, last_activity: ts, ..Default::default() });
            *s.limpets.entry(d.kind.clone()).or_default() += 1;
        }
    }

    /// Ends the session (jumped away or docked).
    pub fn end_session(&mut self) {
        if let Some(s) = self.session.take()
            && (s.prospected > 0 || s.total_refined() > 0) {
                self.history.push(s);
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mining_location_number() {
        assert_eq!(mining_location("$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=4;"), Some(4));
        assert_eq!(mining_location("$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=15;"), Some(15));
        // Other surface signals and places carry an index too, but aren't mining locations.
        assert_eq!(mining_location("$SAA_Unknown_Signal:#type=$SAA_SignalType_Geological;:#index=9;"), None);
        assert_eq!(mining_location("$SAA_Unknown_Signal:#type=$SAA_SignalType_Human;:#index=0;"), None);
        assert_eq!(mining_location("Uutoni Extraction Platform"), None);
        assert_eq!(mining_location(""), None);
        assert_eq!(mining_location("$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=;"), None);
    }

    #[test]
    fn targeted_location_number() {
        assert_eq!(targeted_location("$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=3;", false), Some(3));
        // Bare, as Status.json has also been seen to write it: only on a body with mining locations.
        assert_eq!(targeted_location("$SAA_Unknown_Signal:#index=15;", true), Some(15));
        assert_eq!(targeted_location("$SAA_Unknown_Signal:#index=15;", false), None);
        assert_eq!(targeted_location("$SAA_Unknown_Signal:#type=$SAA_SignalType_Geological;:#index=9;", true), None);
        assert_eq!(targeted_location("$SAA_Unknown_Signal:#index=;", true), None);
        assert_eq!(targeted_location("TCG BARBAROS KFK-L3V", true), None);
    }
}
