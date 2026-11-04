//! A landable planet's ground for Rhino surface mining: its class and its volcanism, the two things a Scan says about
//! its surface. Read from the journal's spellings ("Rocky body", "minor rocky magma volcanism") or Spansh's
//! ("Rocky body", "Minor Rocky Magma"). No journal or feed says what a mining location holds, so a ground describes a
//! planet; it promises nothing about what's under it.

/// The classes of planet a ship can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    MetalRich,
    HighMetal,
    Rocky,
    RockyIce,
    Icy,
}

impl Class {
    pub const ALL: [Class; 5] = [Class::MetalRich, Class::HighMetal, Class::Rocky, Class::RockyIce, Class::Icy];

    pub fn label(self) -> &'static str {
        match self {
            Class::MetalRich => "Metal-rich",
            Class::HighMetal => "High metal content",
            Class::Rocky => "Rocky",
            Class::RockyIce => "Rocky ice",
            Class::Icy => "Icy",
        }
    }

    /// "rocky body", or "rocky bodies" for several.
    pub fn noun(self, several: bool) -> String {
        format!("{} {}", self.label().to_lowercase(), if several { "bodies" } else { "body" })
    }

    /// From the journal's `PlanetClass` ("Metal rich body") or Spansh's sub-type ("Metal-rich body", "High metal
    /// content world"). None for any other class: stars, gas giants, water and earth-like worlds.
    pub fn of(planet_class: &str) -> Option<Class> {
        let c = planet_class.trim().to_ascii_lowercase().replace('-', " ");
        // A rocky ice body's class starts with "rocky" too, so it's tried first.
        [("metal rich", Class::MetalRich), ("high metal content", Class::HighMetal), ("rocky ice", Class::RockyIce), ("rocky", Class::Rocky), ("icy", Class::Icy)]
            .into_iter()
            .find(|(start, _)| c.starts_with(start))
            .map(|(_, class)| class)
    }
}

/// What a planet's volcanism erupts, as the game names it, minor or major alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Volcanism {
    /// No volcanism.
    Inactive,
    MetallicMagma,
    RockyMagma,
    SilicateMagma,
    SilicateVapourGeysers,
    WaterMagma,
    WaterGeysers,
    CarbonDioxideGeysers,
    AmmoniaMagma,
    MethaneMagma,
    NitrogenMagma,
    /// Any other kind the game writes.
    Other,
}

impl Volcanism {
    /// Every kind with a name of its own, in the order above.
    pub const NAMED: [Volcanism; 11] = [
        Volcanism::Inactive,
        Volcanism::MetallicMagma,
        Volcanism::RockyMagma,
        Volcanism::SilicateMagma,
        Volcanism::SilicateVapourGeysers,
        Volcanism::WaterMagma,
        Volcanism::WaterGeysers,
        Volcanism::CarbonDioxideGeysers,
        Volcanism::AmmoniaMagma,
        Volcanism::MethaneMagma,
        Volcanism::NitrogenMagma,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Volcanism::Inactive => "no volcanism",
            Volcanism::MetallicMagma => "metallic magma",
            Volcanism::RockyMagma => "rocky magma",
            Volcanism::SilicateMagma => "silicate magma",
            Volcanism::SilicateVapourGeysers => "silicate vapour geysers",
            Volcanism::WaterMagma => "water magma",
            Volcanism::WaterGeysers => "water geysers",
            Volcanism::CarbonDioxideGeysers => "carbon dioxide geysers",
            Volcanism::AmmoniaMagma => "ammonia magma",
            Volcanism::MethaneMagma => "methane magma",
            Volcanism::NitrogenMagma => "nitrogen magma",
            Volcanism::Other => "other volcanism",
        }
    }

    /// From the journal's `Volcanism` ("major rocky magma volcanism", "" for none) or Spansh's ("Major Rocky Magma",
    /// "No volcanism").
    pub fn of(text: &str) -> Volcanism {
        let t = text.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
        if t.is_empty() || t == "no volcanism" {
            return Volcanism::Inactive;
        }
        Volcanism::NAMED[1..].iter().copied().find(|v| t.contains(v.label())).unwrap_or(Volcanism::Other)
    }
}

/// A planet's ground: its class and its volcanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ground {
    pub class: Class,
    pub volcanism: Volcanism,
}

impl Ground {
    /// None when the class isn't one a ship can land on.
    pub fn of(planet_class: &str, volcanism: &str) -> Option<Ground> {
        Some(Ground { class: Class::of(planet_class)?, volcanism: Volcanism::of(volcanism) })
    }

    /// "Rocky · rocky magma", or "Metal-rich" without volcanism.
    pub fn label(self) -> String {
        match self.volcanism {
            Volcanism::Inactive => self.class.label().to_string(),
            v => format!("{} · {}", self.class.label(), v.label()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_in_the_journal_and_on_spansh() {
        let journal = ["Metal rich body", "High metal content body", "Rocky body", "Rocky ice body", "Icy body"];
        let spansh = ["Metal-rich body", "High metal content world", "Rocky body", "Rocky Ice world", "Icy body"];
        for (j, s) in journal.iter().zip(spansh) {
            assert_eq!(Class::of(j), Class::of(s), "{j} / {s}");
        }
        assert_eq!(journal.map(|j| Class::of(j).expect(j)), Class::ALL);
        for other in ["Sudarsky class I gas giant", "Class I gas giant", "Water world", "Earthlike body", "Ammonia world", ""] {
            assert_eq!(Class::of(other), None, "{other}");
        }
        assert_eq!(Class::RockyIce.noun(true), "rocky ice bodies");
    }

    #[test]
    fn volcanism_in_the_journal_and_on_spansh() {
        assert_eq!(Volcanism::of(""), Volcanism::Inactive);
        assert_eq!(Volcanism::of("No volcanism"), Volcanism::Inactive);
        assert_eq!(Volcanism::of("minor rocky magma volcanism"), Volcanism::RockyMagma);
        assert_eq!(Volcanism::of("Major Rocky Magma"), Volcanism::RockyMagma);
        assert_eq!(Volcanism::of("major silicate vapour geysers volcanism"), Volcanism::SilicateVapourGeysers);
        assert_eq!(Volcanism::of("silicate magma volcanism"), Volcanism::SilicateMagma);
        assert_eq!(Volcanism::of("Minor Water Geysers"), Volcanism::WaterGeysers);
        assert_eq!(Volcanism::of("water magma volcanism"), Volcanism::WaterMagma);
        assert_eq!(Volcanism::of("Carbon Dioxide Geysers"), Volcanism::CarbonDioxideGeysers);
        assert_eq!(Volcanism::of("minor  methane   magma volcanism"), Volcanism::MethaneMagma, "spacing");
        assert_eq!(Volcanism::of("helium geysers volcanism"), Volcanism::Other);
        // Each named kind reads back from its own label.
        for v in Volcanism::NAMED {
            assert_eq!(Volcanism::of(v.label()), v);
        }
    }

    #[test]
    fn a_ground_is_class_and_volcanism() {
        let g = Ground::of("Rocky body", "minor rocky magma volcanism").expect("ground");
        assert_eq!((g.class, g.volcanism, g.label()), (Class::Rocky, Volcanism::RockyMagma, "Rocky · rocky magma".to_string()));
        assert_eq!(Ground::of("Metal rich body", "").map(Ground::label).as_deref(), Some("Metal-rich"));
        assert_eq!(Ground::of("Icy body", "major water geysers volcanism").map(Ground::label).as_deref(), Some("Icy · water geysers"));
        assert_eq!(Ground::of("Class I gas giant", "water geysers volcanism"), None);
    }
}
