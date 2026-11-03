//! What a store entitlement (SKU) on a module or hull means for a player, from EDCD/FDevIDs
//! `sku.csv`: Guardian and human tech-broker unlocks, Powerplay 2.0 unlocks, and hulls or modules
//! that are still ARX early-access purchases.

use std::sync::LazyLock;

use rustc_hash::FxHashMap;

const SKU_CSV: &str = include_str!("../../../data/fdevids/sku.csv");

/// What has to happen before a station will sell an item to the commander.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unlock {
    /// Unlocked at a Guardian technology broker.
    GuardianBroker,
    /// Unlocked at a human technology broker.
    HumanBroker,
    /// Unlocked through Powerplay 2.0 ranks.
    Powerplay,
    /// Still an early-access purchase in the ARX store. FDevIDs is slow to drop this: in November
    /// 2026 it still said so of the Kestrel Mk II, which stations sold for credits.
    ArxEarlyAccess,
}

impl Unlock {
    pub fn label(self) -> &'static str {
        match self {
            Unlock::GuardianBroker => "Guardian tech broker",
            Unlock::HumanBroker => "Human tech broker",
            Unlock::Powerplay => "Powerplay",
            Unlock::ArxEarlyAccess => "ARX early access",
        }
    }

    /// FDevIDs' own words, for a tooltip.
    pub fn explain(self) -> &'static str {
        match self {
            Unlock::GuardianBroker => "Unlocked at a Guardian technology broker before it can be bought.",
            Unlock::HumanBroker => "Unlocked at a human technology broker before it can be bought.",
            Unlock::Powerplay => "A Powerplay 2.0 unlock: earned by ranking up with a Power.",
            Unlock::ArxEarlyAccess => "Still an early-access purchase: bought with ARX in the Frontier store, not with credits.",
        }
    }
}

static TABLE: LazyLock<FxHashMap<String, &'static str>> = LazyLock::new(|| {
    let mut t = FxHashMap::default();
    for line in SKU_CSV.lines().skip(1) {
        if let Some((sku, requirement)) = line.split_once(',') {
            let sku = sku.trim();
            if !sku.is_empty() {
                t.insert(sku.to_ascii_uppercase(), requirement.trim());
            }
        }
    }
    t
});

/// FDevIDs' requirement text for a SKU, e.g. `"unlocked via Guardian Tech Broker"`.
pub fn requirement(sku: &str) -> Option<&'static str> {
    TABLE.get(&sku.trim().to_ascii_uppercase()).copied()
}

/// What an item with this SKU needs before it can be bought. Horizons (free for everyone now),
/// hulls that were free from day one and ARX early access that has ended need nothing.
pub fn unlock(sku: &str) -> Option<Unlock> {
    let r = requirement(sku)?.to_ascii_lowercase();
    if r.starts_with("unlocked via guardian tech broker") {
        Some(Unlock::GuardianBroker)
    } else if r.starts_with("unlocked via human tech broker") {
        Some(Unlock::HumanBroker)
    } else if r.starts_with("unlocked via powerplay") {
        Some(Unlock::Powerplay)
    } else if r == "early access purchase in the arx store" {
        Some(Unlock::ArxEarlyAccess)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocks_from_fdevids() {
        assert_eq!(unlock("ELITE_HORIZONS_V_GUARDIAN_FSDBOOSTER"), Some(Unlock::GuardianBroker));
        assert_eq!(unlock("ELITE_HORIZONS_V_METAHULL"), Some(Unlock::HumanBroker));
        assert_eq!(unlock("ELITE_SPECIFIC_V_POWER_200020"), Some(Unlock::Powerplay));
        assert_eq!(requirement("elite_specific_v_power_200020"), Some("unlocked via Powerplay 2.0 (Prismatic Shield Generator)"));
        // The Anaconda Mk II is ARX-only for now; hulls whose early access ended need nothing.
        assert_eq!(unlock("ELITE_V_ANACONDA_MKII"), Some(Unlock::ArxEarlyAccess));
        assert_eq!(unlock("ELITE_V_PYTHON_NX"), None);
        assert_eq!(unlock("ELITE_V_MEDIUM_TRANSPORT_01"), None);
        assert_eq!(unlock("ELITE_HORIZONS_V_PLANETARY_LANDINGS"), None);
        assert_eq!(unlock("NO_SUCH_SKU"), None);
    }
}
