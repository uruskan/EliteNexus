//! Evaluates `nexus_data::bio_predict` against the commander's own journals (read-only).
//!
//! ```text
//! cargo run -p nexus-data --example bio_eval [-- [journal dir] [--verbose]]
//! ```
//!
//! For every body with biological signals it rebuilds the `Scan` facts (plus parent stars via `Parents`/`BodyID`,
//! `SAASignalsFound` genera and signal counts, the region from `CodexEntry`), takes the species actually found
//! there (`ScanOrganic`, biology `CodexEntry`), runs `predict()` before DSS (signal count only) and after DSS
//! (known genera), and reports per-body results plus recall@1/@3 per genus, genus-level precision/recall,
//! false-positive rate, colour accuracy and the first-logged-bonus rule against `SellOrganicData`.
//! A synthetic pass (one body at the centre of every rule set) checks rule coverage and ambiguity.

use nexus_data::bio_predict::{self, BodyFacts, ParentRef, Prediction, StarFacts, SystemStar};
use nexus_data::exobio::{self, ColourBasis, RuleSet, Species, VolcanismRule};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

type BodyKey = (u64, u32);

#[derive(Default)]
struct System {
    name: String,
    star_pos: Option<[f64; 3]>,
    population: Option<u64>,
    region: Option<String>,
}

#[derive(Default, Clone)]
struct Organic {
    genus: String,
    variant_colour: Option<String>,
    was_logged: Option<bool>,
    analysed: bool,
}

#[derive(Default)]
struct Body {
    name: String,
    first_scan: Option<Value>,
    last_scan: Option<Value>,
    fss_bio: Option<u32>,
    saa_bio: Option<u32>,
    saa_genera: Option<Vec<String>>,
    /// species name → what we saw
    organics: BTreeMap<String, Organic>,
}

#[derive(Default)]
struct Journals {
    systems: BTreeMap<u64, System>,
    bodies: BTreeMap<BodyKey, Body>,
    /// (species codex, variant codex, value, bonus, matched body)
    sales: Vec<(String, String, i64, i64, Option<BodyKey>)>,
    files: usize,
    lines: usize,
    bad_lines: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let dir = args.iter().find(|a| !a.starts_with('-')).map(PathBuf::from).unwrap_or_else(default_journal_dir);
    let j = match read_journals(&dir) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cannot read journals in {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    println!(
        "Read {} journal files ({} lines, {} unparsable) from {}",
        j.files,
        j.lines,
        j.bad_lines,
        dir.display()
    );
    evaluate(&j, verbose);
    synthetic(verbose);
}

fn default_journal_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join("Saved Games").join("Frontier Developments").join("Elite Dangerous")
}

// ------------------------------------------------------------------------------------------------
// Journal reading
// ------------------------------------------------------------------------------------------------

fn read_journals(dir: &std::path::Path) -> std::io::Result<Journals> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("Journal.") && n.ends_with(".log"))
        })
        .collect();
    files.sort();
    let mut j = Journals { files: files.len(), ..Journals::default() };
    // Analysed organics not yet sold: (species codex, variant codex, body).
    let mut pending: Vec<(String, String, BodyKey)> = Vec::new();
    for path in &files {
        let bytes = std::fs::read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            j.lines += 1;
            let Ok(e) = serde_json::from_str::<Value>(line) else {
                j.bad_lines += 1;
                continue;
            };
            handle(&mut j, &mut pending, &e);
        }
    }
    Ok(j)
}

fn s<'a>(e: &'a Value, k: &str) -> Option<&'a str> {
    e.get(k).and_then(Value::as_str)
}

fn u(e: &Value, k: &str) -> Option<u64> {
    e.get(k).and_then(Value::as_u64)
}

fn handle(j: &mut Journals, pending: &mut Vec<(String, String, BodyKey)>, e: &Value) {
    let Some(event) = s(e, "event") else { return };
    let addr = u(e, "SystemAddress");
    match event {
        "FSDJump" | "Location" | "CarrierJump" => {
            let Some(addr) = addr else { return };
            let sys = j.systems.entry(addr).or_default();
            if let Some(n) = s(e, "StarSystem") {
                sys.name = n.to_string();
            }
            if let Some(p) = e.get("StarPos").and_then(Value::as_array)
                && let [x, y, z] = p.as_slice()
                && let (Some(x), Some(y), Some(z)) = (x.as_f64(), y.as_f64(), z.as_f64())
            {
                sys.star_pos = Some([x, y, z]);
            }
            if let Some(pop) = u(e, "Population") {
                sys.population = Some(pop);
            }
        }
        "Scan" => {
            let (Some(addr), Some(id)) = (addr, u(e, "BodyID")) else { return };
            let key = (addr, id as u32);
            let body = j.bodies.entry(key).or_default();
            body.name = s(e, "BodyName").unwrap_or_default().to_string();
            if body.first_scan.is_none() {
                body.first_scan = Some(e.clone());
            }
            body.last_scan = Some(e.clone());
            if let Some(n) = s(e, "StarSystem") {
                let sys = j.systems.entry(addr).or_default();
                if sys.name.is_empty() {
                    sys.name = n.to_string();
                }
            }
        }
        "FSSBodySignals" | "SAASignalsFound" => {
            let (Some(addr), Some(id)) = (addr, u(e, "BodyID")) else { return };
            let body = j.bodies.entry((addr, id as u32)).or_default();
            let bio = e.get("Signals").and_then(Value::as_array).and_then(|sigs| {
                sigs.iter()
                    .find(|sg| s(sg, "Type").is_some_and(|t| t.contains("Biological")))
                    .and_then(|sg| u(sg, "Count"))
                    .map(|c| c as u32)
            });
            if event == "FSSBodySignals" {
                body.fss_bio = bio.or(body.fss_bio);
            } else {
                body.saa_bio = Some(bio.unwrap_or(0));
                let genera: Vec<String> = e
                    .get("Genuses")
                    .and_then(Value::as_array)
                    .map(|g| g.iter().filter_map(|g| s(g, "Genus").map(str::to_string)).collect())
                    .unwrap_or_default();
                body.saa_genera = Some(genera);
            }
        }
        "ScanOrganic" => {
            let (Some(addr), Some(id)) = (addr, u(e, "Body")) else { return };
            let key = (addr, id as u32);
            let species_sym = s(e, "Species").unwrap_or_default();
            let Some(sp) = exobio::species(species_sym).or_else(|| s(e, "Species_Localised").and_then(exobio::species)) else {
                return;
            };
            let variant_sym = s(e, "Variant").unwrap_or_default();
            let colour = exobio::variant_by_codex(variant_sym)
                .map(|(_, v)| v.colour.to_string())
                .or_else(|| s(e, "Variant_Localised").and_then(|v| v.split_once(" - ")).map(|(_, c)| c.trim().to_string()));
            let body = j.bodies.entry(key).or_default();
            let o = body
                .organics
                .entry(sp.name.to_string())
                .or_insert_with(|| Organic { genus: sp.genus.to_string(), ..Organic::default() });
            if o.variant_colour.is_none() {
                o.variant_colour = colour;
            }
            // Latest value: it can flip to true while sampling when someone else sells the species first.
            if let Some(logged) = e.get("WasLogged").and_then(Value::as_bool) {
                o.was_logged = Some(logged);
            }
            if s(e, "ScanType") == Some("Analyse") {
                o.analysed = true;
                pending.push((sp.codex.to_string(), variant_sym.to_string(), key));
            }
        }
        "CodexEntry" => {
            if let (Some(addr), Some(region)) = (addr, s(e, "Region_Localised").or_else(|| s(e, "Region"))) {
                j.systems.entry(addr).or_default().region = Some(region.to_string());
            }
            if !s(e, "Category").is_some_and(|c| c.contains("Biology")) {
                return;
            }
            let (Some(addr), Some(id)) = (addr, u(e, "BodyID")) else { return };
            let name = s(e, "Name").unwrap_or_default();
            let (sp, colour) = match exobio::variant_by_codex(name) {
                Some((sp, v)) => (sp, Some(v.colour.to_string())),
                None => match exobio::species(name) {
                    Some(sp) => (sp, None),
                    None => return,
                },
            };
            if sp.thargoid {
                return;
            }
            let body = j.bodies.entry((addr, id as u32)).or_default();
            let o = body.organics.entry(sp.name.to_string()).or_insert_with(|| Organic {
                genus: sp.genus.to_string(),
                ..Organic::default()
            });
            if o.variant_colour.is_none() {
                o.variant_colour = colour;
            }
        }
        "SellOrganicData" => {
            for b in e.get("BioData").and_then(Value::as_array).into_iter().flatten() {
                let species = s(b, "Species").unwrap_or_default().to_string();
                let variant = s(b, "Variant").unwrap_or_default().to_string();
                let value = b.get("Value").and_then(Value::as_i64).unwrap_or(0);
                let bonus = b.get("Bonus").and_then(Value::as_i64).unwrap_or(0);
                let idx = pending
                    .iter()
                    .position(|(sp, v, _)| sp.eq_ignore_ascii_case(&species) && v.eq_ignore_ascii_case(&variant))
                    .or_else(|| pending.iter().position(|(sp, _, _)| sp.eq_ignore_ascii_case(&species)));
                let body = idx.map(|i| pending.remove(i).2);
                j.sales.push((species, variant, value, bonus, body));
            }
        }
        "Died" => pending.clear(),
        _ => {}
    }
}

// ------------------------------------------------------------------------------------------------
// Facts
// ------------------------------------------------------------------------------------------------

fn facts_for(j: &Journals, key: BodyKey, with_region: bool) -> Option<BodyFacts> {
    let body = j.bodies.get(&key)?;
    let scan = body.last_scan.as_ref()?;
    let mut f = BodyFacts::from_journal_scan(scan)?;
    let (addr, _) = key;
    let system_scans: Vec<&Value> =
        j.bodies.range((addr, 0)..=(addr, u32::MAX)).filter_map(|(_, b)| b.last_scan.as_ref()).collect();
    let stars: Vec<SystemStar> = system_scans
        .iter()
        .filter_map(|sc| {
            Some(SystemStar {
                body_id: u(sc, "BodyID")? as u32,
                parent: sc.get("Parents").map(ParentRef::parse_journal).and_then(|p| p.first().copied()),
                distance_ls: sc.get("DistanceFromArrivalLS").and_then(Value::as_f64).unwrap_or(0.0),
                facts: StarFacts::from_journal_scan(sc)?,
            })
        })
        .collect();
    let parents = scan.get("Parents").map(ParentRef::parse_journal).unwrap_or_default();
    f.parent_stars = bio_predict::resolve_parent_stars(&parents, &stars);
    f.system_stars = stars.iter().map(|s| s.facts.clone()).collect();
    f.system_body_classes = system_scans.iter().filter_map(|sc| s(sc, "PlanetClass").map(str::to_string)).collect();
    if let Some(sys) = j.systems.get(&addr) {
        if f.system_name.is_empty() {
            f.system_name.clone_from(&sys.name);
        }
        f.star_pos = sys.star_pos;
        if with_region {
            f.region.clone_from(&sys.region);
        }
    }
    f.bio_signal_count = body.saa_bio.or(body.fss_bio);
    Some(f)
}

fn describe(f: &BodyFacts) -> String {
    let stars: Vec<String> = f
        .parent_stars
        .iter()
        .map(|s| format!("{}{}", s.star_type, s.luminosity.as_deref().map(|l| format!(" {l}")).unwrap_or_default()))
        .collect();
    format!(
        "{}, {}, {:.3} g, {:.1} K, {:.4} atm, {}; stars {}; region {}; bio {}",
        f.planet_class,
        f.atmosphere_type.as_deref().unwrap_or("no atmosphere"),
        f.gravity_g.unwrap_or(f64::NAN),
        f.temperature_k.unwrap_or(f64::NAN),
        f.pressure_atm.unwrap_or(f64::NAN),
        f.volcanism.as_deref().unwrap_or("no volcanism"),
        if stars.is_empty() { "?".to_string() } else { stars.join(" > ") },
        f.region.as_deref().unwrap_or("?"),
        f.bio_signal_count.map_or("?".to_string(), |n| n.to_string()),
    )
}

// ------------------------------------------------------------------------------------------------
// Metrics
// ------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Recall {
    n: usize,
    at1: usize,
    at3: usize,
    missing: usize,
}

impl Recall {
    fn add(&mut self, rank: Option<usize>) {
        self.n += 1;
        match rank {
            Some(r) => {
                self.at1 += usize::from(r == 1);
                self.at3 += usize::from(r <= 3);
            }
            None => self.missing += 1,
        }
    }
    fn show(&self) -> String {
        if self.n == 0 {
            return "n=0".to_string();
        }
        format!(
            "n={:<3} recall@1 {:>5.1}%  recall@3 {:>5.1}%  not predicted {}",
            self.n,
            pct(self.at1, self.n),
            pct(self.at3, self.n),
            self.missing
        )
    }
}

/// Reliability table: predicted probability bins vs observed frequency.
#[derive(Default)]
struct Calibration {
    bins: [(f64, usize, usize); 5],
}

impl Calibration {
    fn add(&mut self, p: f64, hit: bool) {
        let i = ((p * 5.0) as usize).min(4);
        self.bins[i].0 += p;
        self.bins[i].1 += 1;
        self.bins[i].2 += usize::from(hit);
    }
    fn show(&self) -> String {
        self.bins
            .iter()
            .enumerate()
            .filter(|(_, b)| b.1 > 0)
            .map(|(i, (sum, n, hits))| {
                format!("[{:.1}-{:.1}) n={n} mean p {:.2} observed {:.2}", i as f64 / 5.0, (i + 1) as f64 / 5.0, sum / *n as f64, *hits as f64 / *n as f64)
            })
            .collect::<Vec<_>>()
            .join("
    ")
    }
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 }
}

/// 1-based rank of `species` among the predictions of its genus.
fn rank_in_genus(preds: &[Prediction], genus: &str, species: &str) -> Option<usize> {
    preds.iter().filter(|p| p.genus == genus).position(|p| p.species == species).map(|i| i + 1)
}

fn genus_presence(preds: &[Prediction]) -> BTreeMap<String, f64> {
    let mut m: BTreeMap<String, f64> = BTreeMap::new();
    for p in preds {
        *m.entry(p.genus.clone()).or_default() += f64::from(p.likelihood);
    }
    m
}

fn short(preds: &[Prediction], max: usize) -> String {
    preds
        .iter()
        .take(max)
        .map(|p| {
            format!(
                "{} {:.2}{}",
                p.species,
                p.likelihood,
                p.variant.as_deref().map(|v| format!(" [{v}]")).unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn evaluate(j: &Journals, verbose: bool) {
    let bio_bodies: Vec<BodyKey> = j
        .bodies
        .iter()
        .filter(|(_, b)| {
            b.fss_bio.unwrap_or(0) > 0 || b.saa_bio.unwrap_or(0) > 0 || !b.organics.is_empty()
        })
        .map(|(k, _)| *k)
        .collect();
    println!("\nBodies with biological signals: {}", bio_bodies.len());

    let mut post = Recall::default();
    let mut pre = Recall::default();
    let mut post_noregion = Recall::default();
    let mut per_genus: BTreeMap<String, (Recall, Recall)> = BTreeMap::new();
    let mut colour_ok = 0;
    let mut colour_wrong = 0;
    let mut colour_none = 0;
    // genus level (bodies with DSS genera)
    let (mut tp, mut fp, mut fneg) = (0usize, 0usize, 0usize);
    let (mut topn_hit, mut topn_total) = (0usize, 0usize);
    let mut brier = (0.0f64, 0usize);
    let mut cand_pre = (0usize, 0usize);
    let mut cand_post = (0usize, 0usize);
    let mut cand_genera_pre = 0usize;
    let mut calib_genus = Calibration::default();
    let mut calib_species = Calibration::default();
    let mut value_check: Vec<(String, i64, i64)> = Vec::new();

    for key in &bio_bodies {
        let body = &j.bodies[key];
        let Some(f) = facts_for(j, *key, true) else {
            println!("\n== {} (no Scan event — skipped)", body.name);
            continue;
        };
        let known: Vec<String> = body.saa_genera.clone().unwrap_or_default();
        let known_names: BTreeSet<String> =
            known.iter().filter_map(|g| exobio::genus(g).map(|g| g.name.to_string())).collect();
        let pre_preds = bio_predict::predict(&f, &[]);
        let post_preds = if known.is_empty() { Vec::new() } else { bio_predict::predict(&f, &known) };
        let mut f_noregion = f.clone();
        f_noregion.region = None;
        let post_noregion_preds = if known.is_empty() { Vec::new() } else { bio_predict::predict(&f_noregion, &known) };

        cand_pre.0 += pre_preds.len();
        cand_pre.1 += 1;
        let pre_genera = genus_presence(&pre_preds);
        cand_genera_pre += pre_genera.len();
        if !known.is_empty() {
            cand_post.0 += post_preds.len();
            cand_post.1 += 1;
        }

        println!("\n== {}  [{}]", body.name, describe(&f));
        if !known_names.is_empty() {
            println!("   DSS genera: {}", known_names.iter().cloned().collect::<Vec<_>>().join(", "));
        }
        let actual: Vec<(&String, &Organic)> = body.organics.iter().collect();
        if !actual.is_empty() {
            let list: Vec<String> = actual
                .iter()
                .map(|(n, o)| format!("{n}{}", o.variant_colour.as_deref().map(|c| format!(" ({c})")).unwrap_or_default()))
                .collect();
            println!("   found: {}", list.join(", "));
        }
        println!("   pre-DSS  ({} predictions, {} genera): {}", pre_preds.len(), pre_genera.len(), short(&pre_preds, 8));
        let gp: Vec<String> = pre_genera.iter().map(|(g, p)| format!("{g} {p:.2}")).collect();
        println!("   pre-DSS genus odds: {}", gp.join(", "));
        if !post_preds.is_empty() {
            let mut by_genus: BTreeMap<&str, Vec<&Prediction>> = BTreeMap::new();
            for p in &post_preds {
                by_genus.entry(p.genus.as_str()).or_default().push(p);
            }
            for (g, ps) in by_genus {
                let line: Vec<String> = ps
                    .iter()
                    .map(|p| {
                        let mark = if body.organics.contains_key(&p.species) { "*" } else { "" };
                        format!(
                            "{mark}{} {:.2}{}",
                            p.species.split_once(' ').map_or(p.species.as_str(), |(_, s)| s),
                            p.likelihood,
                            p.variant.as_deref().map(|v| format!(" [{v}]")).unwrap_or_default()
                        )
                    })
                    .collect();
                println!("   post-DSS {g:<11}: {}", line.join(", "));
            }
        }
        if verbose {
            for p in post_preds.iter().chain(if post_preds.is_empty() { pre_preds.iter() } else { [].iter() }) {
                println!("      {} — {}", p.species, p.reasons.join(" | "));
            }
        }

        // Species-level recall.
        for (name, o) in &actual {
            let r_pre = rank_in_genus(&pre_preds, &o.genus, name);
            pre.add(r_pre);
            let entry = per_genus.entry(o.genus.clone()).or_default();
            entry.0.add(r_pre);
            if !known.is_empty() {
                let r_post = rank_in_genus(&post_preds, &o.genus, name);
                post.add(r_post);
                entry.1.add(r_post);
                post_noregion.add(rank_in_genus(&post_noregion_preds, &o.genus, name));
                let predicted_colour = post_preds.iter().find(|p| &p.species == *name).and_then(|p| p.variant.clone());
                match (&o.variant_colour, predicted_colour) {
                    (Some(actual), Some(pred)) if *actual == pred => colour_ok += 1,
                    (Some(_), Some(_)) => colour_wrong += 1,
                    (Some(_), None) => colour_none += 1,
                    (None, _) => {}
                }
            }
        }

        // Species calibration after DSS: genera with a found species.
        for (name, o) in &actual {
            for p in post_preds.iter().filter(|p| p.genus == o.genus) {
                calib_species.add(f64::from(p.likelihood), &p.species == *name);
            }
        }
        // Expected vs realised value for fully identified bodies (every DSS genus has a found species).
        if !known_names.is_empty() && known_names.iter().all(|g| actual.iter().any(|(_, o)| &o.genus == g)) {
            let footfalled = body.first_scan.as_ref().and_then(|sc| sc.get("WasFootfalled")).and_then(Value::as_bool);
            let populated = j.systems.get(&key.0).and_then(|s| s.population).is_some_and(|p| p > 0);
            let bonus = bio_predict::first_logged_bonus_likely(footfalled, populated, None);
            let expected: i64 = post_preds.iter().map(|p| bio_predict::expected_value(p, bonus)).sum();
            let realised: i64 = actual
                .iter()
                .filter_map(|(n, _)| exobio::species(n))
                .map(|s| bio_predict::payout(s.value, bonus))
                .sum();
            value_check.push((body.name.clone(), expected, realised));
        }

        // Genus-level (pre-DSS) against DSS genera.
        if body.saa_genera.is_some() && body.saa_bio.unwrap_or(0) > 0 {
            for (g, p) in &pre_genera {
                let present = known_names.contains(g);
                calib_genus.add(*p, present);
                brier.0 += (p - if present { 1.0 } else { 0.0 }).powi(2);
                brier.1 += 1;
                if *p >= 0.5 {
                    if present { tp += 1 } else { fp += 1 }
                }
            }
            for g in &known_names {
                if pre_genera.get(g).is_none_or(|p| *p < 0.5) {
                    fneg += 1;
                }
                if !pre_genera.contains_key(g) {
                    brier.0 += 1.0;
                    brier.1 += 1;
                    calib_genus.add(0.0, true);
                }
            }
            let n = known_names.len();
            let mut ranked: Vec<(&String, &f64)> = pre_genera.iter().collect();
            ranked.sort_by(|a, b| b.1.total_cmp(a.1));
            topn_total += n;
            topn_hit += ranked.iter().take(n).filter(|(g, _)| known_names.contains(*g)).count();
        }
    }

    println!("\n================ Results on the commander's journals ================");
    println!("Species rank within its genus (actual species from ScanOrganic / CodexEntry):");
    println!("  after DSS (known genera)          : {}", post.show());
    println!("  after DSS, region withheld        : {}", post_noregion.show());
    println!("  before DSS (signal count only)    : {}", pre.show());
    println!("Per genus (before DSS | after DSS):");
    for (g, (a, b)) in &per_genus {
        println!("  {g:<12} {} | {}", a.show(), b.show());
    }
    println!(
        "Colour (variant) prediction for found species: {colour_ok} correct, {colour_wrong} wrong, {colour_none} undetermined"
    );
    println!("Genus level before DSS (vs SAASignalsFound genera, threshold p >= 0.5):");
    println!(
        "  precision {:.1}%  recall {:.1}%  false-positive rate {:.1}% ({fp} of {} predicted genera absent)",
        pct(tp, tp + fp),
        pct(tp, tp + fneg),
        pct(fp, tp + fp),
        tp + fp
    );
    println!("  top-N genera (N = signal count) recall {:.1}% ({topn_hit}/{topn_total})", pct(topn_hit, topn_total));
    if brier.1 > 0 {
        println!("  Brier score of genus odds {:.3} over {} (body, genus) pairs", brier.0 / brier.1 as f64, brier.1);
    }
    println!(
        "Candidates: {:.1} species / {:.1} genera per body before DSS, {:.1} species per body after DSS",
        cand_pre.0 as f64 / cand_pre.1.max(1) as f64,
        cand_genera_pre as f64 / cand_pre.1.max(1) as f64,
        cand_post.0 as f64 / cand_post.1.max(1) as f64
    );
    // Cost of one prediction (pre-DSS, the most expensive case: every genus is scored).
    let all_facts: Vec<BodyFacts> = bio_bodies.iter().filter_map(|k| facts_for(j, *k, true)).collect();
    if !all_facts.is_empty() {
        let rounds = 200;
        let start = std::time::Instant::now();
        let mut n = 0usize;
        for _ in 0..rounds {
            for f in &all_facts {
                n += bio_predict::predict(f, &[]).len();
            }
        }
        let per_call = start.elapsed().as_secs_f64() * 1e6 / (rounds * all_facts.len()) as f64;
        println!("Speed: {per_call:.0} µs per predict() call ({n} predictions over {} calls)", rounds * all_facts.len());
    }
    println!("Calibration of genus odds before DSS:
    {}", calib_genus.show());
    println!("Calibration of species likelihoods after DSS (genera with a found species):
    {}", calib_species.show());
    for (name, expected, realised) in &value_check {
        println!(
            "Value of fully identified body {name}: expected {:.1} M Cr (after DSS), realised {:.1} M Cr",
            *expected as f64 / 1e6,
            *realised as f64 / 1e6
        );
    }

    // First-logged bonus and values against SellOrganicData.
    let (mut bonus_ok, mut bonus_total, mut value_ok) = (0, 0, 0);
    println!("Sales (SellOrganicData) vs predictions:");
    for (species, variant, value, bonus, body) in &j.sales {
        let sp = exobio::species(species);
        if sp.is_some_and(|s| s.value == *value) {
            value_ok += 1;
        }
        let Some(key) = body else {
            println!("  {species}: no matching analysed sample in these journals");
            continue;
        };
        let b = &j.bodies[key];
        let footfalled = b.first_scan.as_ref().and_then(|sc| sc.get("WasFootfalled")).and_then(Value::as_bool);
        let populated = j.systems.get(&key.0).and_then(|s| s.population).is_some_and(|p| p > 0);
        let name = sp.map_or(species.as_str(), |s| s.name);
        let was_logged = b.organics.get(name).and_then(|o| o.was_logged);
        let predicted = bio_predict::first_logged_bonus_likely(footfalled, populated, was_logged);
        let actual = *bonus > 0;
        bonus_total += 1;
        bonus_ok += usize::from(predicted == actual);
        let colour = exobio::variant_by_codex(variant).map(|(_, v)| v.colour).unwrap_or("?");
        println!(
            "  {name} ({colour}) on {}: value {value} ({}), bonus {bonus} — predicted bonus {predicted} [WasFootfalled(first scan) {footfalled:?}, populated {populated}, WasLogged {was_logged:?}]{}",
            b.name,
            if sp.is_some_and(|s| s.value == *value) { "matches dataset" } else { "DIFFERS from dataset" },
            if predicted == actual { "" } else { "  <-- mismatch" }
        );
    }
    println!(
        "  values match dataset: {value_ok}/{}; first-logged bonus rule correct: {bonus_ok}/{bonus_total}",
        j.sales.len()
    );
}

// ------------------------------------------------------------------------------------------------
// Synthetic coverage: one body at the centre of every rule set
// ------------------------------------------------------------------------------------------------

fn synthetic(verbose: bool) {
    let mut all = Recall::default();
    let mut per_genus: BTreeMap<&str, Recall> = BTreeMap::new();
    let mut failures = Vec::new();
    for sp in exobio::all_species().iter().filter(|s| !s.thargoid) {
        for (i, rs) in sp.rulesets.iter().enumerate() {
            let f = synth_body(sp, rs);
            let preds = bio_predict::predict(&f, &[sp.genus.to_string()]);
            let rank = rank_in_genus(&preds, sp.genus, sp.name);
            all.add(rank);
            per_genus.entry(sp.genus).or_default().add(rank);
            if rank != Some(1) {
                let top: Vec<String> = preds.iter().take(3).map(|p| format!("{} {:.2}", p.species, p.likelihood)).collect();
                failures.push(format!("{} rule {}: rank {:?}; top {}", sp.name, i + 1, rank, top.join(", ")));
            }
        }
    }
    println!("\n================ Synthetic check (body at the centre of each rule set, genus known) ================");
    println!("  all rule sets: {}", all.show());
    for (g, r) in &per_genus {
        println!("  {g:<18} {}", r.show());
    }
    let shown = if verbose { failures.len() } else { failures.len().min(25) };
    println!("  not ranked first ({} of {}; ties with equally valid species are expected):", failures.len(), all.n);
    for f in failures.iter().take(shown) {
        println!("    {f}");
    }
}

fn synth_body(sp: &Species, rs: &RuleSet) -> BodyFacts {
    let mid = |lo: Option<f64>, hi: Option<f64>, default: f64| match (lo, hi) {
        (Some(a), Some(b)) => (a + b) / 2.0,
        (Some(a), None) => a * 1.1 + 1.0,
        (None, Some(b)) => b * 0.9,
        (None, None) => default,
    };
    let atmosphere = rs.atmospheres.as_ref().and_then(|a| a.first().cloned()).unwrap_or_else(|| "None".to_string());
    let airless = atmosphere == "None";
    let pressure = match (rs.min_pressure_atm, rs.max_pressure_atm) {
        (_, Some(b)) if b <= 1e-6 => 0.0,
        (Some(a), Some(b)) => (a * b).sqrt(),
        (Some(a), None) => a * 1.2,
        (None, Some(b)) => b * 0.7,
        (None, None) => {
            if airless {
                0.0
            } else {
                0.01
            }
        }
    };
    let volcanism = match &rs.volcanism {
        VolcanismRule::Any | VolcanismRule::None => None,
        VolcanismRule::Required => Some("minor rocky magma volcanism".to_string()),
        VolcanismRule::Match { contains, equals, .. } => {
            equals.first().cloned().or_else(|| contains.first().map(|c| format!("{c} volcanism")))
        }
    };
    // The most specific allowed region: the core regions (1, 4) are shared by many arm-specific species.
    let region = rs.regions.as_ref().and_then(|r| match (&r.inside, &r.outside) {
        (Some(list), _) => list.last().copied(),
        (None, Some(out)) => (1..=42u8).rev().find(|id| !out.contains(id)),
        (None, None) => None,
    });
    let star_from = |req: &exobio::StarReq| StarFacts::new(req.class.clone(), req.luminosity.as_deref());
    let mut parent_stars: Vec<StarFacts> = Vec::new();
    if let Some(req) = rs.parent_star.as_ref().and_then(|r| r.first()) {
        parent_stars.push(star_from(req));
    }
    if sp.colour_basis == ColourBasis::Star
        && !parent_stars.iter().any(|s| s.colour_key().is_some_and(|k| sp.variant(k).is_some()))
        && let Some(v) = sp.variants.iter().find(|v| v.in_codex)
    {
        let class = if v.key == "Ae" { "AeBe".to_string() } else { v.key.to_string() };
        parent_stars.insert(0, StarFacts::new(class, Some("V")));
    }
    let system_stars: Vec<StarFacts> =
        rs.star_in_system.as_ref().and_then(|r| r.first()).map(star_from).into_iter().collect();
    let mut materials: Vec<(String, f64)> = vec![("iron".into(), 20.0), ("nickel".into(), 15.0), ("sulphur".into(), 15.0)];
    if sp.colour_basis == ColourBasis::Element
        && let Some(v) = sp.variants.first()
    {
        materials.push((v.key.to_ascii_lowercase(), 1.0));
    }
    let mut composition: Vec<(String, f64)> = Vec::new();
    if !airless {
        composition.push((atmosphere.trim_end_matches("Rich").to_string(), 98.0));
    }
    for (gas, min) in &rs.min_atmosphere_pct {
        composition.push((gas.clone(), min + 1.0));
    }
    let star_pos = if rs.guardian {
        Some([1099.21875, -146.6875, -133.59375]) // Gamma Velorum bubble
    } else if let Some(zone) = rs.tuber_zones.as_ref().and_then(|z| z.first()) {
        zone_point(zone)
    } else {
        Some([0.0, 0.0, 0.0])
    };
    BodyFacts {
        planet_class: rs.body_types.as_ref().and_then(|b| b.first().cloned()).unwrap_or_else(|| "Rocky body".to_string()),
        atmosphere_type: Some(atmosphere),
        atmosphere_composition: composition,
        gravity_g: Some(mid(rs.min_gravity_g, rs.max_gravity_g, 0.1)),
        temperature_k: Some(mid(rs.min_temp_k, rs.max_temp_k, 170.0)),
        pressure_atm: Some(pressure),
        volcanism,
        landable: true,
        materials,
        parent_stars,
        distance_from_arrival_ls: rs.min_distance_ls.map_or(500.0, |d| d * 1.1),
        system_name: rs.system.clone().unwrap_or_else(|| "Synthetic".to_string()),
        star_pos,
        region: region.map(|r| r.to_string()),
        bio_signal_count: Some(1),
        system_stars,
        system_body_classes: rs.system_body_types.clone().unwrap_or_else(|| vec!["Rocky body".to_string()]),
        orbital_period_s: Some(rs.max_orbital_period_s.map_or(1.0e6, |p| p * 0.5)),
        near_nebula: rs.nebula.as_ref().map(|_| true),
        ..BodyFacts::default()
    }
}

/// A point in the middle of a Sinuous-tuber zone's distance band (offset along +x from the zone's centre).
fn zone_point(zone: &str) -> Option<[f64; 3]> {
    bio_predict::tuber_zone(zone).map(|(c, lo, hi)| [c[0] + (lo + hi) / 2.0, c[1], c[2]])
}
