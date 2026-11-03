//! Ship build export links (EDSY, Coriolis): the journal `Loadout` event, compact JSON, gzipped,
//! URL-safe base64 with `=` escaped as `%3D` — the format both sites (and EDMC) use.

use base64::Engine;
use std::io::{Read, Write};

fn encode_loadout(loadout_line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(loadout_line).ok()?;
    let compact = serde_json::to_string(&value).ok()?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(compact.as_bytes()).ok()?;
    let bytes = gz.finish().ok()?;
    Some(base64::engine::general_purpose::URL_SAFE.encode(bytes).replace('=', "%3D"))
}

pub fn edsy_url(loadout_line: &str) -> Option<String> {
    encode_loadout(loadout_line).map(|d| format!("https://edsy.org/#/I={d}"))
}

pub fn coriolis_url(loadout_line: &str) -> Option<String> {
    encode_loadout(loadout_line).map(|d| format!("https://coriolis.io/import?data={d}"))
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
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let line = r#"{ "timestamp":"2026-09-05T00:13:39Z", "event":"Loadout", "Ship":"empire_courier", "ShipID":17 }"#;
        let url = edsy_url(line).unwrap();
        let data = url.strip_prefix("https://edsy.org/#/I=").unwrap().replace("%3D", "=");
        let gz = base64::engine::general_purpose::URL_SAFE.decode(data).unwrap();
        let mut s = String::new();
        flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut s).unwrap();
        assert!(s.contains("\"Ship\":\"empire_courier\""));
        assert!(coriolis_url(line).unwrap().starts_with("https://coriolis.io/import?data="));
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
