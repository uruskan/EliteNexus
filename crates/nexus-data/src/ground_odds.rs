//! What Rhino surface mining turns up on each ground: how many of a ground's planetary mining locations listed each
//! material, in a survey of 1,131 locations commanders read in the game up to 11 October 2026. A location's target
//! panel lists the materials to find there once it's targeted after a DSS scan; no journal or feed records that, so
//! this survey is the only figure there is short of looking. A listing says the material is there to find, not how
//! much of it, and the locations of one body aren't independent of each other.
//!
//! The survey's grounds are the game's body classes, a rocky body split by its volcanism; magma on a metal-rich or
//! high metal content body counts under its class. Rocky bodies with any other volcanism (water or carbon dioxide
//! geysers, say) weren't read. Materials are named as the game names the commodities. See
//! docs/research/surface-mining.md §4.

use crate::grounds::{Class, Ground, Volcanism};

/// The day of the survey's last read.
pub const SURVEYED: &str = "11 October 2026";

/// One ground of the survey: how many of its locations were read, and how many of those listed each material.
#[derive(Debug, PartialEq, Eq)]
pub struct SurveyGround {
    pub class: Class,
    /// The volcanism the survey kept apart; None: any volcanism counts under the class.
    pub volcanism: Option<Volcanism>,
    pub locations: u32,
    /// (material, locations that listed it), most first.
    pub listed: &'static [(&'static str, u32)],
}

/// `listed` of `of` locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    pub listed: u32,
    pub of: u32,
}

impl Share {
    pub fn pct(self) -> f64 {
        if self.of == 0 { 0.0 } else { f64::from(self.listed) * 100.0 / f64::from(self.of) }
    }

    /// How many of a body's `locations` would list the material if they're like the ones read: an average, which
    /// holds however the locations of one body go together.
    pub fn expected(self, locations: u32) -> f64 {
        self.pct() / 100.0 * f64::from(locations)
    }
}

impl SurveyGround {
    /// "Metal-rich", "Rocky · rocky magma", "Rocky without volcanism".
    pub fn label(&self) -> String {
        match self.volcanism {
            None => self.class.label().to_string(),
            Some(Volcanism::Inactive) => format!("{} without volcanism", self.class.label()),
            Some(v) => format!("{} · {}", self.class.label(), v.label()),
        }
    }

    /// How many of its locations listed `material` (the game's name, any case); 0 when none did.
    pub fn share(&self, material: &str) -> Share {
        let listed = self.listed.iter().find(|(m, _)| m.eq_ignore_ascii_case(material.trim())).map_or(0, |(_, n)| *n);
        Share { listed, of: self.locations }
    }
}

const fn ground(class: Class, volcanism: Option<Volcanism>, locations: u32, listed: &'static [(&'static str, u32)]) -> SurveyGround {
    SurveyGround { class, volcanism, locations, listed }
}

pub static METAL_RICH: SurveyGround = ground(Class::MetalRich, None, 114, &[
    ("Platinum", 53), ("Titanium", 51), ("Copper", 50), ("Haematite", 47), ("Thorium", 45), ("Lithium", 43), ("Samarium", 42),
    ("Osmium", 38), ("Tantalum", 38), ("Diamond", 34), ("Iridium", 34), ("Silver", 33), ("Gold", 32), ("Uranium", 26),
    ("Rhodplumsite", 12), ("Sapphire", 9), ("Ruby", 7), ("Helium", 6), ("Palladium", 6),
]);

pub static HIGH_METAL: SurveyGround = ground(Class::HighMetal, None, 276, &[
    ("Copper", 151), ("Haematite", 145), ("Titanium", 144), ("Lithium", 128), ("Uranium", 108), ("Osmium", 105), ("Platinum", 103),
    ("Tantalum", 100), ("Thorium", 94), ("Samarium", 81), ("Silver", 73), ("Gold", 57), ("Iridium", 51), ("Palladium", 28),
    ("Helium", 15), ("Ruby", 13), ("Sapphire", 12), ("Diamond", 11), ("Rhodplumsite", 8),
]);

pub static ROCKY: SurveyGround = ground(Class::Rocky, Some(Volcanism::Inactive), 242, &[
    ("Copper", 157), ("Uraninite", 124), ("Haematite", 123), ("Thorium", 117), ("Samarium", 106), ("Magnesite", 104), ("Gold", 80),
    ("Silver", 70), ("Palladium", 69), ("Jadeite", 51), ("Thortveitite", 33), ("Grandidierite", 27), ("Bastnasite", 23),
    ("Periclase Dunite", 23), ("Helium", 16), ("Olivine", 16), ("Deuterium", 14), ("Low Temperature Diamonds", 10), ("Monazite", 8),
    ("Alexandrite", 6), ("Diamond", 5), ("Serendibite", 5), ("Sapphire", 3),
]);

pub static ROCKY_METALLIC_MAGMA: SurveyGround = ground(Class::Rocky, Some(Volcanism::MetallicMagma), 99, &[
    ("Quartz Pyroxenite", 45), ("Olivine", 43), ("Alexandrite", 42), ("Bastnasite", 42), ("Monazite", 41), ("Copper", 32),
    ("Samarium", 31), ("Uraninite", 29), ("Serendibite", 27), ("Haematite", 25), ("Magnesite", 23), ("Sapphire", 22), ("Ruby", 21),
    ("Thorium", 21), ("Diamond", 15), ("Gold", 13), ("Palladium", 13), ("Grandidierite", 9), ("Silver", 9), ("Jadeite", 8),
    ("Thortveitite", 5), ("Periclase Dunite", 4), ("Helium", 3), ("Deuterium", 2), ("Low Temperature Diamonds", 2),
]);

pub static ROCKY_MAGMA: SurveyGround = ground(Class::Rocky, Some(Volcanism::RockyMagma), 36, &[
    ("Monazite", 21), ("Olivine", 21), ("Bastnasite", 19), ("Copper", 16), ("Haematite", 13), ("Serendibite", 13), ("Alexandrite", 12),
    ("Gold", 12), ("Silver", 11), ("Thorium", 11), ("Samarium", 10), ("Uraninite", 9), ("Magnesite", 8), ("Palladium", 6),
    ("Diamond", 4), ("Grandidierite", 4), ("Low Temperature Diamonds", 3), ("Thortveitite", 3), ("Helium", 2), ("Jadeite", 2),
    ("Periclase Dunite", 1), ("Ruby", 1),
]);

pub static ROCKY_SILICATE_GEYSERS: SurveyGround = ground(Class::Rocky, Some(Volcanism::SilicateVapourGeysers), 139, &[
    ("Jadeite", 86), ("Helium", 84), ("Copper", 69), ("Haematite", 63), ("Uraninite", 61), ("Thorium", 50), ("Samarium", 47),
    ("Gold", 46), ("Magnesite", 46), ("Silver", 42), ("Palladium", 37), ("Grandidierite", 17), ("Thortveitite", 14),
    ("Low Temperature Diamonds", 9), ("Periclase Dunite", 8), ("Olivine", 6), ("Bastnasite", 4), ("Deuterium", 4), ("Monazite", 2),
    ("Sapphire", 2),
]);

pub static ROCKY_ICE: SurveyGround = ground(Class::RockyIce, None, 47, &[
    ("Copper", 40), ("Methanol Monohydrate Crystals", 36), ("Water", 36), ("Haematite", 35), ("Palladium", 13), ("Deuterium", 11),
    ("Olivine", 9), ("Low Temperature Diamonds", 4), ("Helium", 2), ("Diamond", 1),
]);

pub static ICY: SurveyGround = ground(Class::Icy, None, 178, &[
    ("Water", 177), ("Methanol Monohydrate Crystals", 119), ("Deuterium", 99), ("Tritium", 59), ("Low Temperature Diamonds", 43),
    ("Helium-3", 31),
]);

/// Every ground of the survey, in the order of [`Class::ALL`].
pub static GROUNDS: [&SurveyGround; 8] = [&METAL_RICH, &HIGH_METAL, &ROCKY, &ROCKY_METALLIC_MAGMA, &ROCKY_MAGMA, &ROCKY_SILICATE_GEYSERS, &ROCKY_ICE, &ICY];

/// Locations read in all.
pub fn locations_read() -> u32 {
    GROUNDS.iter().map(|g| g.locations).sum()
}

/// The survey's figures for a body's ground: None for a rocky body with a volcanism nobody read.
pub fn survey(g: Ground) -> Option<&'static SurveyGround> {
    let s: &'static SurveyGround = match (g.class, g.volcanism) {
        (Class::MetalRich, _) => &METAL_RICH,
        (Class::HighMetal, _) => &HIGH_METAL,
        (Class::Rocky, Volcanism::Inactive) => &ROCKY,
        (Class::Rocky, Volcanism::MetallicMagma) => &ROCKY_METALLIC_MAGMA,
        (Class::Rocky, Volcanism::RockyMagma) => &ROCKY_MAGMA,
        (Class::Rocky, Volcanism::SilicateVapourGeysers) => &ROCKY_SILICATE_GEYSERS,
        (Class::Rocky, _) => return None,
        (Class::RockyIce, _) => &ROCKY_ICE,
        (Class::Icy, _) => &ICY,
    };
    Some(s)
}

/// Every material the survey saw, A to Z.
pub fn materials() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = GROUNDS.iter().flat_map(|g| g.listed.iter().map(|(m, _)| *m)).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The grounds whose locations listed `material`, the highest share first.
pub fn grounds_for(material: &str) -> Vec<(&'static SurveyGround, Share)> {
    let mut v: Vec<(&'static SurveyGround, Share)> = GROUNDS.iter().map(|g| (*g, g.share(material))).filter(|(_, s)| s.listed > 0).collect();
    v.sort_by(|a, b| b.1.pct().total_cmp(&a.1.pct()).then(b.1.of.cmp(&a.1.of)));
    v
}

/// The classes where some location listed `material`, in the order of [`Class::ALL`].
pub fn classes_for(material: &str) -> Vec<Class> {
    Class::ALL.into_iter().filter(|c| GROUNDS.iter().any(|g| g.class == *c && g.share(material).listed > 0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shares the survey published, to a tenth of a percent, come back from the counts.
    #[test]
    fn the_counts_give_the_published_shares() {
        let pct = |g: &SurveyGround, m: &str| (g.share(m).pct() * 10.0).round() / 10.0;
        assert_eq!(pct(&ROCKY_MAGMA, "Monazite"), 58.3);
        assert_eq!(pct(&ROCKY_METALLIC_MAGMA, "Quartz Pyroxenite"), 45.5);
        assert_eq!(pct(&METAL_RICH, "Platinum"), 46.5);
        assert_eq!(pct(&HIGH_METAL, "Iridium"), 18.5);
        assert_eq!(pct(&ROCKY_SILICATE_GEYSERS, "Jadeite"), 61.9);
        assert_eq!(pct(&ICY, "Water"), 99.4);
        assert_eq!(pct(&ROCKY_ICE, "Diamond"), 2.1);
        assert_eq!(pct(&ROCKY, "Sapphire"), 1.2);
        assert_eq!(locations_read(), 1_131);
        for g in GROUNDS {
            assert!(g.listed.iter().all(|(_, n)| *n > 0 && *n <= g.locations), "{}", g.label());
            assert!(g.listed.windows(2).all(|w| w[0].1 >= w[1].1), "most first: {}", g.label());
        }
    }

    #[test]
    fn a_body_reads_as_its_survey_ground() {
        let of = |class, volcanism| survey(Ground { class, volcanism }).map(SurveyGround::label);
        assert_eq!(of(Class::MetalRich, Volcanism::MetallicMagma).as_deref(), Some("Metal-rich"), "magma on a metal body counts under its class");
        assert_eq!(of(Class::Rocky, Volcanism::Inactive).as_deref(), Some("Rocky without volcanism"));
        assert_eq!(of(Class::Rocky, Volcanism::RockyMagma).as_deref(), Some("Rocky · rocky magma"));
        assert_eq!(of(Class::Rocky, Volcanism::WaterGeysers), None, "not read");
        assert_eq!(of(Class::Icy, Volcanism::WaterGeysers).as_deref(), Some("Icy"));
    }

    #[test]
    fn materials_and_where_they_turn_up() {
        let all = materials();
        assert_eq!(all.len(), 37);
        // Every name is a commodity the game knows.
        for m in &all {
            assert!(crate::commodities::commodity(m).is_some_and(|c| c.name == *m), "{m}");
        }
        let monazite: Vec<String> = grounds_for("monazite").iter().map(|(g, s)| format!("{} {}/{}", g.label(), s.listed, s.of)).collect();
        assert_eq!(monazite, ["Rocky · rocky magma 21/36", "Rocky · metallic magma 41/99", "Rocky without volcanism 8/242", "Rocky · silicate vapour geysers 2/139"]);
        assert_eq!(classes_for("Platinum"), [Class::MetalRich, Class::HighMetal]);
        assert_eq!(classes_for("Helium-3"), [Class::Icy]);
        assert!(classes_for("Painite").is_empty(), "not a Rhino find");
        assert_eq!(ROCKY_MAGMA.share("Platinum"), Share { listed: 0, of: 36 });
        // 14 locations on a rocky magma body: about 8 would list Monazite.
        assert_eq!(ROCKY_MAGMA.share("Monazite").expected(14).round(), 8.0);
    }
}
