//! Gathers EDCD coriolis-data (ship hulls and their slots, module figures, engineering) into the files Elite Nexus
//! embeds for Fleet › Plan a loadout: `data/coriolis/ships.json`, `ship-slots.json`, `modules.json`,
//! `blueprints.json`, `modifications.json` and `modifier-actions.json`. The figures are coriolis-data's own,
//! unchanged: this only puts its files into one file each, one entry per line, so an update shows as a readable diff.
//!
//! ```text
//! git clone --depth 1 --branch develop https://github.com/EDCD/coriolis-data <dir>
//! cargo run -p nexus-data --example convert_coriolis -- <dir>
//! ```
//!
//! Then put the commit in `data/coriolis/SOURCE.txt`.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

fn main() {
    let Some(src) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: convert_coriolis <coriolis-data checkout>");
        std::process::exit(2);
    };
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/coriolis");
    std::fs::create_dir_all(&out).expect("create data/coriolis");

    // ships/<id>.json each hold { "<id>": { … } }.
    let mut ships = Map::new();
    for f in files(&src.join("ships")) {
        if f.file_name().is_some_and(|n| n == "ship-slots.json") {
            continue;
        }
        for (id, ship) in read(&f).as_object().cloned().unwrap_or_default() {
            ships.insert(id, ship);
        }
    }
    // modules/<kind>/<name>.json each hold { "<grp>": [ { … }, … ] }; every module keeps its group.
    let mut modules = Vec::new();
    for f in files(&src.join("modules")) {
        for (grp, list) in read(&f).as_object().cloned().unwrap_or_default() {
            for mut m in list.as_array().cloned().unwrap_or_default() {
                if let Some(o) = m.as_object_mut() {
                    o.entry("grp").or_insert_with(|| Value::from(grp.clone()));
                }
                modules.push(m);
            }
        }
    }
    let slots = read(&src.join("ships/ship-slots.json"));

    write_lines(&out.join("ships.json"), ships.into_iter().map(|(k, v)| format!("{}: {}", Value::from(k), v)), '{', '}');
    write_lines(&out.join("ship-slots.json"), slots.as_object().cloned().unwrap_or_default().into_iter().map(|(k, v)| format!("{}: {}", Value::from(k), v)), '{', '}');
    let count = modules.len();
    write_lines(&out.join("modules.json"), modules.into_iter().map(|m| m.to_string()), '[', ']');

    // modifications/: each blueprint's feature ranges per grade (the name and features only: the materials come from
    // the Engineering page's data), how each figure changes, and what each experimental effect does.
    let mods = src.join("modifications");
    let blueprints = read(&mods.join("blueprints.json")).as_object().cloned().unwrap_or_default();
    write_lines(
        &out.join("blueprints.json"),
        blueprints.into_iter().map(|(k, v)| {
            let grades: Map<String, Value> = v
                .get("grades")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(g, x)| (g.clone(), serde_json::json!({ "features": x.get("features").cloned().unwrap_or(Value::Null) })))
                .collect();
            format!("{}: {}", Value::from(k), serde_json::json!({ "fdname": v.get("fdname"), "name": v.get("name"), "grades": grades }))
        }),
        '{',
        '}',
    );
    for (from, to) in [("modifications.json", "modifications.json"), ("modifierActions.json", "modifier-actions.json")] {
        let v = read(&mods.join(from));
        write_lines(&out.join(to), v.as_object().cloned().unwrap_or_default().into_iter().map(|(k, v)| format!("{}: {}", Value::from(k), v)), '{', '}');
    }
    if let Err(e) = std::fs::copy(src.join("LICENSE.md"), out.join("LICENSE.md")) {
        eprintln!("LICENSE.md not copied: {e}");
    }
    println!("{} hulls, {} slot tables, {count} modules → {}", read(&out.join("ships.json")).as_object().map_or(0, Map::len), read(&out.join("ship-slots.json")).as_object().map_or(0, Map::len), out.display());
}

/// Every .json file under `dir`, in a stable order.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display())).flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "json") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn read(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A JSON object or array with one entry per line.
fn write_lines(path: &Path, entries: impl Iterator<Item = String>, open: char, close: char) {
    let body: Vec<String> = entries.collect();
    let text = format!("{open}\n{}\n{close}\n", body.join(",\n"));
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
