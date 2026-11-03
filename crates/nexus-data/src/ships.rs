//! Ship display names keyed by the lower-case journal symbol (`empire_courier`, `explorer_nx`, …).
//!
//! Names come from EDCD/FDevIDs `shipyard.csv` (compiled in) with Frontier's "MkIII" spelling
//! normalised to "Mk III"; the hand-written table below covers SRVs, fighters, the Nomad lander,
//! taxis and acts as a fallback for hulls FDevIDs does not list yet.

use std::sync::LazyLock;

use rustc_hash::FxHashMap;

const SHIPYARD_CSV: &str = include_str!("../../../data/fdevids/shipyard.csv");

/// Built-in names (fallback; FDevIDs `shipyard.csv` takes precedence for buyable hulls).
const SHIPS: &[(&str, &str)] = &[
    ("adder", "Adder"),
    ("anaconda", "Anaconda"),
    ("anacondamkii", "Anaconda Mk II"),
    ("asp", "Asp Explorer"),
    ("asp_scout", "Asp Scout"),
    ("belugaliner", "Beluga Liner"),
    ("cobramkiii", "Cobra Mk III"),
    ("cobramkiv", "Cobra Mk IV"),
    ("cobramkv", "Cobra Mk V"),
    ("corsair", "Corsair"),
    ("cutter", "Imperial Cutter"),
    ("diamondback", "Diamondback Scout"),
    ("diamondbackxl", "Diamondback Explorer"),
    ("dolphin", "Dolphin"),
    ("eagle", "Eagle"),
    ("empire_courier", "Imperial Courier"),
    ("empire_eagle", "Imperial Eagle"),
    ("empire_fighter", "Imperial Fighter"),
    ("empire_trader", "Imperial Clipper"),
    ("explorer_nx", "Caspian Explorer"),
    ("federation_corvette", "Federal Corvette"),
    ("federation_dropship", "Federal Dropship"),
    ("federation_dropship_mkii", "Federal Assault Ship"),
    ("federation_fighter", "F63 Condor"),
    ("federation_gunship", "Federal Gunship"),
    ("ferdelance", "Fer-de-Lance"),
    ("hauler", "Hauler"),
    ("independant_trader", "Keelback"),
    ("independent_fighter", "Taipan Fighter"),
    ("krait_light", "Krait Phantom"),
    ("krait_mkii", "Krait Mk II"),
    ("lakonminer", "Type-11 Prospector"),
    ("mamba", "Mamba"),
    ("mandalay", "Mandalay"),
    ("mediumtransport01", "Lynx Highliner"),
    ("orca", "Orca"),
    ("panthermkii", "Panther Clipper Mk II"),
    ("python", "Python"),
    ("python_nx", "Python Mk II"),
    ("sidewinder", "Sidewinder"),
    ("smallcombat01_nx", "Kestrel Mk II"),
    ("testbuggy", "SRV Scarab"),
    ("combat_multicrew_srv_01", "SRV Scorpion"),
    ("mev_rhino", "Rhino MEV"),
    ("lander01", "Nomad"),
    ("gdn_hybrid_fighter_v1", "XG7 Trident"),
    ("gdn_hybrid_fighter_v2", "XG8 Javelin"),
    ("gdn_hybrid_fighter_v3", "XG9 Lance"),
    ("adder_taxi", "Adder (taxi)"),
    ("diamondback_taxi", "Diamondback Scout (taxi)"),
    ("viper_taxi", "Viper Mk III (taxi)"),
    ("vulture_taxi", "Vulture (taxi)"),
    ("type6", "Type-6 Transporter"),
    ("type7", "Type-7 Transporter"),
    ("type8", "Type-8 Transporter"),
    ("type9", "Type-9 Heavy"),
    ("type9_military", "Type-10 Defender"),
    ("typex", "Alliance Chieftain"),
    ("typex_2", "Alliance Crusader"),
    ("typex_3", "Alliance Challenger"),
    ("viper", "Viper Mk III"),
    ("viper_mkiv", "Viper Mk IV"),
    ("vulture", "Vulture"),
];

/// A buyable hull from FDevIDs `shipyard.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ship {
    /// FDevIDs / Shipyard.json id.
    pub id: u64,
    /// Lower-case journal symbol, e.g. `"krait_mkii"`.
    pub symbol: String,
    /// Display name, e.g. `"Krait Mk II"`.
    pub name: String,
    /// The name as FDevIDs spells it (`"Krait MkII"`), which is what Spansh's shipyard lists use.
    pub fdev_name: String,
    /// Store entitlement SKU needed to buy it (FDevIDs `sku.csv`), if any.
    pub entitlement: Option<String>,
}

struct Table {
    list: Vec<Ship>,
    by_symbol: FxHashMap<String, usize>,
    by_id: FxHashMap<u64, usize>,
}

static SHIPYARD: LazyLock<Table> = LazyLock::new(|| {
    let mut list = Vec::with_capacity(64);
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(SHIPYARD_CSV.as_bytes());
    for rec in rdr.records().flatten() {
        let (Some(id), Some(sym), Some(name)) = (rec.get(0), rec.get(1), rec.get(2)) else { continue };
        list.push(Ship {
            id: id.parse().unwrap_or(0),
            symbol: sym.to_ascii_lowercase(),
            name: normalise_mark(name),
            fdev_name: name.to_string(),
            entitlement: rec.get(3).filter(|s| !s.is_empty()).map(str::to_string),
        });
    }
    let by_symbol = list.iter().enumerate().map(|(i, s)| (s.symbol.clone(), i)).collect();
    let by_id = list.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    Table { list, by_symbol, by_id }
});

/// `"Krait MkII"` → `"Krait Mk II"`, `"Viper MkIII"` → `"Viper Mk III"`; other names unchanged.
fn normalise_mark(name: &str) -> String {
    name.split(' ')
        .map(|w| match w.strip_prefix("Mk") {
            Some(num) if !num.is_empty() && num.chars().all(|c| matches!(c, 'I' | 'V' | 'X')) => format!("Mk {num}"),
            _ => w.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// All buyable hulls from FDevIDs, in FDevIDs order.
pub fn all() -> &'static [Ship] {
    &SHIPYARD.list
}

/// Looks a buyable hull up by journal symbol (case-insensitive).
pub fn ship(symbol: &str) -> Option<&'static Ship> {
    let t = &*SHIPYARD;
    t.by_symbol.get(&symbol.trim().to_ascii_lowercase()).map(|&i| &t.list[i])
}

/// Looks a buyable hull up by its FDevIDs / Shipyard.json id.
pub fn ship_by_id(id: u64) -> Option<&'static Ship> {
    let t = &*SHIPYARD;
    t.by_id.get(&id).map(|&i| &t.list[i])
}

/// Display name for a ship (or SRV, fighter, lander, suit) symbol, case-insensitive; unknown
/// symbols are prettified.
pub fn ship_name(symbol: &str) -> String {
    let key = symbol.trim().to_ascii_lowercase();
    if let Some(s) = ship(&key) {
        return s.name.clone();
    }
    if let Some((_, v)) = SHIPS.iter().find(|(k, _)| *k == key) {
        return (*v).to_string();
    }
    if let Some(suit) = suit_name(&key) {
        return suit;
    }
    crate::names::split_camel(&symbol.replace('_', " "))
}

/// On-foot "ships": `Loadout`/`LoadGame` report the suit when the commander is on foot.
fn suit_name(key: &str) -> Option<String> {
    let (base, grade) = match key.rsplit_once("_class") {
        Some((b, g)) => (b, g.parse::<u8>().ok()),
        None => (key, None),
    };
    let name = match base {
        "flightsuit" => "Flight Suit",
        "utilitysuit" => "Maverick Suit",
        "explorationsuit" => "Artemis Suit",
        "tacticalsuit" => "Dominator Suit",
        _ => return None,
    };
    Some(match grade {
        Some(g) => format!("{name} (G{g})"),
        None => name.to_string(),
    })
}

/// Landing pad size a ship needs: 'S', 'M' or 'L' (unknown symbols → 'S').
pub fn pad_size(symbol: &str) -> char {
    known_pad_size(symbol).unwrap_or('S')
}

/// Landing pad size a ship needs, `None` for a hull this table doesn't know yet (a ship newer than this build).
pub fn known_pad_size(symbol: &str) -> Option<char> {
    Some(match symbol.trim().to_ascii_lowercase().as_str() {
        "anaconda"
        | "anacondamkii"
        | "belugaliner"
        | "cutter"
        | "empire_trader"
        | "explorer_nx"
        | "federation_corvette"
        | "orca"
        | "panthermkii"
        | "type7"
        | "type9"
        | "type9_military" => 'L',
        "asp"
        | "asp_scout"
        | "corsair"
        | "federation_dropship"
        | "federation_dropship_mkii"
        | "federation_gunship"
        | "ferdelance"
        | "independant_trader"
        | "krait_light"
        | "krait_mkii"
        | "lakonminer"
        | "mamba"
        | "mandalay"
        | "mediumtransport01"
        | "python"
        | "python_nx"
        | "type6"
        | "type8"
        | "typex"
        | "typex_2"
        | "typex_3" => 'M',
        "adder"
        | "cobramkiii"
        | "cobramkiv"
        | "cobramkv"
        | "diamondback"
        | "diamondbackxl"
        | "dolphin"
        | "eagle"
        | "empire_courier"
        | "empire_eagle"
        | "hauler"
        | "sidewinder"
        | "smallcombat01_nx"
        | "viper"
        | "viper_mkiv"
        | "vulture" => 'S',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(ship_name("Empire_Courier"), "Imperial Courier");
        assert_eq!(ship_name("explorer_nx"), "Caspian Explorer");
        assert_eq!(pad_size("Anaconda"), 'L');
    }

    #[test]
    fn shipyard_names_are_normalised() {
        assert_eq!(all().len(), 49);
        assert_eq!(ship_name("krait_mkii"), "Krait Mk II");
        assert_eq!(ship_name("Viper"), "Viper Mk III");
        assert_eq!(ship_name("cobramkv"), "Cobra Mk V");
        assert_eq!(ship_name("panthermkii"), "Panther Clipper Mk II");
        assert_eq!(ship_name("python_nx"), "Python Mk II");
        let k = ship("SmallCombat01_NX").unwrap();
        assert_eq!((k.id, k.name.as_str()), (129039241, "Kestrel Mk II"));
        assert_eq!(ship("krait_mkii").map(|s| s.fdev_name.as_str()), Some("Krait MkII"));
        assert_eq!(k.entitlement.as_deref(), Some("ELITE_V_SMALL_COMBAT_01"));
        assert_eq!(ship_by_id(129036130).map(|s| s.name.as_str()), Some("Type-11 Prospector"));
    }

    #[test]
    fn newer_hulls_vehicles_and_suits() {
        // 2025–2026 hulls (journal.md §8.1).
        for (sym, name) in [
            ("panthermkii", "Panther Clipper Mk II"),
            ("lakonminer", "Type-11 Prospector"),
            ("explorer_nx", "Caspian Explorer"),
            ("smallcombat01_nx", "Kestrel Mk II"),
            ("mediumtransport01", "Lynx Highliner"),
            ("anacondamkii", "Anaconda Mk II"),
            ("corsair", "Corsair"),
            ("mandalay", "Mandalay"),
            ("type8", "Type-8 Transporter"),
        ] {
            assert_eq!(ship_name(sym), name);
            // The fallback table also knows them.
            assert!(SHIPS.iter().any(|(k, v)| *k == sym && *v == name), "{sym}");
        }
        assert_eq!(ship_name("mev_rhino"), "Rhino MEV");
        assert_eq!(ship_name("lander01"), "Nomad");
        assert_eq!(ship_name("testbuggy"), "SRV Scarab");
        assert_eq!(ship_name("ExplorationSuit_Class1"), "Artemis Suit (G1)");
        assert_eq!(ship_name("some_new_ship"), "Some new ship");
    }

    #[test]
    fn pad_sizes() {
        for s in [
            "sidewinder",
            "cobramkiii",
            "cobramkv",
            "vulture",
            "diamondbackxl",
            "dolphin",
            "empire_courier",
            "viper_mkiv",
            "smallcombat01_nx",
        ] {
            assert_eq!(pad_size(s), 'S', "{s}");
        }
        for s in [
            "python",
            "python_nx",
            "krait_mkii",
            "mandalay",
            "corsair",
            "type8",
            "lakonminer",
            "mediumtransport01",
            "federation_gunship",
        ] {
            assert_eq!(pad_size(s), 'M', "{s}");
        }
        for s in
            ["anaconda", "anacondamkii", "cutter", "empire_trader", "orca", "type7", "type9_military", "panthermkii", "explorer_nx"]
        {
            assert_eq!(pad_size(s), 'L', "{s}");
        }
        // Every buyable hull has its pad in the table, so a new one in shipyard.csv fails here until it's added.
        for s in all() {
            assert!(known_pad_size(&s.symbol).is_some(), "{} ({}) has no pad size", s.name, s.symbol);
        }
        assert_eq!(known_pad_size("SideWinder"), Some('S'));
        assert_eq!(known_pad_size("some_new_ship"), None);
        assert_eq!(pad_size("some_new_ship"), 'S');
    }
}
