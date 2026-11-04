//! The assistant's Spansh body tools: which landable bodies hold a raw material. Each splits "ask Spansh"
//! (`NexusTools`) from "shape the records into JSON" (plain functions), so the request and the shaping are tested
//! offline.

use super::{NexusTools, SPANSH_TIMEOUT, arg_f64, arg_str, r1, strip_nulls};
use nexus_data::materials::{self, Material, MaterialKind};
use nexus_spansh::{BodyRecord, Coords, SearchRequest, SortDir, filter, sort};
use serde_json::{Value, json};
use std::future::Future;

/// Bodies listed.
const BODY_HITS: usize = 10;
/// Spansh stops counting the bodies that match at this many.
const COUNT_CAP: u64 = 10_000;

fn add_note(out: &mut Value, text: &str) {
    let note = out["note"].as_str().map(|n| format!("{n} {text}")).unwrap_or_else(|| text.to_string());
    out["note"] = json!(note);
}

/// How the bodies are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Richest,
    Nearest,
}

/// What `find_material_body` asks Spansh, and how many bodies matched.
struct BodySearch<'a> {
    near: &'a str,
    material: &'a Material,
    radius_ly: f64,
    max_gravity_g: f64,
    max_ls: Option<f64>,
    order: Order,
    /// Bodies within the radius that hold it (Spansh stops counting at [`COUNT_CAP`]).
    matched: u64,
}

/// Spansh's body search for the material: landable bodies within the radius that hold it, light enough and near enough
/// their star, richest or nearest first. Spansh filters on the material's share itself, so one request answers.
fn request(origin: [f64; 3], s: &BodySearch) -> SearchRequest {
    let mut req = SearchRequest::new()
        .near(Coords { x: origin[0], y: origin[1], z: origin[2] })
        .within(s.radius_ly)
        .filter("is_landable", filter::eq(true))
        .filter("gravity", filter::between(0.0, s.max_gravity_g))
        .filter("materials", json!([{ "name": s.material.name, "share": [0.0001, 100], "comparison": "<=>" }]))
        .size(BODY_HITS as u32);
    if let Some(m) = s.max_ls {
        req = req.filter("distance_to_arrival", filter::between(0.0, m));
    }
    match s.order {
        Order::Richest => req.sort(sort::named("materials", &s.material.name, SortDir::Desc)),
        Order::Nearest => req.sort_by("distance", SortDir::Asc),
    }
}

/// The reply of `find_material_body`: the bodies Spansh listed, in its order, and how many hold the material.
fn shape_bodies(s: &BodySearch, records: &[BodyRecord]) -> Value {
    let rows: Vec<Value> = records
        .iter()
        .filter_map(|b| {
            let share = b.materials.iter().find(|m| m.name.eq_ignore_ascii_case(&s.material.name)).map(|m| m.share)?;
            let geo = b.signal("Geological");
            let mut v = json!({
                "body": b.name, "system": b.system_name, "share_pct": (share * 100.0).round() / 100.0,
                "distance_ly": b.distance.map(r1), "distance_ls": b.distance_to_arrival.map(f64::round),
                "gravity_g": b.gravity.map(|g| (g * 100.0).round() / 100.0), "geological_signals": (geo > 0).then_some(geo),
            });
            strip_nulls(&mut v);
            Some(v)
        })
        .collect();
    let holding = if s.matched >= COUNT_CAP { json!(format!("{COUNT_CAP} or more")) } else { json!(s.matched) };
    let order = match s.order {
        Order::Richest => "richest first",
        Order::Nearest => "nearest first",
    };
    let mut out = json!({
        "near": s.near, "material": s.material.name, "rarity_grade": s.material.grade,
        "within_ly": s.radius_ly, "max_gravity_g": s.max_gravity_g, "max_distance_ls": s.max_ls,
        "bodies_holding_it": holding, "order": order, "bodies": rows,
        "note": "share_pct is the material's share of the body's surface materials, from the scans commanders shared through EDDN that Spansh holds: a body nobody scanned isn't listed.",
    });
    strip_nulls(&mut out);
    if s.matched == 0 {
        add_note(&mut out, &format!("No landable body within {} ly is known to hold it: try a larger max_distance_ly or max_gravity_g.", s.radius_ly));
    } else if s.matched > rows.len() as u64 {
        add_note(&mut out, &format!("The {} {order} of them are listed.", rows.len()));
    }
    out
}

impl NexusTools {
    /// One Spansh call, under the usual timeout.
    fn spansh_call<T>(&self, call: impl Future<Output = nexus_spansh::Result<T>>) -> Result<T, String> {
        match self.rt.block_on(async { tokio::time::timeout(SPANSH_TIMEOUT, call).await }) {
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
        let radius_ly = arg_f64(a, "max_distance_ly").unwrap_or(100.0).clamp(1.0, f64::from(*crate::tours::limits::RADIUS_LY.end()));
        let order = if arg_str(a, "sort").is_some_and(|s| s.eq_ignore_ascii_case("nearest")) { Order::Nearest } else { Order::Richest };
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let mut s = BodySearch { near: &near, material, radius_ly, max_gravity_g, max_ls, order, matched: 0 };
        let found = self.spansh_call(self.spansh.search_bodies(&request(origin, &s)))?;
        s.matched = found.count;
        Ok(shape_bodies(&s, &found.results))
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

    fn search(material: &'static Material, order: Order, matched: u64) -> BodySearch<'static> {
        BodySearch { near: "Deciat", material, radius_ly: 100.0, max_gravity_g: 1.0, max_ls: Some(50_000.0), order, matched }
    }

    /// Spansh filters on the material itself: one request, not pages of every landable body nearby.
    #[test]
    fn spansh_is_asked_for_the_material() {
        let polonium = materials::material("Polonium").unwrap();
        let j = request([1.0, 2.0, 3.0], &search(polonium, Order::Richest, 0)).to_json();
        let f = &j["filters"];
        assert_eq!(f["materials"], json!([{ "name": "Polonium", "share": [0.0001, 100], "comparison": "<=>" }]));
        assert_eq!(f["is_landable"]["value"], true);
        assert_eq!(f["gravity"]["value"], json!([0, 1]));
        assert_eq!(f["distance_to_arrival"]["value"], json!([0, 50_000]));
        assert_eq!(f["distance"]["max"], "100");
        assert_eq!(j["sort"], json!([{ "materials": [{ "name": "Polonium", "direction": "desc" }] }]));
        assert_eq!(j["size"], 10);
        let nearest = request([0.0; 3], &search(polonium, Order::Nearest, 0)).to_json();
        assert_eq!(nearest["sort"], json!([{ "distance": { "direction": "asc" } }]));
    }

    #[test]
    fn the_reply_lists_spanshs_bodies_and_counts_them() {
        let polonium = materials::material("Polonium").unwrap();
        let records = vec![body("Deciat 2", 0.3, 100.0, &[("Polonium", 1.12), ("Iron", 20.0)]), body("Deciat 1", 0.2, 90.0, &[("polonium", 0.9)])];
        let v = shape_bodies(&search(polonium, Order::Richest, 4_442), &records);
        assert_eq!((&v["bodies"][0]["body"], &v["bodies"][0]["share_pct"]), (&json!("Deciat 2"), &json!(1.12)));
        assert_eq!(v["bodies"][1]["share_pct"], 0.9, "the name is matched whatever its case");
        assert_eq!((&v["bodies_holding_it"], &v["order"], &v["within_ly"]), (&json!(4_442), &json!("richest first"), &json!(100.0)));
        assert!(v["note"].as_str().unwrap().contains("The 2 richest first of them are listed"), "{v}");
        let many = shape_bodies(&search(polonium, Order::Nearest, COUNT_CAP), &records);
        assert_eq!(many["bodies_holding_it"], "10000 or more");
        let none = shape_bodies(&search(polonium, Order::Richest, 0), &[]);
        assert!(none["note"].as_str().unwrap().contains("No landable body within 100 ly"), "{none}");
        assert!(none.to_string().len() < nexus_ai::agent::MAX_RESULT_CHARS);
    }
}
