//! Ships the commander owns, their loadouts and where they are.

use nexus_data::names::clean_station_name;
use nexus_data::ships::ship_name;
use nexus_journal::events as ev;
use nexus_journal::Timestamp;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct Modifier {
    pub label: String,
    pub value: Option<f64>,
    pub original: Option<f64>,
    pub less_is_good: bool,
}

impl Modifier {
    /// Relative improvement in percent (positive = better), if computable.
    pub fn improvement_pct(&self) -> Option<f64> {
        let (v, o) = (self.value?, self.original?);
        if o == 0.0 {
            return None;
        }
        let pct = (v - o) / o.abs() * 100.0;
        Some(if self.less_is_good { -pct } else { pct })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModuleEngineering {
    pub engineer: Option<String>,
    pub blueprint: String,
    pub level: u32,
    pub quality: f64,
    pub experimental: Option<String>,
    pub modifiers: Vec<Modifier>,
}

#[derive(Debug, Clone, Default)]
pub struct ShipModule {
    pub slot: String,
    pub item: String,
    pub on: bool,
    pub priority: u32,
    pub health: f64,
    pub value: Option<i64>,
    pub engineering: Option<ModuleEngineering>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShipLocation {
    pub system: Option<String>,
    pub station: Option<String>,
    pub market_id: Option<u64>,
    /// The body a ship was left on or near, when it isn't at a station.
    pub body: Option<String>,
    pub in_transit: bool,
    /// When an in-transit ship arrives (ShipyardTransfer time + duration).
    pub arrives: Option<Timestamp>,
    pub updated: Timestamp,
}

impl ShipLocation {
    /// Still on its way at `now`. A transfer has arrived once its time is up, though the journal says so only with
    /// the next `StoredShips`; one whose arrival time isn't known counts as on its way until then.
    pub fn in_transit_at(&self, now: Timestamp) -> bool {
        self.in_transit && self.arrives.is_none_or(|a| a.0 > now.0)
    }

    /// "Jameson Memorial (Shinrarta Dezhra)", "Sol 3 (Sol)", "Sol".
    pub fn text(&self) -> Option<String> {
        let system = self.system.as_deref();
        match (self.station.as_deref().or(self.body.as_deref()), system) {
            (Some(place), Some(sys)) if !place.eq_ignore_ascii_case(sys) => Some(format!("{place} ({sys})")),
            (Some(place), _) => Some(place.to_string()),
            (None, sys) => sys.map(str::to_string),
        }
    }
}

/// Where one of the commander's ships is, as far as the journal tells (`GameState::ship_place`).
#[derive(Debug, Clone, PartialEq)]
pub enum ShipPlace {
    /// The ship they fly, with them aboard it or close by (on foot beside it, in an SRV): where they are.
    WithYou,
    /// The ship they fly, where they stepped out of it or left it to crew another commander's ship, while they're
    /// elsewhere.
    Left(ShipLocation),
    /// The ship they fly, while they ride an Apex taxi or a dropship or crew another commander's ship, left where the
    /// journal Elite Nexus read doesn't say.
    Away,
    /// Another ship: stored at a station, or on its way to one (`ShipLocation::in_transit_at`).
    Stored(ShipLocation),
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct Ship {
    pub id: u64,
    /// Lower-case journal symbol (`empire_courier`).
    pub ship_type: String,
    pub type_name: String,
    pub name: String,
    pub ident: String,
    pub hull_value: i64,
    pub modules_value: i64,
    /// Value from StoredShips when we've never seen a Loadout for it.
    pub stored_value: i64,
    pub rebuy: i64,
    pub hull_health: f64,
    pub unladen_mass: f64,
    pub cargo_capacity: u32,
    pub max_jump_range: f64,
    pub fuel_main_capacity: f64,
    pub fuel_reserve_capacity: f64,
    pub modules: Vec<ShipModule>,
    pub hot: bool,
    pub location: ShipLocation,
    /// The raw `Loadout` journal line: exported to EDSY/Coriolis and uploaded where required.
    pub loadout_json: Option<String>,
    pub loadout_ts: Timestamp,
}

impl Ship {
    pub fn new(id: u64, ship_type: &str) -> Self {
        let t = ship_type.to_ascii_lowercase();
        Ship { id, type_name: ship_name(&t), ship_type: t, hull_health: 1.0, ..Default::default() }
    }

    pub fn total_value(&self) -> i64 {
        let v = self.hull_value + self.modules_value;
        if v > 0 { v } else { self.stored_value }
    }

    pub fn display_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() { self.type_name.clone() } else { format!("{} ({})", name, self.type_name) }
    }

    pub fn engineered_modules(&self) -> usize {
        self.modules.iter().filter(|m| m.engineering.is_some()).count()
    }

    pub fn module_in(&self, slot: &str) -> Option<&ShipModule> {
        self.modules.iter().find(|m| m.slot.eq_ignore_ascii_case(slot))
    }
}

#[derive(Debug, Clone, Default)]
pub struct StoredModuleInfo {
    pub name: String,
    pub name_localised: Option<String>,
    pub system: Option<String>,
    pub market_id: Option<u64>,
    pub engineering: Option<String>,
    pub level: Option<u32>,
    pub quality: Option<f64>,
    pub hot: bool,
    pub in_transit: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Fleet {
    pub current_ship_id: Option<u64>,
    pub ships: BTreeMap<u64, Ship>,
    pub stored_modules: Vec<StoredModuleInfo>,
    pub stored_modules_updated: Timestamp,
    pub fuel_main: f64,
    pub fuel_reservoir: f64,
    /// Where the commander stepped out of the ship they fly, or left it to crew another commander's ship: it stays
    /// there while they walk, ride an Apex taxi or a dropship elsewhere, or crew. None while they're aboard it.
    pub left_at: Option<ShipLocation>,
}

impl Fleet {
    pub fn current(&self) -> Option<&Ship> {
        self.current_ship_id.and_then(|id| self.ships.get(&id))
    }

    pub fn current_mut(&mut self) -> Option<&mut Ship> {
        self.current_ship_id.and_then(move |id| self.ships.get_mut(&id))
    }

    pub fn on_load_game(&mut self, l: &ev::LoadGame) {
        let (Some(id), Some(ship)) = (l.ship_id, l.ship.as_deref()) else { return };
        // On foot / in a taxi the "Ship" is a suit or a dropship: ignore those.
        let lower = ship.to_ascii_lowercase();
        if lower.contains("suit") || lower.contains("taxi") || lower.contains("dropship") {
            return;
        }
        // Logged in aboard it.
        self.left_at = None;
        self.current_ship_id = Some(id);
        let s = self.ships.entry(id).or_insert_with(|| Ship::new(id, ship));
        if let Some(n) = &l.ship_name {
            s.name = n.trim().to_string();
        }
        if let Some(i) = &l.ship_ident {
            s.ident = i.clone();
        }
        if let Some(f) = l.fuel_level {
            self.fuel_main = f;
        }
    }

    pub fn on_loadout(&mut self, l: &ev::Loadout, raw_line: &str, ts: Timestamp) {
        let lower = l.ship.to_ascii_lowercase();
        if lower.contains("suit") || lower.contains("taxi") || lower.is_empty() {
            return;
        }
        self.current_ship_id = Some(l.ship_id);
        let s = self.ships.entry(l.ship_id).or_insert_with(|| Ship::new(l.ship_id, &l.ship));
        s.ship_type = lower;
        s.type_name = ship_name(&s.ship_type);
        s.name = l.ship_name.trim().to_string();
        s.ident = l.ship_ident.clone();
        s.hull_value = l.hull_value.unwrap_or(0);
        s.modules_value = l.modules_value.unwrap_or(0);
        s.rebuy = l.rebuy;
        s.hull_health = l.hull_health;
        s.unladen_mass = l.unladen_mass;
        s.cargo_capacity = l.cargo_capacity;
        s.max_jump_range = l.max_jump_range;
        s.fuel_main_capacity = l.fuel_capacity.main;
        s.fuel_reserve_capacity = l.fuel_capacity.reserve;
        s.hot = l.hot.unwrap_or(false);
        s.modules = l
            .modules
            .iter()
            .map(|m| ShipModule {
                slot: m.slot.clone(),
                item: m.item.to_ascii_lowercase(),
                on: m.on,
                priority: m.priority,
                health: m.health,
                value: m.value,
                engineering: m.engineering.as_ref().map(|e| ModuleEngineering {
                    engineer: e.engineer.clone(),
                    blueprint: e.blueprint_name.clone(),
                    level: e.level,
                    quality: e.quality,
                    experimental: e.experimental_effect_localised.clone().or_else(|| e.experimental_effect.clone()),
                    modifiers: e
                        .modifiers
                        .iter()
                        .map(|m| Modifier {
                            label: m.label.clone(),
                            value: m.value,
                            original: m.original_value,
                            less_is_good: m.less_is_good.unwrap_or(0) == 1,
                        })
                        .collect(),
                }),
            })
            .collect();
        s.loadout_json = Some(raw_line.to_string());
        s.loadout_ts = ts;
        s.location = ShipLocation { updated: ts, ..Default::default() };
    }

    pub fn on_shipyard_buy(&mut self, b: &ev::ShipyardBuy, here: (&str, Option<&str>, u64), ts: Timestamp) {
        if let Some(id) = b.sell_ship_id {
            self.ships.remove(&id);
        }
        if let Some(id) = b.store_ship_id {
            self.store_ship_at(id, here, ts);
        }
    }

    pub fn on_shipyard_new(&mut self, n: &ev::ShipyardNew) {
        self.ships.insert(n.new_ship_id, Ship::new(n.new_ship_id, &n.ship_type));
        self.current_ship_id = Some(n.new_ship_id);
    }

    pub fn on_ship_redeemed(&mut self, r: &ev::ShipRedeemed, here: (&str, Option<&str>, u64), ts: Timestamp) {
        if let Some(id) = r.new_ship_id {
            let mut s = Ship::new(id, &r.ship_type);
            s.location = ShipLocation {
                system: Some(here.0.to_string()),
                station: here.1.map(str::to_string),
                market_id: Some(here.2),
                updated: ts,
                ..Default::default()
            };
            self.ships.entry(id).or_insert(s);
        }
    }

    pub fn on_shipyard_sell(&mut self, s: &ev::ShipyardSell) {
        self.ships.remove(&s.sell_ship_id);
    }

    pub fn on_shipyard_swap(&mut self, s: &ev::ShipyardSwap, here: (&str, Option<&str>, u64), ts: Timestamp) {
        if let Some(id) = s.sell_ship_id {
            self.ships.remove(&id);
        }
        if let Some(id) = s.store_ship_id {
            self.store_ship_at(id, here, ts);
        }
        self.ships.entry(s.ship_id).or_insert_with(|| Ship::new(s.ship_id, &s.ship_type));
        self.current_ship_id = Some(s.ship_id);
    }

    pub fn on_shipyard_transfer(&mut self, t: &ev::ShipyardTransfer, here: (&str, Option<&str>, u64), ts: Timestamp) {
        let s = self.ships.entry(t.ship_id).or_insert_with(|| Ship::new(t.ship_id, &t.ship_type));
        s.location = ShipLocation {
            system: Some(here.0.to_string()),
            station: here.1.map(str::to_string),
            market_id: Some(t.market_id),
            in_transit: t.transfer_time > 0,
            arrives: Some(Timestamp(ts.0 + t.transfer_time)),
            body: None,
            updated: ts,
        };
    }

    fn store_ship_at(&mut self, id: u64, here: (&str, Option<&str>, u64), ts: Timestamp) {
        if let Some(s) = self.ships.get_mut(&id) {
            s.location = ShipLocation {
                system: Some(here.0.to_string()),
                station: here.1.map(str::to_string),
                market_id: Some(here.2),
                updated: ts,
                ..Default::default()
            };
        }
    }

    pub fn on_stored_ships(&mut self, s: &ev::StoredShips, ts: Timestamp) {
        let mut seen: Vec<u64> = Vec::new();
        for (list, here) in [(&s.ships_here, true), (&s.ships_remote, false)] {
            for st in list {
                seen.push(st.ship_id);
                let ship = self.ships.entry(st.ship_id).or_insert_with(|| Ship::new(st.ship_id, &st.ship_type));
                if let Some(n) = &st.name
                    && !n.trim().is_empty() {
                        ship.name = n.trim().to_string();
                    }
                ship.stored_value = st.value;
                ship.hot = st.hot;
                let in_transit = st.in_transit.unwrap_or(false);
                ship.location = if in_transit {
                    // Transfers listed in StoredShips are always headed to the station listing them.
                    ShipLocation {
                        system: Some(s.star_system.clone()),
                        station: Some(clean_station_name(&s.station_name)),
                        market_id: Some(s.market_id),
                        in_transit: true,
                        arrives: ship.location.arrives.filter(|a| a.0 > ts.0),
                        body: None,
                        updated: ts,
                    }
                } else if here {
                    ShipLocation {
                        system: Some(s.star_system.clone()),
                        station: Some(clean_station_name(&s.station_name)),
                        market_id: Some(s.market_id),
                        updated: ts,
                        ..Default::default()
                    }
                } else {
                    ShipLocation {
                        system: st.star_system.clone(),
                        station: None,
                        market_id: st.ship_market_id,
                        in_transit,
                        arrives: None,
                        body: None,
                        updated: ts,
                    }
                };
            }
        }
        // StoredShips lists every ship except the one we're in: anything else we know is gone.
        let current = self.current_ship_id;
        self.ships.retain(|id, _| seen.contains(id) || Some(*id) == current);
    }

    pub fn on_set_name(&mut self, n: &ev::SetUserShipName) {
        if let Some(s) = self.ships.get_mut(&n.ship_id) {
            s.name = n.user_ship_name.trim().to_string();
            s.ident = n.user_ship_id.clone();
        }
    }

    pub fn on_stored_modules(&mut self, m: &ev::StoredModules, ts: Timestamp) {
        self.stored_modules = m
            .items
            .iter()
            .map(|i| StoredModuleInfo {
                name: i.name.trim_start_matches('$').trim_end_matches(';').trim_end_matches("_name").to_ascii_lowercase(),
                name_localised: i.name_localised.clone(),
                system: i.star_system.clone().or_else(|| Some(m.star_system.clone())),
                market_id: i.market_id.or(Some(m.market_id)),
                engineering: i.engineer_modifications.clone(),
                level: i.level,
                quality: i.quality,
                hot: i.hot,
                in_transit: i.in_transit.unwrap_or(false),
            })
            .collect();
        self.stored_modules_updated = ts;
    }

    pub fn on_hull_damage(&mut self, h: &ev::HullDamage) {
        if h.player_pilot.unwrap_or(true) && !h.fighter.unwrap_or(false)
            && let Some(s) = self.current_mut() {
                s.hull_health = h.health;
            }
    }

    pub fn on_repair_all(&mut self) {
        if let Some(s) = self.current_mut() {
            s.hull_health = 1.0;
        }
    }

    pub fn total_value(&self) -> i64 {
        self.ships.values().map(Ship::total_value).sum()
    }
}
