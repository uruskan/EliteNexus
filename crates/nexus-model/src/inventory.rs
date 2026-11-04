//! Engineering materials, cargo, Odyssey micro-resources and suit loadouts.

use nexus_journal::companion::commodity_symbol;
use nexus_journal::events as ev;
use nexus_journal::Timestamp;
use rustc_hash::FxHashMap;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MaterialCategory {
    Raw,
    Manufactured,
    Encoded,
}

impl MaterialCategory {
    pub fn from_journal(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('$').to_ascii_lowercase();
        if s.starts_with("raw") || s.contains("_raw") {
            Some(MaterialCategory::Raw)
        } else if s.starts_with("manufactured") || s.contains("manufactured") {
            Some(MaterialCategory::Manufactured)
        } else if s.starts_with("encoded") || s.contains("encoded") {
            Some(MaterialCategory::Encoded)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MaterialCategory::Raw => "Raw",
            MaterialCategory::Manufactured => "Manufactured",
            MaterialCategory::Encoded => "Encoded",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Materials {
    pub raw: BTreeMap<String, i64>,
    pub manufactured: BTreeMap<String, i64>,
    pub encoded: BTreeMap<String, i64>,
    /// Localised names seen in the journal, keyed by symbol.
    pub names: FxHashMap<String, String>,
    pub updated: Timestamp,
}

impl Materials {
    fn map_mut(&mut self, cat: MaterialCategory) -> &mut BTreeMap<String, i64> {
        match cat {
            MaterialCategory::Raw => &mut self.raw,
            MaterialCategory::Manufactured => &mut self.manufactured,
            MaterialCategory::Encoded => &mut self.encoded,
        }
    }

    pub fn map(&self, cat: MaterialCategory) -> &BTreeMap<String, i64> {
        match cat {
            MaterialCategory::Raw => &self.raw,
            MaterialCategory::Manufactured => &self.manufactured,
            MaterialCategory::Encoded => &self.encoded,
        }
    }

    pub fn count(&self, symbol: &str) -> i64 {
        let s = symbol.to_ascii_lowercase();
        self.raw.get(&s).or_else(|| self.manufactured.get(&s)).or_else(|| self.encoded.get(&s)).copied().unwrap_or(0)
    }

    pub fn category_of(&self, symbol: &str) -> Option<MaterialCategory> {
        let s = symbol.to_ascii_lowercase();
        if self.raw.contains_key(&s) {
            Some(MaterialCategory::Raw)
        } else if self.manufactured.contains_key(&s) {
            Some(MaterialCategory::Manufactured)
        } else if self.encoded.contains_key(&s) {
            Some(MaterialCategory::Encoded)
        } else {
            None
        }
    }

    pub fn display_name(&self, symbol: &str) -> String {
        self.names.get(symbol).cloned().unwrap_or_else(|| nexus_data::names::split_camel(symbol))
    }

    pub fn snapshot(&mut self, m: &ev::Materials, ts: Timestamp) {
        for (cat, list) in [
            (MaterialCategory::Raw, &m.raw),
            (MaterialCategory::Manufactured, &m.manufactured),
            (MaterialCategory::Encoded, &m.encoded),
        ] {
            let mut map = BTreeMap::new();
            for nc in list {
                let sym = nc.name.to_ascii_lowercase();
                if let Some(l) = &nc.name_localised {
                    self.names.insert(sym.clone(), l.clone());
                }
                map.insert(sym, nc.count);
            }
            *self.map_mut(cat) = map;
        }
        self.updated = ts;
    }

    /// Adds (or with a negative `delta`, removes) a material. Unknown category: guesses from what we hold.
    pub fn adjust(&mut self, symbol: &str, category: Option<MaterialCategory>, delta: i64, localised: Option<&str>) {
        let sym = symbol.to_ascii_lowercase();
        if let Some(l) = localised {
            self.names.insert(sym.clone(), l.to_string());
        }
        let Some(cat) = category.or_else(|| self.category_of(&sym)) else { return };
        let map = self.map_mut(cat);
        let v = map.entry(sym).or_insert(0);
        *v = (*v + delta).max(0);
    }

    pub fn total(&self, cat: MaterialCategory) -> i64 {
        self.map(cat).values().sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CargoEntry {
    pub symbol: String,
    pub name: String,
    pub count: i64,
    pub stolen: i64,
    pub mission_id: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct CargoHold {
    pub items: Vec<CargoEntry>,
    pub updated: Timestamp,
}

impl CargoHold {
    pub fn total(&self) -> i64 {
        self.items.iter().map(|i| i.count).sum()
    }

    pub fn count(&self, symbol: &str) -> i64 {
        self.items.iter().filter(|i| i.symbol == symbol).map(|i| i.count).sum()
    }

    pub fn snapshot(&mut self, items: &[ev::CargoItem], ts: Timestamp) {
        self.items = items
            .iter()
            .map(|c| CargoEntry {
                symbol: commodity_symbol(&c.name),
                name: c.name_localised.clone().unwrap_or_else(|| nexus_data::names::split_camel(&c.name)),
                count: c.count,
                stolen: c.stolen,
                mission_id: c.mission_id,
            })
            .collect();
        self.updated = ts;
    }

    pub fn adjust(&mut self, name: &str, localised: Option<&str>, delta: i64, mission_id: Option<u64>) {
        let symbol = commodity_symbol(name);
        if let Some(e) = self.items.iter_mut().find(|e| e.symbol == symbol && e.mission_id == mission_id) {
            e.count += delta;
        } else if delta > 0 {
            self.items.push(CargoEntry {
                name: localised.map(str::to_string).unwrap_or_else(|| nexus_data::names::split_camel(&symbol)),
                symbol,
                count: delta,
                stolen: 0,
                mission_id,
            });
        }
        self.items.retain(|e| e.count > 0);
    }
}

#[derive(Debug, Clone, Default)]
pub struct LockerItem {
    pub name: String,
    pub count: i64,
}

/// Odyssey micro-resources (ship locker or backpack).
#[derive(Debug, Clone, Default)]
pub struct Locker {
    pub items: BTreeMap<String, LockerItem>,
    pub components: BTreeMap<String, LockerItem>,
    pub consumables: BTreeMap<String, LockerItem>,
    pub data: BTreeMap<String, LockerItem>,
    pub updated: Timestamp,
}

impl Locker {
    pub fn snapshot(&mut self, inv: &ev::MicroResourceInventory, ts: Timestamp) {
        fn fill(dst: &mut BTreeMap<String, LockerItem>, src: &Option<Vec<ev::MicroResource>>) {
            if let Some(list) = src {
                dst.clear();
                for r in list {
                    let e = dst.entry(r.name.to_ascii_lowercase()).or_insert_with(|| LockerItem {
                        name: r.name_localised.clone().unwrap_or_else(|| nexus_data::names::split_camel(&r.name)),
                        count: 0,
                    });
                    e.count += r.count;
                }
            }
        }
        fill(&mut self.items, &inv.items);
        fill(&mut self.components, &inv.components);
        fill(&mut self.consumables, &inv.consumables);
        fill(&mut self.data, &inv.data);
        self.updated = ts;
    }

    fn bucket(&mut self, kind: Option<&str>) -> Option<&mut BTreeMap<String, LockerItem>> {
        match kind.map(|k| k.to_ascii_lowercase()).as_deref() {
            Some("item") | Some("items") => Some(&mut self.items),
            Some("component") | Some("components") => Some(&mut self.components),
            Some("consumable") | Some("consumables") => Some(&mut self.consumables),
            Some("data") => Some(&mut self.data),
            _ => None,
        }
    }

    pub fn adjust(&mut self, r: &ev::MicroResource, sign: i64) {
        let Some(map) = self.bucket(r.kind.as_deref().or(r.category.as_deref())) else { return };
        let e = map.entry(r.name.to_ascii_lowercase()).or_insert_with(|| LockerItem {
            name: r.name_localised.clone().unwrap_or_else(|| nexus_data::names::split_camel(&r.name)),
            count: 0,
        });
        e.count = (e.count + sign * r.count).max(0);
        map.retain(|_, v| v.count > 0);
    }

    pub fn count(&self, symbol: &str) -> i64 {
        let s = symbol.to_ascii_lowercase();
        [&self.items, &self.components, &self.consumables, &self.data]
            .iter()
            .filter_map(|m| m.get(&s))
            .map(|i| i.count)
            .sum()
    }

    pub fn total(&self) -> i64 {
        [&self.items, &self.components, &self.consumables, &self.data]
            .iter()
            .flat_map(|m| m.values())
            .map(|i| i.count)
            .sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SuitLoadoutInfo {
    pub loadout_id: u64,
    pub name: String,
    pub suit_id: u64,
    pub suit_name: String,
    pub suit_mods: Vec<String>,
    pub weapons: Vec<(String, String, u32, Vec<String>)>,
}

#[derive(Debug, Clone, Default)]
pub struct Inventory {
    pub materials: Materials,
    pub cargo: CargoHold,
    pub srv_cargo: CargoHold,
    pub ship_locker: Locker,
    pub backpack: Locker,
    pub suit_loadouts: BTreeMap<u64, SuitLoadoutInfo>,
    pub current_suit_loadout: Option<u64>,
}

/// A suit's or weapon's name: the journal's localised one, unless the game wrote its placeholder there
/// (`$TacticalSuit_Class1_Name;`), as it often does in loadout events.
fn gear_name(symbol: &str, localised: Option<&str>) -> String {
    match localised.map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('$')) {
        Some(l) => l.to_string(),
        None => nexus_data::engineering::odyssey_item(symbol).map(|i| i.name.to_string()).unwrap_or_else(|| nexus_data::ships::ship_name(symbol)),
    }
}

impl Inventory {
    pub fn on_suit_loadout(&mut self, s: &ev::SuitLoadout, select: bool) {
        self.suit_loadouts.insert(
            s.loadout_id,
            SuitLoadoutInfo {
                loadout_id: s.loadout_id,
                name: s.loadout_name.clone(),
                suit_id: s.suit_id,
                suit_name: gear_name(&s.suit_name, s.suit_name_localised.as_deref()),
                suit_mods: s.suit_mods.clone(),
                weapons: s.modules.iter().map(|m| (m.slot_name.clone(), gear_name(&m.module_name, m.module_name_localised.as_deref()), m.class, m.weapon_mods.clone())).collect(),
            },
        );
        if select {
            self.current_suit_loadout = Some(s.loadout_id);
        }
    }

    /// `DeleteSuitLoadout`.
    pub fn on_delete_suit_loadout(&mut self, d: &ev::SuitLoadoutName) {
        self.suit_loadouts.remove(&d.loadout_id);
        if self.current_suit_loadout == Some(d.loadout_id) {
            self.current_suit_loadout = None;
        }
    }

    /// `RenameSuitLoadout`. A loadout not seen yet waits for the game to list it whole (`SuitLoadout` and
    /// `SwitchSuitLoadout` give every slot).
    pub fn on_rename_suit_loadout(&mut self, r: &ev::SuitLoadoutName) {
        if let Some(l) = self.suit_loadouts.get_mut(&r.loadout_id) {
            l.name = r.loadout_name.clone();
        }
    }

    /// `LoadoutEquipModule` (`equip`) / `LoadoutRemoveModule`: the weapon in one slot changed, or the slot was emptied.
    /// Only loadouts already known are changed: an unknown one with a single weapon would look complete.
    pub fn on_loadout_module(&mut self, m: &ev::SuitLoadoutSlot, equip: bool) {
        let Some(l) = self.suit_loadouts.get_mut(&m.loadout_id) else { return };
        if !m.loadout_name.is_empty() {
            l.name = m.loadout_name.clone();
        }
        let weapon = (m.slot_name.clone(), gear_name(&m.module_name, m.module_name_localised.as_deref()), m.class, m.weapon_mods.clone());
        match l.weapons.iter().position(|w| w.0.eq_ignore_ascii_case(&m.slot_name)) {
            Some(i) if equip => l.weapons[i] = weapon,
            Some(i) => {
                l.weapons.remove(i);
            }
            None if equip => {
                l.weapons.push(weapon);
                // The game's slot order: PrimaryWeapon1, PrimaryWeapon2, SecondaryWeapon.
                l.weapons.sort_by(|a, b| a.0.cmp(&b.0));
            }
            None => {}
        }
    }
}
