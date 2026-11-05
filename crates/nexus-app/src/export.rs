//! Ship build export links (EDSY, Coriolis): the build as a journal `Loadout` event, compact JSON,
//! gzipped, URL-safe base64 with `=` escaped as `%3D` — the format both sites (and EDMC) use.

use base64::Engine;
use serde_json::Value;
use std::io::{Read, Write};

/// The build as a journal `Loadout` event, the one form both sites import from a link. EDSY tells a
/// build by its `"event":"Loadout"` (or by a whole SLEF export's header), and Coriolis's SLEF
/// export has no event: its build alone made EDSY say "Unrecognized JSON format". A SLEF export,
/// whole or one item, gives its build. `None` for text that isn't a JSON object.
fn loadout_event(json: &str) -> Option<Value> {
    let mut v: Value = serde_json::from_str(json).ok()?;
    if let Value::Array(items) = v {
        v = items.into_iter().find(Value::is_object)?;
    }
    if let Some(data) = v.get("data").filter(|d| d.is_object()) {
        v = data.clone();
    }
    v.as_object_mut()?.insert("event".into(), Value::from("Loadout"));
    Some(v)
}

fn encode_loadout(loadout: &str) -> Option<String> {
    let compact = serde_json::to_string(&loadout_event(loadout)?).ok()?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(compact.as_bytes()).ok()?;
    let bytes = gz.finish().ok()?;
    Some(base64::engine::general_purpose::URL_SAFE.encode(bytes).replace('=', "%3D"))
}

/// An edsy.org link that imports the build: a journal `Loadout` line, a SLEF export or its build.
pub fn edsy_url(loadout: &str) -> Option<String> {
    encode_loadout(loadout).map(|d| format!("https://edsy.org/#/I={d}"))
}

/// A coriolis.io link that imports the build, from the same kinds of text as [`edsy_url`].
pub fn coriolis_url(loadout: &str) -> Option<String> {
    encode_loadout(loadout).map(|d| format!("https://coriolis.io/import?data={d}"))
}

/// The build as a SLEF export (EDSY and Coriolis both import it): one item with Elite Nexus's header and the build's
/// `Loadout` as its data.
pub fn slef(loadout: &str) -> Option<String> {
    let data = loadout_event(loadout)?;
    let header = serde_json::json!({ "appName": "Elite Nexus", "appVersion": env!("CARGO_PKG_VERSION"), "appURL": "https://github.com/uruskan/EliteNexus" });
    serde_json::to_string(&serde_json::json!([{ "header": header, "data": data }])).ok()
}

/// The build JSON inside an EDSY or Coriolis *import* link (`edsy.org/#/I=…`,
/// `coriolis.io/import?data=…`, beta sites too), as EDMC, EDDiscovery and Elite Nexus write them.
/// The sites' own share links (`edsy.org/#/L=…`, `coriolis.io/outfit/…?code=…`) are coded with
/// their module tables and can't be read here.
pub fn decode_link(url: &str) -> Option<String> {
    let url = url.trim();
    if !(url.contains("edsy.org") || url.contains("coriolis.io")) {
        return None;
    }
    let data = ["#/I=", "import?data=", "/import/"].iter().find_map(|m| url.split_once(m).map(|(_, d)| d))?;
    let data: String = data.split(['&', '#', ' ']).next()?.replace("%3D", "=").replace("%3d", "=");
    let engines = [&base64::engine::general_purpose::URL_SAFE, &base64::engine::general_purpose::URL_SAFE_NO_PAD, &base64::engine::general_purpose::STANDARD];
    let bytes = engines.iter().find_map(|e| e.decode(&data).or_else(|_| e.decode(data.trim_end_matches('='))).ok())?;
    let mut text = String::new();
    // EDMC gzips; Coriolis inflates zlib or gzip alike.
    if flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut text).is_ok() && !text.is_empty() {
        return Some(text);
    }
    text.clear();
    (flate2::read::ZlibDecoder::new(&bytes[..]).read_to_string(&mut text).is_ok() && !text.is_empty()).then_some(text)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// EDSY's test for a build it can import (`importData` in edsy.js, read 2026-11-05): a SLEF
    /// export whose first item has `header.appName` and `data.Ship`, or journal events, of which it
    /// takes the `Loadout`s. Anything else is "Unrecognized JSON format".
    fn edsy_imports(v: &Value) -> bool {
        let first = v.get(0).unwrap_or(v);
        let slef = v.is_array() && first.pointer("/header/appName").is_some_and(|a| a.as_str().is_some_and(|s| !s.is_empty())) && first.pointer("/data/Ship").is_some();
        slef || first.get("event").and_then(Value::as_str) == Some("Loadout") && first.get("Ship").is_some_and(Value::is_string) && first.get("Modules").is_some_and(Value::is_array)
    }

    /// Coriolis's (`_importBuild` in Coriolis.jsx, read 2026-11-05): one SLEF item's `data`, a
    /// Frontier API ship (`modules`) or a journal `Loadout` (`Modules`).
    fn coriolis_imports(v: &Value) -> bool {
        let slef = v.get(0).and_then(|i| i.get("data")).is_some();
        slef || v.get("modules").is_some() || v.get("Modules").is_some_and(Value::is_array)
    }

    /// The JSON an import link carries.
    pub(crate) fn link_json(url: &str) -> Value {
        serde_json::from_str(&decode_link(url).expect("decodes")).expect("JSON")
    }

    /// Both sites import the build behind both links.
    pub(crate) fn both_sites_import(build: &str) -> Value {
        let (edsy, coriolis) = (edsy_url(build).expect("EDSY link"), coriolis_url(build).expect("Coriolis link"));
        assert!(edsy.starts_with("https://edsy.org/#/I=H4sI") && coriolis.starts_with("https://coriolis.io/import?data=H4sI"));
        let json = link_json(&edsy);
        assert!(edsy_imports(&json), "EDSY can't import {json}");
        assert!(coriolis_imports(&link_json(&coriolis)), "Coriolis can't import {json}");
        json
    }

    #[test]
    fn roundtrip() {
        let line = r#"{ "timestamp":"2026-09-05T00:13:39Z", "event":"Loadout", "Ship":"empire_courier", "ShipID":17, "Modules":[] }"#;
        let url = edsy_url(line).unwrap();
        let data = url.strip_prefix("https://edsy.org/#/I=").unwrap().replace("%3D", "=");
        let gz = base64::engine::general_purpose::URL_SAFE.decode(data).unwrap();
        let mut s = String::new();
        flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut s).unwrap();
        assert!(s.contains("\"Ship\":\"empire_courier\""));
        assert!(coriolis_url(line).unwrap().starts_with("https://coriolis.io/import?data="));
        // A journal line goes as it is, compact.
        assert_eq!(both_sites_import(line), serde_json::from_str::<Value>(line).unwrap());
    }

    /// Elite Nexus's own SLEF export: EDSY takes it as SLEF, Coriolis by its first item's data, and Where to buy and
    /// the planner read it back.
    #[test]
    fn its_own_slef_export_goes_everywhere() {
        let journal = r#"{"event":"Loadout","Ship":"python","Modules":[{"Slot":"FrameShiftDrive","Item":"int_hyperdrive_size5_class5","On":true,"Priority":0}]}"#;
        let text = slef(journal).expect("slef");
        let v: Value = serde_json::from_str(&text).expect("json");
        assert!(edsy_imports(&v) && coriolis_imports(&v), "{text}");
        assert_eq!(v[0]["header"]["appName"], "Elite Nexus");
        let back = crate::shopping::parse_build(&text).expect("read back");
        assert_eq!((back.ship.as_str(), back.modules.len()), ("python", 1));
        both_sites_import(&text);
    }

    /// Finding 15 of the 2026-11-04 play-test: a build pasted from Coriolis's SLEF export opened
    /// in EDSY as "Import failed: Unrecognized JSON format / No builds found".
    #[test]
    fn a_coriolis_slef_build_opens_in_edsy() {
        let slef = r#"[{"header":{"appName":"Coriolis","appVersion":"4.0","appURL":"https://coriolis.io/outfit/anaconda"},"data":{"Ship":"Anaconda","Modules":[
            {"Slot":"Armour","Item":"anaconda_armour_grade2","On":true,"Priority":1},
            {"Slot":"FrameShiftDrive","Item":"int_hyperdrive_size6_class5","On":true,"Priority":1,"Engineering":{"BlueprintName":"FSD_LongRange","Level":4,"Quality":0,"ExperimentalEffect":"special_fsd_heavy"}}]}}]"#;
        let data = serde_json::from_str::<Value>(slef).unwrap()[0]["data"].to_string();
        // What Where to buy keeps of a pasted SLEF is its build, which has no event.
        assert!(!edsy_imports(&serde_json::from_str(&data).unwrap()), "the failing case");
        for build in [data.as_str(), slef] {
            let json = both_sites_import(build);
            assert_eq!((json["event"].as_str(), json["Ship"].as_str()), (Some("Loadout"), Some("Anaconda")));
            assert_eq!(json["Modules"][1]["Engineering"]["BlueprintName"], "FSD_LongRange", "engineering goes along");
        }
        assert_eq!(edsy_url("not JSON"), None);
        assert_eq!(coriolis_url("[1, 2]"), None);
    }

    #[test]
    fn import_links_decode() {
        let line = r#"{"event":"Loadout","Ship":"anaconda","Modules":[{"Slot":"FrameShiftDrive","Item":"int_hyperdrive_size6_class5"}]}"#;
        for url in [edsy_url(line).unwrap(), coriolis_url(line).unwrap(), coriolis_url(line).unwrap().replace("coriolis.io", "beta.coriolis.io")] {
            let json = decode_link(&url).expect("decodes");
            assert!(json.contains("int_hyperdrive_size6_class5"), "{url}");
        }
        // The example link in docs/research/websites.md §2.1 (EDMC's encoding).
        let edmc = "https://edsy.org/#/I=H4sIABarzGoC_yXLwQqCQBSF4Xc56wnGWbSYtQRCUWCrIkSaG15QR2auhonv3lhwVufnW3Dybmwpwt4XFEIdLLiXqpkHCi7wRFXkD5nq2dYxGiice1gJIylcAvvAMsNqhbL1kuwh1B2VDb8k3zDWR0oNDylFdvTm3lHA_yty2EyBJuo3evS186OkKNxRlLrblNFmv8v0LjNXre1vN6xfxamCobgAAAA%3D";
        assert!(decode_link(edmc).is_some_and(|j| j.contains("Loadout")));
        // Share links need the sites' own tables; other text isn't a link.
        assert_eq!(decode_link("https://coriolis.io/outfit/anaconda?code=A0p0tdFfldddsdf5"), None);
        assert_eq!(decode_link("https://edsy.org/#/L=Aw18ZlAB"), None);
        assert_eq!(decode_link("{\"event\":\"Loadout\"}"), None);
    }
}
