//! Cartographic value estimates: what Universal Cartographics pays for a body's data, from its class, mass and
//! terraformability, with what a mapping, first discovery and first mapping add (the formula and its order of
//! operations: `docs/research/exploration.md` §1).

/// Mass-term constant.
const Q: f64 = 0.565_918_28;

/// Inputs that change what a body is worth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueOptions {
    /// Nobody has sold discovery data for this body yet (`WasDiscovered == false`).
    pub first_discoverer: bool,
    /// We mapped it with the DSS.
    pub mapped: bool,
    /// Nobody had mapped it before us (`WasMapped == false`).
    pub first_mapped: bool,
    /// Mapped using no more probes than the efficiency target.
    pub efficient: bool,
    /// Odyssey client (adds the 30% mapping bonus).
    pub odyssey: bool,
    /// Data sold at a fleet carrier's Universal Cartographics (25% cut).
    pub fleet_carrier_sale: bool,
}

impl Default for ValueOptions {
    fn default() -> Self {
        ValueOptions {
            first_discoverer: false,
            mapped: false,
            first_mapped: false,
            efficient: true,
            odyssey: true,
            fleet_carrier_sale: false,
        }
    }
}

/// k-value for a planet class (journal `PlanetClass`) and terraformability.
pub fn planet_k(planet_class: &str, terraformable: bool) -> f64 {
    let c = planet_class.to_ascii_lowercase();
    if c.contains("metal rich") {
        21_790.0
    } else if c.contains("ammonia world") {
        96_932.0
    } else if c.contains("sudarsky class i gas giant") {
        1_656.0
    } else if c.contains("high metal content") || c.contains("sudarsky class ii gas giant") {
        9_654.0 + if terraformable { 100_677.0 } else { 0.0 }
    } else if c.contains("earthlike") || c.contains("earth-like") {
        // Earth-likes always carry the terraformable bonus.
        64_831.0 + 116_295.0
    } else if c == "water world" {
        64_831.0 + if terraformable { 116_295.0 } else { 0.0 }
    } else {
        300.0 + if terraformable { 93_328.0 } else { 0.0 }
    }
}

/// k-value for a star type (journal `StarType`).
pub fn star_k(star_type: &str) -> f64 {
    match star_type {
        "N" | "H" => 22_628.0,
        "SupermassiveBlackHole" => 33.567_8,
        t if t.starts_with('D') => 14_057.0,
        _ => 1_200.0,
    }
}

/// Value of a planet in credits.
pub fn planet_value(k: f64, mass_em: f64, o: ValueOptions) -> i64 {
    let mapping_multiplier = if o.mapped {
        if o.first_discoverer && o.first_mapped {
            3.699_622_554
        } else if o.first_mapped {
            8.0956
        } else {
            10.0 / 3.0
        }
    } else {
        1.0
    };
    let mut value = (k + k * Q * mass_em.max(0.0).powf(0.2)) * mapping_multiplier;
    if o.mapped {
        if o.odyssey {
            value += (value * 0.3).max(555.0);
        }
        if o.efficient {
            value *= 1.25;
        }
    }
    value = value.max(500.0);
    if o.first_discoverer {
        value *= 2.6;
    }
    if o.fleet_carrier_sale {
        value *= 0.75;
    }
    value.round() as i64
}

/// Value of a star in credits.
pub fn star_value(star_type: &str, stellar_mass: f64, first_discoverer: bool) -> i64 {
    let k = star_k(star_type);
    let mut value = k + stellar_mass.max(0.0) * k / 66.25;
    if first_discoverer {
        value *= 2.6;
    }
    value.round() as i64
}

/// Bonus for FSS-scanning every body in a system (`FSSAllBodiesFound`), per body.
pub const FULL_SCAN_BONUS_PER_BODY: i64 = 1_000;
/// Bonus for mapping every mappable body in a system, per mappable body.
pub const FULL_MAP_BONUS_PER_BODY: i64 = 10_000;

/// True for planet classes worth mapping on value alone (the classic explorer heuristic).
pub fn is_high_value_class(planet_class: &str) -> bool {
    let c = planet_class.to_ascii_lowercase();
    c.contains("earthlike") || c == "water world" || c.contains("ammonia world")
}

/// Short, readable class names for dense tables.
pub fn short_planet_class(planet_class: &str) -> &str {
    match planet_class {
        "Metal rich body" => "Metal-rich",
        "High metal content body" => "HMC",
        "Rocky body" => "Rocky",
        "Icy body" => "Icy",
        "Rocky ice body" => "Rocky ice",
        "Earthlike body" => "Earth-like",
        "Water world" => "Water world",
        "Ammonia world" => "Ammonia world",
        "Water giant" => "Water giant",
        "Water giant with life" => "Water giant (life)",
        "Gas giant with water based life" => "GG water life",
        "Gas giant with ammonia based life" => "GG ammonia life",
        "Sudarsky class I gas giant" => "Class I GG",
        "Sudarsky class II gas giant" => "Class II GG",
        "Sudarsky class III gas giant" => "Class III GG",
        "Sudarsky class IV gas giant" => "Class IV GG",
        "Sudarsky class V gas giant" => "Class V GG",
        "Helium rich gas giant" => "Helium-rich GG",
        "Helium gas giant" => "Helium GG",
        other => other,
    }
}

/// Is this star type scoopable (KGBFOAM)?
pub fn is_scoopable(star_type: &str) -> bool {
    const SCOOPABLE: [&str; 7] = ["O", "B", "A", "F", "G", "K", "M"];
    // Main sequence ("K") or a giant/supergiant of a scoopable class ("K_OrangeGiant").
    let class = star_type.split('_').next().unwrap_or("");
    SCOOPABLE.contains(&class) && (star_type.len() == 1 || star_type.contains('_'))
}

/// Human-readable star class (`K` -> "K (orange)", `DA` -> "White dwarf (DA)", `N` -> "Neutron star").
pub fn star_description(star_type: &str) -> String {
    match star_type {
        "O" => "O (blue-white)".into(),
        "B" => "B (blue-white)".into(),
        "A" => "A (white)".into(),
        "F" => "F (yellow-white)".into(),
        "G" => "G (yellow)".into(),
        "K" => "K (orange)".into(),
        "M" => "M (red dwarf)".into(),
        "L" => "L (brown dwarf)".into(),
        "T" => "T (brown dwarf)".into(),
        "Y" => "Y (brown dwarf)".into(),
        "TTS" => "T Tauri".into(),
        "AeBe" => "Herbig Ae/Be".into(),
        "N" => "Neutron star".into(),
        "H" => "Black hole".into(),
        "SupermassiveBlackHole" => "Supermassive black hole".into(),
        "X" => "Exotic".into(),
        "A_BlueWhiteSuperGiant" => "A blue-white supergiant".into(),
        "F_WhiteSuperGiant" => "F white supergiant".into(),
        "M_RedSuperGiant" => "M red supergiant".into(),
        "M_RedGiant" => "M red giant".into(),
        "K_OrangeGiant" => "K orange giant".into(),
        t if t.starts_with('W') => format!("Wolf-Rayet ({t})"),
        t if t.starts_with('D') => format!("White dwarf ({t})"),
        t if t.starts_with('C') || t == "S" || t == "MS" => format!("Carbon star ({t})"),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earthlike_values_are_in_known_ranges() {
        let k = planet_k("Earthlike body", false);
        // ~1 Earth mass ELW, already discovered, just FSS-scanned: ~283k
        let fss = planet_value(k, 1.0, ValueOptions::default());
        assert!((270_000..300_000).contains(&fss), "{fss}");
        // first discovered + first mapped + efficient in Odyssey: a few million
        let best = planet_value(
            k,
            1.0,
            ValueOptions { first_discoverer: true, mapped: true, first_mapped: true, ..Default::default() },
        );
        assert!(best > 3_000_000 && best < 5_000_000, "{best}");
    }

    #[test]
    fn minimum_value_and_stars() {
        assert_eq!(planet_value(planet_k("Icy body", false), 0.0001, ValueOptions::default()), 500);
        assert!(star_value("N", 1.4, false) > 22_000);
        assert_eq!(star_k("DA"), 14_057.0);
    }

    #[test]
    fn scoopable() {
        assert!(is_scoopable("K"));
        assert!(is_scoopable("M"));
        assert!(is_scoopable("A_BlueWhiteSuperGiant"));
        assert!(!is_scoopable("N"));
        assert!(!is_scoopable("DA"));
        assert!(!is_scoopable("L"));
        assert!(!is_scoopable("TTS"));
    }
}
