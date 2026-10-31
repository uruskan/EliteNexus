//! The assistant's Spansh body tool: which landable bodies hold a raw material. It splits "ask Spansh" (`NexusTools`)
//! from "shape the records into JSON" (plain functions), so the shaping is tested offline.

use super::{NexusTools, SPANSH_TIMEOUT, arg_f64, arg_str, r1};
use nexus_data::materials::{self, Material, MaterialKind};
use nexus_spansh::{BodyRecord, Coords, SearchRequest, SortDir, filter};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::future::Future;
use std::time::{Duration, Instant};

/// Spansh's body search answers 100 rows a page here; five pages are 500 bodies. The cap keeps the answer quick: each
/// page is one request to Spansh.
const BODY_PAGE: u32 = 100;
const MAX_BODY_PAGES: u32 = 5;
/// Bodies listed, and the number of hits after which the search stops reading further pages.
const BODY_HITS: usize = 10;
/// What `BodySearch::interrupted` holds when the search ran out of time.
const TIME_LIMIT: &str = "time limit";
/// The whole body search, all pages together, stops after this.
const BODY_SEARCH_LIMIT: Duration = Duration::from_secs(60);

/// How long the next Spansh page may take, or `None` when the search has used up its time.
fn page_budget(elapsed: Duration) -> Option<Duration> {
    let left = BODY_SEARCH_LIMIT.checked_sub(elapsed).filter(|d| !d.is_zero())?;
    Some(left.min(SPANSH_TIMEOUT))
}

/// The error when the search's time is gone before it read any bodies: looking up the starting system counts too.
fn no_time_left() -> String {
    format!("The search used up its {} second time limit before reading any bodies (finding the starting system took too long); try again.", BODY_SEARCH_LIMIT.as_secs())
}

/// The error when the first page fails: the time limit if that is what cut it short, else what Spansh said.
fn first_page_failure(error: String, elapsed: Duration) -> String {
    if page_budget(elapsed).is_none() { no_time_left() } else { error }
}

/// Why a body search stopped: its own time limit when that is used up (a page cut short by it comes back as a Spansh
/// timeout), else the error Spansh gave.
fn interruption(error: String, elapsed: Duration) -> String {
    if page_budget(elapsed).is_none() { TIME_LIMIT.to_string() } else { error }
}

fn add_note(out: &mut Value, text: &str) {
    let note = out["note"].as_str().map(|n| format!("{n} {text}")).unwrap_or_else(|| text.to_string());
    out["note"] = json!(note);
}

fn round_ls(ls: Option<f64>) -> Option<i64> {
    ls.map(|v| v.round() as i64)
}

fn strip_nulls_in(rows: &mut [Value]) {
    for r in rows {
        super::strip_nulls(r);
    }
}

/// The bodies of one Spansh page that hold `material` (any share), light enough and near enough their star, with
/// their share.
fn bodies_with(records: Vec<BodyRecord>, material: &str, max_gravity_g: f64, max_ls: Option<f64>) -> Vec<(f64, BodyRecord)> {
    records
        .into_iter()
        .filter(|b| b.gravity.is_none_or(|g| g <= max_gravity_g))
        .filter(|b| max_ls.is_none_or(|m| b.distance_to_arrival.is_none_or(|d| d <= m)))
        .filter_map(|b| {
            let share = b.materials.iter().find(|m| m.name.eq_ignore_ascii_case(material)).map(|m| m.share)?;
            (share > 0.0).then_some((share, b))
        })
        .collect()
}

/// What `find_material_body` read, for `shape_bodies`.
struct BodySearch<'a> {
    near: &'a str,
    material: &'a Material,
    max_gravity_g: f64,
    max_ls: Option<f64>,
    /// Bodies read from Spansh.
    searched: usize,
    /// The search stopped before the cap because it had enough hits.
    enough: bool,
    /// Why it stopped early, when Spansh stopped answering.
    interrupted: Option<String>,
}

/// The reply of `find_material_body`: the richest bodies first, and how far the search looked.
fn shape_bodies(s: &BodySearch, mut hits: Vec<(f64, BodyRecord)>) -> Value {
    hits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal).then_with(|| a.1.distance.partial_cmp(&b.1.distance).unwrap_or(Ordering::Equal)));
    let found = hits.len();
    let mut rows: Vec<Value> = hits
        .iter()
        .take(BODY_HITS)
        .map(|(share, b)| {
            let geo = b.signal("Geological");
            json!({
                "body": b.name, "system": b.system_name, "share_pct": (share * 100.0).round() / 100.0,
                "distance_ly": b.distance.map(r1), "distance_ls": round_ls(b.distance_to_arrival),
                "gravity_g": b.gravity.map(|g| (g * 100.0).round() / 100.0), "geological_signals": (geo > 0).then_some(geo),
            })
        })
        .collect();
    strip_nulls_in(&mut rows);
    let cap = (BODY_PAGE * MAX_BODY_PAGES) as usize;
    let mut out = json!({
        "near": s.near, "material": s.material.name, "rarity_grade": s.material.grade, "bodies": rows,
        "searched_bodies": s.searched, "search_cap": cap, "max_gravity_g": s.max_gravity_g,
        "note": format!("Looked at the {} nearest landable bodies with gravity up to {} g, nearest first, never more than {cap} (a cap that keeps the answer quick): a richer body may lie farther out. share_pct is the material's share of the surface deposits as Spansh holds it, from scans players sent to EDDN; a body nobody scanned is missing.", s.searched, s.max_gravity_g),
    });
    if let Some(m) = s.max_ls {
        out["max_distance_ls"] = json!(m);
    }
    if found > BODY_HITS {
        out["found"] = json!(found);
        add_note(&mut out, &format!("{found} bodies hold it in the part searched: the richest {BODY_HITS} are listed."));
    }
    if s.enough {
        add_note(&mut out, "The search stopped as soon as it had enough bodies, so the nearer ones are listed, not the richest in the whole cap.");
    }
    match s.interrupted.as_deref() {
        Some(TIME_LIMIT) => add_note(&mut out, &format!("The search stopped early: it reached its {} second time limit.", BODY_SEARCH_LIMIT.as_secs())),
        Some(e) => add_note(&mut out, &format!("The search stopped early: {e}.")),
        None => {}
    }
    if found == 0 {
        add_note(&mut out, "None of them holds it: try a larger max_gravity_g or another near_system.");
    }
    out
}

impl NexusTools {
    /// One Spansh call, under `timeout`.
    fn spansh_call_within<T>(&self, timeout: Duration, call: impl Future<Output = nexus_spansh::Result<T>>) -> Result<T, String> {
        match self.rt.block_on(async { tokio::time::timeout(timeout, call).await }) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(format!("Spansh: {e}")),
            Err(_) => Err("Spansh: no answer in time".to_string()),
        }
    }

    pub(super) fn find_material_body(&self, a: &Value) -> Result<Value, String> {
        let asked = arg_str(a, "material").ok_or("material is required")?;
        let material = materials::material(&asked).ok_or_else(|| format!("Unknown material \"{asked}\". Give a raw material, e.g. Polonium, Tellurium, Niobium or Arsenic."))?;
        if material.kind != MaterialKind::Raw {
            return Err(format!(
                "{} is a {} material, not a raw one: this tool finds raw materials, the ones lying on planetary surfaces (e.g. Polonium).",
                material.name,
                material.kind.label().to_lowercase()
            ));
        }
        let max_gravity_g = arg_f64(a, "max_gravity_g").unwrap_or(1.0);
        if max_gravity_g <= 0.0 {
            return Err("max_gravity_g must be above 0.".to_string());
        }
        let max_ls = arg_f64(a, "max_distance_ls").filter(|d| *d > 0.0);
        // The limit counts from here: looking up near_system takes its share.
        let started = Instant::now();
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let mut hits: Vec<(f64, BodyRecord)> = Vec::new();
        let (mut searched, mut enough, mut interrupted) = (0usize, false, None);
        for page in 0..MAX_BODY_PAGES {
            // Every page gets what is left of the search's time; none starts once it is used up.
            let Some(budget) = page_budget(started.elapsed()) else {
                if page == 0 {
                    return Err(no_time_left());
                }
                interrupted = Some(TIME_LIMIT.to_string());
                break;
            };
            let req = SearchRequest::new()
                .near(Coords { x: origin[0], y: origin[1], z: origin[2] })
                .filter("is_landable", filter::eq(true))
                .filter("gravity", filter::between(0.0, max_gravity_g))
                .sort_by("distance", SortDir::Asc)
                .size(BODY_PAGE)
                .page(page);
            let rows = match self.spansh_call_within(budget, self.spansh.search_bodies(&req)) {
                Ok(res) => res.results,
                Err(e) if page == 0 => return Err(first_page_failure(e, started.elapsed())),
                Err(e) => {
                    interrupted = Some(interruption(e, started.elapsed()));
                    break;
                }
            };
            let read = rows.len();
            searched += read;
            hits.extend(bodies_with(rows, &material.name, max_gravity_g, max_ls));
            if hits.len() >= BODY_HITS {
                enough = page + 1 < MAX_BODY_PAGES && read == BODY_PAGE as usize;
                break;
            }
            if read < BODY_PAGE as usize {
                break;
            }
        }
        Ok(shape_bodies(&BodySearch { near: &near, material, max_gravity_g, max_ls, searched, enough, interrupted }, hits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(name: &str, gravity: f64, ls: f64, shares: &[(&str, f64)]) -> BodyRecord {
        BodyRecord {
            name: name.into(),
            system_name: "Deciat".into(),
            gravity: Some(gravity),
            distance: Some(3.0),
            distance_to_arrival: Some(ls),
            materials: shares.iter().map(|(n, s)| nexus_spansh::records::NameShare { name: (*n).into(), share: *s }).collect(),
            ..BodyRecord::default()
        }
    }

    #[test]
    fn bodies_are_ranked_by_share_within_gravity_and_distance() {
        let polonium = materials::material("Polonium").unwrap();
        let records = vec![
            body("Deciat 1", 0.2, 100.0, &[("Polonium", 1.5), ("Iron", 20.0)]),
            body("Deciat 2", 0.3, 100.0, &[("Polonium", 2.75)]),
            body("Deciat 3", 0.3, 100.0, &[("Iron", 20.0)]),
            body("Deciat 4", 1.7, 100.0, &[("Polonium", 9.0)]),
            body("Deciat 5", 0.3, 90_000.0, &[("polonium", 8.0)]),
        ];
        let hits = bodies_with(records, "Polonium", 1.0, Some(50_000.0));
        assert_eq!(hits.len(), 2, "no polonium, too heavy and too far are left out");
        let s = BodySearch { near: "Deciat", material: polonium, max_gravity_g: 1.0, max_ls: Some(50_000.0), searched: 5, enough: false, interrupted: None };
        let v = shape_bodies(&s, hits);
        assert_eq!(v["bodies"][0]["body"], "Deciat 2");
        assert_eq!(v["bodies"][0]["share_pct"], 2.75);
        assert_eq!(v["bodies"][1]["body"], "Deciat 1");
        assert_eq!((v["searched_bodies"].as_u64(), v["search_cap"].as_u64()), (Some(5), Some(500)));
        assert_eq!(v["max_distance_ls"], 50_000.0);
        let none = shape_bodies(&s, Vec::new());
        assert!(none["note"].as_str().unwrap().contains("None of them"), "{none}");
    }

    #[test]
    fn the_body_search_has_a_time_limit() {
        let s = Duration::from_secs;
        assert_eq!(page_budget(s(0)), Some(SPANSH_TIMEOUT), "a page never gets more than the usual timeout");
        assert_eq!(page_budget(s(40)), Some(s(20)), "later pages get what is left");
        assert_eq!(page_budget(s(59)), Some(s(1)));
        assert_eq!(page_budget(BODY_SEARCH_LIMIT), None);
        assert_eq!(page_budget(s(90)), None);
        // A page cut short by the limit comes back as a Spansh timeout; it is reported as the limit, not as Spansh's.
        let timeout = || "Spansh: no answer in time".to_string();
        assert_eq!(interruption(timeout(), s(60)), TIME_LIMIT);
        assert_eq!(interruption(timeout(), s(75)), TIME_LIMIT);
        assert_eq!(interruption(timeout(), s(30)), "Spansh: no answer in time", "time was left: Spansh's own error stands");
        // The same on the first page: the limit's own error, which doesn't blame Spansh; else Spansh's.
        let limit = first_page_failure(timeout(), s(61));
        assert!(limit.contains("60 second time limit") && limit.contains("finding the starting system") && !limit.contains("Spansh"), "{limit}");
        assert_eq!(limit, no_time_left());
        assert_eq!(first_page_failure(timeout(), s(10)), "Spansh: no answer in time");
        // So five pages can never take longer than the limit, whatever Spansh does.
        let polonium = materials::material("Polonium").unwrap();
        let search = BodySearch { near: "Deciat", material: polonium, max_gravity_g: 1.0, max_ls: None, searched: 200, enough: false, interrupted: Some(TIME_LIMIT.into()) };
        let note = shape_bodies(&search, Vec::new())["note"].as_str().unwrap().to_string();
        assert!(note.contains("60 second time limit"), "{note}");
    }

    #[test]
    fn many_bodies_are_cut_to_the_richest_ten() {
        let polonium = materials::material("Polonium").unwrap();
        let hits: Vec<(f64, BodyRecord)> = (0..40).map(|i| (f64::from(i), body(&format!("Deciat {i}"), 0.2, 100.0, &[("Polonium", f64::from(i))]))).collect();
        let s = BodySearch { near: "Deciat", material: polonium, max_gravity_g: 1.0, max_ls: None, searched: 300, enough: true, interrupted: Some("Spansh: no answer in time".into()) };
        let v = shape_bodies(&s, hits);
        assert_eq!(v["bodies"].as_array().unwrap().len(), 10);
        assert_eq!(v["bodies"][0]["share_pct"], 39.0);
        assert_eq!(v["found"], 40);
        let note = v["note"].as_str().unwrap();
        assert!(note.contains("stopped as soon") && note.contains("stopped early"), "{note}");
        assert!(v.to_string().len() < nexus_ai::agent::MAX_RESULT_CHARS);
    }
}
