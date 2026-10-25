//! Pages shown in the main content area.

pub mod accounts;
pub mod ask;
pub mod bgs;
pub mod bindings;
pub mod carrier;
pub mod cockpit;
pub mod colonisation;
pub mod dashboard;
pub mod engineering;
pub mod exobio;
pub mod exploration;
pub mod fleet;
pub mod galaxy_map;
pub mod glare;
pub mod hud;
pub mod logbook;
pub mod market;
pub mod mining;
pub mod missions;
pub mod navigation;
pub mod settings;
pub mod stats;
pub mod welcome;

use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Page {
    Ask,
    Dashboard,
    Navigation,
    Exploration,
    Exobiology,
    GalaxyMap,
    Market,
    Mining,
    Colonisation,
    Carrier,
    Engineering,
    Fleet,
    Missions,
    Bgs,
    Logbook,
    Statistics,
    Bindings,
    Hud,
    Accounts,
    Settings,
}

impl Page {
    pub const GROUPS: &'static [(&'static str, &'static [Page])] = &[
        ("Cockpit", &[Page::Ask, Page::Dashboard, Page::Navigation]),
        ("Exploration", &[Page::Exploration, Page::Exobiology, Page::GalaxyMap]),
        ("Economy", &[Page::Market, Page::Mining, Page::Colonisation, Page::Carrier]),
        ("Commander", &[Page::Engineering, Page::Fleet, Page::Missions, Page::Bgs]),
        ("Records", &[Page::Logbook, Page::Statistics]),
        ("System", &[Page::Bindings, Page::Hud, Page::Accounts, Page::Settings]),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Ask => "Ask",
            Page::Dashboard => "Dashboard",
            Page::Navigation => "Navigation",
            Page::Exploration => "Exploration",
            Page::Exobiology => "Exobiology",
            Page::GalaxyMap => "Galaxy map",
            Page::Market => "Market",
            Page::Mining => "Mining",
            Page::Colonisation => "Colonisation",
            Page::Carrier => "Fleet carrier",
            Page::Engineering => "Engineering",
            Page::Fleet => "Fleet",
            Page::Missions => "Missions",
            Page::Bgs => "BGS & Powerplay",
            Page::Logbook => "Logbook",
            Page::Statistics => "Statistics",
            Page::Bindings => "Bindings",
            Page::Hud => "HUD & cockpit",
            Page::Accounts => "Accounts & sync",
            Page::Settings => "Settings",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Page::Ask => icon::SPARKLE,
            Page::Dashboard => icon::GAUGE,
            Page::Navigation => icon::NAVIGATION_ARROW,
            Page::Exploration => icon::PLANET,
            Page::Exobiology => icon::PLANT,
            Page::GalaxyMap => icon::GLOBE_HEMISPHERE_WEST,
            Page::Market => icon::CHART_LINE_UP,
            Page::Mining => icon::DIAMONDS_FOUR,
            Page::Colonisation => icon::BUILDINGS,
            Page::Carrier => icon::ANCHOR,
            Page::Engineering => icon::WRENCH,
            Page::Fleet => icon::ROCKET,
            Page::Missions => icon::TARGET,
            Page::Bgs => icon::SCALES,
            Page::Logbook => icon::BOOK_OPEN,
            Page::Statistics => icon::CHART_BAR,
            Page::Bindings => icon::GAME_CONTROLLER,
            Page::Hud => icon::PALETTE,
            Page::Accounts => icon::PLUGS_CONNECTED,
            Page::Settings => icon::GEAR_SIX,
        }
    }
}
