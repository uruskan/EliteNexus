//! Market: live EDDN-fed galaxy database — commodity pages, abnormal prices, trade routes, services,
//! hotspots. Commodity and station names are links: they open the commodity's page (where to buy,
//! where to sell, latest prices) or the station's market.

use crate::finder::{self, carriers_here, presence, Presence, SellPlan};
use crate::jobs::Slot;
use crate::market::{ImportJob, ImportSource, MarketService, SPANSH_STATIONS_URL};
use crate::runplan::{self, Order, PlanInput, RunPlan};
use crate::settings::PriceWatch;
use crate::theme::{display, pal, size};
use crate::widgets::{Number, Suggestion, TableSort, badge, bar, card, cmp_opt, cmp_text, duration, empty, short_credits};
use egui::{Color32, RichText, Ui};
use egui_extras::Column;
use egui_phosphor::regular as icon;
use nexus_galaxy::{
    CommodityHit, CommodityInfo, CommodityQuery, CommodityQuote, CommoditySummary, Deal, DealQuery, GalaxyDb, HotspotHit, HotspotQuery, MaterialTrader, PricePoint,
    PriceSince, Services, Sort, StationHit, StationInfo, StationMarket, StationQuery, Stood, TechBroker, TradeMode, TradeQuery, TradeRoute,
};
use nexus_model::GameState;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tab {
    #[default]
    Commodity,
    Abnormal,
    Plan,
    Sell,
    Routes,
    Services,
    Hotspots,
    Alerts,
    Docked,
    Database,
}

impl Tab {
    /// For `--tab` (screenshots): `commodity`, `abnormal`, `sell`, `routes`, `services`, `hotspots`,
    /// `alerts`, `station` (or `docked`), `database`.
    pub fn from_name(name: &str) -> Option<Tab> {
        Some(match name.to_ascii_lowercase().as_str() {
            "commodity" => Tab::Commodity,
            "abnormal" => Tab::Abnormal,
            "plan" => Tab::Plan,
            "sell" => Tab::Sell,
            "routes" => Tab::Routes,
            "services" => Tab::Services,
            "hotspots" => Tab::Hotspots,
            "alerts" => Tab::Alerts,
            "docked" | "station" => Tab::Docked,
            "database" => Tab::Database,
            _ => return None,
        })
    }
}

/// Which list a commodity page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Side {
    /// Markets selling it (you buy).
    #[default]
    Buy,
    /// Markets buying it (you sell).
    Sell,
    /// Every market trading it, newest price first.
    Latest,
}

/// The Market page's choices, kept in settings.json so the page opens as it was left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarketPrefs {
    pub tab: Tab,
    /// The commodity page shown last (its display name), and the ones before it, newest first.
    pub commodity: String,
    pub recent: Vec<String>,
    pub side: Side,
    pub sort: Sort,
    pub max_dist: f64,
    pub min_pad: Option<char>,
    /// The pad filter follows the current ship until the commander picks one.
    pub pad_auto: bool,
    pub max_age_h: f64,
    pub min_qty: i64,
    pub carriers: bool,
    pub planetary: bool,
    /// Find commodity: the category chosen ("" = every category, or the pseudo category [`RARE_GOODS`]).
    pub category: String,
    /// Find commodity: rare goods taken as one are open instead of `commodity`: one category's
    /// (`Some("Legal Drugs")`) or all of them (`Some("")`).
    pub rare_goods: Option<String>,
    /// Trade routes: area and longest hop.
    pub radius: f64,
    pub hop: f64,
    /// Trade routes, while docked: only the routes that start at that station.
    pub routes_here: bool,
    /// Sell my cargo: buyers within.
    pub sell_radius: f64,
    pub hotspot_signal: String,
    pub hotspot_min: i64,
    /// Abnormal prices: stations paying far above (or selling far below) the galactic average.
    pub abn_mode: TradeMode,
    pub abn_ratio: f64,
    /// `None` = the whole galaxy.
    pub abn_radius: Option<f64>,
    pub abn_age_h: f64,
    pub abn_carriers: bool,
    /// Pinned market entries, oldest first, and whether their panel is open.
    pub pins: Vec<Pin>,
    pub pins_open: bool,
    /// Run planner: how far from an order to look for its seller or buyer, sellers by price (not
    /// detour), sell what is aboard first, fleet carriers as found sellers or buyers.
    pub plan_radius: f64,
    pub plan_cheapest: bool,
    pub plan_hold: bool,
    pub plan_carriers: bool,
    /// Station tab: the markets open side by side (market ids, oldest first, at most [`MAX_TILES`]).
    pub tiles: Vec<u64>,
    /// Station tab: the list of markets on the left is shown.
    pub tiles_list: bool,
    /// Station tab: the list shows this system's stations and carriers without a market in the database too.
    pub tiles_unknown: bool,
    /// Station tab: which goods the markets list.
    pub tiles_show: TileShow,
    /// Markets opened lately, newest first (the list's Recent section).
    pub recent_markets: Vec<RecentMarket>,
    /// The market the game wrote last (its id and time): a market opened in the game opens here once.
    pub seen_docked: (u64, i64),
    /// Services: fleet carriers and planetary ports among the stations listed.
    pub services_carriers: bool,
    pub services_planetary: bool,
}

impl Default for MarketPrefs {
    fn default() -> Self {
        MarketPrefs {
            tab: Tab::Commodity,
            commodity: "Tritium".into(),
            recent: Vec::new(),
            side: Side::Buy,
            sort: Sort::BestPrice,
            max_dist: 100.0,
            min_pad: None,
            pad_auto: true,
            max_age_h: 72.0,
            min_qty: 1,
            carriers: true,
            planetary: true,
            category: String::new(),
            rare_goods: None,
            radius: 40.0,
            hop: 25.0,
            routes_here: false,
            sell_radius: 60.0,
            hotspot_signal: "Low Temperature Diamonds".into(),
            hotspot_min: 2,
            abn_mode: TradeMode::SellToStation,
            abn_ratio: 2.0,
            abn_radius: None,
            abn_age_h: 72.0,
            abn_carriers: true,
            pins: Vec::new(),
            pins_open: true,
            plan_radius: 100.0,
            plan_cheapest: false,
            plan_hold: true,
            plan_carriers: true,
            tiles: Vec::new(),
            tiles_list: true,
            tiles_unknown: false,
            tiles_show: TileShow::All,
            recent_markets: Vec::new(),
            seen_docked: (0, 0),
            services_carriers: false,
            services_planetary: true,
        }
    }
}

impl MarketPrefs {
    /// Values a hand-edited settings file could put out of range, brought back into the controls' ranges.
    fn sanitized(mut self) -> Self {
        let clamp = |v: f64, lo: f64, hi: f64, def: f64| if v.is_finite() { v.clamp(lo, hi) } else { def };
        self.max_dist = clamp(self.max_dist, 1.0, MAX_SEARCH_LY, 100.0);
        self.max_age_h = clamp(self.max_age_h, 1.0, 720.0, 72.0);
        self.min_qty = self.min_qty.clamp(1, 100_000);
        self.radius = clamp(self.radius, 5.0, 1000.0, 40.0);
        self.hop = clamp(self.hop, 1.0, 500.0, 25.0);
        self.sell_radius = clamp(self.sell_radius, 5.0, MAX_SEARCH_LY, 60.0);
        self.hotspot_min = self.hotspot_min.clamp(1, 6);
        self.abn_ratio = clamp(self.abn_ratio, 1.5, 10.0, 2.0);
        self.abn_age_h = clamp(self.abn_age_h, 1.0, 720.0, 72.0);
        self.min_pad = self.min_pad.filter(|p| matches!(p, 'S' | 'M' | 'L'));
        self.plan_radius = clamp(self.plan_radius, 5.0, MAX_SEARCH_LY, 100.0);
        self.pins.truncate(MAX_PINS);
        self.recent.truncate(RECENT);
        let mut seen = rustc_hash::FxHashSet::default();
        self.tiles.retain(|t| *t != 0 && seen.insert(*t));
        self.tiles.truncate(MAX_TILES);
        self.recent_markets.truncate(RECENT_MARKETS);
        self
    }

    /// Puts a market at the top of the Station tab's Recent list.
    fn remember_market(&mut self, s: &StationInfo) {
        self.recent_markets.retain(|r| r.market_id != s.market_id);
        self.recent_markets.insert(0, RecentMarket { market_id: s.market_id, name: s.name.clone(), system: s.system.clone(), carrier: s.is_carrier });
        self.recent_markets.truncate(RECENT_MARKETS);
    }

    /// Makes `name` the page shown and the newest of the recent ones.
    fn remember(&mut self, name: &str) {
        self.commodity = name.to_string();
        self.recent.retain(|r| r != name);
        self.recent.insert(0, name.to_string());
        self.recent.truncate(RECENT);
    }
}

/// Recent commodities kept for the search suggestions.
const RECENT: usize = 8;

/// Pins kept at most (the run planner tries every stop order up to eight stations).
const MAX_PINS: usize = 24;

/// Markets open side by side on the Station tab, at most.
const MAX_TILES: usize = 8;
/// Markets the Station tab's Recent list keeps.
const RECENT_MARKETS: usize = 12;

/// Which goods the Station tab's markets list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TileShow {
    #[default]
    All,
    /// What the station sells: where you buy.
    Sells,
    /// What the station buys: where you sell.
    Buys,
}

/// A market opened lately, for the Station tab's Recent list.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentMarket {
    pub market_id: u64,
    pub name: String,
    pub system: String,
    pub carrier: bool,
}

/// A market entry the commander pinned: it stays on top of the Market page, priced from the latest
/// data, and the run planner fills it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pin {
    pub market_id: u64,
    pub station: String,
    pub system: String,
    pub carrier: bool,
    /// The commodity's symbol (`unknownartifact2`) and display name (Thargoid Probe).
    pub symbol: String,
    pub name: String,
    /// true: the station buys it (you sell there); false: the station sells it (you buy there).
    pub sell_here: bool,
    /// Price and quantity when pinned.
    pub price: i64,
    pub qty: i64,
    pub pinned_unix: i64,
    /// Part of the run the planner builds.
    pub in_plan: bool,
}

impl Default for Pin {
    fn default() -> Self {
        Pin {
            market_id: 0,
            station: String::new(),
            system: String::new(),
            carrier: false,
            symbol: String::new(),
            name: String::new(),
            sell_here: true,
            price: 0,
            qty: 0,
            pinned_unix: 0,
            in_plan: true,
        }
    }
}

impl Pin {
    fn same(&self, market_id: u64, symbol: &str, sell_here: bool) -> bool {
        self.market_id == market_id && self.symbol == symbol && self.sell_here == sell_here
    }
}

impl MarketPrefs {
    fn is_pinned(&self, market_id: u64, symbol: &str, sell_here: bool) -> bool {
        self.pins.iter().any(|p| p.same(market_id, symbol, sell_here))
    }

    /// Pins an entry, or unpins it when it already is.
    fn toggle_pin(&mut self, pin: Pin) {
        let before = self.pins.len();
        self.pins.retain(|p| !p.same(pin.market_id, &pin.symbol, pin.sell_here));
        if self.pins.len() == before && self.pins.len() < MAX_PINS {
            self.pins_open = true;
            self.pins.push(Pin { pinned_unix: nexus_journal::Timestamp::now().0, ..pin });
        }
    }
}

/// The pin toggle at the start of a row.
fn pin_button(ui: &mut Ui, pinned: bool) -> bool {
    let (col, tip) = if pinned {
        (pal::ACCENT, "Pinned: click to unpin")
    } else {
        (pal::FAINT, "Pin it: it stays on top of the Market page with its latest price, and the run planner fills it")
    };
    ui.add(egui::Button::new(RichText::new(icon::PUSH_PIN).size(15.0).color(col)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked()
}

/// A row of the commodity page's lists: its market and its commodity (symbol). Rare goods taken as one
/// can list two rows for one market.
type PickKey = (u64, String);

/// What a click in one of the commodity page's lists asks for.
enum RowAction {
    Select(PickKey),
    Pin(Pin),
    /// A rare goods row's commodity name: its own page.
    Open(String),
}

type Res<T> = Result<Vec<T>, String>;
/// The database's carriers by callsign (those the game listed here), and every station and carrier it places in this
/// system.
type HereView = (FxHashMap<String, StationInfo>, Vec<StationInfo>);
/// Abnormal-price filters: mode, ratio ×100, radius, age, carriers, pad.
type AbnKey = (TradeMode, u64, Option<u64>, u64, bool, Option<char>);

/// Everything on a commodity's page.
pub struct CommodityPage {
    summary: CommoditySummary,
    buy: Vec<CommodityHit>,
    sell: Vec<CommodityHit>,
    latest: Vec<CommodityQuote>,
    /// When a list came back empty: the nearest market that trades it if the distance, quantity,
    /// pad and station-type filters are ignored (the age limit still applies).
    buy_hint: Option<CommodityHit>,
    sell_hint: Option<CommodityHit>,
}

/// A market picked in a commodity page's list: everything it trades, this commodity's price history there, and how
/// long its prices have stood.
pub struct PickDetail {
    market: Option<StationMarket>,
    history: Vec<PricePoint>,
    since: Option<PriceSince>,
}

/// Somewhere a click on a commodity or station name leads.
enum Nav {
    Commodity { name: String, side: Option<Side> },
    Station(u64),
}

pub struct MarketUi {
    pub tab: Tab,
    pub prefs: MarketPrefs,
    /// Find commodity: what's typed in the search field.
    input: String,
    /// Find commodity: what's typed in the category field.
    category_input: String,
    /// The list the page shows; starts at the saved one and may move to a list that has results.
    side: Side,
    /// Every known commodity, for the suggestions (loaded on first use).
    commodities: Vec<CommodityInfo>,
    page: Slot<Result<Option<CommodityPage>, String>>,
    /// The rare goods and query the shown page was loaded with: any change (filters, commodity, position) reloads it.
    page_key: Option<(Option<String>, CommodityQuery)>,
    /// A new commodity is loading: once it arrives, an empty list may give way to one with results.
    fresh: bool,
    /// Text that matched no commodity.
    not_found: Option<String>,
    pick: Option<PickKey>,
    detail: Slot<Result<PickDetail, String>>,
    detail_key: Option<(u64, String)>,
    /// Station tab: the open markets as the database has them, by market id; reloaded when the tiles change, a little
    /// after the game writes a market, and every few seconds while the tab shows.
    tiles_live: Slot<Result<FxHashMap<u64, Option<StationMarket>>, String>>,
    tiles_key: Option<Vec<u64>>,
    tiles_at: Option<std::time::Instant>,
    /// When to read the tiles again after the game wrote a market: the database stores it a moment later.
    tiles_again: Option<std::time::Instant>,
    /// A market just opened, or asked for again while open: its tile lights up for a moment.
    tile_flash: Option<(u64, std::time::Instant)>,
    /// Markets opened this run whose names the Recent list doesn't have yet.
    tiles_fresh: Vec<u64>,
    /// Station tab: the goods filter above the tiles.
    tile_filter: String,
    /// Station tab: a station or carrier being looked for, and what the database found.
    station_find: String,
    station_found: Slot<Res<StationInfo>>,
    station_found_for: String,
    /// The market of the station you're docked at (the Station tab keeps its tile when eight are open).
    docked_market: Option<u64>,
    routes: Slot<Res<TradeRoute>>,
    /// The station the shown routes start at (`None`: routes around you).
    routes_from: Option<String>,
    /// Search on the next frame: Trade routes (screenshot runs: `--tab routes`), or Mining hotspots (opened from a
    /// "where to get it" panel).
    pub autorun: bool,
    service: usize,
    stations: Slot<Res<StationHit>>,
    /// What the Services list was searched for: (service, system, pad, carriers, planetary, how many). It searches again
    /// when that changes.
    services_key: Option<(usize, u64, Option<char>, bool, bool, usize)>,
    /// How many of the nearest stations Services lists (Show more adds [`SERVICES_STEP`]).
    services_limit: usize,
    hotspots: Slot<Res<HotspotHit>>,
    hotspot_prices: Slot<Res<CommodityHit>>,
    /// Path typed into "import a downloaded dump".
    import_path: String,
    sell: Slot<Result<SellPlan, String>>,
    /// (cargo snapshot time, tonnes, system, how far, how old a price may be): re-plan when any of them changes.
    sell_key: (i64, i64, u64, u64, u64),
    /// The rule being composed in "Price alerts", and the prices it's measured against (with what they were found for).
    new_watch: PriceWatch,
    alert_ref: Slot<Result<AlertRef, String>>,
    alert_ref_key: Option<String>,
    abn: Slot<Res<Deal>>,
    /// The pins' markets as the database has them now, refreshed every half minute and on arrival.
    pins_live: Slot<Result<FxHashMap<u64, Option<StationMarket>>, String>>,
    pins_key: Option<(Vec<u64>, i64)>,
    pins_at: Option<std::time::Instant>,
    plan: Slot<Result<RunPlan, String>>,
    /// Station tab: the database's view of the carriers listed here (by callsign), and every station it places here.
    here: Slot<Result<HereView, String>>,
    here_key: Option<(u64, Vec<String>)>,
    /// What the shown answer was asked for, and when: it's asked again every minute (the import or the live feed may
    /// have added stations).
    here_for: Option<(u64, Vec<String>)>,
    here_at: Option<std::time::Instant>,
    plan_key: Option<String>,
    /// A run the commander chose to follow: the app moves it to the Navigation page.
    pub follow: Option<crate::pages::navigation::PlannedRoute>,
    /// The filters the shown results were found with (a change re-runs the scan).
    abn_key: Option<AbnKey>,
}

impl Default for MarketUi {
    fn default() -> Self {
        MarketUi::new(MarketPrefs::default())
    }
}

impl MarketUi {
    pub fn new(prefs: MarketPrefs) -> Self {
        let prefs = prefs.sanitized();
        MarketUi {
            tab: prefs.tab,
            input: if prefs.rare_goods.is_some() { RARE_GOODS.to_string() } else { prefs.commodity.clone() },
            category_input: prefs.category.clone(),
            side: prefs.side,
            prefs,
            commodities: Vec::new(),
            page: Slot::default(),
            page_key: None,
            fresh: true,
            not_found: None,
            pick: None,
            detail: Slot::default(),
            detail_key: None,
            tiles_live: Slot::default(),
            tiles_key: None,
            tiles_at: None,
            tiles_again: None,
            tile_flash: None,
            tiles_fresh: Vec::new(),
            tile_filter: String::new(),
            station_find: String::new(),
            station_found: Slot::default(),
            station_found_for: String::new(),
            docked_market: None,
            routes: Slot::default(),
            routes_from: None,
            autorun: false,
            service: 0,
            stations: Slot::default(),
            services_key: None,
            services_limit: SERVICES_STEP,
            hotspots: Slot::default(),
            hotspot_prices: Slot::default(),
            import_path: String::new(),
            sell: Slot::default(),
            sell_key: (0, 0, 0, 0, 0),
            new_watch: PriceWatch::default(),
            alert_ref: Slot::default(),
            alert_ref_key: None,
            abn: Slot::default(),
            abn_key: None,
            pins_live: Slot::default(),
            pins_key: None,
            pins_at: None,
            plan: Slot::default(),
            here: Slot::default(),
            here_key: None,
            here_for: None,
            here_at: None,
            plan_key: None,
            follow: None,
        }
    }

    /// Opens a commodity's page (any spelling: display name, symbol, journal name, or a partial
    /// name the suggestions understand). Returns false when nothing matches.
    fn open_commodity(&mut self, db: &GalaxyDb, text: &str) -> bool {
        if self.commodities.is_empty() {
            self.commodities = db.commodities();
        }
        let mut name = resolve_commodity(db, &self.commodities, text);
        if name.is_none() && db.commodity_symbol(text).is_some() {
            // Known to the database but newer than our list (first seen on the live feed).
            self.commodities = db.commodities();
            name = resolve_commodity(db, &self.commodities, text);
        }
        let Some(name) = name else {
            self.not_found = Some(text.trim().to_string());
            return false;
        };
        if name != self.prefs.commodity || self.prefs.rare_goods.is_some() {
            self.pick = None;
            self.fresh = true;
        }
        self.prefs.rare_goods = None;
        if let Some(c) = self.commodities.iter().find(|c| c.name == name) {
            self.prefs.category.clone_from(&c.category);
        }
        self.prefs.remember(&name);
        self.input = name;
        self.not_found = None;
        // Searching again reloads even when nothing changed (fresh prices from the feed).
        self.page_key = None;
        true
    }

    /// No commodity page: the search field's item is picked afresh.
    fn close_page(&mut self) {
        self.prefs.commodity.clear();
        self.prefs.rare_goods = None;
        self.pick = None;
        self.not_found = None;
    }

    /// Opens rare goods taken as one: one category's, or ("") all of them.
    fn open_rare_goods(&mut self, category: String) {
        if self.prefs.rare_goods.as_deref() != Some(category.as_str()) {
            self.pick = None;
            self.fresh = true;
        }
        self.prefs.category = if category.is_empty() { RARE_GOODS.to_string() } else { category.clone() };
        self.prefs.rare_goods = Some(category);
        self.input = RARE_GOODS.to_string();
        self.not_found = None;
        self.page_key = None;
    }

    /// Opens what the search field holds: rare goods taken as one, or a commodity. `rares` is the rare goods the
    /// pseudo item stands for there: one category's, or "" for all.
    fn open_item(&mut self, db: &GalaxyDb, text: &str, rares: String) {
        if text.trim().eq_ignore_ascii_case(RARE_GOODS) {
            self.open_rare_goods(rares);
        } else {
            self.open_commodity(db, text);
        }
    }

    fn go(&mut self, db: &GalaxyDb, nav: Nav) {
        match nav {
            Nav::Commodity { name, side } => {
                if self.open_commodity(db, &name) {
                    if let Some(s) = side {
                        self.side = s;
                        self.fresh = false;
                    }
                    self.set_tab(Tab::Commodity);
                }
            }
            Nav::Station(id) => {
                self.open_tile(id, false);
                self.set_tab(Tab::Docked);
            }
        }
    }

    /// Opens a market on the Station tab beside the ones open, last (`front`: first, for the market you dock at). One
    /// already open keeps its place and lights up. With eight open the oldest closes, never the one you're docked at.
    fn open_tile(&mut self, id: u64, front: bool) {
        if id == 0 {
            return;
        }
        let keep = self.docked_market;
        let tiles = &mut self.prefs.tiles;
        match tiles.iter().position(|t| *t == id) {
            Some(i) if front && i != 0 => {
                tiles.remove(i);
                tiles.insert(0, id);
            }
            Some(_) => {}
            None => {
                if tiles.len() >= MAX_TILES {
                    // The previous docked market leaves first when a new one comes in front.
                    let gone = if front { Some(0) } else { tiles.iter().position(|t| Some(*t) != keep) };
                    tiles.remove(gone.unwrap_or(0));
                }
                if front {
                    tiles.insert(0, id);
                } else {
                    tiles.push(id);
                }
                self.tiles_fresh.push(id);
            }
        }
        self.tile_flash = Some((id, std::time::Instant::now()));
    }

    fn set_tab(&mut self, t: Tab) {
        self.tab = t;
        self.prefs.tab = t;
    }

    /// Opens what a "where to get it" panel (Engineering, Missions) asked for: the material traders of a kind, where
    /// to buy a commodity, or rings with hotspots for it (searched at once).
    pub fn open_source(&mut self, db: Option<&GalaxyDb>, to: crate::pages::where_to_get::Goto) {
        use crate::pages::where_to_get::Goto;
        match to {
            Goto::Trader(kind) => {
                let name = match kind {
                    nexus_data::materials::MaterialKind::Raw => "raw",
                    nexus_data::materials::MaterialKind::Manufactured => "manufactured",
                    nexus_data::materials::MaterialKind::Encoded => "encoded",
                };
                self.service = SERVICES.iter().position(|(_, k)| *k == name).unwrap_or(0);
                self.services_key = None;
                self.services_limit = SERVICES_STEP;
                self.set_tab(Tab::Services);
            }
            Goto::Commodity(name) => match db {
                Some(db) => self.go(db, Nav::Commodity { name, side: Some(Side::Buy) }),
                None => self.set_tab(Tab::Commodity),
            },
            Goto::Hotspots(signal) => {
                self.prefs.hotspot_signal = signal;
                self.autorun = true;
                self.set_tab(Tab::Hotspots);
            }
        }
    }
}

/// Farthest a local-database search can reach, ly: the whole galaxy (it's about 100,000 ly
/// across, and the database itself has no limit).
const MAX_SEARCH_LY: f64 = 100_000.0;

/// Most rows a commodity page lists per side.
const PAGE_LIMIT: usize = 100;

const SERVICES: &[(&str, &str)] = &[
    ("Raw material trader", "raw"),
    ("Manufactured material trader", "manufactured"),
    ("Encoded material trader", "encoded"),
    ("Human technology broker", "human"),
    ("Guardian technology broker", "guardian"),
    ("Interstellar Factors", "factors"),
    ("Universal Cartographics", "cartographics"),
    ("Vista Genomics", "vista"),
    ("Shipyard", "shipyard"),
    ("Outfitting", "outfitting"),
    ("Black market", "blackmarket"),
];

const HOTSPOT_SIGNALS: &[&str] = &[
    "Low Temperature Diamonds",
    "Painite",
    "Void Opal",
    "Platinum",
    "Alexandrite",
    "Benitoite",
    "Grandidierite",
    "Monazite",
    "Musgravite",
    "Rhodplumsite",
    "Serendibite",
    "Tritium",
    "Bromellite",
];

/// The Mining hotspots tab's name for a commodity, when it has hotspots there.
pub(crate) fn hotspot_signal(name: &str) -> Option<&'static str> {
    HOTSPOT_SIGNALS.iter().copied().find(|s| s.eq_ignore_ascii_case(name.trim()))
}

/// Commodities matching what was typed, best first: the whole name, the start of the name, the
/// initials ("ltd" is Low Temperature Diamonds), the start of every typed word ("thar pro"), anywhere
/// in the name, the game's internal name (Thargoid Probe is `UnknownArtifact2`), then the category
/// ("salvage" lists all salvage).
pub fn rank_commodities<'a>(all: impl IntoIterator<Item = &'a CommodityInfo>, typed: &str, limit: usize) -> Vec<&'a CommodityInfo> {
    let q = typed.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let words_of = |s: &str| s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let q_words = words_of(&q);
    let q_key: String = q.chars().filter(|c| c.is_alphanumeric()).collect();
    let mut scored: Vec<(u8, &CommodityInfo)> = all
        .into_iter()
        .filter_map(|c| {
            let name = c.name.to_lowercase();
            let words = words_of(&name);
            let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
            let score = if name == q {
                0
            } else if name.starts_with(&q) || (q_key.len() >= 2 && initials == q_key) {
                1
            } else if !q_words.is_empty() && q_words.iter().all(|qw| words.iter().any(|w| w.starts_with(qw.as_str()))) {
                2
            } else if name.contains(&q) {
                3
            } else if q_key.len() >= 3 && c.symbol.contains(&q_key) {
                4
            } else if c.category.to_lowercase().contains(&q) {
                5
            } else {
                return None;
            };
            Some((score, c))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.is_rare.cmp(&b.1.is_rare)).then_with(|| a.1.name.cmp(&b.1.name)));
    scored.into_iter().take(limit).map(|(_, c)| c).collect()
}

/// The commodity a text means (its display name): an exact name, any symbol spelling, or the best suggestion.
fn resolve_commodity(db: &GalaxyDb, all: &[CommodityInfo], text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(c) = all.iter().find(|c| c.name.eq_ignore_ascii_case(t)) {
        return Some(c.name.clone());
    }
    if let Some(sym) = db.commodity_symbol(t) {
        return all.iter().find(|c| c.symbol == sym).map(|c| c.name.clone());
    }
    rank_commodities(all, t, 1).first().map(|c| c.name.clone())
}

/// A commodity as a suggestion: its category on the right, rare goods starred.
fn commodity_suggestion(c: &CommodityInfo) -> Suggestion {
    Suggestion { text: c.name.clone(), detail: if c.is_rare { format!("{} · rare", c.category) } else { c.category.clone() }, starred: c.is_rare }
}

fn commodity_suggestions(all: &[CommodityInfo], recent: &[String], typed: &str) -> Vec<Suggestion> {
    if typed.trim().is_empty() {
        return recent
            .iter()
            .map(|r| match all.iter().find(|c| &c.name == r) {
                Some(c) => Suggestion { detail: format!("recent · {}", commodity_suggestion(c).detail), ..commodity_suggestion(c) },
                None => Suggestion { text: r.clone(), detail: "recent".into(), starred: false },
            })
            .collect();
    }
    rank_commodities(all, typed, 40).into_iter().map(commodity_suggestion).collect()
}

/// The pseudo category, and the pseudo item: rare goods taken as one commodity.
const RARE_GOODS: &str = "Rare Goods";
/// The category field's entry for no category.
const ALL_CATEGORIES: &str = "All categories";
/// Rare goods sold only during past events, which can't be bought any more: left out of the lists.
const RETIRED_RARES: [&str; 2] = ["classifiedexperimentalequipment", "galactictravelguide"];

/// A commodity Find commodity lists: traded on markets ("NonMarketable" isn't), and still sold.
fn listed(c: &CommodityInfo) -> bool {
    c.category != "NonMarketable" && !RETIRED_RARES.iter().any(|r| c.symbol.eq_ignore_ascii_case(r))
}

/// A commodity of a category's own item list: every category ("") lists every commodity, a category its
/// normal goods (its rare goods are one item), the pseudo category none (it has only that item).
fn in_category(c: &CommodityInfo, category: &str) -> bool {
    category.is_empty() || (c.category == category && !c.is_rare)
}

/// The rare goods a category's pseudo item takes as one: every category's ("" or the pseudo category), or its own.
fn rare_count(all: &[CommodityInfo], category: &str) -> usize {
    all.iter().filter(|c| listed(c) && c.is_rare && (category.is_empty() || category == RARE_GOODS || c.category == category)).count()
}

/// The pseudo item: one category's rare goods, or (`every`) those of every category.
fn rare_goods_item(count: usize, every: bool) -> Suggestion {
    let goods = if count == 1 { "rare good" } else { "rare goods" };
    Suggestion { text: RARE_GOODS.into(), detail: if every { format!("all {count} {goods}") } else { format!("{count} {goods}") }, starred: true }
}

/// Every category by name, then the pseudo category of rare goods.
fn commodity_categories(all: &[CommodityInfo]) -> Vec<String> {
    let mut categories: Vec<String> = all.iter().filter(|c| listed(c)).map(|c| c.category.clone()).collect();
    categories.sort();
    categories.dedup();
    categories.push(RARE_GOODS.into());
    categories
}

/// The category field's list, narrowed to what's typed; "All categories" first while nothing is.
fn category_suggestions(all: &[CommodityInfo], typed: &str) -> Vec<Suggestion> {
    let q = typed.trim().to_lowercase();
    let mut out = Vec::new();
    if q.is_empty() {
        out.push(Suggestion { text: ALL_CATEGORIES.into(), detail: String::new(), starred: false });
    }
    out.extend(commodity_categories(all).into_iter().filter(|c| c.to_lowercase().contains(&q)).map(|c| Suggestion { starred: c == RARE_GOODS, detail: String::new(), text: c }));
    out
}

/// A category's items: its commodities by name, then its rare goods as one item (when it has any).
fn category_items(all: &[CommodityInfo], category: &str) -> Vec<Suggestion> {
    let mut items: Vec<&CommodityInfo> = all.iter().filter(|c| listed(c) && in_category(c, category)).collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out: Vec<Suggestion> = items.into_iter().map(commodity_suggestion).collect();
    let rares = rare_count(all, category);
    if rares > 0 {
        out.push(rare_goods_item(rares, category.is_empty() || category == RARE_GOODS));
    }
    out
}

/// The search field's list. Nothing typed (or ▾): the category's items (every category: the recent ones first).
/// Typed: the best matches among every commodity, whatever the category, and every rare good as one when the words
/// fit. The category narrows only the list for nothing typed: a page opened earlier sets it, and typing must still
/// find anything.
fn item_suggestions(all: &[CommodityInfo], recent: &[String], category: &str, typed: &str) -> Vec<Suggestion> {
    let q = typed.trim().to_lowercase();
    if q.is_empty() {
        // A blank entry first: taking it leaves the field empty.
        let mut out = vec![Suggestion { text: String::new(), detail: String::new(), starred: false }];
        if category.is_empty() {
            out.extend(commodity_suggestions(all, recent, ""));
        }
        out.extend(category_items(all, category));
        return out;
    }
    let mut out: Vec<Suggestion> = rank_commodities(all.iter().filter(|c| listed(c)), typed, 40).into_iter().map(commodity_suggestion).collect();
    let rares = rare_count(all, "");
    if rares > 0 && RARE_GOODS.to_lowercase().contains(&q) {
        out.push(rare_goods_item(rares, true));
    }
    out
}

/// Distances the − and + of a distance step through, ly.
const LY: &[f64] = &[
    5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 75.0, 100.0, 150.0, 200.0, 250.0, 300.0, 400.0, 500.0, 750.0, 1_000.0, 1_500.0, 2_000.0, 2_500.0, 5_000.0, 10_000.0, 25_000.0,
    50_000.0, 100_000.0,
];
/// How old a price may be, hours.
const HOURS: &[f64] = &[1.0, 2.0, 3.0, 6.0, 12.0, 24.0, 48.0, 72.0, 96.0, 120.0, 168.0, 240.0, 336.0, 504.0, 720.0];
/// Least stock or demand, tonnes.
const TONNES: &[f64] = &[1.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0, 25_000.0, 50_000.0, 100_000.0];

/// What the landing-pad choice means, for its tooltip.
const PAD_TIP: &str = "The landing pad your ship needs. S fits every station, as Any does; M lists stations with medium or large pads, \
                       L only those with large ones. M and L leave out stations whose pads were never reported.";

/// The landing pad a search needs: Any, S, M or L. Returns true when changed.
fn pad_picker(ui: &mut Ui, pad: &mut Option<char>) -> bool {
    crate::widgets::segmented(ui, pad, &[(None, "Any"), (Some('S'), "S"), (Some('M'), "M"), (Some('L'), "L")])
}

/// Find commodity's filters, all in view: how far, which pad, how old a price may be, how much stock or demand, which
/// kinds of market, and the order. A reset shows once any of them differs from the defaults.
fn search_filters(ui: &mut Ui, p: &mut MarketPrefs, gs: &GameState) {
    let d = MarketPrefs::default();
    let changed = p.max_dist != d.max_dist || !p.pad_auto || p.max_age_h != d.max_age_h || p.min_qty != d.min_qty || p.carriers != d.carriers || p.planetary != d.planetary || p.sort != d.sort;
    crate::widgets::filter_bar(ui, "find-commodity-filters", |bar| {
        bar.field("Within", |ui| {
            ui.add(Number::new(&mut p.max_dist, 1.0..=MAX_SEARCH_LY).ladder(LY).unit(" ly"))
                .on_hover_text("How far from you to look. The local database has no limit of its own: up to the whole galaxy, 100,000 ly.");
        });
        let (pad_caption, pad_tip) = if p.pad_auto { ("Landing pad · your ship's", format!("{PAD_TIP}\n\nIt follows the ship you fly until you pick one.")) } else { ("Landing pad", PAD_TIP.to_string()) };
        bar.field_tip(pad_caption, &pad_tip, |ui| {
            if pad_picker(ui, &mut p.min_pad) {
                // Picked by hand: it no longer follows the ship.
                p.pad_auto = false;
            }
        });
        bar.field("Prices no older than", |ui| {
            ui.add(Number::new(&mut p.max_age_h, 1.0..=720.0).ladder(HOURS).unit(" h")).on_hover_text("Leaves out prices reported longer ago than this (up to 30 days).");
        });
        bar.field("Stock or demand at least", |ui| {
            let mut q = p.min_qty as f64;
            if ui.add(Number::new(&mut q, 1.0..=100_000.0).ladder(TONNES).unit(" t")).on_hover_text("Least stock where you buy, least demand where you sell. 1 lists every market.").changed() {
                p.min_qty = q as i64;
            }
        });
        bar.field("Include", |ui| {
            crate::widgets::chip_toggle(ui, &mut p.carriers, "Fleet carriers").on_hover_text("Fleet carriers' markets: their owners set the prices and who may dock, and they move.");
            crate::widgets::chip_toggle(ui, &mut p.planetary, "Planetary").on_hover_text("Ports and settlements on planets' surfaces.");
        });
        bar.field("Sort by", |ui| {
            crate::widgets::segmented(ui, &mut p.sort, &[(Sort::BestPrice, "Best price"), (Sort::Nearest, "Nearest"), (Sort::Freshest, "Newest")]);
        });
        if changed {
            bar.end(|ui| {
                if crate::widgets::icon_button(ui, icon::ARROW_COUNTER_CLOCKWISE, "Back to the default filters").clicked() {
                    (p.max_dist, p.min_pad, p.pad_auto, p.max_age_h, p.min_qty, p.carriers, p.planetary, p.sort) =
                        (d.max_dist, finder::ship_pad(gs), true, d.max_age_h, d.min_qty, d.carriers, d.planetary, d.sort);
                }
            });
        }
    });
}

fn pad_rank(p: Option<char>) -> u8 {
    match p {
        Some('L') => 3,
        Some('M') => 2,
        Some('S') => 1,
        _ => 0,
    }
}

/// How old a price is ("5m", "3h", "2d"), in a colour from fresh to stale.
pub(crate) fn age(updated_unix: i64) -> (String, egui::Color32) {
    let s = (nexus_journal::Timestamp::now().0 - updated_unix).max(0);
    let col = if s < 3600 { pal::GOOD } else if s < 6 * 3600 { pal::CYAN } else if s < 48 * 3600 { pal::WARN } else { pal::FAINT };
    (span(s), col)
}

/// A stretch of time in its largest whole unit: "40s", "5m", "3h", "2d".
fn span(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

fn cr(v: i64) -> String {
    format!("{} Cr", nexus_data::names::group_digits(v))
}

/// A table's headings: (label, sortable as (column, biggest or newest first)).
type Heads<'a> = Vec<(&'a str, Option<(u8, bool)>)>;

fn headings(h: &mut egui_extras::TableRow<'_, '_>, heads: &[(&str, Option<(u8, bool)>)], sort: &mut TableSort, table: &str) {
    for (t, col) in heads {
        h.col(|ui| match col {
            Some((c, desc)) => crate::widgets::th_sort(ui, t, *c, sort, table, *desc),
            None => crate::widgets::th(ui, t),
        });
    }
}

/// Text that leads somewhere when clicked: a pointing hand and an underline on hover.
fn nav_label(ui: &mut Ui, text: impl Into<RichText>, tip: &str) -> bool {
    let text: RichText = text.into();
    let r = ui.add(egui::Label::new(text).truncate().sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    if r.hovered() {
        let y = r.rect.bottom() - 1.0;
        ui.painter().hline(r.rect.x_range(), y, egui::Stroke::new(1.0, pal::DIM));
    }
    r.clicked()
}

/// Station name over its system (click the system to copy it), with a carrier or planet mark; a carrier
/// also shows who may dock (`access`, as last reported).
fn station_cell(ui: &mut Ui, station: &str, system: &str, carrier: bool, planetary: bool, dist_ls: Option<f64>, access: Option<&str>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let (name, tip) = if carrier {
            (format!("{} {station}", icon::ANCHOR), Some("Fleet carrier: its owner sets the prices and who may dock"))
        } else if planetary {
            (format!("{} {station}", icon::PLANET), Some("Planetary port or settlement"))
        } else {
            (station.to_string(), None)
        };
        let r = if carrier {
            named_with_access(ui, RichText::new(name).color(pal::TEXT), access, |ui, text| ui.add(egui::Label::new(text).truncate()))
        } else {
            ui.add(egui::Label::new(RichText::new(name).color(pal::TEXT)).truncate())
        };
        if let Some(t) = tip {
            r.on_hover_text(t);
        }
        let sys = match dist_ls {
            Some(ls) => format!("{system} · {ls:.0} ls"),
            None => system.to_string(),
        };
        if ui.add(egui::Label::new(RichText::new(sys).size(size::CAPTION).color(pal::DIM)).truncate().sense(egui::Sense::click())).on_hover_text("Click to copy the system").clicked() {
            ui.ctx().copy_text(system.to_string());
        }
    });
}

/// "−12 %" against the galactic average, green when it's the good side for the trade.
fn vs_average(ui: &mut Ui, price: i64, average: Option<i64>, buying: bool) {
    let Some(avg) = average.filter(|a| *a > 0) else { return };
    let pct = (price - avg) as f64 * 100.0 / avg as f64;
    let good = if buying { pct < 0.0 } else { pct > 0.0 };
    let col = if pct.abs() < 0.5 { pal::DIM } else if good { pal::GOOD } else { pal::WARN };
    let txt = if pct.abs() >= 1000.0 { format!("{:+.0}×", price as f64 / avg as f64) } else { format!("{pct:+.0}%") };
    ui.label(RichText::new(txt).color(col)).on_hover_text(format!("Galactic average {}", cr(avg)));
}

/// Returns true when the price-alert rules changed (settings to save).
pub fn show(ui: &mut Ui, gs: &GameState, svc: &mut MarketService, st: &mut MarketUi, rt: &tokio::runtime::Handle, watches: &mut Vec<PriceWatch>) -> bool {
    st.page.tick();
    st.detail.tick();
    st.tiles_live.tick();
    st.station_found.tick();
    st.routes.tick();
    st.stations.tick();
    st.hotspots.tick();
    st.hotspot_prices.tick();
    st.sell.tick();
    st.abn.tick();
    st.pins_live.tick();
    st.alert_ref.tick();
    st.plan.tick();
    // An answer is for the question asked last: no new one is asked while one runs.
    if st.here.tick() {
        st.here_for.clone_from(&st.here_key);
    }
    // The Station tab keeps the market you're docked at, and opens a market the moment you open it in the game.
    st.docked_market = gs.location.station.as_ref().filter(|s| gs.location.docked && s.market_id != 0).map(|s| s.market_id);
    if let Some(m) = gs.market.as_ref().filter(|m| m.market_id != 0)
        && st.prefs.seen_docked != (m.market_id, m.timestamp.0)
    {
        st.prefs.seen_docked = (m.market_id, m.timestamp.0);
        st.open_tile(m.market_id, true);
        // The database stores the game's copy a moment after the game writes it: read the tiles again then too.
        st.tiles_again = Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
    }

    if let Some(t) = crate::widgets::tabs(
        ui,
        st.tab,
        &[
            (Tab::Commodity, "Find commodity"),
            (Tab::Abnormal, "Abnormal prices"),
            (Tab::Plan, "Run planner"),
            (Tab::Sell, "Sell my cargo"),
            (Tab::Routes, "Trade routes"),
            (Tab::Services, "Services"),
            (Tab::Hotspots, "Mining hotspots"),
            (Tab::Alerts, "Price alerts"),
            (Tab::Docked, "Station"),
            (Tab::Database, "Database & feed"),
        ],
    ) {
        st.set_tab(t);
    }
    ui.add_space(12.0);
    // The live feed's state sits in a slim bar along the bottom; the tab gets everything above it.
    let full = ui.available_rect_before_wrap();
    let bar_rect = egui::Rect::from_min_max(egui::pos2(full.left(), full.bottom() - STATUS_BAR_H), full.max);
    let body_rect = egui::Rect::from_min_max(full.min, egui::pos2(full.right(), bar_rect.top() - 6.0));
    ui.advance_cursor_after_rect(full);
    if status_bar(ui, bar_rect, svc) {
        st.set_tab(Tab::Database);
    }
    let mut body = ui.new_child(egui::UiBuilder::new().id_salt("market-body").max_rect(body_rect).layout(egui::Layout::top_down(egui::Align::Min)));
    // Nothing the tab draws runs over the bar.
    body.set_clip_rect(body_rect.intersect(body.clip_rect()));
    let ui = &mut body;
    let Some(db) = svc.db.clone() else {
        empty(ui, icon::DATABASE, "Market database unavailable", svc.db_error.as_deref().unwrap_or("Starting…"));
        return false;
    };
    let origin = gs.location.star_pos;
    let ctx = ui.ctx().clone();
    let mut changed = false;
    // How many stations the database knows, for what Services says about its reach.
    let known_stations = svc.db_stats().map(|s| s.stations);
    refresh_pins(&ctx, gs, &db, st);
    let mut nav = None;
    if !st.prefs.pins.is_empty() && !matches!(st.tab, Tab::Database | Tab::Alerts) {
        nav = pins_panel(ui, gs, st);
    }
    match st.tab {
        Tab::Commodity => nav = commodity_tab(ui, gs, &db, st).or(nav),
        Tab::Plan => nav = plan_tab(ui, gs, &db, st).or(nav),
        Tab::Routes => {
            let cargo = gs.fleet.current().map(|s| s.cargo_capacity as i64).unwrap_or(100);
            // Docked, routes can start at this station: buy here, sell elsewhere.
            let docked = gs.location.station.as_ref().filter(|s| gs.location.docked && s.market_id != 0);
            if docked.is_some() {
                if crate::widgets::segmented(ui, &mut st.prefs.routes_here, &[(false, "Around you"), (true, "From this station")]) {
                    // The other kind's results would read as this kind's.
                    st.routes = Slot::default();
                }
                ui.add_space(6.0);
            }
            let from = docked.filter(|_| st.prefs.routes_here);
            let mut find = std::mem::take(&mut st.autorun);
            let p = &mut st.prefs;
            crate::widgets::filter_bar(ui, "routes-filters", |bar| {
                match from {
                    Some(s) => {
                        bar.field(&format!("Buy at {}, sell within", s.name), |ui| {
                            ui.add(Number::new(&mut p.hop, 1.0..=500.0).ladder(LY).unit(" ly")).on_hover_text("Up to 500 ly. Every market within reach is loaded, so far reaches take a few seconds.");
                        });
                    }
                    None => {
                        bar.field("Routes within", |ui| {
                            ui.add(Number::new(&mut p.radius, 5.0..=1000.0).ladder(LY).unit(" ly"))
                                .on_hover_text("How far from you the routes may start and end, up to 1,000 ly. Every market in the area is loaded, so wide areas take a few seconds.");
                        });
                        bar.field("Longest hop", |ui| {
                            ui.add(Number::new(&mut p.hop, 1.0..=500.0).ladder(LY).unit(" ly")).on_hover_text("The longest trip between buying and selling, up to 500 ly.");
                        });
                    }
                }
                bar.field_tip("Landing pad", PAD_TIP, |ui| {
                    if pad_picker(ui, &mut p.min_pad) {
                        p.pad_auto = false;
                    }
                });
                bar.field("Include", |ui| {
                    crate::widgets::chip_toggle(ui, &mut p.carriers, "Fleet carriers").on_hover_text("Fleet carriers' markets: their owners set the prices and who may dock, and they move.");
                });
                bar.field_tip("For your ship", "Each route fills this hold, with what these credits buy.", |ui| {
                    ui.label(RichText::new(format!("{cargo} t · {}", short_credits(gs.cmdr.credits))).font(display(size::CALLOUT)).color(pal::TEXT));
                });
                bar.end(|ui| find |= crate::widgets::primary_button(ui, "Find routes").clicked());
            });
            ui.add_space(6.0);
            if find && let Some(o) = origin {
                let p = &st.prefs;
                let q = TradeQuery {
                    origin: o,
                    // From a station, the hop is how far from here a route sells.
                    radius_ly: if from.is_some() { p.hop } else { p.radius },
                    max_hop_ly: p.hop,
                    min_pad: p.min_pad,
                    cargo_capacity: cargo.max(1),
                    capital: Some(gs.cmdr.credits.max(0)),
                    max_age_hours: Some(p.max_age_h),
                    include_carriers: p.carriers,
                    include_planetary: p.planetary,
                    from_market: from.map(|s| s.market_id),
                    ..Default::default()
                };
                st.routes_from = from.map(|s| s.name.clone());
                let db = db.clone();
                st.routes.run(&ctx, move || db.trade_routes(&q).map_err(|e| e.to_string()));
            }
            // The game writes a station's prices (Market.json) when its commodity market is opened.
            if from.is_some_and(|s| gs.market.as_ref().is_none_or(|m| m.market_id != s.market_id)) {
                crate::widgets::footnote(ui, "Open the commodity market in the game for this station's latest prices. Until then Elite Nexus uses what its database holds for it, if anything.");
            }
            if st.routes.last.is_none() && !st.routes.busy() {
                let what = if from.is_some() { "how far to sell" } else { "the area" };
                crate::widgets::footnote(ui, &format!("Set {what} and press Find routes. Every market in the area is loaded, so wide areas take a few seconds: this search waits for you."));
            }
            nav = results_routes(ui, &st.routes, st.routes_from.as_deref()).or(nav);
        }
        Tab::Services => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for (i, (label, _)) in SERVICES.iter().enumerate() {
                    if crate::widgets::pill(ui, st.service == i, label).clicked() && st.service != i {
                        st.service = i;
                        st.services_limit = SERVICES_STEP;
                    }
                }
            });
            ui.add_space(8.0);
            let p = &mut st.prefs;
            crate::widgets::filter_bar(ui, "services-filters", |bar| {
                bar.field_tip("Landing pad", PAD_TIP, |ui| {
                    // A pad picked here stays: Find commodity no longer sets it from the ship.
                    if pad_picker(ui, &mut p.min_pad) {
                        p.pad_auto = false;
                    }
                });
                bar.field("Include", |ui| {
                    crate::widgets::chip_toggle(ui, &mut p.services_carriers, "Fleet carriers")
                        .on_hover_text("Fleet carriers move, and their owners switch services on and off, so they're left out unless you include them.");
                    crate::widgets::chip_toggle(ui, &mut p.services_planetary, "Planetary").on_hover_text("Ports and settlements on planets' surfaces.");
                });
            });
            ui.add_space(6.0);
            // Searches by itself: when the tab opens, when a service or filter is picked, and after every jump.
            match origin {
                None => empty(ui, icon::MAP_PIN, "Position unknown", "Jump or relog once so Elite Nexus knows where you are."),
                Some(o) => {
                    let p = &st.prefs;
                    let key = (st.service, gs.location.system_address, p.min_pad, p.services_carriers, p.services_planetary, st.services_limit);
                    if st.services_key != Some(key) && !st.stations.busy() {
                        st.services_key = Some(key);
                        let q = StationQuery { include_carriers: p.services_carriers, include_planetary: p.services_planetary, limit: st.services_limit, ..service_query(SERVICES[st.service].1, o, p.min_pad) };
                        let db = db.clone();
                        st.stations.run(&ctx, move || db.stations_near(&q).map_err(|e| e.to_string()));
                    }
                    let name = SERVICES[st.service].0;
                    services_reach(ui, name, known_stations, st.services_limit);
                    if !st.stations.busy() && matches!(&st.stations.last, Some(Ok(v)) if v.is_empty()) {
                        let p = &st.prefs;
                        let pad = p.min_pad.filter(|c| *c != 'S').map(|c| format!(" with an {c} pad")).unwrap_or_default();
                        let mut out = Vec::new();
                        if !p.services_carriers {
                            out.push("fleet carriers");
                        }
                        if !p.services_planetary {
                            out.push("planetary ports");
                        }
                        let left_out = if out.is_empty() { String::new() } else { format!(" ({} left out)", out.join(" and ")) };
                        let hint = format!(
                            "No station Elite Nexus knows has {}{pad}, at any distance{left_out}. It learns stations from the live feed, your journal and the Spansh import (Database & feed).",
                            with_article(name)
                        );
                        empty(ui, icon::MAGNIFYING_GLASS, "None found", &hint);
                    } else if services_summary(ui, &st.stations, st.services_limit) {
                        st.services_limit += SERVICES_STEP;
                    }
                    results_stations(ui, &st.stations);
                }
            }
        }
        Tab::Hotspots => {
            // Opened from a "where to get it" panel: search at once.
            let mut find = std::mem::take(&mut st.autorun);
            let p = &mut st.prefs;
            crate::widgets::filter_bar(ui, "hotspot-filters", |bar| {
                bar.field("Hotspots of", |ui| {
                    egui::ComboBox::from_id_salt("hsig").selected_text(p.hotspot_signal.clone()).width(200.0).show_ui(ui, |ui| {
                        for s in HOTSPOT_SIGNALS {
                            ui.selectable_value(&mut p.hotspot_signal, (*s).to_string(), *s);
                        }
                    });
                });
                bar.field("In one ring, at least", |ui| {
                    let mut n = p.hotspot_min as f64;
                    if ui.add(Number::new(&mut n, 1.0..=6.0)).on_hover_text("Hotspots of this commodity in the same ring: overlapping hotspots yield more.").changed() {
                        p.hotspot_min = n as i64;
                    }
                });
                bar.field("Within", |ui| {
                    ui.add(Number::new(&mut p.max_dist, 1.0..=MAX_SEARCH_LY).ladder(LY).unit(" ly")).on_hover_text("Every ring Elite Nexus knows, up to the whole galaxy.");
                });
                bar.end(|ui| find |= crate::widgets::primary_button(ui, "Find hotspots").clicked());
            });
            if find && let Some(o) = origin {
                let p = &st.prefs;
                let q = HotspotQuery { origin: o, signal: p.hotspot_signal.clone(), max_distance_ly: Some(p.max_dist), min_count: p.hotspot_min, limit: 50 };
                let db2 = db.clone();
                st.hotspots.run(&ctx, move || db2.hotspots_near(&q).map_err(|e| e.to_string()));
                let pq = CommodityQuery {
                    commodity: p.hotspot_signal.clone(),
                    mode: TradeMode::SellToStation,
                    origin: Some(o),
                    max_distance_ly: Some(p.max_dist * 2.0),
                    max_age_hours: Some(72.0),
                    limit: 10,
                    ..Default::default()
                };
                let db3 = db.clone();
                st.hotspot_prices.run(&ctx, move || db3.search_commodity(&pq).map_err(|e| e.to_string()));
            }
            ui.add_space(4.0);
            crate::widgets::footnote(ui, "Hotspots come from the ring scans (DSS) commanders share over the live feed, and your own. For galaxy-wide searches use Navigation › Spansh.");
            ui.add_space(8.0);
            let signal = st.prefs.hotspot_signal.clone();
            crate::widgets::split(
                ui,
                0.5,
                360.0,
                |ui| results_hotspots(ui, &st.hotspots),
                |ui| {
                    card(ui, "Best places to sell nearby", |ui| {
                        if st.hotspot_prices.last.is_none() && !st.hotspot_prices.busy() {
                            ui.label(RichText::new("Where it sells best within twice that distance, with prices up to 72 h old.").size(size::FOOTNOTE).color(pal::DIM));
                        }
                        if !status(ui, &st.hotspot_prices) {
                            return;
                        }
                        if let Some(Ok(hits)) = &st.hotspot_prices.last {
                            if hits.is_empty() {
                                ui.label(RichText::new("No buyer in range with recent prices.").color(pal::DIM));
                            } else if hits_table(ui, "hotspot-sell", hits, TradeMode::SellToStation, None, None, None).is_some() {
                                nav = Some(Nav::Commodity { name: signal.clone(), side: Some(Side::Sell) });
                            }
                        }
                    });
                },
            );
        }
        Tab::Abnormal => nav = abnormal_tab(ui, gs, &db, st).or(nav),
        // Card-style tabs scroll as a whole (the page itself does not scroll: tables scroll on their own).
        Tab::Sell => {
            egui::ScrollArea::vertical().id_salt("msell").auto_shrink([false, false]).show(ui, |ui| nav = sell_tab(ui, gs, &db, st).or(nav.take()));
        }
        Tab::Alerts => {
            egui::ScrollArea::vertical().id_salt("malerts").auto_shrink([false, false]).show(ui, |ui| changed |= alerts_tab(ui, gs, svc, &db, st, watches));
        }
        Tab::Docked => nav = station_tab(ui, gs, &db, st).or(nav),
        Tab::Database => {
            egui::ScrollArea::vertical().id_salt("mdb").auto_shrink([false, false]).show(ui, |ui| database_tab(ui, svc, st, rt));
        }
    }
    if let Some(n) = nav {
        st.go(&db, n);
    }
    changed
}

/// Find commodity: a search with suggestions, then the commodity's page: galactic average, how many
/// markets trade it, where to buy, where to sell, the latest prices, and a picked market's history.
fn commodity_tab(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) -> Option<Nav> {
    // Smart default: the landing pad the current ship needs (until the commander picks one).
    if st.prefs.pad_auto {
        st.prefs.min_pad = finder::ship_pad(gs);
    }
    if st.commodities.is_empty() {
        st.commodities = db.commodities();
    }
    let origin = gs.location.star_pos;
    // What to open, and which rare goods "Rare Goods" means there (a category's, or "" for all).
    let mut look_up: Option<(String, String)> = None;
    // Another category, or the blank item: no page until an item is picked.
    let mut close = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        let (all, recent) = (&st.commodities, &st.prefs.recent);
        // The category narrows what the search field lists; "Rare Goods" holds rare goods taken as one.
        let c = crate::widgets::combobox(ui, "commodity-category", &mut st.category_input, ALL_CATEGORIES, 180.0, |t| category_suggestions(all, t));
        let before = st.prefs.category.clone();
        if let Some(p) = c.picked {
            st.prefs.category = if p == ALL_CATEGORIES { String::new() } else { p };
        } else if c.submitted {
            let typed = st.category_input.trim();
            st.prefs.category = if typed.is_empty() { String::new() } else { category_suggestions(all, typed).into_iter().map(|s| s.text).next().unwrap_or_default() };
        }
        // Another category: its items are picked afresh, and the page below waits for one.
        if st.prefs.category != before {
            st.input.clear();
            close = true;
        }
        // Not being typed in: it shows the category in use (an open page sets it too).
        if !c.focused && st.category_input != st.prefs.category {
            st.category_input.clone_from(&st.prefs.category);
        }
        let category = st.prefs.category.as_str();
        let c = crate::widgets::combobox(ui, "commodity-search", &mut st.input, "Commodity: Tritium, LTD, Thargoid…", 320.0, |t| item_suggestions(all, recent, category, t));
        // Rare Goods from the category's list (▾) are that category's; matched to typed words, all of them; searched
        // again, the ones open.
        let open_rares = st.prefs.rare_goods.clone().unwrap_or_default();
        if c.picked.as_deref() == Some("") {
            close = true;
        } else if let Some(p) = c.picked {
            let rares = if !c.whole_list || category == RARE_GOODS { String::new() } else { category.to_string() };
            look_up = Some((p, rares));
        } else if c.submitted {
            look_up = Some((st.input.clone(), open_rares.clone()));
        }
        if crate::widgets::primary_button(ui, "Search").clicked() {
            look_up = Some((st.input.clone(), open_rares));
        }
        if st.page.busy() && st.page.last.is_some() {
            ui.spinner();
        }
    });
    if close {
        st.close_page();
    }
    if let Some((t, rares)) = look_up
        && !t.trim().is_empty()
    {
        st.open_item(db, &t, rares);
    }
    ui.add_space(2.0);
    search_filters(ui, &mut st.prefs, gs);
    if let Some(t) = &st.not_found {
        ui.add_space(6.0);
        ui.label(RichText::new(format!("{} No commodity called \"{t}\". Pick one from the suggestions as you type.", icon::WARNING)).color(pal::WARN));
    }
    ui.add_space(8.0);

    // (Re)load whenever the commodity, a filter or the position changes; one query at a time.
    let p = &st.prefs;
    if p.commodity.trim().is_empty() && p.rare_goods.is_none() {
        empty(ui, icon::MAGNIFYING_GLASS, "Find the best price for anything", "Type a commodity: where to buy it, where to sell it, and the latest prices, nearest or best first.");
        return None;
    }
    let q = CommodityQuery {
        commodity: p.commodity.clone(),
        mode: TradeMode::BuyFromStation,
        origin,
        max_distance_ly: origin.map(|_| p.max_dist),
        min_pad: p.min_pad,
        max_age_hours: Some(p.max_age_h),
        min_quantity: p.min_qty,
        include_carriers: p.carriers,
        include_planetary: p.planetary,
        limit: PAGE_LIMIT,
        sort: p.sort,
    };
    let key = (p.rare_goods.clone(), q);
    if st.page_key.as_ref() != Some(&key) && !st.page.busy() {
        st.page_key = Some(key.clone());
        let db = db.clone();
        st.page.run(ui.ctx(), move || load_page(&db, key.0.as_deref(), &key.1).map_err(|e| e.to_string()));
    }
    let page = match &st.page.last {
        None => {
            ui.spinner();
            return None;
        }
        Some(Err(e)) => {
            ui.label(RichText::new(e).color(pal::BAD));
            return None;
        }
        Some(Ok(None)) => {
            empty(ui, icon::QUESTION, "Unknown commodity", "The database has no commodity by that name. Pick one from the suggestions as you type.");
            return None;
        }
        Some(Ok(Some(page))) => page,
    };
    // A new commodity whose saved list is empty opens on a list that has something in range, or else
    // on the side that exists somewhere (salvage is only ever bought: show where, and how far).
    if st.fresh && !st.page.busy() {
        st.fresh = false;
        let s = &page.summary;
        let n = |x: Side| match x {
            Side::Buy => page.buy.len(),
            Side::Sell => page.sell.len(),
            Side::Latest => page.latest.len(),
        };
        let anywhere = |x: Side| match x {
            Side::Buy => s.sellers,
            Side::Sell => s.buyers,
            Side::Latest => s.sellers + s.buyers,
        };
        let sides = [Side::Buy, Side::Sell];
        if n(st.side) == 0
            && let Some(x) = sides.into_iter().find(|x| n(*x) > 0).or_else(|| sides.into_iter().find(|x| anywhere(st.side) == 0 && anywhere(*x) > 0))
        {
            st.side = x;
        }
    }
    let mut nav = None;
    commodity_header(ui, page, &st.prefs, origin.is_some());
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        let count = |n: usize| if n >= PAGE_LIMIT { format!("{PAGE_LIMIT}+") } else { n.to_string() };
        let labels = [format!("Where to buy  {}", count(page.buy.len())), format!("Where to sell  {}", count(page.sell.len())), format!("Latest prices  {}", count(page.latest.len()))];
        if crate::widgets::segmented(ui, &mut st.side, &[(Side::Buy, &labels[0]), (Side::Sell, &labels[1]), (Side::Latest, &labels[2])]) {
            st.prefs.side = st.side;
        }
        if let Some(t) = st.page.took {
            ui.label(RichText::new(format!("{:.0} ms", t.as_secs_f64() * 1e3)).small().color(pal::FAINT));
        }
    });
    ui.add_space(6.0);

    // The picked row: its market's details and its commodity's price history there.
    if let Some((id, symbol)) = st.pick.clone() {
        let key = (id, symbol.clone());
        if st.detail_key.as_ref() != Some(&key) && !st.detail.busy() {
            st.detail_key = Some(key.clone());
            let db = db.clone();
            st.detail.run(ui.ctx(), move || {
                let market = db.station_market(key.0).map_err(|e| e.to_string())?;
                let history = db.price_history(key.0, &key.1).map_err(|e| e.to_string())?;
                let since = db.price_since(key.0, &key.1).map_err(|e| e.to_string())?;
                Ok(PickDetail { market, history, since })
            });
        }
        let current = st.detail_key.as_ref().is_some_and(|k| k.0 == id) && !st.detail.busy();
        match (&st.detail.last, current) {
            (Some(Ok(d)), true) => match pick_card(ui, d, (&symbol, &row_name(page, &st.commodities, &symbol)), origin, &st.prefs, st.side == Side::Sell) {
                PickAction::None => {}
                PickAction::Pin(p) => st.prefs.toggle_pin(p),
                PickAction::Close => st.pick = None,
                PickAction::Open(id) => nav = Some(Nav::Station(id)),
            },
            (Some(Err(e)), true) => {
                ui.label(RichText::new(e).color(pal::BAD));
            }
            _ => {
                ui.spinner();
            }
        }
        ui.add_space(6.0);
    }

    let good = (page.summary.symbol.as_str(), page.summary.name.as_str());
    let action = match st.side {
        Side::Buy | Side::Sell => {
            let (hits, mode) = if st.side == Side::Buy { (&page.buy, TradeMode::BuyFromStation) } else { (&page.sell, TradeMode::SellToStation) };
            if hits.is_empty() {
                if let Some(r) = empty_side(ui, page, st.side, &st.prefs, origin.is_some()) {
                    r.apply(&mut st.prefs);
                }
                None
            } else {
                hits_table(ui, "commodity-hits", hits, mode, page.summary.average, st.pick.as_ref(), Some((good, &st.prefs, page.summary.is_rare)))
            }
        }
        Side::Latest => {
            if page.latest.is_empty() {
                if let Some(r) = empty_side(ui, page, Side::Latest, &st.prefs, origin.is_some()) {
                    r.apply(&mut st.prefs);
                }
                None
            } else {
                latest_table(ui, &page.latest, page.summary.average, st.pick.as_ref(), good, &st.prefs, page.summary.is_rare)
            }
        }
    };
    match action {
        Some(RowAction::Select(k)) => st.pick = if st.pick.as_ref() == Some(&k) { None } else { Some(k) },
        Some(RowAction::Pin(p)) => st.prefs.toggle_pin(p),
        Some(RowAction::Open(name)) => {
            st.open_commodity(db, &name);
        }
        None => {}
    }
    nav
}

/// The name of a picked row's commodity: the page's, or (rare goods taken as one) the row's own.
fn row_name(page: &CommodityPage, all: &[CommodityInfo], symbol: &str) -> String {
    if page.summary.symbol == symbol {
        return page.summary.name.clone();
    }
    all.iter().find(|c| c.symbol == symbol).map_or_else(|| symbol.to_string(), |c| c.name.clone())
}

/// Loads a commodity's page, or rare goods taken as one (`rares`: one category's, or "" for all), on a
/// worker thread. `Ok(None)`: no such commodity.
fn load_page(db: &GalaxyDb, rares: Option<&str>, q: &CommodityQuery) -> nexus_galaxy::Result<Option<CommodityPage>> {
    // Some(None): every rare good.
    let rares = rares.map(|c| (!c.is_empty()).then_some(c));
    let summary = match rares {
        Some(category) => db.rare_goods_summary(category, q.max_age_hours)?.map(|s| CommoditySummary { name: RARE_GOODS.to_string(), ..s }),
        None => db.commodity_summary(&q.commodity, q.max_age_hours)?,
    };
    let Some(summary) = summary else {
        return Ok(None);
    };
    let search = |q: &CommodityQuery| match rares {
        Some(category) => db.search_rare_goods(category, q),
        None => db.search_commodity(q),
    };
    let buy = search(&CommodityQuery { mode: TradeMode::BuyFromStation, ..q.clone() })?;
    let sell = search(&CommodityQuery { mode: TradeMode::SellToStation, ..q.clone() })?;
    let latest = match rares {
        Some(category) => db.latest_rare_prices(category, q)?,
        None => db.latest_prices(q)?,
    };
    let nearest = |mode| CommodityQuery {
        mode,
        max_distance_ly: None,
        min_pad: None,
        min_quantity: 1,
        include_carriers: true,
        include_planetary: true,
        sort: Sort::Nearest,
        limit: 1,
        ..q.clone()
    };
    let hint = |mode, found: bool, any: u32| -> nexus_galaxy::Result<Option<CommodityHit>> {
        if found || any == 0 { Ok(None) } else { Ok(search(&nearest(mode))?.into_iter().next()) }
    };
    let buy_hint = hint(TradeMode::BuyFromStation, !buy.is_empty(), summary.sellers)?;
    let sell_hint = hint(TradeMode::SellToStation, !sell.is_empty(), summary.buyers)?;
    Ok(Some(CommodityPage { summary, buy, sell, latest, buy_hint, sell_hint }))
}

fn commodity_header(ui: &mut Ui, page: &CommodityPage, p: &MarketPrefs, have_origin: bool) {
    let s = &page.summary;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&s.name).font(display(size::TITLE)).color(pal::TEXT));
        if !s.category.is_empty() {
            badge(ui, &s.category, pal::GRAY);
        }
        if s.is_rare {
            // Yellow with a ★, as the game and the lists mark rare goods.
            badge(ui, "★ Rare", pal::GOLD).on_hover_text("Rare goods are sold only at their home station, in small allocations, and are worth more the farther you carry them.");
        }
    });
    ui.add_space(6.0);
    let range = if have_origin { format!(" within {:.0} ly", p.max_dist) } else { String::new() };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(32.0, 8.0);
        let avg_tip = if s.symbol.is_empty() {
            "Rare goods taken as one: the mean of their galactic averages, each good counted once. Each rare good's own price differs widely: each row compares with its own good's average."
        } else {
            "The average price the game reports for this commodity (the one most stations quote). Fleet carriers don't report one."
        };
        crate::widgets::stat(ui, "Galactic average", s.average.map_or_else(|| "—".into(), cr), pal::TEXT).on_hover_text(avg_tip);
        let cheapest = page.buy.iter().map(|h| h.price).min();
        crate::widgets::stat(ui, &format!("Cheapest{range}"), cheapest.map_or_else(|| "—".into(), cr), pal::CYAN);
        let best = page.sell.iter().map(|h| h.price).max();
        crate::widgets::stat(ui, &format!("Best sale{range}"), best.map_or_else(|| "—".into(), cr), pal::GOOD);
        crate::widgets::stat(ui, "Markets selling · buying", format!("{} · {}", s.sellers, s.buyers), pal::TEXT).on_hover_text(format!(
            "Across the whole database, prices up to {:.0} h old: {} markets sell it ({} of them fleet carriers), {} buy it ({} carriers).",
            p.max_age_h, s.sellers, s.carrier_sellers, s.buyers, s.carrier_buyers
        ));
    });
    let mut facts = Vec::new();
    if let Some(v) = s.lowest_buy {
        facts.push(format!("cheapest anywhere {}", cr(v)));
    }
    if let Some(v) = s.highest_sell {
        facts.push(format!("best price anywhere {}", cr(v)));
    }
    if let Some(t) = s.newest_unix {
        facts.push(format!("newest price {} ago", age(t).0));
    }
    if !facts.is_empty() {
        ui.add_space(2.0);
        ui.label(RichText::new(format!("Whole database: {}. Click a row for that market's details and price history.", facts.join(" · "))).size(size::FOOTNOTE).color(pal::DIM));
    }
}

/// Filters loosened so that a market outside them shows up.
#[derive(Default)]
struct Relax {
    dist: Option<f64>,
    qty: Option<i64>,
    pad: bool,
    carriers: bool,
    planetary: bool,
}

impl Relax {
    fn for_hit(h: &CommodityHit, p: &MarketPrefs, have_origin: bool) -> Relax {
        Relax {
            dist: h.distance_ly.filter(|d| have_origin && *d > p.max_dist),
            qty: (h.quantity < p.min_qty).then_some(h.quantity),
            // S leaves nothing out (every station has one), as in the database's searches.
            pad: p.min_pad.is_some_and(|m| m != 'S') && pad_rank(h.max_pad) < pad_rank(p.min_pad),
            carriers: h.is_carrier && !p.carriers,
            planetary: h.is_planetary && !p.planetary,
        }
    }

    fn reasons(&self, p: &MarketPrefs) -> Vec<String> {
        let mut r = Vec::new();
        if self.dist.is_some() {
            r.push(format!("farther than {:.0} ly", p.max_dist));
        }
        if self.qty.is_some() {
            r.push(format!("fewer than {} t", p.min_qty));
        }
        if self.pad {
            r.push(format!("no {} pad", p.min_pad.unwrap_or('L')));
        }
        if self.carriers {
            r.push("fleet carrier (switched off)".into());
        }
        if self.planetary {
            r.push("planetary (switched off)".into());
        }
        r
    }

    fn apply(&self, p: &mut MarketPrefs) {
        if let Some(d) = self.dist {
            p.max_dist = ((d / 10.0).ceil() * 10.0).clamp(p.max_dist, MAX_SEARCH_LY);
        }
        if let Some(q) = self.qty {
            p.min_qty = q.max(1);
        }
        if self.pad {
            p.min_pad = None;
            p.pad_auto = false;
        }
        p.carriers |= self.carriers;
        p.planetary |= self.planetary;
    }
}

/// An empty list explained: what the database knows, the nearest market outside the filters, and a
/// button that widens them. Returns the loosened filters when clicked.
fn empty_side(ui: &mut Ui, page: &CommodityPage, side: Side, p: &MarketPrefs, have_origin: bool) -> Option<Relax> {
    let s = &page.summary;
    let (title, any, hint, verb) = match side {
        Side::Buy => (format!("Nobody in range sells {}", s.name), s.sellers, page.buy_hint.as_ref(), "sells"),
        Side::Sell => (format!("Nobody in range buys {}", s.name), s.buyers, page.sell_hint.as_ref(), "buys"),
        Side::Latest => (format!("No market in range trades {}", s.name), s.sellers + s.buyers, page.buy_hint.as_ref().or(page.sell_hint.as_ref()), "trades"),
    };
    let mut relax = None;
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(560.0);
        ui.label(RichText::new(title).font(display(size::TITLE3)).color(pal::TEXT));
        ui.add_space(4.0);
        let mut lines: Vec<String> = Vec::new();
        if any == 0 {
            let did = match side {
                Side::Buy => "sold",
                Side::Sell => "bought",
                Side::Latest => "traded",
            };
            lines.push(format!("No market in the database has {did} it in the last {:.0} h.", p.max_age_h));
            if side != Side::Sell && s.category.eq_ignore_ascii_case("Salvage") {
                lines.push("Salvage isn't sold at markets: it's found in space, at signal sources and wreck sites, or earned from missions.".into());
            } else if side != Side::Sell && s.is_rare {
                lines.push("Rare goods are sold only at their home station, in small allocations.".into());
            }
            lines.push("The database holds the markets commanders visit while the live feed runs, the ones you open yourself, and the Spansh import if you ran it (Database & feed).".into());
        }
        for l in &lines {
            ui.add(egui::Label::new(RichText::new(l).size(size::CALLOUT).color(pal::DIM)).wrap().halign(egui::Align::Center));
        }
        if let Some(h) = hint {
            let r = Relax::for_hit(h, p, have_origin);
            let qty = if h.quantity > 0 { format!(", {} {}", nexus_data::names::group_digits(h.quantity), if side == Side::Sell { "wanted" } else { "in stock" }) } else { String::new() };
            let dist = h.distance_ly.map(|d| format!(", {d:.0} ly away")).unwrap_or_default();
            let kind = if h.is_carrier { " (fleet carrier)" } else { "" };
            ui.add(
                egui::Label::new(
                    RichText::new(format!("The nearest market that {verb} it: {}{kind} in {}{dist}, {}{qty}, {} ago.", h.station, h.system, cr(h.price), age(h.updated_unix).0))
                        .size(size::CALLOUT)
                        .color(pal::TEXT),
                )
                .wrap()
                .halign(egui::Align::Center),
            );
            let why = r.reasons(p);
            if !why.is_empty() {
                ui.label(RichText::new(format!("Left out because: {}.", why.join(", "))).size(size::FOOTNOTE).color(pal::DIM));
                ui.add_space(6.0);
                if crate::widgets::primary_button(ui, "Widen the filters to include it").clicked() {
                    relax = Some(r);
                }
            }
        }
    });
    relax
}

/// A commodity's (symbol, name) with the pins to compare against, and whether it's a rare good.
type PinCtx<'a> = ((&'a str, &'a str), &'a MarketPrefs, bool);

/// A list row's commodity: rare goods behind a yellow ★. On a rare goods page (`own_row`: the row names a commodity
/// other than the page's) the name opens that commodity's own page.
fn commodity_cell(ui: &mut Ui, name: &str, own_row: bool, page_rare: bool) -> Option<RowAction> {
    let text = if own_row || page_rare { RichText::new(format!("★ {name}")).color(pal::GOLD) } else { RichText::new(name).color(pal::TEXT) };
    if own_row {
        nav_label(ui, text, "Its own page: where to buy it, where to sell it, latest prices").then(|| RowAction::Open(name.to_string()))
    } else {
        ui.add(egui::Label::new(text).truncate());
        None
    }
}

/// Buy or sell offers for one commodity: price (and against the galactic average), quantity, where,
/// how far, how old; with `pins`, a pin toggle in front. Returns what a click asked for.
fn hits_table(ui: &mut Ui, id: &str, hits: &[CommodityHit], mode: TradeMode, average: Option<i64>, selected: Option<&PickKey>, pins: Option<PinCtx>) -> Option<RowAction> {
    let buying = mode == TradeMode::BuyFromStation;
    let wide = ui.available_width() >= 700.0;
    // On the commodity page each row names its commodity, as Abnormal prices does.
    let many = pins.is_some();
    let mut action = None;
    let mut sort = TableSort::load(ui, id);
    let layout = format!("{id}/{}", if wide { "wide" } else { "narrow" });
    crate::widgets::table(ui, &layout, |mut table| {
        table = table.sense(egui::Sense::click());
        if pins.is_some() {
            table = table.column(Column::exact(24.0).resizable(false));
        }
        if many {
            table = table.column(Column::initial(180.0).at_least(80.0).clip(true));
        }
        table = table
            .column(Column::initial(118.0).at_least(70.0).clip(true))
            .column(Column::initial(60.0).at_least(40.0).clip(true))
            .column(Column::initial(76.0).at_least(40.0).clip(true))
            .column(crate::widgets::flex_column(150.0))
            .column(Column::initial(72.0).at_least(50.0).clip(true));
        if wide {
            table = table.column(Column::initial(36.0).at_least(28.0).clip(true));
        }
        table = table.column(Column::initial(60.0).at_least(40.0).clip(true));
        // Sortable columns: 0 price, 1 quantity, 2 station, 3 distance, 4 pad, 5 age, 6 commodity. Cheapest
        // first where you buy, best price first where you sell.
        let mut heads: Heads = Vec::new();
        if pins.is_some() {
            heads.push(("", None));
        }
        if many {
            heads.push(("Commodity", Some((6, false))));
        }
        heads.extend([
            (if buying { "Buy for" } else { "Sell for" }, Some((0, !buying))),
            ("vs avg", Some((0, !buying))),
            (if buying { "Stock" } else { "Demand" }, Some((1, true))),
            ("Station", Some((2, false))),
            ("Distance", Some((3, false))),
        ]);
        if wide {
            heads.push(("Pad", Some((4, true))));
        }
        heads.push(("Updated", Some((5, true))));
        let table = table.header(28.0, |mut h| headings(&mut h, &heads, &mut sort, id));
        let order = sort.order(hits, |a, b, c| match c {
            0 => a.price.cmp(&b.price),
            1 => a.quantity.cmp(&b.quantity),
            2 => cmp_text(&a.station, &b.station),
            3 => cmp_opt(a.distance_ly, b.distance_ly),
            4 => pad_rank(a.max_pad).cmp(&pad_rank(b.max_pad)),
            6 => cmp_text(a.commodity.as_ref().map_or("", |c| c.name.as_str()), b.commodity.as_ref().map_or("", |c| c.name.as_str())),
            _ => a.updated_unix.cmp(&b.updated_unix),
        });
        table
            .body(|body| {
                body.rows(40.0, hits.len(), |mut row| {
                    let h = &hits[order[row.index()]];
                    // The page's commodity, or (rare goods taken as one) the row's own.
                    let page_good = pins.map_or(("", ""), |p| p.0);
                    let page_rare = pins.is_some_and(|p| p.2);
                    let (symbol, name) = h.commodity.as_ref().map_or(page_good, |c| (c.symbol.as_str(), c.name.as_str()));
                    row.set_selected(selected.is_some_and(|k| k.0 == h.market_id && k.1 == symbol));
                    if let Some((_, prefs, _)) = pins {
                        row.col(|ui| {
                            if pin_button(ui, prefs.is_pinned(h.market_id, symbol, !buying)) {
                                action = Some(RowAction::Pin(Pin {
                                    market_id: h.market_id,
                                    station: h.station.clone(),
                                    system: h.system.clone(),
                                    carrier: h.is_carrier,
                                    symbol: symbol.to_string(),
                                    name: name.to_string(),
                                    sell_here: !buying,
                                    price: h.price,
                                    qty: h.quantity,
                                    ..Default::default()
                                }));
                            }
                        });
                    }
                    if many {
                        row.col(|ui| {
                            if let Some(open) = commodity_cell(ui, name, h.commodity.is_some(), page_rare) {
                                action = Some(open);
                            }
                        });
                    }
                    row.col(|ui| {
                        ui.label(RichText::new(cr(h.price)).font(display(size::CALLOUT)).color(if buying { pal::CYAN } else { pal::GOOD }));
                    });
                    // A rare good against its own average (0 when no market reports one), never the one over all of them.
                    let avg = h.commodity.as_ref().map_or(average, |c| Some(c.mean));
                    row.col(|ui| vs_average(ui, h.price, avg, buying));
                    row.col(|ui| {
                        ui.label(RichText::new(nexus_data::names::group_digits(h.quantity)).color(pal::DIM));
                    });
                    row.col(|ui| station_cell(ui, &h.station, &h.system, h.is_carrier, h.is_planetary, h.dist_ls, h.carrier_access.as_deref()));
                    row.col(|ui| {
                        ui.label(RichText::new(h.distance_ly.map(|d| format!("{d:.1} ly")).unwrap_or_default()).color(pal::DIM));
                    });
                    if wide {
                        row.col(|ui| {
                            ui.label(RichText::new(h.max_pad.map(String::from).unwrap_or_else(|| "?".into())).color(pal::DIM));
                        });
                    }
                    row.col(|ui| {
                        let (t, c) = age(h.updated_unix);
                        ui.label(RichText::new(t).color(c));
                    });
                    if row.response().clicked() && action.is_none() {
                        action = Some(RowAction::Select((h.market_id, symbol.to_string())));
                    }
                });
            });
    });
    action
}

/// Every market trading the commodity, newest price first, with both directions. Returns what a click asked for.
fn latest_table(ui: &mut Ui, quotes: &[CommodityQuote], average: Option<i64>, selected: Option<&PickKey>, page_good: (&str, &str), prefs: &MarketPrefs, page_rare: bool) -> Option<RowAction> {
    let wide = ui.available_width() >= 760.0;
    let mut action = None;
    const TABLE: &str = "commodity-latest";
    let mut sort = TableSort::load(ui, TABLE);
    crate::widgets::table(ui, if wide { "commodity-latest/wide" } else { "commodity-latest/narrow" }, |table| {
        let mut table = table
            .sense(egui::Sense::click())
            .column(Column::exact(24.0).resizable(false))
            // Each row names its commodity, as Abnormal prices does.
            .column(Column::initial(180.0).at_least(80.0).clip(true))
            .column(crate::widgets::flex_column(150.0))
            .column(Column::initial(104.0).at_least(60.0).clip(true));
        if wide {
            table = table.column(Column::initial(66.0).at_least(40.0).clip(true));
        }
        table = table.column(Column::initial(104.0).at_least(60.0).clip(true));
        if wide {
            table = table.column(Column::initial(66.0).at_least(40.0).clip(true));
        }
        table = table.column(Column::initial(72.0).at_least(50.0).clip(true)).column(Column::initial(60.0).at_least(40.0).clip(true));
        // Sortable columns: 0 station, 1 buy price, 2 stock, 3 sell price, 4 demand, 5 distance, 6 age,
        // 7 commodity. A market that doesn't trade that way sorts after the ones that do.
        let mut heads: Heads = vec![("", None), ("Commodity", Some((7, false))), ("Station", Some((0, false))), ("Buy for", Some((1, false)))];
        if wide {
            heads.push(("Stock", Some((2, true))));
        }
        heads.push(("Sell for", Some((3, true))));
        if wide {
            heads.push(("Demand", Some((4, true))));
        }
        heads.extend([("Distance", Some((5, false))), ("Updated", Some((6, true)))]);
        let table = table.header(28.0, |mut h| headings(&mut h, &heads, &mut sort, TABLE));
        let desc = sort.descending;
        // A missing price: last whichever way the column is sorted.
        let price = move |v: i64| if v > 0 { Some(v as f64) } else if desc { Some(f64::NEG_INFINITY) } else { None };
        let order = sort.order(quotes, |a, b, c| match c {
            0 => cmp_text(&a.station, &b.station),
            1 => cmp_opt(price(a.buy), price(b.buy)),
            2 => a.stock.cmp(&b.stock),
            3 => cmp_opt(price(a.sell), price(b.sell)),
            4 => a.demand.cmp(&b.demand),
            5 => cmp_opt(a.distance_ly, b.distance_ly),
            7 => cmp_text(a.commodity.as_ref().map_or("", |c| c.name.as_str()), b.commodity.as_ref().map_or("", |c| c.name.as_str())),
            _ => a.updated_unix.cmp(&b.updated_unix),
        });
        table
            .body(|body| {
                body.rows(40.0, quotes.len(), |mut row| {
                    let q = &quotes[order[row.index()]];
                    // The page's commodity, or (rare goods taken as one) the row's own.
                    let (symbol, name) = q.commodity.as_ref().map_or(page_good, |c| (c.symbol.as_str(), c.name.as_str()));
                    row.set_selected(selected.is_some_and(|k| k.0 == q.market_id && k.1 == symbol));
                    // A rare good against its own average, never the one over all of them; the page's commodity against
                    // the one this market reports or (carriers report none) the page's.
                    let avg = q.commodity.as_ref().map_or_else(|| Some(q.mean).filter(|m| *m > 0).or(average), |c| Some(c.mean));
                    row.col(|ui| {
                        // Pins the side it trades (where it buys, when it does both).
                        let pinned = [true, false].into_iter().find(|s| prefs.is_pinned(q.market_id, symbol, *s));
                        let sell_here = pinned.unwrap_or(q.sell > 0 && q.demand > 0);
                        if pin_button(ui, pinned.is_some()) {
                            let (price, qty) = if sell_here { (q.sell, q.demand) } else { (q.buy, q.stock) };
                            action = Some(RowAction::Pin(Pin {
                                market_id: q.market_id,
                                station: q.station.clone(),
                                system: q.system.clone(),
                                carrier: q.is_carrier,
                                symbol: symbol.to_string(),
                                name: name.to_string(),
                                sell_here,
                                price,
                                qty,
                                ..Default::default()
                            }));
                        }
                    });
                    row.col(|ui| {
                        if let Some(open) = commodity_cell(ui, name, q.commodity.is_some(), page_rare) {
                            action = Some(open);
                        }
                    });
                    row.col(|ui| station_cell(ui, &q.station, &q.system, q.is_carrier, q.is_planetary, q.dist_ls, q.carrier_access.as_deref()));
                    let price = |ui: &mut Ui, v: i64, qty: i64, col: Color32, what: &str| {
                        if v > 0 && qty > 0 {
                            let r = ui.label(RichText::new(cr(v)).color(col));
                            if let Some(a) = avg.filter(|a| *a > 0) {
                                r.on_hover_text(format!("{} {what} · galactic average {}", nexus_data::names::group_digits(qty), cr(a)));
                            }
                        } else {
                            ui.label(RichText::new("—").color(pal::FAINT));
                        }
                    };
                    row.col(|ui| price(ui, q.buy, q.stock, pal::CYAN, "in stock"));
                    if wide {
                        row.col(|ui| {
                            ui.label(RichText::new(if q.stock > 0 { nexus_data::names::group_digits(q.stock) } else { String::new() }).color(pal::DIM));
                        });
                    }
                    row.col(|ui| price(ui, q.sell, q.demand, pal::GOOD, "wanted"));
                    if wide {
                        row.col(|ui| {
                            ui.label(RichText::new(if q.demand > 0 { nexus_data::names::group_digits(q.demand) } else { String::new() }).color(pal::DIM));
                        });
                    }
                    row.col(|ui| {
                        ui.label(RichText::new(q.distance_ly.map(|d| format!("{d:.1} ly")).unwrap_or_default()).color(pal::DIM));
                    });
                    row.col(|ui| {
                        let (t, c) = age(q.updated_unix);
                        ui.label(RichText::new(t).color(c));
                    });
                    if row.response().clicked() && action.is_none() {
                        action = Some(RowAction::Select((q.market_id, symbol.to_string())));
                    }
                });
            });
    });
    action
}

enum PickAction {
    None,
    Close,
    Open(u64),
    Pin(Pin),
}

/// "all" / "squadronfriends" / ... as words (Spansh writes "Squadron Friends").
fn carrier_access(a: &str) -> &str {
    match access_key(a).as_str() {
        "all" => "everyone",
        "none" => "nobody but the owner",
        "friends" => "friends only",
        "squadron" => "squadron only",
        "squadronfriends" => "squadron and friends",
        _ => a,
    }
}

/// A docking access value in the journal's spelling: lower case, no spaces.
fn access_key(a: &str) -> String {
    a.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase()
}

/// A fleet carrier's docking access as a small badge: open, friends, squadron, owner only, or "?" when it was never
/// reported. The tooltip has it in words, and says it's as last reported.
pub(crate) fn access_badge(ui: &mut Ui, access: Option<&str>) -> egui::Response {
    let (text, col) = access_badge_text(access);
    let tip = match access {
        Some(a) => format!("Docking: {}, as last reported with its market. Its owner can change that at any time, so it may differ when you get there.", carrier_access(a)),
        None => "Who may dock was never reported for this carrier: older reports of its market don't include it.".to_string(),
    };
    badge(ui, text, col).on_hover_text(tip)
}

/// The badge's word and colour for a docking access (see [`access_badge`]).
fn access_badge_text(access: Option<&str>) -> (&'static str, Color32) {
    match access.map(access_key).as_deref() {
        Some("all") => ("open", pal::GOOD),
        Some("none") => ("owner only", pal::BAD),
        Some("friends") => ("friends", pal::WARN),
        Some("squadron") => ("squadron", pal::WARN),
        Some("squadronfriends") => ("squadron, friends", pal::WARN),
        Some(_) | None => ("?", pal::GRAY),
    }
}

/// A carrier's name with its docking-access badge after it: the name gives up room (and ends in "…") before the
/// badge does. `name` draws the name in the room left, so it can be a plain label or a link.
fn named_with_access<R>(ui: &mut Ui, text: RichText, access: Option<&str>, name: impl FnOnce(&mut Ui, RichText) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let badge_w = crate::widgets::badge_size(ui, access_badge_text(access).0).x + ui.spacing().item_spacing.x;
        let room = (ui.available_width() - badge_w).max(40.0);
        let r = ui.scope(|ui| {
            ui.set_max_width(room);
            name(ui, text)
        });
        access_badge(ui, access);
        r.inner
    })
    .inner
}

/// The picked market: what it is, the row's commodity there, its price history, and a way to its whole market.
fn pick_card(ui: &mut Ui, d: &PickDetail, (symbol, name): (&str, &str), origin: Option<[f64; 3]>, prefs: &MarketPrefs, prefer_sell: bool) -> PickAction {
    let Some(m) = &d.market else {
        ui.label(RichText::new("This market is no longer in the database.").color(pal::DIM));
        return PickAction::Close;
    };
    let st = &m.station;
    let mut action = PickAction::None;
    let title = if st.is_carrier { format!("{} {}", icon::ANCHOR, st.name) } else { st.name.clone() };
    crate::widgets::card_with(
        ui,
        &title,
        |ui| {
            if crate::widgets::icon_button(ui, icon::X, "Close").clicked() {
                action = PickAction::Close;
            }
            if crate::widgets::button(ui, &format!("{} Whole market", icon::STOREFRONT)).on_hover_text("Everything this station buys and sells").clicked() {
                action = PickAction::Open(st.market_id);
            }
            if crate::widgets::button(ui, &format!("{} Copy system", icon::COPY)).clicked() {
                ui.ctx().copy_text(st.system.clone());
            }
            if let Some(r) = m.items.iter().find(|r| r.symbol == symbol) {
                let (buys, sells) = (r.sell > 0 && r.demand > 0, r.buy > 0 && r.stock > 0);
                if buys || sells {
                    let sell_here = if buys && sells { prefer_sell } else { buys };
                    let pinned = prefs.is_pinned(st.market_id, symbol, sell_here);
                    let label = if pinned { format!("{} Pinned", icon::PUSH_PIN) } else { format!("{} Pin", icon::PUSH_PIN) };
                    if crate::widgets::button(ui, &label).on_hover_text("Keep it on top of the Market page with its latest price; the run planner fills it").clicked() {
                        let (price, qty) = if sell_here { (r.sell, r.demand) } else { (r.buy, r.stock) };
                        action = PickAction::Pin(Pin {
                            market_id: st.market_id,
                            station: st.name.clone(),
                            system: st.system.clone(),
                            carrier: st.is_carrier,
                            symbol: symbol.to_string(),
                            name: name.to_string(),
                            sell_here,
                            price,
                            qty,
                            ..Default::default()
                        });
                    }
                }
            }
        },
        |ui| {
            crate::widgets::split(
                ui,
                0.48,
                300.0,
                |ui| {
                    let mut facts = vec![st.system.clone()];
                    if let Some(t) = &st.station_type {
                        facts.push(t.clone());
                    }
                    if let Some(p) = st.max_pad {
                        facts.push(format!("{p} pad"));
                    }
                    if let Some(ls) = st.dist_ls {
                        facts.push(format!("{ls:.0} ls from arrival"));
                    }
                    if let (Some(o), Some(pos)) = (origin, st.pos) {
                        let d = ((o[0] - pos[0]).powi(2) + (o[1] - pos[1]).powi(2) + (o[2] - pos[2]).powi(2)).sqrt();
                        facts.push(format!("{d:.1} ly from you"));
                    }
                    ui.add(egui::Label::new(RichText::new(facts.join(" · ")).color(pal::DIM)).wrap());
                    if st.is_carrier {
                        let who = st.carrier_docking_access.as_deref().map(carrier_access).unwrap_or("unknown");
                        ui.add(egui::Label::new(RichText::new(format!("Fleet carrier: its owner sets these prices. Docking: {who}.")).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                    }
                    ui.add_space(6.0);
                    // A carrier's order: how long it has stood at this price, as far as the price history tells.
                    let since = |ui: &mut Ui, s: Option<Stood>, selling: bool, last: i64| {
                        if let Some(s) = s.filter(|_| st.is_carrier) {
                            ui.add(egui::Label::new(RichText::new(stood_words(s, selling, last)).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                        }
                    };
                    match m.items.iter().find(|r| r.symbol == symbol) {
                        Some(r) => {
                            if r.buy > 0 && r.stock > 0 {
                                ui.label(RichText::new(format!("Sells {} for {} · {} in stock", name, cr(r.buy), nexus_data::names::group_digits(r.stock))).color(pal::CYAN));
                                since(ui, d.since.and_then(|s| s.buy), true, r.updated_unix);
                            }
                            if r.sell > 0 && r.demand > 0 {
                                ui.label(RichText::new(format!("Buys {} for {} · {} wanted", name, cr(r.sell), nexus_data::names::group_digits(r.demand))).color(pal::GOOD));
                                since(ui, d.since.and_then(|s| s.sell), false, r.updated_unix);
                            }
                            let (a, col) = age(r.updated_unix);
                            ui.label(RichText::new(format!("Updated {a} ago · {} goods on this market", m.items.len())).size(size::FOOTNOTE).color(col));
                        }
                        None => {
                            ui.label(RichText::new(format!("Its latest market no longer lists {}.", name)).color(pal::DIM));
                        }
                    }
                },
                |ui| history_plot(ui, st.market_id, &d.history),
            );
        },
    );
    action
}

/// How long a carrier's order has stood at its price, in words: what it sells for (`selling`) or what it pays, up to
/// its latest report (`last`), the newest anyone sent: nothing is known after it. The price history is all there is
/// to go by: neither the game nor the live feed says when an order was set up, only what a market listed each time
/// it was reported.
fn stood_words(s: Stood, selling: bool, last: i64) -> String {
    let when = |t: i64| nexus_journal::Timestamp(t).to_local_string("%d %b %H:%M");
    let reports = |from: i64| format!("in every report from {} to the latest, {} ({})", when(from), when(last), span(last - from));
    // The report before: what it had on this side (a buy order may have been a sale before, or the other way round).
    let had = |previous: i64| match previous {
        0 if selling => "had none for sale".to_string(),
        0 => "had no buy order for it".to_string(),
        p => format!("had {}", cr(p)),
    };
    let new = if selling { "For sale" } else { "Wanted" };
    match s {
        Stood::Since { since, before, previous } if since < last => {
            let head = if previous == 0 { format!("{new} at this price") } else { "Price unchanged".to_string() };
            format!("{head} {}. The report before, {}, {}.", reports(since), when(before), had(previous))
        }
        Stood::Since { before, previous, .. } => {
            let head = if previous == 0 { format!("{new} since the latest report") } else { "Price changed in the latest report".to_string() };
            format!("{head}, {}. The report before, {}, {}.", when(last), when(before), had(previous))
        }
        Stood::AtLeast { since } if since < last => format!(
            "Price unchanged {}. Elite Nexus can't see further back: it keeps 30 days of price changes, from the first report of this market it received.",
            reports(since)
        ),
        Stood::AtLeast { .. } => format!("Only the latest report, {}, is on record for this price: how long it stood before that isn't known.", when(last)),
        Stood::Unknown => "How long this price has stood isn't known: Elite Nexus follows this market's prices from its next report.".to_string(),
    }
}

/// This commodity's recorded prices at one market.
fn history_plot(ui: &mut Ui, market_id: u64, pts: &[PricePoint]) {
    use egui_plot::{Line, Plot, PlotPoints};
    use nexus_journal::Timestamp;
    const DAY: f64 = 86_400.0;
    let buy: Vec<[f64; 2]> = pts.iter().filter(|p| p.buy > 0).map(|p| [p.ts_unix as f64 / DAY, p.buy as f64]).collect();
    let sell: Vec<[f64; 2]> = pts.iter().filter(|p| p.sell > 0).map(|p| [p.ts_unix as f64 / DAY, p.sell as f64]).collect();
    if buy.len() < 2 && sell.len() < 2 {
        ui.add(
            egui::Label::new(
                RichText::new("No price changes recorded here yet. History builds up from the live feed and your own visits: each price change is kept for 30 days.")
                    .size(size::FOOTNOTE)
                    .color(pal::DIM),
            )
            .wrap(),
        );
        return;
    }
    ui.label(RichText::new(format!("Price history · {} changes", pts.len())).size(size::FOOTNOTE).color(pal::DIM));
    Plot::new(("price-history", market_id))
        .show_background(false)
        .show_grid([false, true])
        .height(110.0)
        .allow_scroll(false)
        .allow_drag(false)
        .allow_zoom(false)
        .x_axis_formatter(|m, _| Timestamp((m.value * DAY) as i64).to_local_string("%d %b"))
        .y_axis_formatter(|m, _| nexus_data::names::compact_credits(m.value as i64))
        .label_formatter(|h| {
            let (x, y) = crate::widgets::hover_xy(h);
            Some(format!("{}\n{}", Timestamp((x * DAY) as i64).to_local_string("%d %b %H:%M"), cr(y as i64)))
        })
        .show(ui, |p| {
            if buy.len() >= 2 {
                p.line(Line::new("Sells for", PlotPoints::from(buy)).color(pal::CYAN).width(2.0));
            }
            if sell.len() >= 2 {
                p.line(Line::new("Buys for", PlotPoints::from(sell)).color(pal::GOOD).width(2.0));
            }
        });
}

/// Abnormal prices: anything a station pays far above the galactic average, or sells far below it,
/// across the whole database (or within a radius). The scan re-runs whenever a filter changes.
fn abnormal_tab(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) -> Option<Nav> {
    let origin = gs.location.star_pos;
    let p = &mut st.prefs;
    let mut refresh = false;
    crate::widgets::filter_bar(ui, "abnormal-filters", |bar| {
        bar.field("Stations and carriers", |ui| {
            crate::widgets::segmented(ui, &mut p.abn_mode, &[(TradeMode::SellToStation, "Paying far above average"), (TradeMode::BuyFromStation, "Selling far below average")]);
        });
        bar.field_tip("At least", "How far from the galactic average: at 2×, a station pays twice the average, or sells for half of it.", |ui| {
            let ratios: &[(u32, &str)] = &[(150, "1.5×"), (200, "2×"), (300, "3×"), (500, "5×"), (1000, "10×")];
            let mut r = (p.abn_ratio * 100.0).round() as u32;
            if crate::widgets::segmented(ui, &mut r, ratios) {
                p.abn_ratio = f64::from(r) / 100.0;
            }
        });
        bar.field("Where", |ui| {
            let scopes: &[(u32, &str)] = &[(0, "Whole galaxy"), (100, "Within 100 ly"), (500, "Within 500 ly")];
            let mut scope = p.abn_radius.map_or(0, |d| d as u32);
            if crate::widgets::segmented(ui, &mut scope, scopes) {
                p.abn_radius = (scope > 0).then_some(f64::from(scope));
            }
        });
        bar.field("Prices no older than", |ui| {
            ui.add(Number::new(&mut p.abn_age_h, 1.0..=720.0).ladder(HOURS).unit(" h"));
        });
        bar.field_tip("Landing pad", PAD_TIP, |ui| {
            if pad_picker(ui, &mut p.min_pad) {
                p.pad_auto = false;
            }
        });
        bar.field("Include", |ui| {
            crate::widgets::chip_toggle(ui, &mut p.abn_carriers, "Fleet carriers").on_hover_text("Fleet carriers' orders: their owners set the prices, and they move.");
        });
        bar.end(|ui| refresh = crate::widgets::icon_button(ui, icon::ARROW_CLOCKWISE, "Scan again with the latest prices").clicked());
    });
    if refresh {
        st.abn_key = None;
    }
    let p = &st.prefs;
    let key = (p.abn_mode, (p.abn_ratio * 100.0) as u64, p.abn_radius.map(|r| r as u64), p.abn_age_h as u64, p.abn_carriers, p.min_pad);
    if st.abn_key != Some(key) && !st.abn.busy() && (p.abn_radius.is_none() || origin.is_some()) {
        st.abn_key = Some(key);
        let q = DealQuery {
            mode: p.abn_mode,
            origin,
            max_distance_ly: p.abn_radius,
            min_ratio: p.abn_ratio,
            min_pad: p.min_pad,
            include_carriers: p.abn_carriers,
            max_age_hours: Some(p.abn_age_h),
            cargo_capacity: gs.fleet.current().map(|s| i64::from(s.cargo_capacity.max(1))).unwrap_or(100),
            limit: 200,
        };
        let db = db.clone();
        st.abn.run(ui.ctx(), move || db.price_deals(&q).map_err(|e| e.to_string()));
    }
    ui.add_space(8.0);
    let explain = match p.abn_mode {
        TradeMode::SellToStation => "Stations and carriers buying a commodity for far more than its galactic average: sell there. Click a commodity for where to buy it, or a station for its whole market.",
        TradeMode::BuyFromStation => "Stations and carriers selling a commodity for far less than its galactic average: buy there. Click a commodity for where to sell it, or a station for its whole market.",
    };
    ui.add(egui::Label::new(RichText::new(explain).size(size::FOOTNOTE).color(pal::DIM)).wrap());
    if !status(ui, &st.abn) {
        return None;
    }
    let Some(Ok(deals)) = &st.abn.last else { return None };
    if deals.is_empty() {
        empty(ui, icon::MAGNIFYING_GLASS, "Nothing abnormal right now", "No price in range is that far from its galactic average. Try a lower multiple or older prices.");
        return None;
    }
    let sell = p.abn_mode == TradeMode::SellToStation;
    let side = if sell { Side::Sell } else { Side::Buy };
    let mut nav = None;
    let mut pin = None;
    const TABLE: &str = "abnormal";
    let mut sort = TableSort::load(ui, TABLE);
    // Narrow windows drop the average and quantity columns rather than spilling off the page.
    let wide = ui.available_width() >= 1040.0;
    crate::widgets::table(ui, if wide { "abnormal/wide" } else { "abnormal/narrow" }, |table| {
        let mut table = table
            .column(Column::exact(24.0).resizable(false))
            .column(Column::initial(180.0).at_least(80.0).clip(true))
            .column(Column::initial(118.0).at_least(70.0).clip(true));
        if wide {
            table = table.column(Column::initial(100.0).at_least(60.0).clip(true)).column(Column::initial(70.0).at_least(44.0).clip(true)).column(Column::initial(84.0).at_least(44.0).clip(true));
        } else {
            table = table.column(Column::initial(64.0).at_least(44.0).clip(true));
        }
        // Sortable columns: 0 commodity, 1 price, 2 average, 3 multiple, 4 quantity, 5 station, 6 distance, 7 age.
        let mut heads: Heads = vec![("", None), ("Commodity", Some((0, false))), (if sell { "They pay" } else { "They sell for" }, Some((1, sell)))];
        if wide {
            heads.extend([("Average", Some((2, true))), ("×", Some((3, true))), (if sell { "Demand" } else { "Stock" }, Some((4, true)))]);
        } else {
            heads.push(("×", Some((3, true))));
        }
        heads.extend([("Station", Some((5, false))), ("Distance", Some((6, false))), ("Updated", Some((7, true)))]);
        let table = table
            .column(crate::widgets::flex_column(140.0))
            .column(Column::initial(76.0).at_least(50.0).clip(true))
            .column(Column::initial(76.0).at_least(50.0).clip(true))
            .header(30.0, |mut h| headings(&mut h, &heads, &mut sort, TABLE));
        let order = sort.order(deals, |a, b, c| match c {
            0 => cmp_text(&a.commodity_name, &b.commodity_name),
            1 => a.price.cmp(&b.price),
            2 => a.average.cmp(&b.average),
            3 => a.ratio.total_cmp(&b.ratio),
            4 => a.quantity.cmp(&b.quantity),
            5 => cmp_text(&a.station, &b.station),
            6 => cmp_opt(a.distance_ly, b.distance_ly),
            _ => a.updated_unix.cmp(&b.updated_unix),
        });
        table
            .body(|body| {
                // As tall as the commodity lists' rows: a carrier's name line has its docking badge.
                body.rows(40.0, deals.len(), |mut row| {
                    let d = &deals[order[row.index()]];
                    row.col(|ui| {
                        if pin_button(ui, p.is_pinned(d.market_id, &d.commodity, sell)) {
                            pin = Some(Pin {
                                market_id: d.market_id,
                                station: d.station.clone(),
                                system: d.system.clone(),
                                carrier: d.is_carrier,
                                symbol: d.commodity.clone(),
                                name: d.commodity_name.clone(),
                                sell_here: sell,
                                price: d.price,
                                qty: d.quantity,
                                ..Default::default()
                            });
                        }
                    });
                    row.col(|ui| {
                        let tip = if sell { "Where to buy it, where else to sell it, latest prices" } else { "Where to sell it, where else to buy it, latest prices" };
                        if nav_label(ui, RichText::new(&d.commodity_name).color(pal::TEXT), tip) {
                            nav = Some(Nav::Commodity { name: d.commodity.clone(), side: Some(side) });
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(cr(d.price)).font(display(size::CALLOUT)).color(if sell { pal::GOOD } else { pal::CYAN }));
                    });
                    if wide {
                        row.col(|ui| {
                            ui.label(RichText::new(cr(d.average)).color(pal::DIM));
                        });
                    }
                    row.col(|ui| {
                        let col = if d.ratio >= 10.0 { pal::PINK } else if d.ratio >= 3.0 { pal::ACCENT } else { pal::GOLD };
                        ui.label(RichText::new(format!("{:.1}×", d.ratio)).font(display(size::CALLOUT)).color(col))
                            .on_hover_text(format!("Galactic average {} · {} {}", cr(d.average), nexus_data::names::group_digits(d.quantity), if sell { "wanted" } else { "in stock" }));
                    });
                    if wide {
                        row.col(|ui| {
                            ui.label(RichText::new(nexus_data::names::group_digits(d.quantity)).color(pal::DIM));
                        });
                    }
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let name = if d.is_carrier { format!("{} {}", icon::ANCHOR, d.station) } else { d.station.clone() };
                            let tip = if d.is_carrier { "Fleet carrier: its owner sets these prices and who may dock. Click for its whole market." } else { "Click for the station's whole market" };
                            let clicked = if d.is_carrier {
                                named_with_access(ui, RichText::new(name).color(pal::TEXT), d.carrier_access.as_deref(), |ui, text| nav_label(ui, text, tip))
                            } else {
                                nav_label(ui, RichText::new(name).color(pal::TEXT), tip)
                            };
                            if clicked {
                                nav = Some(Nav::Station(d.market_id));
                            }
                            if ui.add(egui::Label::new(RichText::new(&d.system).size(size::CAPTION).color(pal::DIM)).truncate().sense(egui::Sense::click())).on_hover_text("Click to copy the system").clicked() {
                                ui.ctx().copy_text(d.system.clone());
                            }
                        });
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(d.distance_ly.map(|x| format!("{x:.0} ly")).unwrap_or_default()).color(pal::DIM));
                    });
                    row.col(|ui| {
                        let (a, col) = age(d.updated_unix);
                        ui.label(RichText::new(format!("{a} ago")).color(col));
                    });
                });
            });
    });
    if let Some(x) = pin {
        st.prefs.toggle_pin(x);
    }
    nav
}

// ---------------------------------------------------------------------------------------------
// Pins: entries kept on top of the page, priced from the latest data, checked on arrival
// ---------------------------------------------------------------------------------------------

/// Reloads the pinned markets when the pins change, on arrival in a system, and every half minute.
fn refresh_pins(ctx: &egui::Context, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) {
    let mut ids: Vec<u64> = st.prefs.pins.iter().map(|p| p.market_id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return;
    }
    let key = (ids.clone(), gs.location.last_arrival.0);
    let stale = st.pins_at.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(30));
    if (st.pins_key.as_ref() != Some(&key) || stale) && !st.pins_live.busy() {
        st.pins_key = Some(key);
        st.pins_at = Some(std::time::Instant::now());
        let db = db.clone();
        st.pins_live.run(ctx, move || {
            let mut out = FxHashMap::default();
            for id in ids {
                out.insert(id, db.station_market(id).map_err(|e| e.to_string())?);
            }
            Ok(out)
        });
    }
}

/// A pinned entry as its market stands now.
struct PinNow {
    system: String,
    pos: Option<[f64; 3]>,
    price: i64,
    qty: i64,
    updated: i64,
}

/// A pin as the database has it now (`None` while loading), `Some(None)` when its market no longer
/// lists it on that side.
fn pin_now(p: &Pin, live: Option<&FxHashMap<u64, Option<StationMarket>>>) -> Option<Option<PinNow>> {
    let m = live?.get(&p.market_id)?.as_ref();
    Some(m.and_then(|m| {
        let r = m.items.iter().find(|r| r.symbol == p.symbol)?;
        let (price, qty) = if p.sell_here { (r.sell, r.demand) } else { (r.buy, r.stock) };
        (price > 0 && qty > 0).then(|| PinNow { system: m.station.system.clone(), pos: m.station.pos, price, qty, updated: r.updated_unix })
    }))
}

/// The pinned entries: latest price, how far, and, in their system, whether a pinned fleet
/// carrier is really there. Returns where a click leads.
fn pins_panel(ui: &mut Ui, gs: &GameState, st: &mut MarketUi) -> Option<Nav> {
    let n = st.prefs.pins.len();
    let mut nav = None;
    ui.horizontal(|ui| {
        crate::widgets::disclosure(ui, &mut st.prefs.pins_open, &format!("{} Pinned  {n}", icon::PUSH_PIN));
        if st.tab != Tab::Plan && crate::widgets::button(ui, &format!("{} Plan a run", icon::PATH)).on_hover_text("Find where to buy what the pinned buyers want (and buyers for pinned sellers), and the order to fly it in").clicked() {
            st.set_tab(Tab::Plan);
        }
    });
    if !st.prefs.pins_open {
        return None;
    }
    let listed = carriers_here(gs);
    let live = match &st.pins_live.last {
        Some(Ok(m)) => Some(m),
        _ => None,
    };
    let here = gs.location.star_pos;
    let mut remove = None;
    let mut flip = None;
    crate::widgets::table(ui, "pins", |table| {
        table
            .vscroll(true)
            .max_scroll_height(30.0 * 5.0)
            .auto_shrink([false, true])
            .column(Column::exact(24.0).resizable(false))
            .column(Column::exact(44.0).resizable(false))
            .column(Column::initial(150.0).at_least(70.0).clip(true))
            .column(Column::initial(124.0).at_least(70.0).clip(true))
            .column(crate::widgets::flex_column(140.0))
            .column(Column::initial(66.0).at_least(44.0).clip(true))
            .column(Column::initial(118.0).at_least(60.0).clip(true))
            .column(Column::exact(26.0).resizable(false))
            .body(|body| {
                body.rows(30.0, n, |mut row| {
                    let i = row.index();
                    let p = &st.prefs.pins[i];
                    let now = pin_now(p, live);
                    row.col(|ui| {
                        let mut on = p.in_plan;
                        if ui.checkbox(&mut on, "").on_hover_text("Part of the run planner's run").changed() {
                            flip = Some(i);
                        }
                    });
                    row.col(|ui| {
                        let (t, c) = if p.sell_here { ("SELL", pal::GOOD) } else { ("BUY", pal::CYAN) };
                        ui.label(RichText::new(t).size(size::CAPTION).color(c)).on_hover_text(if p.sell_here { "Pinned buyer: you sell here" } else { "Pinned seller: you buy here" });
                    });
                    row.col(|ui| {
                        if nav_label(ui, RichText::new(&p.name).color(pal::TEXT), "Its page: where to buy, where to sell, latest prices") {
                            nav = Some(Nav::Commodity { name: p.symbol.clone(), side: Some(if p.sell_here { Side::Sell } else { Side::Buy }) });
                        }
                    });
                    row.col(|ui| match &now {
                        Some(Some(n)) => {
                            let what = if p.sell_here { "wanted" } else { "in stock" };
                            let mut tip = format!("{} {what}", nexus_data::names::group_digits(n.qty));
                            if n.price != p.price {
                                tip += &format!(" · {} when pinned", cr(p.price));
                            }
                            ui.label(RichText::new(cr(n.price)).color(if p.sell_here { pal::GOOD } else { pal::CYAN })).on_hover_text(tip);
                        }
                        Some(None) => {
                            ui.label(RichText::new(if p.sell_here { "no longer buys" } else { "sold out" }).color(pal::BAD))
                                .on_hover_text(format!("Its latest market has no {} {} it. It was {} when pinned.", if p.sell_here { "demand for" } else { "stock of" }, p.name, cr(p.price)));
                        }
                        None => {
                            ui.label(RichText::new(cr(p.price)).color(pal::DIM)).on_hover_text("The price when pinned (loading the latest)");
                        }
                    });
                    let system = match &now {
                        Some(Some(n)) => n.system.clone(),
                        _ => p.system.clone(),
                    };
                    row.col(|ui| {
                        let name = if p.carrier { format!("{} {}", icon::ANCHOR, p.station) } else { p.station.clone() };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if nav_label(ui, RichText::new(name).color(pal::TEXT), "Click for the station's whole market") {
                                nav = Some(Nav::Station(p.market_id));
                            }
                            // Who may dock, once the pin's market has loaded.
                            if p.carrier
                                && let Some(Some(m)) = live.and_then(|l| l.get(&p.market_id))
                            {
                                access_badge(ui, m.station.carrier_docking_access.as_deref());
                            }
                            if ui.add(egui::Label::new(RichText::new(&system).size(size::CAPTION).color(pal::DIM)).truncate().sense(egui::Sense::click())).on_hover_text("Click to copy the system").clicked() {
                                ui.ctx().copy_text(system.clone());
                            }
                        });
                    });
                    row.col(|ui| {
                        let pos = match &now {
                            Some(Some(n)) => n.pos,
                            _ => None,
                        };
                        let d = here.zip(pos).map(|(a, b)| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt());
                        ui.label(RichText::new(d.map(|d| format!("{d:.0} ly")).unwrap_or_default()).color(pal::DIM));
                    });
                    row.col(|ui| {
                        let in_system = system.eq_ignore_ascii_case(&gs.location.system);
                        let seen = p.carrier && presence(&listed, &p.station) == Presence::Here;
                        if p.carrier && (in_system || seen) {
                            match presence(&listed, &p.station) {
                                Presence::Here => {
                                    let tip = if in_system { "The game listed this carrier in this system since you arrived." } else { "The game listed it here since you arrived: it has moved here since its market was last reported." };
                                    ui.label(RichText::new(format!("{} here", icon::CHECK_CIRCLE)).color(pal::GOOD)).on_hover_text(tip);
                                }
                                Presence::NotHere(k) => {
                                    ui.label(RichText::new(format!("{} not here", icon::X_CIRCLE)).color(pal::BAD)).on_hover_text(format!(
                                        "The game listed {k} fleet carriers in {} since you arrived, and not this one: it has probably jumped away. The database still places it here.",
                                        gs.location.system
                                    ));
                                }
                                Presence::Unknown => {
                                    ui.label(RichText::new(format!("{} not listed yet", icon::QUESTION)).color(pal::DIM))
                                        .on_hover_text("The game lists the carriers of a system as they show up in your navigation panel: on arrival, or when you drop out of supercruise. None has been listed since you arrived.");
                                }
                            }
                        } else if in_system {
                            ui.label(RichText::new("in this system").color(pal::GOOD));
                        } else if let Some(Some(n)) = &now {
                            let (a, c) = age(n.updated);
                            ui.label(RichText::new(format!("{a} ago")).color(c)).on_hover_text("When its market was last reported");
                        }
                    });
                    row.col(|ui| {
                        if crate::widgets::icon_button(ui, icon::X, "Unpin").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            });
    });
    if let Some(i) = flip {
        st.prefs.pins[i].in_plan = !st.prefs.pins[i].in_plan;
    }
    if let Some(i) = remove {
        st.prefs.pins.remove(i);
    }
    ui.add_space(8.0);
    nav
}

// ---------------------------------------------------------------------------------------------
// Run planner
// ---------------------------------------------------------------------------------------------

/// Plans a run from the pins: where to buy what the pinned buyers want, where to sell what the
/// pinned sellers offer, what fits the hold and the credits, and the order to fly it in.
fn plan_tab(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) -> Option<Nav> {
    if st.prefs.pins.is_empty() {
        empty(
            ui,
            icon::PUSH_PIN,
            "Pin the orders you want to fill",
            "Pin entries with the pin in front of a row (Find commodity, Abnormal prices, a station's market): places that buy what you'll sell, and, if you like, places to buy from. The planner finds a seller for every pinned buyer and a buyer for every pinned seller, fits your hold and credits, and orders the stops.",
        );
        return None;
    }
    let Some(origin) = gs.location.star_pos else {
        empty(ui, icon::MAP_PIN, "Position unknown", "Jump or relog once so Elite Nexus knows where you are.");
        return None;
    };
    let pad = finder::ship_pad(gs);
    let capacity = gs.fleet.current().map(|s| i64::from(s.cargo_capacity)).unwrap_or(0);
    let cargo_used: i64 = gs.inventory.cargo.items.iter().map(|c| c.count).sum();
    let p = &mut st.prefs;
    let mut again = false;
    crate::widgets::filter_bar(ui, "plan-filters", |bar| {
        bar.field("Sellers and buyers within", |ui| {
            ui.add(Number::new(&mut p.plan_radius, 5.0..=MAX_SEARCH_LY).ladder(LY).unit(" ly")).on_hover_text("Around each pinned order: sellers near a pinned buyer, buyers near a pinned seller.");
        });
        bar.field("Pick sellers by", |ui| {
            crate::widgets::segmented(ui, &mut p.plan_cheapest, &[(false, "Shortest detour"), (true, "Cheapest price")]);
        });
        bar.field("Include", |ui| {
            crate::widgets::chip_toggle(ui, &mut p.plan_hold, "What's aboard first").on_hover_text("Cargo you already carry fills a pinned buyer before anything is bought for it.");
            crate::widgets::chip_toggle(ui, &mut p.plan_carriers, "Fleet carriers").on_hover_text("Let the planner pick fleet carriers as sellers or buyers (pinned carriers always count).");
        });
        bar.field_tip("For your ship", "The run fits this hold and these credits, at stations with this pad, with prices up to Find commodity's age limit.", |ui| {
            let text = format!("{} pad · {} t, {} t free · {}", pad.unwrap_or('L'), capacity, (capacity - cargo_used).max(0), short_credits(gs.cmdr.credits));
            ui.label(RichText::new(text).font(display(size::CALLOUT)).color(pal::TEXT));
        });
        bar.end(|ui| again = crate::widgets::button(ui, &format!("{} Plan again", icon::ARROWS_CLOCKWISE)).clicked());
    });
    let orders: Vec<Order> =
        p.pins.iter().filter(|x| x.in_plan).map(|x| Order { market_id: x.market_id, symbol: x.symbol.clone(), name: x.name.clone(), sell_here: x.sell_here }).collect();
    if orders.is_empty() {
        empty(ui, icon::PUSH_PIN, "No pin in the run", "Tick the pins the run should fill (the box in front of each).");
        return None;
    }
    let input = PlanInput {
        orders,
        origin,
        cargo_capacity: capacity,
        cargo_used,
        credits: gs.cmdr.credits,
        hold: finder::sellable_hold(gs),
        use_hold: p.plan_hold,
        min_pad: pad,
        carriers: p.plan_carriers,
        radius: p.plan_radius,
        cheapest: p.plan_cheapest,
        max_age_h: p.max_age_h,
    };
    // Re-plan when anything it depends on changes (a purchase changes the hold and the credits).
    let key = format!("{:?}", (&input.orders, input.origin, input.cargo_used, input.credits, &input.hold, input.use_hold, input.min_pad, input.carriers, input.radius, input.cheapest, input.max_age_h, capacity));
    if (again || st.plan_key.as_deref() != Some(key.as_str())) && !st.plan.busy() {
        st.plan_key = Some(key);
        let db = db.clone();
        st.plan.run(ui.ctx(), move || runplan::plan(&db, &input));
    }
    ui.add_space(8.0);
    let plan = match &st.plan.last {
        None => {
            ui.spinner();
            return None;
        }
        Some(Err(e)) => {
            ui.label(RichText::new(e).color(pal::BAD));
            return None;
        }
        Some(Ok(plan)) => plan,
    };
    if st.plan.busy() {
        ui.spinner();
    }
    let mut nav = None;
    let stats = !plan.stops.is_empty();
    ui.horizontal_wrapped(|ui| {
        if !stats {
            return;
        }
        ui.spacing_mut().item_spacing = egui::vec2(32.0, 8.0);
        crate::widgets::stat(ui, "Profit", short_credits(plan.profit()), if plan.profit() >= 0 { pal::GOOD } else { pal::BAD });
        crate::widgets::stat(ui, "Spend", short_credits(plan.cost), pal::CYAN);
        crate::widgets::stat(ui, "Earn", short_credits(plan.revenue), pal::GOLD);
        crate::widgets::stat(ui, "Stops", plan.stops.len().to_string(), pal::TEXT);
        crate::widgets::stat(ui, "Straight-line distance", format!("{:.0} ly", plan.distance_ly), pal::TEXT)
            .on_hover_text("Sum of the straight lines between the stops; the jumps you fly add a little.");
    });
    for n in &plan.notes {
        ui.add(egui::Label::new(RichText::new(format!("{} {n}", icon::WARNING)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
    }
    if plan.stops.is_empty() {
        empty(ui, icon::PATH, "Nothing to fly", "No pinned order can be filled with what the database knows. The notes above say why; a wider search distance may help.");
        return None;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if crate::widgets::primary_button(ui, &format!("{} Follow this run", icon::PATH)).on_hover_text("Shows the stops on the Navigation page; after each jump the next stop's system goes to the clipboard (if that's on there)").clicked() {
            st.follow = Some(run_route(plan));
        }
        if let Some(first) = plan.stops.first()
            && crate::widgets::button(ui, &format!("{} Copy {}", icon::COPY, first.place.system)).clicked()
        {
            ui.ctx().copy_text(first.place.system.clone());
        }
    });
    ui.add_space(6.0);
    egui::ScrollArea::vertical().id_salt("plan-stops").auto_shrink([false, false]).show(ui, |ui| {
        for (i, s) in plan.stops.iter().enumerate() {
            crate::widgets::card(ui, "", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{}", i + 1)).font(display(size::TITLE3)).color(pal::ACCENT));
                    let name = if s.place.is_carrier { format!("{} {}", icon::ANCHOR, s.place.station) } else { s.place.station.clone() };
                    if nav_label(ui, RichText::new(name).font(display(size::HEADLINE)).color(pal::TEXT), "Click for the station's whole market") {
                        nav = Some(Nav::Station(s.place.market_id));
                    }
                    if ui.link(&s.place.system).on_hover_text("Click to copy the system").clicked() {
                        ui.ctx().copy_text(s.place.system.clone());
                    }
                    let mut facts = Vec::new();
                    if let Some(d) = s.leg_ly {
                        facts.push(format!("{d:.0} ly {}", if i == 0 { "from you" } else { "from the last stop" }));
                    }
                    if let Some(pad) = s.place.max_pad {
                        facts.push(format!("{pad} pad"));
                    }
                    if s.place.updated_unix > 0 {
                        facts.push(format!("prices {} old", age(s.place.updated_unix).0));
                    }
                    ui.label(RichText::new(facts.join(" · ")).color(pal::DIM));
                });
                for (name, t, price) in &s.buys {
                    ui.label(RichText::new(format!("Buy {} t {name} at {}", nexus_data::names::group_digits(*t), cr(*price))).color(pal::CYAN));
                }
                for (name, t, price) in &s.sells {
                    ui.label(RichText::new(format!("Sell {} t {name} at {} = {}", nexus_data::names::group_digits(*t), cr(*price), short_credits(t * price))).color(pal::GOOD));
                }
            });
            ui.add_space(6.0);
        }
        let found: Vec<String> = plan
            .legs
            .iter()
            .filter(|l| l.found_from || l.found_to)
            .map(|l| if l.found_from { format!("{} from {}", l.name, l.from.as_ref().map(|f| f.0.station.as_str()).unwrap_or("")) } else { format!("{} to {}", l.name, l.to.0.station) })
            .collect();
        if !found.is_empty() {
            crate::widgets::footnote(ui, &format!("Picked by the planner (not pinned): {}. Prices and stock are the latest reports, and can change before you arrive.", found.join(", ")));
        }
    });
    nav
}

/// The run as a Navigation route: one waypoint per system, with what to do there.
fn run_route(plan: &RunPlan) -> crate::pages::navigation::PlannedRoute {
    use crate::pages::navigation::{PlannedRoute, Waypoint};
    let mut points: Vec<Waypoint> = Vec::new();
    for s in &plan.stops {
        let mut acts: Vec<String> = s.buys.iter().map(|(n, t, _)| format!("buy {t} t {n}")).collect();
        acts.extend(s.sells.iter().map(|(n, t, _)| format!("sell {t} t {n}")));
        let note = format!("{}: {}", s.place.station, acts.join(", "));
        match points.last_mut() {
            Some(w) if w.system.eq_ignore_ascii_case(&s.place.system) => {
                w.note = format!("{}; {note}", w.note);
            }
            _ => points.push(Waypoint { system: s.place.system.clone(), pos: s.place.pos, star_class: None, jumps: 0, neutron: false, refuel: false, scoopable: None, value: None, note }),
        }
    }
    let summary = format!("{} stops, {} profit, about {:.0} ly", plan.stops.len(), short_credits(plan.profit()), plan.distance_ly);
    PlannedRoute::new("Trade run", summary, String::new(), points)
}

fn service_query(kind: &str, origin: [f64; 3], pad: Option<char>) -> StationQuery {
    let mut q = StationQuery { origin, min_pad: pad, limit: 25, ..Default::default() };
    match kind {
        "raw" => q.material_trader = Some(MaterialTrader::Raw),
        "manufactured" => q.material_trader = Some(MaterialTrader::Manufactured),
        "encoded" => q.material_trader = Some(MaterialTrader::Encoded),
        "human" => q.tech_broker = Some(TechBroker::Human),
        "guardian" => q.tech_broker = Some(TechBroker::Guardian),
        "factors" => q.services = Services::INTERSTELLAR_FACTORS,
        "cartographics" => q.services = Services::UNIVERSAL_CARTOGRAPHICS,
        "vista" => q.services = Services::VISTA_GENOMICS,
        "shipyard" => q.services = Services::SHIPYARD,
        "outfitting" => q.services = Services::OUTFITTING,
        "blackmarket" => q.services = Services::BLACK_MARKET,
        _ => {}
    }
    q
}

/// Height of the Market page's status bar.
const STATUS_BAR_H: f32 = 30.0;

/// The live feed and the database in one slim bar along the bottom of the page, the way an app's status bar sits under
/// its content: the Live badge and the feed's rate, what the database holds, and on the right the feed's newest message.
/// A click anywhere on it opens Database & feed (returns true).
fn status_bar(ui: &mut Ui, rect: egui::Rect, svc: &mut MarketService) -> bool {
    let latest = svc.latest_headline();
    // The bar's tooltip is the one that shows over it, so it carries the newest message whole.
    let mut tip = "The live market feed (EDDN): what commanders all over the galaxy report as they dock, and what Elite Nexus's database holds from it, your journal and the Spansh import. Click for Database & feed.".to_string();
    if let Some(h) = &latest {
        tip.push_str(&format!("\n\nNewest message: {h}"));
    }
    let resp = ui.interact(rect, ui.id().with("market-status-bar"), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, egui::CornerRadius::same(8), Color32::from_white_alpha(6));
        }
        p.hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, pal::SEPARATOR));
    }
    let mut bar = ui.new_child(egui::UiBuilder::new().id_salt("market-status").max_rect(rect.shrink2(egui::vec2(4.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    bar.spacing_mut().item_spacing.x = 8.0;
    // Plain text that lets the click through to the bar.
    let text = |ui: &mut Ui, t: String, c: Color32| {
        ui.add(egui::Label::new(RichText::new(t).size(size::CAPTION).color(c)).selectable(false).truncate());
    };
    match svc.feed_summary() {
        Some((connected, line)) => {
            badge(&mut bar, if connected { "Live" } else { "Connecting" }, if connected { pal::GOOD } else { pal::WARN });
            text(&mut bar, line, pal::DIM);
        }
        None => {
            badge(&mut bar, "Feed off", pal::GRAY);
            text(&mut bar, "Turn on the live market feed in Accounts & sync for prices from all over the galaxy.".into(), pal::DIM);
        }
    }
    if let Some(s) = svc.db_summary() {
        text(&mut bar, format!("·  {s}"), pal::DIM);
    }
    // The newest message, where there's room for it to say something.
    if bar.available_width() > 200.0
        && let Some(h) = latest
    {
        bar.add_space(16.0);
        bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| text(ui, format!("{} {h}", icon::BROADCAST), pal::FAINT));
    }
    resp.clicked()
}

fn status<T>(ui: &mut Ui, slot: &Slot<Res<T>>) -> bool {
    if slot.busy() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Searching…").color(pal::DIM));
        });
        return false;
    }
    match &slot.last {
        None => false,
        Some(Err(e)) => {
            ui.label(RichText::new(e).color(pal::BAD));
            false
        }
        Some(Ok(v)) => {
            if let Some(t) = slot.took {
                ui.label(RichText::new(format!("{} results in {:.1} ms", v.len(), t.as_secs_f64() * 1e3)).small().color(pal::DIM));
            }
            true
        }
    }
}

/// `from`: the station the routes start at, when they were searched from one.
fn results_routes(ui: &mut Ui, slot: &Slot<Res<TradeRoute>>, from: Option<&str>) -> Option<Nav> {
    if !status(ui, slot) {
        return None;
    }
    let Some(Ok(routes)) = &slot.last else { return None };
    if routes.is_empty() {
        let text = match from {
            Some(station) => format!("Nothing you can buy at {station} sells for more within reach, with the prices Elite Nexus knows."),
            None => "No profitable routes in range with the current data.".into(),
        };
        ui.label(RichText::new(text).color(pal::DIM));
        return None;
    }
    let mut nav = None;
    // From one station, every route buys there: that column only needs the price.
    let here = from.is_some();
    const TABLE: &str = "trade-routes";
    let mut sort = TableSort::load(ui, TABLE);
    crate::widgets::table(ui, if here { "trade-routes/here" } else { "trade-routes/around" }, |table| {
        let table = table
            .column(Column::initial(170.0).at_least(90.0).clip(true))
            .column(Column::initial(90.0).at_least(60.0).clip(true))
            .column(Column::initial(100.0).at_least(60.0).clip(true))
            .column(Column::initial(if here { 110.0 } else { 260.0 }).at_least(90.0).clip(true))
            .column(crate::widgets::flex_column(200.0))
            .column(Column::initial(70.0).at_least(50.0).clip(true))
            .header(30.0, |mut h| {
                // Buy at / Sell at sort by how far from you the station is (Buy here by price).
                let buy = if here { "Buy here" } else { "Buy at" };
                let heads: Heads = vec![("Commodity", Some((0, false))), ("Profit/t", Some((1, true))), ("Trip profit", Some((2, true))), (buy, Some((3, false))), ("Sell at", Some((4, false))), ("Hop", Some((5, false)))];
                headings(&mut h, &heads, &mut sort, TABLE);
            });
        let order = sort.order(routes, |a, b, c| match c {
            0 => cmp_text(&a.commodity_name, &b.commodity_name),
            1 => a.profit_per_ton.cmp(&b.profit_per_ton),
            2 => a.total_profit.cmp(&b.total_profit),
            3 if here => a.buy_price.cmp(&b.buy_price),
            3 => a.from.distance_from_origin_ly.total_cmp(&b.from.distance_from_origin_ly),
            4 => a.to.distance_from_origin_ly.total_cmp(&b.to.distance_from_origin_ly),
            _ => a.hop_ly.total_cmp(&b.hop_ly),
        });
        table
            .body(|body| {
                body.rows(44.0, routes.len(), |mut row| {
                    let r = &routes[order[row.index()]];
                    row.col(|ui| {
                        if nav_label(ui, RichText::new(&r.commodity_name).color(pal::TEXT), "Where to buy and sell it, latest prices") {
                            nav = Some(Nav::Commodity { name: r.commodity.clone(), side: None });
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(nexus_data::names::group_digits(r.profit_per_ton)).font(display(14.0)).color(pal::GOOD));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(short_credits(r.total_profit)).color(pal::GOLD));
                    });
                    for (end, price, buy) in [(&r.from, r.buy_price, true), (&r.to, r.sell_price, false)] {
                        row.col(|ui| {
                            ui.vertical(|ui| {
                                let (a, c) = age(end.market_updated_unix);
                                if buy && here {
                                    ui.label(format!("{} Cr", nexus_data::names::group_digits(price)));
                                    ui.label(RichText::new(format!("{a} ago")).small().color(c));
                                    return;
                                }
                                if nav_label(ui, format!("{} — {} Cr", end.station, nexus_data::names::group_digits(price)), "Click for the station's whole market") {
                                    nav = Some(Nav::Station(end.market_id));
                                }
                                ui.label(RichText::new(format!("{} · {:.1} ly · {}", end.system, end.distance_from_origin_ly, a)).small().color(c));
                            });
                        });
                    }
                    row.col(|ui| {
                        ui.label(RichText::new(format!("{:.1} ly", r.hop_ly)).color(pal::DIM));
                    });
                });
            });
    });
    nav
}

/// A service's name after "a" or "an": "an Interstellar Factors", "a Shipyard".
fn with_article(name: &str) -> String {
    let an = name.chars().next().is_some_and(|c| "AEIOUaeiou".contains(c));
    format!("{} {name}", if an { "an" } else { "a" })
}

/// How many stations Services lists at first, and how many more each Show more adds.
const SERVICES_STEP: usize = 25;

/// What Services' search covers, said plainly: the nearest stations with the service among every station the database
/// knows, with no distance limit (a station far away shows once the nearer ones are listed), and where the database's
/// stations come from.
fn services_reach(ui: &mut Ui, service: &str, known: Option<u64>, limit: usize) {
    let count = known.map(|n| format!(" ({} now)", nexus_data::names::group_digits(n as i64))).unwrap_or_default();
    let mut text = format!(
        "The {limit} nearest stations with {}, out of every station Elite Nexus knows{count}: those commanders report over the live feed, the ones you visit, and the Spansh import if you ran it. There's no distance limit: one 3,000 ly away is listed once the nearer ones are, so press Show more to reach further.",
        with_article(service)
    );
    // The galaxy has some 90,000 stations and carriers: far fewer means the import hasn't been run.
    if known.is_none_or(|n| n < 30_000) {
        text.push_str(" To search the whole galaxy, import every station from Spansh in Database & feed.");
    }
    crate::widgets::footnote(ui, &text);
    ui.add_space(4.0);
}

/// The line over Services' list: how many, how far they reach, how long it took, and Show more when there may be more.
/// Returns true when Show more is pressed.
fn services_summary(ui: &mut Ui, slot: &Slot<Res<StationHit>>, limit: usize) -> bool {
    let mut more = false;
    ui.horizontal(|ui| {
        match &slot.last {
            Some(Ok(list)) if !list.is_empty() => {
                let (near, far) = (list.first().map_or(0.0, |h| h.distance_ly), list.last().map_or(0.0, |h| h.distance_ly));
                let what = if list.len() < limit { format!("All {} known", list.len()) } else { format!("The {} nearest", list.len()) };
                ui.label(RichText::new(format!("{what} · {near:.1} to {far:.1} ly")).font(display(size::FOOTNOTE)).color(pal::TEXT));
                if let Some(t) = slot.took {
                    ui.label(RichText::new(format!("· {:.1} ms", t.as_secs_f64() * 1e3)).size(size::FOOTNOTE).color(pal::FAINT));
                }
                // A full page may have more behind it.
                if list.len() >= limit {
                    more = crate::widgets::button(ui, &format!("Show {SERVICES_STEP} more")).on_hover_text("The next nearest stations with this service, however far").clicked();
                }
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(pal::BAD));
            }
            _ => {}
        }
        if slot.busy() {
            ui.spinner();
        }
    });
    more
}

fn results_stations(ui: &mut Ui, slot: &Slot<Res<StationHit>>) {
    let Some(Ok(list)) = &slot.last else { return };
    if list.is_empty() {
        return;
    }
    const TABLE: &str = "services";
    let mut sort = TableSort::load(ui, TABLE);
    crate::widgets::table(ui, TABLE, |table| {
        let table = table
            .column(Column::initial(260.0).at_least(100.0).clip(true))
            .column(Column::initial(220.0).at_least(100.0).clip(true))
            .column(Column::initial(80.0).at_least(56.0).clip(true))
            .column(Column::initial(80.0).at_least(56.0).clip(true))
            .column(Column::initial(50.0).at_least(32.0).clip(true))
            .column(Column::remainder().at_least(80.0).clip(true))
            .header(30.0, |mut h| {
                let heads: Heads = vec![("Station", Some((0, false))), ("System", Some((1, false))), ("Distance", Some((2, false))), ("From star", Some((3, false))), ("Pad", Some((4, true))), ("Economy", Some((5, false)))];
                headings(&mut h, &heads, &mut sort, TABLE);
            });
        let order = sort.order(list, |a, b, c| match c {
            0 => cmp_text(&a.station.name, &b.station.name),
            1 => cmp_text(&a.station.system, &b.station.system),
            2 => a.distance_ly.total_cmp(&b.distance_ly),
            3 => cmp_opt(a.station.dist_ls, b.station.dist_ls),
            4 => pad_rank(a.station.max_pad).cmp(&pad_rank(b.station.max_pad)),
            _ => cmp_text(a.station.economy.as_deref().unwrap_or_default(), b.station.economy.as_deref().unwrap_or_default()),
        });
        table
            .body(|body| {
                body.rows(32.0, list.len(), |mut row| {
                    let s = &list[order[row.index()]];
                    row.col(|ui| {
                        ui.add(egui::Label::new(RichText::new(&s.station.name).color(pal::TEXT)).truncate());
                    });
                    row.col(|ui| {
                        if ui.link(&s.station.system).on_hover_text("Click to copy").clicked() {
                            ui.ctx().copy_text(s.station.system.clone());
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(format!("{:.1} ly", s.distance_ly)).color(pal::DIM));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(s.station.dist_ls.map(|d| format!("{d:.0} ls")).unwrap_or_default()).color(pal::DIM));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(s.station.max_pad.map(String::from).unwrap_or_else(|| "?".into())).color(pal::DIM));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(s.station.economy.clone().unwrap_or_default()).color(pal::DIM));
                    });
                });
            });
    });
}

fn results_hotspots(ui: &mut Ui, slot: &Slot<Res<HotspotHit>>) {
    card(ui, "Rings", |ui| {
        if slot.last.is_none() && !slot.busy() {
            ui.label(RichText::new("Pick a commodity and press Find hotspots: the rings with the most hotspots of it, nearest first.").size(size::FOOTNOTE).color(pal::DIM));
        }
        if !status(ui, slot) {
            return;
        }
        let Some(Ok(list)) = &slot.last else { return };
        if list.is_empty() {
            ui.label(RichText::new("No known hotspots in range yet.").color(pal::DIM));
        }
        for h in list {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}×", h.count)).font(display(15.0)).color(pal::GOLD));
                ui.label(&h.body_name);
                ui.label(RichText::new(format!("{:.1} ly · {}", h.distance_ly, h.ring_type.clone().unwrap_or_default())).small().color(pal::DIM));
                if ui.small_button(icon::COPY).on_hover_text("Copy system").clicked() {
                    ui.ctx().copy_text(h.system.clone());
                }
            });
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Station: markets side by side
// ---------------------------------------------------------------------------------------------

/// Width of the Station tab's list of markets.
const LIST_W: f32 = 300.0;
/// A market tile's width with room for long commodity names, the narrowest it gets, and the lowest before the tiles
/// scroll.
const TILE_GOOD_W: f32 = 400.0;
const TILE_MIN_W: f32 = 300.0;
const TILE_MIN_H: f32 = 220.0;

/// How many columns `n` market tiles take in `w` × `h`: side by side as long as each keeps a comfortable width (markets
/// are long lists, so tall narrow tiles beat short wide ones), then more and narrower while the rows would get too
/// low, never below [`TILE_MIN_W`].
fn tile_columns(n: usize, w: f32, h: f32, gap: f32) -> usize {
    let width = |cols: usize| (w - gap * (cols as f32 - 1.0)) / cols as f32;
    let height = |cols: usize| {
        let rows = n.div_ceil(cols) as f32;
        (h - gap * (rows - 1.0)) / rows
    };
    let n = n.max(1);
    let mut cols = (1..=n).rev().find(|c| width(*c) >= TILE_GOOD_W).unwrap_or(1);
    while cols < n && height(cols) < TILE_MIN_H && width(cols + 1) >= TILE_MIN_W {
        cols += 1;
    }
    cols
}
/// How often the open markets are read again from the database while the Station tab shows.
const TILES_EVERY: std::time::Duration = std::time::Duration::from_secs(10);
/// How long a market just opened stays lit.
const FLASH: f32 = 1.6;

/// The Station tab: on the left a list of markets to open (a search, the station you're docked at, this system's
/// stations and carriers, the ones opened lately); on the right up to eight markets side by side, tiled to the window.
fn station_tab(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) -> Option<Nav> {
    refresh_tiles(ui.ctx(), db, st);
    let full = ui.available_rect_before_wrap();
    let gap = crate::widgets::GAP;
    let list = st.prefs.tiles_list;
    let list_w = LIST_W.min(full.width() * 0.45);
    if list {
        let rect = egui::Rect::from_min_size(full.min, egui::vec2(list_w, full.height()));
        let mut c = ui.new_child(egui::UiBuilder::new().id_salt("station-list").max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        market_list(&mut c, gs, db, st);
    }
    let tiles = if list { egui::Rect::from_min_max(egui::pos2(full.left() + list_w + gap, full.top()), full.max) } else { full };
    let mut c = ui.new_child(egui::UiBuilder::new().id_salt("station-tiles").max_rect(tiles).layout(egui::Layout::top_down(egui::Align::Min)));
    let nav = market_tiles(&mut c, gs, st);
    ui.advance_cursor_after_rect(full);
    nav
}

/// Reads the open markets again when they change and every few seconds (sooner after the game writes a market: see
/// [`show`]).
fn refresh_tiles(ctx: &egui::Context, db: &GalaxyDb, st: &mut MarketUi) {
    if st.prefs.tiles.is_empty() {
        return;
    }
    let ids = st.prefs.tiles.clone();
    let now = std::time::Instant::now();
    let stale = st.tiles_at.is_none_or(|t| t.elapsed() > TILES_EVERY);
    let again = st.tiles_again.is_some_and(|t| t <= now);
    if let Some(t) = st.tiles_again.filter(|t| *t > now) {
        ctx.request_repaint_after(t - now);
    }
    if (st.tiles_key.as_ref() != Some(&ids) || stale || again) && !st.tiles_live.busy() {
        if again {
            st.tiles_again = None;
        }
        st.tiles_key = Some(ids.clone());
        st.tiles_at = Some(now);
        let db = db.clone();
        st.tiles_live.run(ctx, move || {
            let mut out = FxHashMap::default();
            for id in ids {
                out.insert(id, db.station_market(id).map_err(|e| e.to_string())?);
            }
            Ok(out)
        });
    }
    ctx.request_repaint_after(TILES_EVERY);
}

/// A small heading in the Station tab's list, with room for a button on its right.
fn list_heading(ui: &mut Ui, text: &str, right: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).font(display(size::FOOTNOTE)).color(pal::DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

/// One market in the Station tab's list: its name (a carrier's with who may dock, when `access` is given: `Some(None)`
/// for never reported), a line under it, and a tick when its market is open. `open_able`: it has a market to open;
/// otherwise it's listed dimmed and doesn't react.
fn list_row(ui: &mut Ui, name: &str, carrier: bool, access: Option<Option<&str>>, detail: &[(String, Color32)], open: bool, open_able: bool) -> egui::Response {
    let sense = if open_able { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), sense);
    let resp = match access {
        Some(a) => resp.on_hover_text(match a {
            Some(a) => format!("Docking: {}, as last reported with its market. Its owner can change that at any time.", carrier_access(a)),
            None => "Who may dock was never reported for this carrier.".to_string(),
        }),
        None => resp,
    };
    let resp = if open_able { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    if open {
        p.rect_filled(rect, egui::CornerRadius::same(8), Color32::from_white_alpha(12));
    } else if open_able && resp.hovered() {
        p.rect_filled(rect, egui::CornerRadius::same(8), Color32::from_white_alpha(8));
    }
    let mark = if open {
        Some((icon::CHECK_CIRCLE, pal::ACCENT))
    } else if open_able && resp.hovered() {
        Some((icon::PLUS_CIRCLE, pal::DIM))
    } else {
        None
    };
    if let Some((glyph, col)) = mark {
        p.text(egui::pos2(rect.right() - 16.0, rect.center().y), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(17.0), col);
    }
    let left = rect.left() + 10.0;
    let room = (rect.right() - 34.0 - left).max(40.0);
    // The badge first, so the name gives up room before it does.
    let badge = access.map(|a| {
        let (text, col) = access_badge_text(a);
        (p.layout_no_wrap(text.to_string(), display(size::CAPTION), col), col)
    });
    let badge_w = badge.as_ref().map_or(0.0, |(g, _)| g.size().x + 16.0 + 6.0);
    // Single lines that end in "…" where they don't fit, each part in its own colour.
    let line = |parts: &[(String, Color32)], size: f32, width: f32| {
        let mut job = egui::text::LayoutJob::default();
        for (text, col) in parts {
            job.append(text, 0.0, egui::TextFormat::simple(egui::FontId::proportional(size), *col));
        }
        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
        p.layout_job(job)
    };
    let col = if open_able { pal::TEXT } else { pal::DIM };
    let title = line(&[(if carrier { format!("{} {name}", icon::ANCHOR) } else { name.to_string() }, col)], size::CALLOUT, room - badge_w);
    let sub = line(detail, size::CAPTION, room);
    let top = rect.center().y - (title.size().y + 2.0 + sub.size().y) / 2.0;
    let title_w = title.size().x;
    let title_h = title.size().y;
    p.galley(egui::pos2(left, top), title, col);
    if let Some((g, bcol)) = badge {
        let pad = egui::vec2(8.0, 2.0);
        let r = egui::Rect::from_min_size(egui::pos2(left + title_w + 6.0, top + (title_h - g.size().y) / 2.0 - pad.y), g.size() + pad * 2.0);
        p.rect_filled(r, egui::CornerRadius::same(255), bcol.gamma_multiply(0.18));
        p.galley(r.min + pad, g, bcol);
    }
    p.galley(egui::pos2(left, top + title_h + 2.0), sub, pal::DIM);
    resp
}

/// What a market says under its name in the list after `lead` (its kind, a note): how many goods it trades and how old
/// its prices are, the age in its colour from fresh to stale.
fn market_line(lead: &str, s: &StationInfo) -> Vec<(String, Color32)> {
    if s.market_updated_unix <= 0 {
        return vec![(format!("{lead}no market in the database"), pal::FAINT)];
    }
    let (a, col) = age(s.market_updated_unix);
    vec![(format!("{lead}{} goods · ", s.commodity_count), pal::DIM), (format!("reported {a} ago"), col)]
}

/// A plain line under a name in the list.
fn plain(text: &str) -> Vec<(String, Color32)> {
    vec![(text.to_string(), pal::DIM)]
}

/// The Station tab's list: a search for any station or carrier, the station you're docked at, this system's stations and
/// carriers (only those with a market in the database unless asked), and the markets opened lately. A click opens one.
fn market_list(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) {
    let mut open: Vec<u64> = Vec::new();
    let mut open_here: Option<Vec<u64>> = None;
    egui::Frame::new().fill(pal::CARD).corner_radius(egui::CornerRadius::same(14)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        if let Some(id) = station_search(ui, db, st) {
            open.push(id);
        }
        egui::ScrollArea::vertical().id_salt("station-list-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let tiles = &st.prefs.tiles;
            // The station you're docked at, or else the last market the game showed you.
            let docked = gs.location.station.as_ref().filter(|s| gs.location.docked && s.market_id != 0);
            match (docked, &gs.market) {
                (Some(s), _) => {
                    list_heading(ui, "Docked at", |_| {});
                    let carrier = s.station_type.eq_ignore_ascii_case("FleetCarrier");
                    if list_row(ui, &s.name, carrier, None, &plain(&gs.location.system), tiles.contains(&s.market_id), true).clicked() {
                        open.push(s.market_id);
                    }
                }
                (None, Some(m)) if m.market_id != 0 => {
                    list_heading(ui, "Last market you opened", |_| {});
                    let when = format!("{} · {}", m.star_system, m.timestamp.to_local_string("%d %b %H:%M"));
                    if list_row(ui, &m.station_name, false, None, &plain(&when), tiles.contains(&m.market_id), true).clicked() {
                        open.push(m.market_id);
                    }
                }
                _ => {}
            }
            here_list(ui, gs, db, st, docked.map(|s| s.market_id), &mut open, &mut open_here);
            if !st.prefs.recent_markets.is_empty() {
                list_heading(ui, "Opened lately", |_| {});
                for r in &st.prefs.recent_markets {
                    if list_row(ui, &r.name, r.carrier, None, &plain(&r.system), st.prefs.tiles.contains(&r.market_id), true).clicked() {
                        open.push(r.market_id);
                    }
                }
            }
            ui.add_space(10.0);
            crate::widgets::footnote(ui, "A station's name anywhere on the Market page opens its market here too.");
        });
    });
    if let Some(ids) = open_here {
        // In place of the markets open now: the one you're docked at, then this system's.
        let mut tiles: Vec<u64> = st.docked_market.into_iter().collect();
        for id in ids {
            if tiles.len() >= MAX_TILES {
                break;
            }
            if !tiles.contains(&id) {
                tiles.push(id);
            }
        }
        st.tiles_fresh.extend(tiles.iter().filter(|t| !st.prefs.tiles.contains(t)));
        st.prefs.tiles = tiles;
    }
    for id in open {
        st.open_tile(id, false);
    }
}

/// The search at the top of the Station tab's list: any station or carrier the database knows, by the start of its name
/// (a carrier goes by its ID, such as J7F-10F). Returns the market picked.
fn station_search(ui: &mut Ui, db: &GalaxyDb, st: &mut MarketUi) -> Option<u64> {
    let typed = st.station_find.trim().to_string();
    if typed.chars().count() >= 2 && typed != st.station_found_for && !st.station_found.busy() {
        st.station_found_for.clone_from(&typed);
        let db = db.clone();
        st.station_found.run(ui.ctx(), move || db.find_stations(&typed, 12).map_err(|e| e.to_string()));
    }
    let found: &[StationInfo] = match &st.station_found.last {
        Some(Ok(v)) => v,
        _ => &[],
    };
    let width = ui.available_width();
    // Names repeat across systems: each suggestion carries its system, so the one picked is the one opened.
    let label = |s: &StationInfo| format!("{} ({})", s.name, s.system);
    let c = crate::widgets::autocomplete(ui, "station-find", &mut st.station_find, "Open a station or carrier ID…", width, |t| {
        let q = t.trim().to_lowercase();
        if q.chars().count() < 2 {
            return Vec::new();
        }
        found
            .iter()
            .filter(|s| s.name.to_lowercase().starts_with(&q))
            .map(|s| {
                let what = if s.market_updated_unix > 0 { format!("{} goods", s.commodity_count) } else { "no market".to_string() };
                Suggestion { text: label(s), detail: what, starred: false }
            })
            .collect()
    });
    let picked = c.picked.and_then(|p| found.iter().find(|s| label(s) == p).map(|s| s.market_id));
    if picked.is_some() {
        st.station_find.clear();
    }
    picked
}

/// The list's section for this system: its stations, then the carriers the game listed since you arrived, then those the
/// database still places here. Only markets the database has are listed until asked for the rest. "Open all" (and a
/// screenshot run's first frame) asks for this system's markets side by side, through `open_here`.
fn here_list(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi, docked: Option<u64>, open: &mut Vec<u64>, open_here: &mut Option<Vec<u64>>) {
    let listed = carriers_here(gs);
    let key = (gs.location.system_address, listed.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>());
    let stale = st.here_at.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(60));
    if (st.here_key.as_ref() != Some(&key) || stale) && !st.here.busy() {
        st.here_key = Some(key.clone());
        st.here_at = Some(std::time::Instant::now());
        let db = db.clone();
        let job = key.clone();
        st.here.run(ui.ctx(), move || {
            let mut known = FxHashMap::default();
            for c in &job.1 {
                if let Some(s) = db.find_stations(c, 4).map_err(|e| e.to_string())?.into_iter().find(|s| s.is_carrier && s.name.eq_ignore_ascii_case(c)) {
                    known.insert(c.clone(), s);
                }
            }
            let recorded = db.stations_in_system(job.0).map_err(|e| e.to_string())?;
            Ok((known, recorded))
        });
    }
    let system = if gs.location.system.is_empty() { "this system".to_string() } else { gs.location.system.clone() };
    // Only this system's answer (right after a jump the last one is still the previous system's); while the same question
    // is asked again, the answer before stays.
    let view = match &st.here.last {
        Some(Ok(v)) if st.here_for.as_ref() == Some(&key) => Some(v),
        _ => None,
    };
    let Some((known, recorded)) = view else {
        list_heading(ui, &format!("In {system}"), |ui| {
            ui.spinner();
        });
        return;
    };
    let has_market = |s: &StationInfo| s.market_updated_unix > 0;
    let mut stations: Vec<&StationInfo> = recorded.iter().filter(|s| !s.is_carrier && Some(s.market_id) != docked).collect();
    stations.sort_by(|a, b| has_market(b).cmp(&has_market(a)).then_with(|| cmp_text(&a.name, &b.name)));
    let gone: Vec<&StationInfo> = recorded.iter().filter(|s| s.is_carrier && !listed.iter().any(|(_, c)| c.eq_ignore_ascii_case(&s.name))).collect();
    let mut carriers: Vec<(&str, Option<&StationInfo>)> = listed.iter().map(|(name, cs)| (name.as_str(), known.get(cs))).collect();
    carriers.sort_by(|a, b| a.1.is_some_and(has_market).cmp(&b.1.is_some_and(has_market)).reverse().then_with(|| cmp_text(a.0, b.0)));
    // The markets that can be opened here, in list order.
    let openable: Vec<u64> = stations
        .iter()
        .copied()
        .chain(carriers.iter().filter_map(|c| c.1))
        .chain(gone.iter().copied())
        .filter(|s| has_market(s))
        .map(|s| s.market_id)
        .collect();
    let unknown = stations.iter().filter(|s| !has_market(s)).count() + carriers.iter().filter(|c| !c.1.is_some_and(has_market)).count();
    if std::mem::take(&mut st.autorun) {
        // A screenshot run of the Station tab: four markets side by side, this system's first, then the nearest stations'.
        let mut ids: Vec<u64> = openable.iter().copied().take(4).collect();
        if let Some(origin) = gs.location.star_pos.filter(|_| ids.len() < 4) {
            let q = StationQuery { origin, services: Services::MARKET, limit: 12, ..Default::default() };
            for h in db.stations_near(&q).unwrap_or_default() {
                if ids.len() < 4 && h.station.commodity_count > 5 && !ids.contains(&h.station.market_id) {
                    ids.push(h.station.market_id);
                }
            }
        }
        *open_here = Some(ids);
    }
    let all = st.prefs.tiles_unknown;
    list_heading(ui, &format!("In {system}"), |ui| {
        if openable.len() >= 2 && crate::widgets::button(ui, "Open all").on_hover_text("This system's markets side by side, up to eight, in place of the ones open now (the one you're docked at stays)").clicked() {
            *open_here = Some(openable.clone());
        }
    });
    let tiles = &st.prefs.tiles;
    for s in &stations {
        if !all && !has_market(s) {
            continue;
        }
        let kind = s.station_type.as_deref().map(|t| format!("{t} · ")).unwrap_or_default();
        if list_row(ui, &s.name, false, None, &market_line(&kind, s), tiles.contains(&s.market_id), has_market(s)).clicked() {
            open.push(s.market_id);
        }
    }
    for (name, s) in &carriers {
        match s {
            Some(s) if all || has_market(s) => {
                if list_row(ui, name, true, Some(s.carrier_docking_access.as_deref()), &market_line("", s), tiles.contains(&s.market_id), has_market(s)).clicked() {
                    open.push(s.market_id);
                }
            }
            None if all => {
                list_row(ui, name, true, None, &[("market not in the database".to_string(), pal::FAINT)], false, false);
            }
            _ => {}
        }
    }
    for s in gone.iter().filter(|s| has_market(s)) {
        // Missing from a list the game has made: probably gone. With no list yet, nothing is known.
        let lead = if listed.is_empty() { "not listed yet this visit · " } else { "not seen since you arrived · " };
        let r = list_row(ui, &s.name, true, Some(s.carrier_docking_access.as_deref()), &market_line(lead, s), tiles.contains(&s.market_id), true);
        let r = r.on_hover_text(if listed.is_empty() {
            "The database places this carrier here; the game hasn't listed any carrier yet this visit."
        } else {
            "The database places this carrier here, but the game hasn't listed it since you arrived: it has probably jumped away. Its market is the last one reported."
        });
        if r.clicked() {
            open.push(s.market_id);
        }
    }
    if unknown > 0 {
        ui.add_space(4.0);
        let what = if all { "Hide those without a known market".to_string() } else { format!("{} {unknown} more without a known market", icon::CARET_DOWN) };
        let tip = "Elite Nexus knows a market once a commander who shares data, or you, opened it (or the Spansh import had it). Without one there's nothing to open yet.";
        if ui.add(egui::Button::new(RichText::new(what).size(size::FOOTNOTE).color(pal::ACCENT_SOFT)).frame(false).wrap_mode(egui::TextWrapMode::Truncate)).on_hover_text(tip).clicked() {
            st.prefs.tiles_unknown = !all;
        }
    }
    if stations.is_empty() && carriers.is_empty() && gone.is_empty() {
        crate::widgets::footnote(ui, "The database knows no station here.");
    }
    if listed.is_empty() {
        crate::widgets::footnote(ui, "The game lists a system's carriers as they show up in your navigation panel: on arrival, or when you drop out of supercruise. None has been listed yet this visit.");
    }
}

/// The best price among the open markets for each good: the lowest to buy at and the highest to sell at, with how many
/// open markets trade it that way (a best among one is no best).
#[derive(Clone, Copy, Default)]
struct Best {
    buy: i64,
    buys: u8,
    sell: i64,
    sells: u8,
}

fn best_prices(live: Option<&FxHashMap<u64, Option<StationMarket>>>, tiles: &[u64]) -> FxHashMap<String, Best> {
    let mut out: FxHashMap<String, Best> = FxHashMap::default();
    let Some(live) = live else { return out };
    for id in tiles {
        let Some(Some(m)) = live.get(id) else { continue };
        for i in &m.items {
            let b = out.entry(i.symbol.clone()).or_default();
            if i.buy > 0 && i.stock > 0 {
                b.buy = if b.buys == 0 { i.buy } else { b.buy.min(i.buy) };
                b.buys = b.buys.saturating_add(1);
            }
            if i.sell > 0 && i.demand > 0 {
                b.sell = b.sell.max(i.sell);
                b.sells = b.sells.saturating_add(1);
            }
        }
    }
    out
}

/// What every market tile needs to know besides its own market.
struct TileCtx<'a> {
    origin: Option<[f64; 3]>,
    prefs: &'a MarketPrefs,
    /// The goods filter, lower case.
    filter: &'a str,
    best: &'a FxHashMap<String, Best>,
    docked: Option<u64>,
    /// The market just opened and how lit it still is (1 to 0).
    flash: Option<(u64, f32)>,
}

enum TileAction {
    Close,
    Nav(Nav),
    Pin(Pin),
}

/// The open markets side by side under a bar: show or hide the list, filter the goods, which of them, close them all.
/// The tiles fill the room: as many columns as fit at [`TILE_MIN_W`], rows sharing the height, and a last row's tiles
/// sharing its width.
fn market_tiles(ui: &mut Ui, gs: &GameState, st: &mut MarketUi) -> Option<Nav> {
    // The Recent list learns the names of the markets opened, once they have loaded.
    if let Some(Ok(live)) = &st.tiles_live.last {
        for id in std::mem::take(&mut st.tiles_fresh) {
            match live.get(&id) {
                Some(Some(m)) => st.prefs.remember_market(&m.station),
                Some(None) => {}
                // Not loaded yet; one closed before it loaded is forgotten.
                None if st.prefs.tiles.contains(&id) => st.tiles_fresh.push(id),
                None => {}
            }
        }
    }
    let n = st.prefs.tiles.len();
    let mut close_all = false;
    // The count and Close all on the right: on the same line where there's room, under it in a narrow window.
    let right = |ui: &mut Ui, close_all: &mut bool| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if n > 0 && crate::widgets::button(ui, "Close all").clicked() {
                *close_all = true;
            }
            ui.label(RichText::new(format!("{n} of {MAX_TILES} open")).size(size::CAPTION).color(pal::FAINT));
        });
    };
    let wide = ui.available_width() >= 700.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let tip = if st.prefs.tiles_list { "Hide the list of markets" } else { "Show the list of markets" };
        if crate::widgets::icon_button(ui, icon::SIDEBAR_SIMPLE, tip).clicked() {
            st.prefs.tiles_list = !st.prefs.tiles_list;
        }
        crate::widgets::search_field(ui, &mut st.tile_filter, "Filter goods", 190.0).on_hover_text("Shows only the goods whose name or category holds this, in every market");
        if wide {
            crate::widgets::segmented(ui, &mut st.prefs.tiles_show, &[(TileShow::All, "All goods"), (TileShow::Sells, "Buy here"), (TileShow::Buys, "Sell here")]);
        }
        right(ui, &mut close_all);
    });
    if !wide {
        ui.add_space(6.0);
        crate::widgets::segmented(ui, &mut st.prefs.tiles_show, &[(TileShow::All, "All goods"), (TileShow::Sells, "Buy here"), (TileShow::Buys, "Sell here")]);
    }
    if close_all {
        st.prefs.tiles.clear();
    }
    ui.add_space(8.0);
    if n == 0 {
        empty(
            ui,
            icon::STOREFRONT,
            "No market open",
            "Pick one in the list, search for a station or carrier, or click a station's name anywhere on the Market page. Up to eight open side by side, and a market you open in the game opens here by itself.",
        );
        return None;
    }
    let live = match &st.tiles_live.last {
        Some(Ok(l)) => Some(l),
        _ => None,
    };
    if let Some(Err(e)) = &st.tiles_live.last {
        ui.label(RichText::new(e).color(pal::BAD));
    }
    let tiles = st.prefs.tiles.clone();
    let best = best_prices(live, &tiles);
    let filter = st.tile_filter.trim().to_lowercase();
    let flash = st.tile_flash.and_then(|(id, t)| {
        let left = 1.0 - t.elapsed().as_secs_f32() / FLASH;
        (left > 0.0).then_some((id, left))
    });
    if flash.is_some() {
        ui.ctx().request_repaint();
    }
    let cx = TileCtx { origin: gs.location.star_pos, prefs: &st.prefs, filter: &filter, best: &best, docked: st.docked_market, flash };
    let area = ui.available_rect_before_wrap();
    let gap = 12.0;
    let cols = tile_columns(n, area.width(), area.height(), gap);
    let rows = n.div_ceil(cols);
    let tile_h = ((area.height() - gap * (rows as f32 - 1.0)) / rows as f32).max(TILE_MIN_H);
    let content_h = tile_h * rows as f32 + gap * (rows as f32 - 1.0);
    let mut sort = TableSort::load(ui, "db-market");
    let mut acted: Option<(u64, TileAction)> = None;
    // Too many rows for the height: the tiles keep a usable height and the area scrolls.
    egui::ScrollArea::vertical().id_salt("station-tiles-scroll").auto_shrink([false, false]).show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(area.width(), content_h), egui::Sense::hover());
        for (i, id) in tiles.iter().enumerate() {
            let (r, c) = (i / cols, i % cols);
            let in_row = (n - r * cols).min(cols);
            let w = (rect.width() - gap * (in_row as f32 - 1.0)) / in_row as f32;
            let at = egui::pos2(rect.left() + c as f32 * (w + gap), rect.top() + r as f32 * (tile_h + gap));
            let tile = egui::Rect::from_min_size(at, egui::vec2(w, tile_h));
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt(("market-tile", i)).max_rect(tile).layout(egui::Layout::top_down(egui::Align::Min)));
            // Nothing a tile holds spills onto its neighbours.
            child.set_clip_rect(tile.expand(1.0).intersect(ui.clip_rect()));
            if let Some(a) = market_tile(&mut child, *id, live.and_then(|l| l.get(id)), &cx, &mut sort) {
                acted = Some((*id, a));
            }
        }
    });
    match acted {
        Some((id, TileAction::Close)) => {
            st.prefs.tiles.retain(|t| *t != id);
            None
        }
        Some((_, TileAction::Pin(p))) => {
            st.prefs.toggle_pin(p);
            None
        }
        Some((_, TileAction::Nav(n))) => Some(n),
        None => None,
    }
}

/// One open market: its name, where it is and how fresh, and its goods in a table that scrolls inside the tile. The
/// best price among the open markets stands out on a tinted capsule.
fn market_tile(ui: &mut Ui, id: u64, m: Option<&Option<StationMarket>>, cx: &TileCtx, sort: &mut TableSort) -> Option<TileAction> {
    let lit = cx.flash.filter(|(f, _)| *f == id).map(|(_, left)| left);
    let stroke = match lit {
        Some(left) => egui::Stroke::new(1.5, pal::ACCENT.gamma_multiply(left)),
        None => egui::Stroke::new(1.0, Color32::from_white_alpha(10)),
    };
    let mut action = None;
    egui::Frame::new()
        .fill(pal::CARD)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin { left: 12, right: 6, top: 6, bottom: 8 })
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            match m {
                Some(Some(m)) => {
                    tile_header(ui, m, cx, &mut action);
                    ui.add_space(2.0);
                    tile_table(ui, m, cx, sort, &mut action);
                }
                Some(None) => {
                    tile_close_only(ui, &mut action);
                    tile_note(ui, "Not in the database", "No market with this id is stored: it may have been a fleet carrier that has since been decommissioned.");
                }
                None => {
                    tile_close_only(ui, &mut action);
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| ui.spinner());
                }
            }
        });
    action
}

/// Why a tile shows no goods, small and centred so it fits a low tile.
fn tile_note(ui: &mut Ui, title: &str, hint: &str) {
    ui.add_space((ui.available_height() * 0.25).clamp(8.0, 60.0));
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon::STOREFRONT).size(26.0).color(pal::FAINT));
        ui.label(RichText::new(title).font(display(size::HEADLINE)).color(pal::TEXT));
        ui.scope(|ui| {
            ui.set_max_width(ui.available_width().min(360.0));
            ui.add(egui::Label::new(RichText::new(hint).size(size::FOOTNOTE).color(pal::DIM)).wrap().halign(egui::Align::Center));
        });
    });
}

/// A tile's close button on its own (while it loads, or when there's nothing to show).
fn tile_close_only(ui: &mut Ui, action: &mut Option<TileAction>) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        if crate::widgets::icon_button(ui, icon::X, "Close this market").clicked() {
            *action = Some(TileAction::Close);
        }
    });
}

/// A tile's head: the station (a carrier with who may dock), Docked when you are, the close button; then its system
/// (click to copy), how far, its pad, its distance from the star and how old its prices are.
fn tile_header(ui: &mut Ui, m: &StationMarket, cx: &TileCtx, action: &mut Option<TileAction>) {
    let s = &m.station;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::widgets::icon_button(ui, icon::X, "Close this market").clicked() {
                *action = Some(TileAction::Close);
            }
            if cx.docked == Some(s.market_id) {
                badge(ui, "Docked", pal::GOOD).on_hover_text("You're docked here");
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let title = RichText::new(if s.is_carrier { format!("{} {}", icon::ANCHOR, s.name) } else { s.name.clone() }).font(display(size::HEADLINE)).color(pal::TEXT);
                if s.is_carrier {
                    named_with_access(ui, title, s.carrier_docking_access.as_deref(), |ui, t| {
                        ui.add(egui::Label::new(t).truncate());
                    });
                } else {
                    ui.add(egui::Label::new(title).truncate());
                }
            });
        });
    });
    let mut facts = Vec::new();
    if let (Some(o), Some(p)) = (cx.origin, s.pos) {
        facts.push(format!("{:.1} ly", ((o[0] - p[0]).powi(2) + (o[1] - p[1]).powi(2) + (o[2] - p[2]).powi(2)).sqrt()));
    }
    if let Some(p) = s.max_pad {
        facts.push(format!("{p} pad"));
    }
    if let Some(ls) = s.dist_ls {
        facts.push(format!("{} ls", nexus_data::names::group_digits(ls.round() as i64)));
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
        if nav_label(ui, RichText::new(&s.system).size(size::FOOTNOTE).color(pal::DIM), "Click to copy the system") {
            ui.ctx().copy_text(s.system.clone());
        }
        if !facts.is_empty() {
            ui.label(RichText::new(format!("· {}", facts.join(" · "))).size(size::FOOTNOTE).color(pal::DIM));
        }
        if s.market_updated_unix > 0 {
            let (a, col) = age(s.market_updated_unix);
            ui.label(RichText::new(format!("· {a} ago")).size(size::FOOTNOTE).color(col)).on_hover_text("When this market was last reported");
        }
        if !m.prohibited.is_empty() {
            ui.label(RichText::new(format!("· {} {} prohibited", icon::PROHIBIT, m.prohibited.len())).size(size::FOOTNOTE).color(pal::DIM))
                .on_hover_text(format!("Prohibited here: {}", m.prohibited.join(", ")));
        }
    });
}

/// A price in a market tile: a tinted capsule when it's the best among the open markets.
fn tile_price(ui: &mut Ui, v: i64, col: Color32, best: bool) -> egui::Response {
    let text = nexus_data::names::group_digits(v);
    if !best {
        return ui.label(RichText::new(text).color(col));
    }
    let g = ui.painter().layout_no_wrap(text, display(size::CALLOUT), col);
    let pad = egui::vec2(7.0, 2.0);
    let (rect, r) = ui.allocate_exact_size(g.size() + pad * 2.0, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(255), col.gamma_multiply(0.2));
        ui.painter().galley(rect.min + pad, g, col);
    }
    r
}

/// A tile's goods: what it sells (Buy) and buys (Sell), with stock, demand and the galactic average when it's wide
/// enough. Sorted as every tile is (a heading sorts them all), filtered by the bar above the tiles.
fn tile_table(ui: &mut Ui, m: &StationMarket, cx: &TileCtx, sort: &mut TableSort, action: &mut Option<TileAction>) {
    let s = &m.station;
    let trades = |i: &nexus_galaxy::MarketRow| ((i.buy > 0 && i.stock > 0), (i.sell > 0 && i.demand > 0));
    let rows: Vec<&nexus_galaxy::MarketRow> = m
        .items
        .iter()
        .filter(|i| {
            let (sells, buys) = trades(i);
            let side = match cx.prefs.tiles_show {
                TileShow::All => sells || buys,
                TileShow::Sells => sells,
                TileShow::Buys => buys,
            };
            side && (cx.filter.is_empty() || i.name.to_lowercase().contains(cx.filter) || i.category.to_lowercase().contains(cx.filter))
        })
        .collect();
    if rows.is_empty() {
        let any = m.items.iter().any(|i| {
            let (a, b) = trades(i);
            a || b
        });
        let (title, hint) = if any {
            ("Nothing to show", "None of its goods matches the filter above.")
        } else if !m.items.is_empty() {
            ("Nothing for sale or wanted", "Its latest report lists goods, but none in stock and none wanted: a fleet carrier often has no orders set.")
        } else {
            ("No prices yet", "The database has no prices for this market. They arrive when a commander who shares data opens it, or when you do.")
        };
        tile_note(ui, title, hint);
        return;
    }
    // Narrow tiles keep the prices; stock, demand and the average come back as the tile widens.
    let w = ui.available_width();
    let layout = if w >= 520.0 { 2 } else if w >= 360.0 { 1 } else { 0 };
    // Narrow, a long name takes two lines rather than ending in "…".
    let (price_w, row_h) = if layout == 0 { (62.0, 38.0) } else { (76.0, 30.0) };
    // Keyed by market: its widths and where it was scrolled stay with it when the tiles move.
    crate::widgets::table(ui, &format!("market-tile/{}/{layout}", m.station.market_id), |t| {
        // A low tile's table scrolls in what's left of it (egui_extras would take at least 200 points).
        let mut t = t.vscroll(true).auto_shrink([false, false]).min_scrolled_height(0.0).column(Column::exact(22.0).resizable(false)).column(crate::widgets::flex_column(90.0)).column(Column::initial(price_w).at_least(50.0).clip(true));
        if layout == 2 {
            t = t.column(Column::initial(64.0).at_least(40.0).clip(true));
        }
        t = t.column(Column::initial(price_w).at_least(50.0).clip(true));
        if layout == 2 {
            t = t.column(Column::initial(64.0).at_least(40.0).clip(true));
        }
        if layout >= 1 {
            t = t.column(Column::initial(54.0).at_least(40.0).clip(true));
        }
        // Sortable columns, as every tile has them: 0 commodity, 2 buy price, 3 stock, 4 sell price, 5 demand, 6 against
        // the average.
        let mut heads: Heads = vec![("", None), ("Commodity", Some((0, false))), ("Buy", Some((2, false)))];
        if layout == 2 {
            heads.push(("Stock", Some((3, true))));
        }
        heads.push(("Sell", Some((4, true))));
        if layout == 2 {
            heads.push(("Demand", Some((5, true))));
        }
        if layout >= 1 {
            heads.push(("vs avg", Some((6, true))));
        }
        let t = t.header(26.0, |mut h| headings(&mut h, &heads, sort, "db-market"));
        let desc = sort.descending;
        // A price the station doesn't trade at: last whichever way the column is sorted.
        let price = move |v: i64, qty: i64| if v > 0 && qty > 0 { Some(v as f64) } else if desc { Some(f64::NEG_INFINITY) } else { None };
        // How far its price is from the galactic average, on the side it trades (good for you = high).
        let deal = |i: &&nexus_galaxy::MarketRow| {
            if i.mean <= 0 {
                None
            } else if i.buy > 0 && i.stock > 0 {
                Some((i.mean - i.buy) as f64 / i.mean as f64)
            } else {
                Some((i.sell - i.mean) as f64 / i.mean as f64)
            }
        };
        let none_last = move |v: Option<f64>| v.or(if desc { Some(f64::NEG_INFINITY) } else { None });
        let order = sort.order(&rows, |a, b, c| match c {
            0 => cmp_text(&a.name, &b.name),
            2 => cmp_opt(price(a.buy, a.stock), price(b.buy, b.stock)),
            3 => a.stock.cmp(&b.stock),
            4 => cmp_opt(price(a.sell, a.demand), price(b.sell, b.demand)),
            5 => a.demand.cmp(&b.demand),
            6 => cmp_opt(none_last(deal(a)), none_last(deal(b))),
            // Another tab's column (the category): the database's order, by category then name.
            _ => std::cmp::Ordering::Equal,
        });
        t.body(|body| {
            body.rows(row_h, rows.len(), |mut row| {
                let i = rows[order[row.index()]];
                let (sells, buys) = trades(i);
                let best = cx.best.get(&i.symbol).copied().unwrap_or_default();
                row.col(|ui| {
                    // Pins the side it trades (where it buys, when it does both).
                    if pin_button(ui, cx.prefs.is_pinned(s.market_id, &i.symbol, buys)) {
                        let (price, qty) = if buys { (i.sell, i.demand) } else { (i.buy, i.stock) };
                        *action = Some(TileAction::Pin(Pin {
                            market_id: s.market_id,
                            station: s.name.clone(),
                            system: s.system.clone(),
                            carrier: s.is_carrier,
                            symbol: i.symbol.clone(),
                            name: i.name.clone(),
                            sell_here: buys,
                            price,
                            qty,
                            ..Default::default()
                        }));
                    }
                });
                row.col(|ui| {
                    let tip = format!("{} ({}): where to buy and sell it, latest prices", i.name, i.category);
                    let clicked = if layout == 0 {
                        let r = ui.add(egui::Label::new(RichText::new(&i.name).size(size::FOOTNOTE).color(pal::TEXT)).wrap().sense(egui::Sense::click()));
                        let r = r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
                        if r.hovered() {
                            ui.painter().hline(r.rect.x_range(), r.rect.bottom() - 1.0, egui::Stroke::new(1.0, pal::DIM));
                        }
                        r.clicked()
                    } else {
                        nav_label(ui, RichText::new(&i.name).color(pal::TEXT), &tip)
                    };
                    if clicked {
                        *action = Some(TileAction::Nav(Nav::Commodity { name: i.symbol.clone(), side: None }));
                    }
                });
                let avg = if i.mean > 0 { format!(" · galactic average {}", cr(i.mean)) } else { String::new() };
                row.col(|ui| {
                    if sells {
                        let top = best.buys >= 2 && best.buy == i.buy;
                        let r = tile_price(ui, i.buy, pal::CYAN, top);
                        let more = if top { "\nThe cheapest of the open markets" } else { "" };
                        r.on_hover_text(format!("{} Cr · {} in stock{avg}{more}", nexus_data::names::group_digits(i.buy), nexus_data::names::group_digits(i.stock)));
                    }
                });
                if layout == 2 {
                    row.col(|ui| {
                        if sells {
                            ui.label(RichText::new(nexus_data::names::group_digits(i.stock)).color(pal::DIM));
                        }
                    });
                }
                row.col(|ui| {
                    if buys {
                        let top = best.sells >= 2 && best.sell == i.sell;
                        let r = tile_price(ui, i.sell, pal::GOOD, top);
                        let more = if top { "\nThe best price of the open markets" } else { "" };
                        r.on_hover_text(format!("{} Cr · {} wanted{avg}{more}", nexus_data::names::group_digits(i.sell), nexus_data::names::group_digits(i.demand)));
                    }
                });
                if layout == 2 {
                    row.col(|ui| {
                        if buys {
                            ui.label(RichText::new(nexus_data::names::group_digits(i.demand)).color(pal::DIM));
                        }
                    });
                }
                if layout >= 1 {
                    row.col(|ui| {
                        // Against the side the station trades: what it sells is judged as a purchase.
                        if sells {
                            vs_average(ui, i.buy, Some(i.mean), true);
                        } else if buys {
                            vs_average(ui, i.sell, Some(i.mean), false);
                        }
                    });
                }
            });
        });
    });
}

fn mb(bytes: u64) -> String {
    if bytes >= 1 << 30 { format!("{:.2} GB", bytes as f64 / (1u64 << 30) as f64) } else { format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64) }
}

fn database_tab(ui: &mut Ui, svc: &mut MarketService, st: &mut MarketUi, rt: &tokio::runtime::Handle) {
    let digits = |n: u64| nexus_data::names::group_digits(n as i64);
    crate::widgets::cards(ui, 2, 380.0, |c| {
      c.add(|ui| {
        card(ui, "Local galaxy database", |ui| match svc.db_stats().cloned() {
            Some(s) => {
                egui::Grid::new("dbstats").max_col_width(ui.available_width()).num_columns(2).spacing([16.0, 3.0]).show(ui, |ui| {
                    for (k, v) in [
                        ("Stations", digits(s.stations)),
                        ("Markets", digits(s.stations_with_market)),
                        ("Prices", digits(s.market_rows)),
                        ("Systems", digits(s.systems)),
                        ("Ring hotspots", digits(s.ring_signals)),
                        ("Faction entries", digits(s.faction_rows)),
                        ("Price history points", s.price_history_rows.map_or_else(|| "counting…".to_string(), digits)),
                        ("Size on disk", mb(s.db_bytes)),
                    ] {
                        ui.label(RichText::new(k).color(pal::DIM));
                        ui.label(RichText::new(v).font(display(14.0)));
                        ui.end_row();
                    }
                    ui.label(RichText::new("Newest market").color(pal::DIM));
                    match s.newest_market_update {
                        Some(t) => {
                            let (a, col) = age(t);
                            ui.label(RichText::new(format!("{a} ago")).color(col))
                        }
                        None => ui.label(RichText::new("none yet").color(pal::FAINT)),
                    };
                    ui.end_row();
                });
                ui.label(
                    RichText::new(format!("{} snapshots stored this session, {} older than what was already known", digits(s.market_snapshots), digits(s.stale_markets)))
                        .small().color(pal::DIM),
                );
            }
            None => {
                ui.label(RichText::new("No statistics yet.").color(pal::DIM));
            }
        });
        ui.add_space(8.0);
        card(ui, "Live feed (EDDN)", |ui| match svc.feed_snapshot() {
            Some(f) => {
                ui.horizontal(|ui| {
                    badge(ui, if f.connected { "Connected" } else { "Connecting" }, if f.connected { pal::GOOD } else { pal::WARN });
                    ui.label(RichText::new(format!("{} msgs/min", f.messages_per_minute)).font(display(15.0)).color(pal::ACCENT_SOFT));
                });
                ui.label(
                    RichText::new(format!(
                        "{} received · {} stored · {} duplicates · {} test messages skipped",
                        digits(f.total_messages),
                        digits(f.stored),
                        digits(f.duplicates),
                        digits(f.test_dropped)
                    ))
                    .small()
                    .color(pal::DIM),
                );
                if f.decode_errors > 0 || f.dropped_backlog > 0 {
                    ui.label(RichText::new(format!("{} undecodable, {} dropped (backlog)", f.decode_errors, f.dropped_backlog)).small().color(pal::WARN));
                }
                if let Some(e) = &f.last_error {
                    ui.label(RichText::new(e).small().color(pal::BAD));
                }
                ui.add_space(4.0);
                for (schema, n) in f.per_schema.iter().take(8) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(schema).small().color(pal::DIM));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(digits(*n)).small().color(pal::DIM));
                        });
                    });
                }
                // The stream itself lives here, not over the other tabs (the status bar shows the newest message).
                if !f.recent_headlines.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Latest messages").size(size::FOOTNOTE).color(pal::TEXT));
                    for h in f.recent_headlines.iter().take(10) {
                        ui.add(egui::Label::new(RichText::new(format!("{} {}", icon::BROADCAST, h.text)).size(size::FOOTNOTE).color(pal::DIM)).truncate());
                    }
                }
            }
            None => {
                ui.label(RichText::new("Off. Turn on \"Receive the live market feed\" in Accounts & sync.").color(pal::DIM));
            }
        });
      });
      c.add(|ui| {
        card(ui, "Baseline import (Spansh)", |ui| {
            ui.label(
                RichText::new(
                    "The live feed only sees markets that commanders visit while Elite Nexus runs. For a complete picture from day one, \
                     import Spansh's daily dump of every station: services, landing pads and the last known market of each.",
                )
                .color(pal::DIM),
            );
            ui.label(
                RichText::new(
                    "About 4.5 GB is streamed straight into the database (nothing else is saved; the database grows to roughly 1 GB). \
                     Takes from a few minutes to half an hour depending on your connection. The live feed keeps running meanwhile, \
                     and newer EDDN prices are never overwritten.",
                )
                .small().color(pal::DIM),
            );
            ui.add_space(6.0);
            let running = svc.import().is_some_and(|j| j.running());
            ui.add_enabled_ui(!running, |ui| {
                if crate::widgets::primary_button(ui, &format!("{} Download and import all stations", icon::DOWNLOAD_SIMPLE)).clicked() {
                    let ctx = ui.ctx().clone();
                    svc.start_import(ImportSource::Url(SPANSH_STATIONS_URL.into()), &ctx, rt);
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.import_path).hint_text(r"…or a dump you downloaded, e.g. D:\galaxy_stations.json.gz").desired_width(320.0));
                    if ui.button("Import file").clicked() && !st.import_path.trim().is_empty() {
                        let ctx = ui.ctx().clone();
                        let path = std::path::PathBuf::from(st.import_path.trim().trim_matches('"'));
                        svc.start_import(ImportSource::File(path), &ctx, rt);
                    }
                });
            });
            if let Some(job) = svc.import() {
                ui.add_space(6.0);
                import_progress(ui, job);
            }
        });
        ui.add_space(8.0);
        card(ui, "Where the data comes from", |ui| {
            for line in [
                "EDDN: every commander running EDMC, EDDiscovery, EDDI or Elite Nexus publishes station data the moment they dock. It arrives here in real time, usually before Inara or Spansh have processed it.",
                "Your own journal: every station you dock at and every market you open.",
                "Spansh dump: the daily baseline above.",
            ] {
                ui.add(egui::Label::new(RichText::new(format!("• {line}")).small().color(pal::DIM)).wrap());
            }
        });
      });
    });
}

fn import_progress(ui: &mut Ui, job: &ImportJob) {
    let digits = |n: u64| nexus_data::names::group_digits(n as i64);
    let s = job.status.lock();
    ui.label(RichText::new(format!("{} {}", icon::DATABASE, s.source)).small().color(pal::DIM));
    let p = &s.progress;
    let frac = s.total_bytes.map(|t| (p.bytes_read as f64 / t.max(1) as f64).clamp(0.0, 1.0) as f32);
    let elapsed = s.started.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
    let eta = frac.filter(|f| *f > 0.01 && s.finished.is_none()).map(|f| elapsed * (1.0 - f as f64) / f as f64);
    let text = match (frac, eta) {
        (Some(f), Some(e)) => format!("{:.1}% · {} of {} · about {} left", f * 100.0, mb(p.bytes_read), mb(s.total_bytes.unwrap_or(0)), duration(e as i64)),
        (Some(f), None) => format!("{:.1}% · {}", f * 100.0, mb(p.bytes_read)),
        _ => mb(p.bytes_read),
    };
    bar(ui, frac.unwrap_or(0.0), if s.finished.is_some() { pal::GOOD } else { pal::ACCENT }, 12.0, Some(&text));
    ui.label(
        RichText::new(format!(
            "{} systems · {} stations · {} markets · {} prices · {} hotspots",
            digits(p.systems),
            digits(p.stations),
            digits(p.markets),
            digits(p.market_rows),
            digits(p.ring_signals)
        ))
        .small()
        .color(pal::DIM),
    );
    let finished = s.finished.clone();
    drop(s);
    match &finished {
        None => {
            if ui.button(format!("{} Cancel", icon::X_CIRCLE)).on_hover_text("What was imported so far is kept").clicked() {
                job.cancel();
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        Some(Ok(sum)) => {
            let what = if sum.cancelled { "Cancelled" } else if sum.stream_error.is_some() { "Stopped early" } else { "Done" };
            ui.label(RichText::new(format!("{} {what} after {}", icon::CHECK_CIRCLE, duration(sum.progress.elapsed.as_secs() as i64))).color(pal::GOOD));
            if sum.stale_markets > 0 {
                ui.label(RichText::new(format!("{} markets kept from newer live data", digits(sum.stale_markets))).small().color(pal::DIM));
            }
            if let Some(e) = &sum.stream_error {
                ui.label(RichText::new(format!("Download interrupted ({e}). Everything before it was imported; run it again to finish.")).small().color(pal::WARN));
            }
        }
        Some(Err(e)) => {
            ui.label(RichText::new(format!("{} {e}", icon::WARNING)).color(pal::BAD));
        }
    }
}

/// Where to sell what is in the hold: best single stops (everything they buy, limited by demand)
/// and the best buyers of each commodity. Re-planned whenever the hold or the system changes.
fn sell_tab(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) -> Option<Nav> {
    let hold = finder::sellable_hold(gs);
    if hold.is_empty() {
        empty(ui, icon::PACKAGE, "Nothing to sell", "No sellable cargo in your hold (mission cargo, stolen goods and limpets are left out).");
        return None;
    }
    let Some(origin) = gs.location.star_pos else {
        empty(ui, icon::MAP_PIN, "Position unknown", "Jump or relog once so Elite Nexus knows where you are.");
        return None;
    };
    let tons: i64 = hold.iter().map(|l| l.2).sum();
    let pad = finder::ship_pad(gs);
    let mut search = false;
    let took = st.sell.took;
    let p = &mut st.prefs;
    crate::widgets::filter_bar(ui, "sell-filters", |bar| {
        bar.field("In your hold", |ui| {
            let goods = if hold.len() == 1 { "commodity" } else { "commodities" };
            ui.label(RichText::new(format!("{} {} t · {} {goods}", icon::PACKAGE, nexus_data::names::group_digits(tons), hold.len())).font(display(size::CALLOUT)).color(pal::TEXT))
                .on_hover_text("Mission cargo, stolen goods and limpets are left out: they can't be sold at a market.");
        });
        bar.field("Buyers within", |ui| {
            search |= ui.add(Number::new(&mut p.sell_radius, 5.0..=MAX_SEARCH_LY).ladder(LY).unit(" ly")).on_hover_text("Every market Elite Nexus knows within this distance, up to the whole galaxy.").changed();
        });
        bar.field("Prices no older than", |ui| {
            search |= ui.add(Number::new(&mut p.max_age_h, 1.0..=720.0).ladder(HOURS).unit(" h")).on_hover_text("The same setting as Find commodity's.").changed();
        });
        bar.field_tip("Landing pad", "The pad the ship you fly needs: buyers without one are left out.", |ui| {
            let text = match pad {
                Some(p) => format!("{p} · your ship's"),
                None => "Any".to_string(),
            };
            ui.label(RichText::new(text).font(display(size::CALLOUT)).color(pal::TEXT));
        });
        bar.end(|ui| {
            if let Some(t) = took {
                ui.label(RichText::new(format!("Planned in {:.1} ms", t.as_secs_f64() * 1e3)).size(size::CAPTION).color(pal::FAINT));
            }
            search |= crate::widgets::icon_button(ui, icon::ARROWS_CLOCKWISE, "Plan again with the latest prices").clicked();
        });
    });
    ui.add_space(10.0);
    let key = (gs.inventory.cargo.updated.0, tons, gs.location.system_address, st.prefs.sell_radius.to_bits(), st.prefs.max_age_h.to_bits());
    // Asked to plan again while it plans: once it's done (the key it remembers no longer matches).
    if search {
        st.sell_key.0 = i64::MIN;
    }
    if key != st.sell_key && !st.sell.busy() {
        st.sell_key = key;
        let (db, radius, age_h) = (db.clone(), st.prefs.sell_radius, st.prefs.max_age_h);
        st.sell.run(ui.ctx(), move || finder::sell_plan(&db, origin, &hold, pad, radius, age_h));
    }
    if st.sell.busy() && st.sell.last.is_none() {
        ui.spinner();
        return None;
    }
    let plan = match &st.sell.last {
        Some(Ok(p)) => p,
        Some(Err(e)) => {
            ui.label(RichText::new(e).color(pal::BAD));
            return None;
        }
        None => return None,
    };
    if plan.stops.is_empty() {
        empty(ui, icon::STOREFRONT, "No known buyer in range", "Widen Buyers within, or fill the database: Database & feed imports every station from Spansh.");
        return None;
    }
    let mut nav = None;
    let buyers = if plan.stops.len() == 1 { "1 station".to_string() } else { format!("{} stations", plan.stops.len()) };
    crate::widgets::card_with(ui, "Best single stop", |ui| {
        ui.label(RichText::new(buyers).size(size::CAPTION).color(pal::FAINT));
    }, |ui| {
        const TABLE: &str = "sell-stops";
        let mut sort = TableSort::load(ui, TABLE);
        crate::widgets::table(ui, TABLE, |table| {
            let table = table
                .vscroll(false)
                .column(Column::initial(250.0).at_least(100.0).clip(true))
                .column(crate::widgets::flex_column(160.0))
                .column(Column::initial(70.0).at_least(50.0).clip(true))
                .column(Column::initial(110.0).at_least(60.0).clip(true))
                .column(Column::initial(120.0).at_least(70.0).clip(true))
                .column(Column::initial(60.0).at_least(40.0).clip(true))
                .header(30.0, |mut h| {
                    let heads: Heads = vec![("Station", Some((0, false))), ("System", Some((1, false))), ("Distance", Some((2, false))), ("Pays", Some((3, true))), ("Buys", Some((4, true))), ("Age", Some((5, false)))];
                    headings(&mut h, &heads, &mut sort, TABLE);
                });
            // "Buys": how many tonnes of the hold it takes. "Age": newest first.
            let tonnes = |s: &finder::SellStop| s.lines.iter().map(|l| l.1).sum::<i64>();
            let order = sort.order(&plan.stops, |a, b, c| match c {
                0 => cmp_text(&a.hit.station, &b.hit.station),
                1 => cmp_text(&a.hit.system, &b.hit.system),
                2 => cmp_opt(a.hit.distance_ly, b.hit.distance_ly),
                3 => a.total.cmp(&b.total),
                4 => tonnes(a).cmp(&tonnes(b)),
                _ => b.hit.updated_unix.cmp(&a.hit.updated_unix),
            });
            table
                .body(|body| {
                    body.rows(32.0, plan.stops.len(), |mut row| {
                        let s = &plan.stops[order[row.index()]];
                        let h = &s.hit;
                        row.col(|ui| {
                            ui.horizontal(|ui| {
                                if h.is_carrier {
                                    ui.label(RichText::new(icon::ANCHOR).color(pal::CYAN)).on_hover_text("Fleet carrier");
                                } else if h.is_planetary {
                                    ui.label(RichText::new(icon::PLANET).color(pal::DIM)).on_hover_text("Planetary");
                                }
                                let tip = "Click for the station's whole market";
                                let clicked = if h.is_carrier {
                                    named_with_access(ui, RichText::new(&h.station).color(pal::TEXT), h.carrier_access.as_deref(), |ui, text| nav_label(ui, text, tip))
                                } else {
                                    nav_label(ui, RichText::new(&h.station).color(pal::TEXT), tip)
                                };
                                if clicked {
                                    nav = Some(Nav::Station(h.market_id));
                                }
                            });
                        });
                        // The system with the station's distance from the arrival star, as the other lists show it.
                        row.col(|ui| {
                            if ui.link(&h.system).on_hover_text("Click to copy").clicked() {
                                ui.ctx().copy_text(h.system.clone());
                            }
                            if let Some(ls) = h.dist_ls {
                                ui.label(RichText::new(format!("{ls:.0} ls")).small().color(pal::DIM));
                            }
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(h.distance_ly.map(|d| format!("{d:.1} ly")).unwrap_or_default()).color(pal::DIM));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(short_credits(s.total)).font(display(14.0)).color(pal::GOLD));
                        });
                        row.col(|ui| {
                            let took: i64 = s.lines.iter().map(|l| l.1).sum();
                            let detail = s.lines.iter().map(|(n, t, p)| format!("{n}: {t} t × {} Cr", nexus_data::names::group_digits(*p))).collect::<Vec<_>>().join("\n");
                            ui.label(RichText::new(format!("{}/{} items · {} t", s.lines.len(), plan.per_item.len(), took)).color(pal::DIM)).on_hover_text(detail);
                        });
                        row.col(|ui| {
                            let (t, c) = age(h.updated_unix);
                            ui.label(RichText::new(t).color(c));
                        });
                    });
                });
        });
    });
    ui.add_space(8.0);
    card(ui, "Best buyer per commodity", |ui| {
        const TABLE: &str = "sell-items";
        let mut sort = TableSort::load(ui, TABLE);
        crate::widgets::table(ui, TABLE, |table| {
            let table = table
                .vscroll(false)
                .column(Column::initial(230.0).at_least(100.0).clip(true))
                .column(Column::initial(64.0).at_least(44.0).clip(true))
                .column(Column::initial(110.0).at_least(70.0).clip(true))
                .column(crate::widgets::flex_column(160.0))
                .column(Column::initial(200.0).at_least(90.0).clip(true))
                .column(Column::initial(76.0).at_least(50.0).clip(true))
                .column(Column::initial(90.0).at_least(70.0).clip(true))
                .header(30.0, |mut h| {
                    let heads: Heads = vec![
                        ("Commodity", Some((0, false))),
                        ("Tonnes", Some((1, true))),
                        ("Best price", Some((2, true))),
                        ("Station", Some((3, false))),
                        ("System", Some((4, false))),
                        ("Distance", Some((5, false))),
                        ("Demand", Some((6, true))),
                    ];
                    headings(&mut h, &heads, &mut sort, TABLE);
                });
            // A commodity nobody in range buys sorts last whichever way.
            let desc = sort.descending;
            let missing = move |v: Option<f64>| v.or(if desc { Some(f64::NEG_INFINITY) } else { None });
            let order = sort.order(&plan.per_item, |a, b, c| {
                let (x, y) = (a.2.first(), b.2.first());
                match c {
                    0 => cmp_text(&a.0, &b.0),
                    1 => a.1.cmp(&b.1),
                    2 => cmp_opt(missing(x.map(|h| h.price as f64)), missing(y.map(|h| h.price as f64))),
                    3 => cmp_text(x.map_or("", |h| h.station.as_str()), y.map_or("", |h| h.station.as_str())),
                    4 => cmp_text(x.map_or("", |h| h.system.as_str()), y.map_or("", |h| h.system.as_str())),
                    5 => cmp_opt(x.and_then(|h| h.distance_ly), y.and_then(|h| h.distance_ly)),
                    _ => cmp_opt(missing(x.map(|h| h.quantity as f64)), missing(y.map(|h| h.quantity as f64))),
                }
            });
            table.body(|body| {
                body.rows(32.0, plan.per_item.len(), |mut row| {
                    let (name, count, hits) = &plan.per_item[order[row.index()]];
                    row.col(|ui| {
                        if nav_label(ui, RichText::new(name).color(pal::TEXT), "Every buyer and the latest prices") {
                            nav = Some(Nav::Commodity { name: name.clone(), side: Some(Side::Sell) });
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(nexus_data::names::group_digits(*count)).color(pal::DIM));
                    });
                    let Some(h) = hits.first() else {
                        row.col(|ui| {
                            ui.label(RichText::new("—").color(pal::FAINT));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new("no buyer in range").color(pal::FAINT));
                        });
                        for _ in 0..3 {
                            row.col(|_| {});
                        }
                        return;
                    };
                    row.col(|ui| {
                        let others = hits.iter().skip(1).map(|o| format!("{} at {}, {}", cr(o.price), o.station, o.system)).collect::<Vec<_>>().join("\n");
                        let r = ui.label(RichText::new(cr(h.price)).font(display(14.0)).color(pal::GOLD));
                        if !others.is_empty() {
                            r.on_hover_text(format!("Next best:\n{others}"));
                        }
                    });
                    row.col(|ui| {
                        let tip = "Click for the station's whole market";
                        let text = RichText::new(if h.is_carrier { format!("{} {}", icon::ANCHOR, h.station) } else { h.station.clone() }).color(pal::TEXT);
                        let clicked = if h.is_carrier { named_with_access(ui, text, h.carrier_access.as_deref(), |ui, text| nav_label(ui, text, tip)) } else { nav_label(ui, text, tip) };
                        if clicked {
                            nav = Some(Nav::Station(h.market_id));
                        }
                    });
                    row.col(|ui| {
                        if ui.add(egui::Label::new(RichText::new(&h.system).color(pal::DIM)).truncate().sense(egui::Sense::click())).on_hover_text("Click to copy the system").clicked() {
                            ui.ctx().copy_text(h.system.clone());
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(h.distance_ly.map(|d| format!("{d:.1} ly")).unwrap_or_default()).color(pal::DIM));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(format!("{} t", nexus_data::names::group_digits(h.quantity))).color(pal::DIM));
                    });
                });
            });
        });
    });
    nav
}

fn describe_watch(w: &PriceWatch) -> String {
    let digits = nexus_data::names::group_digits;
    let mut s = format!("{}: a station {} {} Cr", w.commodity, if w.sell { "pays at least" } else { "sells it for at most" }, digits(w.price));
    s += &if w.max_ly > 0.0 { format!(" within {:.0} ly", w.max_ly) } else { " anywhere".to_string() };
    if let Some(p) = w.min_pad {
        s += &format!(", {p} pad");
    }
    s += &format!(", {} {} t+", if w.sell { "demand" } else { "stock" }, digits(w.min_qty.max(1)));
    if w.carriers {
        s += ", carriers too";
    }
    s
}

/// Distances an alert's "within" steps through, ly; 0 is anywhere.
const ALERT_LY: &[f64] = &[0.0, 10.0, 25.0, 50.0, 75.0, 100.0, 150.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0, 20_000.0];

/// What a new price alert can be measured against: the commodity's galactic average, and the best price in its range
/// right now (the highest a station pays, or the lowest one sells for).
pub struct AlertRef {
    average: Option<i64>,
    best: Option<CommodityHit>,
}

/// The prices under the new alert to measure it against: the galactic average and the best price in its range now
/// (with its filters), each a click away from being the alert's price.
fn alert_reference(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) {
    let w = &st.new_watch;
    let key = format!("{:?}", (w.commodity.trim().to_lowercase(), w.sell, w.max_ly.round() as i64, w.min_pad, w.carriers, w.min_qty, gs.location.system_address));
    if st.alert_ref_key.as_deref() != Some(key.as_str()) && !st.alert_ref.busy() {
        st.alert_ref_key = Some(key);
        let (db, origin) = (db.clone(), gs.location.star_pos);
        let q = CommodityQuery {
            commodity: w.commodity.trim().to_string(),
            mode: if w.sell { TradeMode::SellToStation } else { TradeMode::BuyFromStation },
            origin,
            max_distance_ly: (w.max_ly > 0.0).then_some(w.max_ly),
            min_pad: w.min_pad,
            max_age_hours: Some(72.0),
            min_quantity: w.min_qty.max(1),
            include_carriers: w.carriers,
            include_planetary: true,
            limit: 1,
            sort: Sort::BestPrice,
        };
        st.alert_ref.run(ui.ctx(), move || {
            let average = db.commodity_summary(&q.commodity, q.max_age_hours).map_err(|e| e.to_string())?.and_then(|s| s.average);
            let best = db.search_commodity(&q).map_err(|e| e.to_string())?.into_iter().next();
            Ok(AlertRef { average, best })
        });
    }
    let Some(Ok(r)) = &st.alert_ref.last else { return };
    let w = &st.new_watch;
    let mut set = None;
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
        ui.label(RichText::new("Measure it against").size(size::FOOTNOTE).color(pal::DIM));
        if let Some(a) = r.average
            && crate::widgets::button(ui, &format!("Galactic average {}", cr(a))).on_hover_text("The average price the game reports for it. Click to make it the alert's price.").clicked()
        {
            set = Some(a);
        }
        let range = if w.max_ly > 0.0 { format!(" within {:.0} ly", w.max_ly) } else { String::new() };
        match &r.best {
            Some(h) => {
                let what = if w.sell { "Best paid" } else { "Cheapest" };
                let tip = format!(
                    "At {} in {}, reported {} ago, with the filters above (prices up to 72 h old). Click to make it the alert's price: the alert then fires when a station {} that.",
                    h.station,
                    h.system,
                    age(h.updated_unix).0,
                    if w.sell { "pays at least" } else { "sells for at most" }
                );
                if crate::widgets::button(ui, &format!("{what}{range} now {}", cr(h.price))).on_hover_text(tip).clicked() {
                    set = Some(h.price);
                }
            }
            None => {
                let side = if w.sell { "buys" } else { "sells" };
                ui.label(RichText::new(format!("No market{range} {side} it with these filters now (prices up to 72 h old).")).size(size::FOOTNOTE).color(pal::FAINT));
            }
        }
    });
    if let Some(p) = set {
        st.new_watch.price = p;
    }
}

/// Rules checked against every live market update. Returns true when the rules changed.
fn alerts_tab(ui: &mut Ui, gs: &GameState, svc: &MarketService, db: &GalaxyDb, st: &mut MarketUi, watches: &mut Vec<PriceWatch>) -> bool {
    let mut changed = false;
    if st.commodities.is_empty() {
        st.commodities = db.commodities();
    }
    ui.add(
        egui::Label::new(
            RichText::new(
                "Every market update from the live feed (and every market you open) is checked against these rules the moment it arrives, \
                 usually seconds after a commander docks. A rule fires at most once per station per hour, as a toast and, with voice on, spoken.",
            )
            .size(size::FOOTNOTE)
            .color(pal::DIM),
        )
        .wrap(),
    );
    crate::widgets::section(ui, "New alert");
    let known = !st.new_watch.commodity.trim().is_empty() && svc.resolves(st.new_watch.commodity.trim());
    let mut add = false;
    {
        let w = &mut st.new_watch;
        let (all, recent) = (&st.commodities, &st.prefs.recent);
        crate::widgets::filter_bar(ui, "alert-new", |bar| {
            bar.field("Commodity", |ui| {
                crate::widgets::autocomplete(ui, "alert-commodity", &mut w.commodity, "Commodity", 200.0, |t| commodity_suggestions(all, recent, t));
            });
            bar.field("Alert when a station", |ui| {
                crate::widgets::segmented(ui, &mut w.sell, &[(true, "pays at least"), (false, "sells for at most")]);
            });
            bar.field("Price", |ui| {
                let mut price = w.price as f64;
                let tip = "Type a price (48300, 48k or 1.5m), or step it with − and +: by 1,000 Cr around 48,000 Cr, by 100,000 Cr around 1.5 M Cr. Shift steps ten times as far.";
                if ui.add(Number::new(&mut price, 1.0..=100_000_000.0).unit(" Cr")).on_hover_text(tip).changed() {
                    w.price = price.round() as i64;
                }
            });
            bar.field("Within", |ui| {
                ui.add(Number::new(&mut w.max_ly, 0.0..=20_000.0).ladder(ALERT_LY).unit(" ly").zero("anywhere")).on_hover_text("From where you are when the market report arrives. 0 is anywhere.");
            });
            bar.field_tip("Landing pad", PAD_TIP, |ui| {
                pad_picker(ui, &mut w.min_pad);
            });
            bar.field(if w.sell { "Demand at least" } else { "Stock at least" }, |ui| {
                let mut q = w.min_qty as f64;
                if ui.add(Number::new(&mut q, 1.0..=1_000_000.0).ladder(TONNES).unit(" t")).changed() {
                    w.min_qty = q as i64;
                }
            });
            bar.field("Include", |ui| {
                crate::widgets::chip_toggle(ui, &mut w.carriers, "Fleet carriers").on_hover_text("Fleet carriers' orders: their owners set the prices, and they move.");
            });
            bar.end(|ui| {
                add = ui.add_enabled_ui(known, |ui| crate::widgets::primary_button(ui, &format!("{} Add alert", icon::BELL_RINGING))).inner.clicked();
            });
        });
    }
    if !known && !st.new_watch.commodity.trim().is_empty() {
        ui.label(RichText::new("Unknown commodity: pick one from the suggestions").color(pal::BAD));
    }
    if known {
        alert_reference(ui, gs, db, st);
    }
    if add {
        let mut rule = st.new_watch.clone();
        rule.commodity = rule.commodity.trim().to_string();
        rule.enabled = true;
        watches.push(rule);
        changed = true;
    }
    crate::widgets::section(ui, "Your alerts");
    if watches.is_empty() {
        crate::widgets::footnote(ui, "No alerts yet. Example: Tritium, a station pays at least 60,000 Cr, within 50 ly.");
    } else {
        let mut remove = None;
        crate::widgets::group(ui, |g| {
            for (i, w) in watches.iter_mut().enumerate() {
                let detail = if svc.resolves(&w.commodity) { "" } else { "Unknown commodity: this rule can't fire" };
                g.row(&describe_watch(w), detail, |ui| {
                    if crate::widgets::icon_button(ui, icon::TRASH, "Delete this alert").clicked() {
                        remove = Some(i);
                    }
                    changed |= crate::widgets::switch(ui, &mut w.enabled).on_hover_text(if w.enabled { "On: click to pause it" } else { "Paused: click to switch it on" }).changed();
                });
            }
        });
        if let Some(i) = remove {
            watches.remove(i);
            changed = true;
        }
    }
    if svc.feed_state().is_none() {
        ui.add_space(6.0);
        ui.label(RichText::new(format!("{} The live feed is off, so only markets you open yourself are checked. Turn on \"Receive the live market feed\" in Accounts & sync.", icon::WARNING)).color(pal::WARN));
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, symbol: &str, category: &str, rare: bool) -> CommodityInfo {
        CommodityInfo { symbol: symbol.into(), name: name.into(), category: category.into(), is_rare: rare }
    }

    fn sample() -> Vec<CommodityInfo> {
        vec![
            info("Gold", "gold", "Metals", false),
            info("Low Temperature Diamonds", "lowtemperaturediamond", "Minerals", false),
            info("Thargoid Probe", "unknownartifact2", "Salvage", false),
            info("Thargoid Sensor", "unknownartifact", "Salvage", false),
            info("Tritium", "tritium", "Chemicals", false),
            info("Void Opal", "opal", "Minerals", false),
            info("Goliath Spider Silk", "goliathspidersilk", "Textiles", true),
            info("Painite", "painite", "Minerals", false),
        ]
    }

    fn names(all: &[CommodityInfo], typed: &str) -> Vec<String> {
        rank_commodities(all, typed, 10).into_iter().map(|c| c.name.clone()).collect()
    }

    /// Categories end with the pseudo category "Rare Goods"; a category lists its normal goods, then its rare
    /// goods as one item; typing searches every category; nothing untradeable or retired is listed.
    #[test]
    fn categories_items_and_rare_goods() {
        let all = vec![
            info("Silver", "silver", "Metals", false),
            info("Gold", "gold", "Metals", false),
            info("Sothis Crystalline Gold", "sothiscrystallinegold", "Metals", true),
            info("Rusani Old Smokey", "rusanioldsmokey", "Legal Drugs", true),
            info("Tobacco", "tobacco", "Legal Drugs", false),
            info("Bauxite", "bauxite", "Minerals", false),
            info("Unknown", "unknown", "NonMarketable", false),
            info("Galactic Travel Guide", "galactictravelguide", "Salvage", true),
        ];
        let texts = |v: Vec<Suggestion>| v.into_iter().map(|s| s.text).collect::<Vec<_>>();
        assert_eq!(commodity_categories(&all), vec!["Legal Drugs", "Metals", "Minerals", "Rare Goods"], "no Salvage: its only item is retired");
        assert_eq!(texts(category_items(&all, "Metals")), vec!["Gold", "Silver", "Rare Goods"]);
        assert_eq!(texts(category_items(&all, "Minerals")), vec!["Bauxite"], "no rare goods there");
        assert_eq!(texts(category_items(&all, "Rare Goods")), vec!["Rare Goods"]);
        assert_eq!(category_items(&all, "Rare Goods")[0].detail, "all 2 rare goods");
        assert_eq!(category_items(&all, "Metals")[2].detail, "1 rare good", "the category's own");
        assert_eq!(item_suggestions(&all, &[], "Metals", "")[0].text, "", "a blank entry first");
        assert_eq!(texts(item_suggestions(&all, &[], "Metals", "")), vec!["", "Gold", "Silver", "Rare Goods"], "nothing typed: the category's list");
        let every = texts(category_items(&all, ""));
        assert_eq!(every.len(), 7, "every listed commodity, rare goods one by one, then Rare Goods: {every:?}");
        assert!(!every.contains(&"Unknown".to_string()) && !every.contains(&"Galactic Travel Guide".to_string()));
        assert!(category_items(&all, "").iter().any(|s| s.text == "Sothis Crystalline Gold" && s.starred));

        // Typing searches every commodity, whatever the category (a page opened earlier sets it), and offers every
        // rare good as one when the words fit.
        for category in ["", "Metals", "Minerals", "Rare Goods"] {
            assert_eq!(texts(item_suggestions(&all, &[], category, "gol")), vec!["Gold", "Sothis Crystalline Gold"], "in {category:?}");
            assert_eq!(texts(item_suggestions(&all, &[], category, "toba")), vec!["Tobacco"], "in {category:?}");
        }
        let rare = item_suggestions(&all, &[], "Minerals", "rare");
        assert_eq!(rare.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), vec!["Rare Goods"]);
        assert_eq!(rare[0].detail, "all 2 rare goods");
        assert!(item_suggestions(&all, &[], "Metals", "unknown").is_empty(), "nothing untradeable");
        assert!(item_suggestions(&all, &[], "", "travel").is_empty(), "nothing retired");
        // The category field narrows to what's typed.
        assert_eq!(texts(category_suggestions(&all, "")), vec!["All categories", "Legal Drugs", "Metals", "Minerals", "Rare Goods"]);
        assert_eq!(texts(category_suggestions(&all, "me")), vec!["Metals"]);
    }

    /// A carrier's docking access as its badge says it, in the journal's spelling or Spansh's.
    #[test]
    fn docking_access_badges() {
        let word = |a: Option<&str>| access_badge_text(a).0;
        assert_eq!(word(Some("all")), "open");
        assert_eq!(word(Some("All")), "open");
        assert_eq!(word(Some("none")), "owner only");
        assert_eq!(word(Some("friends")), "friends");
        assert_eq!(word(Some("squadron")), "squadron");
        assert_eq!(word(Some("squadronfriends")), "squadron, friends");
        assert_eq!(word(Some("Squadron Friends")), "squadron, friends");
        assert_eq!(word(None), "?", "never reported");
        assert_eq!(word(Some("something new")), "?");
        assert_eq!(carrier_access("Squadron Friends"), "squadron and friends");
        assert_eq!(carrier_access("none"), "nobody but the owner");
    }

    /// How long an order stood, in words: up to the latest report, never to now, and per side (a buy order the report
    /// before didn't have may have been a sale then).
    #[test]
    fn how_long_an_order_stood_in_words() {
        const D: i64 = 86_400;
        let last = 100 * D;
        let since = |s, selling| stood_words(s, selling, last);
        let changed = since(Stood::Since { since: 88 * D, before: 82 * D, previous: 50_000 }, false);
        assert!(changed.starts_with("Price unchanged in every report from ") && changed.contains("to the latest") && changed.contains("(12d)"), "{changed}");
        assert!(changed.ends_with(&format!("had {}.", cr(50_000))), "{changed}");
        let wanted = since(Stood::Since { since: 95 * D, before: 90 * D, previous: 0 }, false);
        assert!(wanted.starts_with("Wanted at this price") && wanted.ends_with("had no buy order for it."), "{wanted}");
        let for_sale = since(Stood::Since { since: last, before: 90 * D, previous: 0 }, true);
        assert!(for_sale.starts_with("For sale since the latest report") && for_sale.ends_with("had none for sale."), "{for_sale}");
        let kept = since(Stood::AtLeast { since: 75 * D }, false);
        assert!(kept.contains("(25d)") && kept.contains("can't see further back"), "{kept}");
        // One report on record (or one older than the history kept): nothing about how long.
        for one in [Stood::AtLeast { since: last }, Stood::AtLeast { since: last + 5 * D }] {
            assert!(since(one, false).starts_with("Only the latest report"), "{one:?}");
        }
    }

    #[test]
    fn suggestions_rank_names_initials_and_words() {
        let all = sample();
        assert_eq!(names(&all, "gold"), vec!["Gold"]);
        assert_eq!(names(&all, "go")[..2], ["Gold".to_string(), "Goliath Spider Silk".to_string()], "common goods before rare ones");
        assert_eq!(names(&all, "ltd"), vec!["Low Temperature Diamonds"], "initials");
        assert_eq!(names(&all, "LTD"), vec!["Low Temperature Diamonds"]);
        assert_eq!(names(&all, "thar pro"), vec!["Thargoid Probe"], "the start of every word");
        assert_eq!(names(&all, "probe"), vec!["Thargoid Probe"]);
        assert_eq!(names(&all, "opal"), vec!["Void Opal"]);
        assert_eq!(names(&all, "unknownartifact2"), vec!["Thargoid Probe"], "the internal name");
        assert_eq!(names(&all, "thargoid")[..2], ["Thargoid Probe".to_string(), "Thargoid Sensor".to_string()]);
        assert_eq!(names(&all, "salvage"), vec!["Thargoid Probe", "Thargoid Sensor"], "by category");
        assert!(names(&all, "zzz").is_empty());
        assert!(names(&all, "  ").is_empty());
    }

    #[test]
    fn empty_text_suggests_recent_commodities() {
        let all = sample();
        let s = commodity_suggestions(&all, &["Painite".into(), "Gold".into()], "");
        let texts: Vec<&str> = s.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["Painite", "Gold"]);
        assert_eq!(s[0].detail, "recent · Minerals");
    }

    #[test]
    fn prefs_round_trip_and_heal() {
        let mut p = MarketPrefs { commodity: "Gold".into(), side: Side::Sell, sort: Sort::Nearest, max_dist: 250.0, min_qty: 7, tab: Tab::Abnormal, ..Default::default() };
        p.remember("Painite");
        p.remember("Gold");
        let json = serde_json::to_string(&p).unwrap();
        let back: MarketPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.recent, vec!["Gold", "Painite"], "newest first, no duplicates");
        // Missing fields take their defaults; out-of-range values come back into range.
        let old: MarketPrefs = serde_json::from_str(r#"{"max_dist": -5, "min_qty": 0, "min_pad": "X"}"#).unwrap();
        let healed = old.sanitized();
        assert_eq!((healed.max_dist, healed.min_qty, healed.min_pad), (1.0, 1, None));
        assert_eq!(healed.commodity, "Tritium");
    }

    #[test]
    fn widening_includes_the_nearest_market() {
        let p = MarketPrefs { max_dist: 60.0, min_qty: 100, min_pad: Some('L'), pad_auto: true, carriers: false, ..Default::default() };
        let hit = CommodityHit {
            system: "Luyten's Star".into(),
            system_id64: None,
            pos: None,
            distance_ly: Some(812.4),
            station: "B4Z-LQY".into(),
            market_id: 1,
            station_type: None,
            dist_ls: None,
            max_pad: Some('L'),
            price: 44_100_700,
            quantity: 45,
            updated_unix: 0,
            is_carrier: true,
            is_planetary: false,
            carrier_access: None,
            commodity: None,
        };
        let r = Relax::for_hit(&hit, &p, true);
        assert_eq!(r.reasons(&p), vec!["farther than 60 ly", "fewer than 100 t", "fleet carrier (switched off)"]);
        let mut wider = p.clone();
        r.apply(&mut wider);
        assert_eq!((wider.max_dist, wider.min_qty, wider.carriers, wider.min_pad), (820.0, 45, true, Some('L')));
    }

    #[test]
    fn market_tiles_stay_readable() {
        // Side by side while each keeps a comfortable width: two or three markets in a 970 point area are two columns.
        assert_eq!(tile_columns(1, 970.0, 800.0, 12.0), 1);
        assert_eq!(tile_columns(2, 970.0, 800.0, 12.0), 2);
        assert_eq!(tile_columns(3, 970.0, 800.0, 12.0), 2);
        // Eight would be four rows too low to read: three narrower columns instead. Wider, three comfortable ones.
        assert_eq!(tile_columns(8, 970.0, 800.0, 12.0), 3);
        assert_eq!(tile_columns(8, 1284.0, 800.0, 12.0), 3);
        // Too narrow for two: one column, and the tiles scroll.
        assert_eq!(tile_columns(4, 600.0, 800.0, 12.0), 1);
    }

    #[test]
    fn markets_open_side_by_side_up_to_eight() {
        let mut st = MarketUi::default();
        for id in 1..=8 {
            st.open_tile(id, false);
        }
        assert_eq!(st.prefs.tiles, (1..=8).collect::<Vec<_>>());
        st.open_tile(3, false);
        assert_eq!(st.prefs.tiles.len(), 8, "one already open keeps its place");
        st.open_tile(9, false);
        assert_eq!(st.prefs.tiles, vec![2, 3, 4, 5, 6, 7, 8, 9], "the oldest closes");
        // The market you're docked at stays when another opens.
        st.docked_market = Some(2);
        st.open_tile(10, false);
        assert_eq!(st.prefs.tiles, vec![2, 4, 5, 6, 7, 8, 9, 10]);
        // A market opened in the game comes first, in place of the one that was first.
        st.open_tile(11, true);
        assert_eq!(st.prefs.tiles, vec![11, 4, 5, 6, 7, 8, 9, 10]);
        st.open_tile(0, false);
        assert_eq!(st.prefs.tiles.len(), 8, "no market id, nothing to open");
    }
}
