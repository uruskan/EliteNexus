//! Market: live EDDN-fed galaxy database — commodity pages, abnormal prices, trade routes, services,
//! hotspots. Commodity and station names are links: they open the commodity's page (where to buy,
//! where to sell, latest prices) or the station's market.

use crate::finder::{self, carriers_here, presence, Presence, SellPlan};
use crate::jobs::Slot;
use crate::market::{ImportJob, ImportSource, MarketService, SPANSH_STATIONS_URL};
use crate::runplan::{self, Order, PlanInput, RunPlan};
use crate::settings::PriceWatch;
use crate::theme::{display, pal, size};
use crate::widgets::{Suggestion, TableSort, badge, bar, card, cmp_opt, cmp_text, duration, empty, short_credits};
use egui::{Color32, Grid, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_galaxy::{
    CommodityHit, CommodityInfo, CommodityQuery, CommodityQuote, CommoditySummary, Deal, DealQuery, GalaxyDb, HotspotHit, HotspotQuery, MaterialTrader, PricePoint,
    Services, Sort, StationHit, StationInfo, StationMarket, StationQuery, TechBroker, TradeMode, TradeQuery, TradeRoute,
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
    /// Find commodity: the filter row is open.
    pub show_options: bool,
    /// Trade routes: area and longest hop.
    pub radius: f64,
    pub hop: f64,
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
            show_options: false,
            radius: 40.0,
            hop: 25.0,
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
        self
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

/// What a click in one of the commodity page's lists asks for.
enum RowAction {
    Select(u64),
    Pin(Pin),
}

type Res<T> = Result<Vec<T>, String>;
/// The database's carriers by callsign (those the game listed here), and the carriers it places in this system.
type CarrierView = (FxHashMap<String, StationInfo>, Vec<StationInfo>);
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

/// A market picked in a commodity page's list: everything it trades, and this commodity's price history there.
pub struct PickDetail {
    market: Option<StationMarket>,
    history: Vec<PricePoint>,
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
    /// The list the page shows; starts at the saved one and may move to a list that has results.
    side: Side,
    /// Every known commodity, for the suggestions (loaded on first use).
    commodities: Vec<CommodityInfo>,
    page: Slot<Result<Option<CommodityPage>, String>>,
    /// The query the shown page was loaded with: any change (filters, commodity, position) reloads it.
    page_key: Option<CommodityQuery>,
    /// A new commodity is loading: once it arrives, an empty list may give way to one with results.
    fresh: bool,
    /// Text that matched no commodity.
    not_found: Option<String>,
    pick: Option<u64>,
    detail: Slot<Result<PickDetail, String>>,
    detail_key: Option<(u64, String)>,
    /// Station tab: a market opened from a list (`None` = the market of the station you're docked at).
    station: Option<u64>,
    station_market: Slot<Result<Option<StationMarket>, String>>,
    station_key: Option<u64>,
    routes: Slot<Res<TradeRoute>>,
    service: usize,
    stations: Slot<Res<StationHit>>,
    /// What the Services list was searched for: (service, system, pad). It searches again when that changes.
    services_key: Option<(usize, u64, Option<char>)>,
    hotspots: Slot<Res<HotspotHit>>,
    hotspot_prices: Slot<Res<CommodityHit>>,
    /// Path typed into "import a downloaded dump".
    import_path: String,
    sell: Slot<Result<SellPlan, String>>,
    /// (cargo snapshot time, tonnes, system): re-plan when any of them changes.
    sell_key: (i64, i64, u64),
    /// The rule being composed in "Price alerts".
    new_watch: PriceWatch,
    abn: Slot<Res<Deal>>,
    /// The pins' markets as the database has them now, refreshed every half minute and on arrival.
    pins_live: Slot<Result<FxHashMap<u64, Option<StationMarket>>, String>>,
    pins_key: Option<(Vec<u64>, i64)>,
    pins_at: Option<std::time::Instant>,
    plan: Slot<Result<RunPlan, String>>,
    /// Station tab: the database's view of the carriers listed here (by callsign), and the ones it places here.
    carriers: Slot<Result<CarrierView, String>>,
    carriers_key: Option<(u64, Vec<String>)>,
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
            input: prefs.commodity.clone(),
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
            station: None,
            station_market: Slot::default(),
            station_key: None,
            routes: Slot::default(),
            service: 0,
            stations: Slot::default(),
            services_key: None,
            hotspots: Slot::default(),
            hotspot_prices: Slot::default(),
            import_path: String::new(),
            sell: Slot::default(),
            sell_key: (0, 0, 0),
            new_watch: PriceWatch::default(),
            abn: Slot::default(),
            abn_key: None,
            pins_live: Slot::default(),
            pins_key: None,
            pins_at: None,
            plan: Slot::default(),
            carriers: Slot::default(),
            carriers_key: None,
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
        if name != self.prefs.commodity {
            self.pick = None;
            self.fresh = true;
        }
        self.prefs.remember(&name);
        self.input = name;
        self.not_found = None;
        // Searching again reloads even when nothing changed (fresh prices from the feed).
        self.page_key = None;
        true
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
                self.station = Some(id);
                self.set_tab(Tab::Docked);
            }
        }
    }

    fn set_tab(&mut self, t: Tab) {
        self.tab = t;
        self.prefs.tab = t;
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

/// Commodities matching what was typed, best first: the whole name, the start of the name, the
/// initials ("ltd" is Low Temperature Diamonds), the start of every typed word ("thar pro"), anywhere
/// in the name, the game's internal name (Thargoid Probe is `UnknownArtifact2`), then the category
/// ("salvage" lists all salvage).
pub fn rank_commodities<'a>(all: &'a [CommodityInfo], typed: &str, limit: usize) -> Vec<&'a CommodityInfo> {
    let q = typed.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let words_of = |s: &str| s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let q_words = words_of(&q);
    let q_key: String = q.chars().filter(|c| c.is_alphanumeric()).collect();
    let mut scored: Vec<(u8, &CommodityInfo)> = all
        .iter()
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

fn commodity_suggestions(all: &[CommodityInfo], recent: &[String], typed: &str) -> Vec<Suggestion> {
    let detail = |c: &CommodityInfo| if c.is_rare { format!("{} · rare", c.category) } else { c.category.clone() };
    if typed.trim().is_empty() {
        return recent
            .iter()
            .map(|r| Suggestion { text: r.clone(), detail: all.iter().find(|c| &c.name == r).map_or_else(|| "recent".into(), |c| format!("recent · {}", detail(c))) })
            .collect();
    }
    rank_commodities(all, typed, 40).into_iter().map(|c| Suggestion { text: c.name.clone(), detail: detail(c) }).collect()
}

fn pad_combo(ui: &mut Ui, id: &str, pad: &mut Option<char>) {
    egui::ComboBox::from_id_salt(id)
        .width(70.0)
        .selected_text(match pad {
            Some(c) => format!("{c} pad"),
            None => "Any pad".into(),
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(pad, None, "Any pad");
            ui.selectable_value(pad, Some('S'), "S pad");
            ui.selectable_value(pad, Some('M'), "M pad");
            ui.selectable_value(pad, Some('L'), "L pad");
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

fn age(updated_unix: i64) -> (String, egui::Color32) {
    let s = (nexus_journal::Timestamp::now().0 - updated_unix).max(0);
    let col = if s < 3600 { pal::GOOD } else if s < 6 * 3600 { pal::CYAN } else if s < 48 * 3600 { pal::WARN } else { pal::FAINT };
    let txt = if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    };
    (txt, col)
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

/// Station name over its system (click the system to copy it), with a carrier or planet mark.
fn station_cell(ui: &mut Ui, station: &str, system: &str, carrier: bool, planetary: bool, dist_ls: Option<f64>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let (name, tip) = if carrier {
            (format!("{} {station}", icon::ANCHOR), Some("Fleet carrier: its owner sets the prices and who may dock"))
        } else if planetary {
            (format!("{} {station}", icon::PLANET), Some("Planetary port or settlement"))
        } else {
            (station.to_string(), None)
        };
        let r = ui.add(egui::Label::new(RichText::new(name).color(pal::TEXT)).truncate());
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
    st.station_market.tick();
    st.routes.tick();
    st.stations.tick();
    st.hotspots.tick();
    st.hotspot_prices.tick();
    st.sell.tick();
    st.abn.tick();
    st.pins_live.tick();
    st.plan.tick();
    st.carriers.tick();

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
        // Station clicked again: back to the station you're docked at.
        if t == Tab::Docked && st.tab == Tab::Docked {
            st.station = None;
        }
        st.set_tab(t);
    }
    ui.add_space(8.0);
    feed_header(ui, svc);
    ui.add_space(12.0);
    let Some(db) = svc.db.clone() else {
        empty(ui, icon::DATABASE, "Market database unavailable", svc.db_error.as_deref().unwrap_or("Starting…"));
        return false;
    };
    let origin = gs.location.star_pos;
    let ctx = ui.ctx().clone();
    let mut changed = false;
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
            let p = &mut st.prefs;
            ui.horizontal_wrapped(|ui| {
                ui.label("Routes within");
                ui.add(egui::DragValue::new(&mut p.radius).range(5.0..=1000.0).suffix(" ly")).on_hover_text("Up to 1,000 ly. Every market in the area is loaded, so wide areas take a few seconds.");
                ui.label("of here, hop up to");
                ui.add(egui::DragValue::new(&mut p.hop).range(1.0..=500.0).suffix(" ly")).on_hover_text("Longest trip between buying and selling, up to 500 ly.");
                pad_combo(ui, "rpad", &mut p.min_pad);
                ui.label(format!("cargo {cargo} t, capital {}", short_credits(gs.cmdr.credits)));
                crate::widgets::toggle(ui, &mut p.carriers, "Carriers");
                if crate::widgets::primary_button(ui, "Find routes").clicked()
                    && let Some(o) = origin {
                        let q = TradeQuery {
                            origin: o,
                            radius_ly: p.radius,
                            max_hop_ly: p.hop,
                            min_pad: p.min_pad,
                            cargo_capacity: cargo.max(1),
                            capital: Some(gs.cmdr.credits.max(0)),
                            max_age_hours: Some(p.max_age_h),
                            include_carriers: p.carriers,
                            include_planetary: p.planetary,
                            ..Default::default()
                        };
                        let db = db.clone();
                        st.routes.run(&ctx, move || db.trade_routes(&q).map_err(|e| e.to_string()));
                    }
            });
            if st.routes.last.is_none() && !st.routes.busy() {
                crate::widgets::footnote(ui, "Set the area and press Find routes. Every market in the area is loaded, so wide areas take a few seconds: this search waits for you.");
            }
            nav = results_routes(ui, &st.routes).or(nav);
        }
        Tab::Services => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for (i, (label, _)) in SERVICES.iter().enumerate() {
                    if crate::widgets::pill(ui, st.service == i, label).clicked() {
                        st.service = i;
                    }
                }
                let pad = st.prefs.min_pad;
                pad_combo(ui, "spad", &mut st.prefs.min_pad);
                // A pad picked here stays: Find commodity no longer sets it from the ship.
                if st.prefs.min_pad != pad {
                    st.prefs.pad_auto = false;
                }
            });
            ui.add_space(8.0);
            // Searches by itself: when the tab opens, when a service or pad is picked, and after every jump.
            match origin {
                None => empty(ui, icon::MAP_PIN, "Position unknown", "Jump or relog once so Elite Nexus knows where you are."),
                Some(o) => {
                    let key = (st.service, gs.location.system_address, st.prefs.min_pad);
                    if st.services_key != Some(key) && !st.stations.busy() {
                        st.services_key = Some(key);
                        let q = service_query(SERVICES[st.service].1, o, st.prefs.min_pad);
                        let db = db.clone();
                        st.stations.run(&ctx, move || db.stations_near(&q).map_err(|e| e.to_string()));
                    }
                    if !st.stations.busy() && matches!(&st.stations.last, Some(Ok(v)) if v.is_empty()) {
                        let pad = st.prefs.min_pad.map(|p| format!(" with an {p} pad")).unwrap_or_default();
                        let hint = format!("No station Elite Nexus knows has a {}{pad}. It learns stations from the live feed, your journal and the Spansh import (Database & feed).", SERVICES[st.service].0);
                        empty(ui, icon::MAGNIFYING_GLASS, "None found", &hint);
                    } else {
                        results_stations(ui, &st.stations);
                    }
                }
            }
        }
        Tab::Hotspots => {
            let p = &mut st.prefs;
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("hsig").selected_text(p.hotspot_signal.clone()).show_ui(ui, |ui| {
                    for s in HOTSPOT_SIGNALS {
                        ui.selectable_value(&mut p.hotspot_signal, (*s).to_string(), *s);
                    }
                });
                ui.label("at least");
                ui.add(egui::DragValue::new(&mut p.hotspot_min).range(1..=6));
                ui.label("hotspots within");
                ui.add(egui::DragValue::new(&mut p.max_dist).range(1.0..=MAX_SEARCH_LY).suffix(" ly")).on_hover_text("Searches every market Elite Nexus knows, up to the whole galaxy.");
                if crate::widgets::primary_button(ui, "Find hotspots").clicked()
                    && let Some(o) = origin {
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
            });
            ui.add(egui::Label::new(RichText::new("Hotspots come from DSS ring scans shared over EDDN, and your own. For galaxy-wide searches use Navigation › Spansh.").small().color(pal::DIM)).wrap());
            ui.add_space(8.0);
            let signal = st.prefs.hotspot_signal.clone();
            crate::widgets::split(
                ui,
                0.5,
                360.0,
                |ui| results_hotspots(ui, &st.hotspots),
                |ui| {
                    card(ui, "Best places to sell nearby", |ui| {
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
            egui::ScrollArea::vertical().id_salt("malerts").auto_shrink([false, false]).show(ui, |ui| changed |= alerts_tab(ui, svc, &db, st, watches));
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
    let mut look_up: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        let (all, recent) = (&st.commodities, &st.prefs.recent);
        let c = crate::widgets::autocomplete(ui, "commodity-search", &mut st.input, "Commodity: Tritium, LTD, Thargoid…", 320.0, |t| commodity_suggestions(all, recent, t));
        if let Some(p) = c.picked {
            look_up = Some(p);
        } else if c.submitted {
            look_up = Some(st.input.clone());
        }
        if crate::widgets::primary_button(ui, "Search").clicked() {
            look_up = Some(st.input.clone());
        }
        crate::widgets::disclosure(ui, &mut st.prefs.show_options, "Options");
        if st.page.busy() && st.page.last.is_some() {
            ui.spinner();
        }
    });
    if let Some(t) = look_up
        && !t.trim().is_empty()
    {
        st.open_commodity(db, &t);
    }
    if st.prefs.show_options {
        let p = &mut st.prefs;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            ui.label("within");
            ui.add(egui::DragValue::new(&mut p.max_dist).range(1.0..=MAX_SEARCH_LY).suffix(" ly")).on_hover_text("Searches every market Elite Nexus knows, up to the whole galaxy (100,000 ly).");
            let before = p.min_pad;
            pad_combo(ui, "mpad", &mut p.min_pad);
            if p.min_pad != before {
                p.pad_auto = false;
            }
            ui.label("max age");
            ui.add(egui::DragValue::new(&mut p.max_age_h).range(1.0..=720.0).suffix(" h")).on_hover_text("Leave out prices older than this.");
            ui.label("min qty");
            ui.add(egui::DragValue::new(&mut p.min_qty).range(1..=100_000)).on_hover_text("Least stock (where you buy) or demand (where you sell). 1 shows every market.");
            crate::widgets::toggle(ui, &mut p.carriers, "Carriers");
            crate::widgets::toggle(ui, &mut p.planetary, "Planetary");
            egui::ComboBox::from_id_salt("msort")
                .selected_text(match p.sort {
                    Sort::BestPrice => "Best price",
                    Sort::Nearest => "Nearest",
                    Sort::Freshest => "Freshest",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.sort, Sort::BestPrice, "Best price");
                    ui.selectable_value(&mut p.sort, Sort::Nearest, "Nearest");
                    ui.selectable_value(&mut p.sort, Sort::Freshest, "Freshest");
                });
            if ui.add(egui::Button::new(RichText::new("Reset").color(pal::DIM)).frame(false)).on_hover_text("Back to the default filters").clicked() {
                let d = MarketPrefs::default();
                (p.max_dist, p.min_pad, p.pad_auto, p.max_age_h, p.min_qty, p.carriers, p.planetary, p.sort) =
                    (d.max_dist, d.min_pad, d.pad_auto, d.max_age_h, d.min_qty, d.carriers, d.planetary, d.sort);
            }
        });
    }
    if let Some(t) = &st.not_found {
        ui.add_space(6.0);
        ui.label(RichText::new(format!("{} No commodity called \"{t}\". Pick one from the suggestions as you type.", icon::WARNING)).color(pal::WARN));
    }
    ui.add_space(8.0);

    // (Re)load whenever the commodity, a filter or the position changes; one query at a time.
    let p = &st.prefs;
    if p.commodity.trim().is_empty() {
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
    if st.page_key.as_ref() != Some(&q) && !st.page.busy() {
        st.page_key = Some(q.clone());
        let db = db.clone();
        st.page.run(ui.ctx(), move || load_commodity(&db, &q).map_err(|e| e.to_string()));
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

    // The picked market: its details and this commodity's price history there.
    if let Some(id) = st.pick {
        let key = (id, page.summary.symbol.clone());
        if st.detail_key.as_ref() != Some(&key) && !st.detail.busy() {
            st.detail_key = Some(key.clone());
            let db = db.clone();
            st.detail.run(ui.ctx(), move || {
                let market = db.station_market(key.0).map_err(|e| e.to_string())?;
                let history = db.price_history(key.0, &key.1).map_err(|e| e.to_string())?;
                Ok(PickDetail { market, history })
            });
        }
        let current = st.detail_key.as_ref().is_some_and(|k| k.0 == id) && !st.detail.busy();
        match (&st.detail.last, current) {
            (Some(Ok(d)), true) => match pick_card(ui, d, &page.summary, origin, &st.prefs, st.side == Side::Sell) {
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
                hits_table(ui, "commodity-hits", hits, mode, page.summary.average, st.pick, Some((good, &st.prefs)))
            }
        }
        Side::Latest => {
            if page.latest.is_empty() {
                if let Some(r) = empty_side(ui, page, Side::Latest, &st.prefs, origin.is_some()) {
                    r.apply(&mut st.prefs);
                }
                None
            } else {
                latest_table(ui, &page.latest, page.summary.average, st.pick, good, &st.prefs)
            }
        }
    };
    match action {
        Some(RowAction::Select(id)) => st.pick = if st.pick == Some(id) { None } else { Some(id) },
        Some(RowAction::Pin(p)) => st.prefs.toggle_pin(p),
        None => {}
    }
    nav
}

/// Loads a commodity's page (runs on a worker thread). `Ok(None)`: no such commodity.
fn load_commodity(db: &GalaxyDb, q: &CommodityQuery) -> nexus_galaxy::Result<Option<CommodityPage>> {
    let Some(summary) = db.commodity_summary(&q.commodity, q.max_age_hours)? else {
        return Ok(None);
    };
    let buy = db.search_commodity(&CommodityQuery { mode: TradeMode::BuyFromStation, ..q.clone() })?;
    let sell = db.search_commodity(&CommodityQuery { mode: TradeMode::SellToStation, ..q.clone() })?;
    let latest = db.latest_prices(q)?;
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
        if found || any == 0 { Ok(None) } else { Ok(db.search_commodity(&nearest(mode))?.into_iter().next()) }
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
            badge(ui, "Rare", pal::PURPLE).on_hover_text("Rare goods are sold only at their home station, in small allocations, and are worth more the farther you carry them.");
        }
    });
    ui.add_space(6.0);
    let range = if have_origin { format!(" within {:.0} ly", p.max_dist) } else { String::new() };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(32.0, 8.0);
        crate::widgets::stat(ui, "Galactic average", s.average.map_or_else(|| "—".into(), cr), pal::TEXT)
            .on_hover_text("The average price the game reports for this commodity (the one most stations quote). Fleet carriers don't report one.");
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
            pad: pad_rank(h.max_pad) < pad_rank(p.min_pad),
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
        p.show_options = true;
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

/// A commodity's (symbol, name) with the pins to compare against.
type PinCtx<'a> = ((&'a str, &'a str), &'a MarketPrefs);

/// Buy or sell offers for one commodity: price (and against the galactic average), quantity, where,
/// how far, how old; with `pins`, a pin toggle in front. Returns what a click asked for.
fn hits_table(ui: &mut Ui, id: &str, hits: &[CommodityHit], mode: TradeMode, average: Option<i64>, selected: Option<u64>, pins: Option<PinCtx>) -> Option<RowAction> {
    let buying = mode == TradeMode::BuyFromStation;
    let wide = ui.available_width() >= 700.0;
    let mut action = None;
    let mut sort = TableSort::load(ui, id);
    let mut table = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(false)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    if pins.is_some() {
        table = table.column(Column::initial(24.0));
    }
    table = table
        .column(Column::initial(118.0))
        .column(Column::initial(60.0))
        .column(Column::initial(76.0))
        .column(Column::remainder().at_least(150.0).clip(true))
        .column(Column::initial(72.0));
    if wide {
        table = table.column(Column::initial(36.0));
    }
    table = table.column(Column::initial(60.0));
    // Sortable columns: 0 price, 1 quantity, 2 station, 3 distance, 4 pad, 5 age. Cheapest first
    // where you buy, best price first where you sell.
    let mut heads: Heads = Vec::new();
    if pins.is_some() {
        heads.push(("", None));
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
        _ => a.updated_unix.cmp(&b.updated_unix),
    });
    table
        .body(|body| {
            body.rows(40.0, hits.len(), |mut row| {
                let h = &hits[order[row.index()]];
                row.set_selected(selected == Some(h.market_id));
                if let Some(((symbol, name), prefs)) = pins {
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
                row.col(|ui| {
                    ui.label(RichText::new(cr(h.price)).font(display(size::CALLOUT)).color(if buying { pal::CYAN } else { pal::GOOD }));
                });
                row.col(|ui| vs_average(ui, h.price, average, buying));
                row.col(|ui| {
                    ui.label(RichText::new(nexus_data::names::group_digits(h.quantity)).color(pal::DIM));
                });
                row.col(|ui| station_cell(ui, &h.station, &h.system, h.is_carrier, h.is_planetary, h.dist_ls));
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
                    action = Some(RowAction::Select(h.market_id));
                }
            });
        });
    action
}

/// Every market trading the commodity, newest price first, with both directions. Returns what a click asked for.
fn latest_table(ui: &mut Ui, quotes: &[CommodityQuote], average: Option<i64>, selected: Option<u64>, (symbol, name): (&str, &str), prefs: &MarketPrefs) -> Option<RowAction> {
    let wide = ui.available_width() >= 760.0;
    let mut action = None;
    const TABLE: &str = "commodity-latest";
    let mut sort = TableSort::load(ui, TABLE);
    let mut table = TableBuilder::new(ui)
        .id_salt("commodity-latest")
        .striped(true)
        .resizable(false)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(24.0))
        .column(Column::remainder().at_least(150.0).clip(true))
        .column(Column::initial(104.0));
    if wide {
        table = table.column(Column::initial(66.0));
    }
    table = table.column(Column::initial(104.0));
    if wide {
        table = table.column(Column::initial(66.0));
    }
    table = table.column(Column::initial(72.0)).column(Column::initial(60.0));
    // Sortable columns: 0 station, 1 buy price, 2 stock, 3 sell price, 4 demand, 5 distance, 6 age. A
    // market that doesn't trade that way sorts after the ones that do.
    let mut heads: Heads = vec![("", None), ("Station", Some((0, false))), ("Buy for", Some((1, false)))];
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
        _ => a.updated_unix.cmp(&b.updated_unix),
    });
    table
        .body(|body| {
            body.rows(40.0, quotes.len(), |mut row| {
                let q = &quotes[order[row.index()]];
                row.set_selected(selected == Some(q.market_id));
                let avg = Some(q.mean).filter(|m| *m > 0).or(average);
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
                row.col(|ui| station_cell(ui, &q.station, &q.system, q.is_carrier, q.is_planetary, q.dist_ls));
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
                    action = Some(RowAction::Select(q.market_id));
                }
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

/// "all" / "squadronfriends" / ... as words.
fn carrier_access(a: &str) -> &str {
    match a.to_ascii_lowercase().as_str() {
        "all" => "everyone",
        "none" => "nobody but the owner",
        "friends" => "friends only",
        "squadron" => "squadron only",
        "squadronfriends" => "squadron and friends",
        _ => a,
    }
}

/// The picked market: what it is, this commodity there, its price history, and a way to its whole market.
fn pick_card(ui: &mut Ui, d: &PickDetail, s: &CommoditySummary, origin: Option<[f64; 3]>, prefs: &MarketPrefs, prefer_sell: bool) -> PickAction {
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
            if let Some(r) = m.items.iter().find(|r| r.symbol == s.symbol) {
                let (buys, sells) = (r.sell > 0 && r.demand > 0, r.buy > 0 && r.stock > 0);
                if buys || sells {
                    let sell_here = if buys && sells { prefer_sell } else { buys };
                    let pinned = prefs.is_pinned(st.market_id, &s.symbol, sell_here);
                    let label = if pinned { format!("{} Pinned", icon::PUSH_PIN) } else { format!("{} Pin", icon::PUSH_PIN) };
                    if crate::widgets::button(ui, &label).on_hover_text("Keep it on top of the Market page with its latest price; the run planner fills it").clicked() {
                        let (price, qty) = if sell_here { (r.sell, r.demand) } else { (r.buy, r.stock) };
                        action = PickAction::Pin(Pin {
                            market_id: st.market_id,
                            station: st.name.clone(),
                            system: st.system.clone(),
                            carrier: st.is_carrier,
                            symbol: s.symbol.clone(),
                            name: s.name.clone(),
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
                    match m.items.iter().find(|r| r.symbol == s.symbol) {
                        Some(r) => {
                            if r.buy > 0 && r.stock > 0 {
                                ui.label(RichText::new(format!("Sells {} for {} · {} in stock", s.name, cr(r.buy), nexus_data::names::group_digits(r.stock))).color(pal::CYAN));
                            }
                            if r.sell > 0 && r.demand > 0 {
                                ui.label(RichText::new(format!("Buys {} for {} · {} wanted", s.name, cr(r.sell), nexus_data::names::group_digits(r.demand))).color(pal::GOOD));
                            }
                            let (a, col) = age(r.updated_unix);
                            ui.label(RichText::new(format!("Updated {a} ago · {} goods on this market", m.items.len())).size(size::FOOTNOTE).color(col));
                        }
                        None => {
                            ui.label(RichText::new(format!("Its latest market no longer lists {}.", s.name)).color(pal::DIM));
                        }
                    }
                },
                |ui| history_plot(ui, st.market_id, &d.history),
            );
        },
    );
    action
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
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        crate::widgets::segmented(ui, &mut p.abn_mode, &[(TradeMode::SellToStation, "Paying far above average"), (TradeMode::BuyFromStation, "Selling far below average")]);
        ui.label(RichText::new("at least").color(pal::DIM));
        let ratios: &[(u32, &str)] = &[(150, "1.5×"), (200, "2×"), (300, "3×"), (500, "5×"), (1000, "10×")];
        let mut r = (p.abn_ratio * 100.0).round() as u32;
        if crate::widgets::segmented(ui, &mut r, ratios) {
            p.abn_ratio = f64::from(r) / 100.0;
        }
        let scopes: &[(u32, &str)] = &[(0, "Whole galaxy"), (100, "Within 100 ly"), (500, "Within 500 ly")];
        let mut scope = p.abn_radius.map_or(0, |d| d as u32);
        if crate::widgets::segmented(ui, &mut scope, scopes) {
            p.abn_radius = (scope > 0).then_some(f64::from(scope));
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(14.0, 8.0);
        crate::widgets::toggle(ui, &mut p.abn_carriers, "Fleet carriers");
        ui.label(RichText::new("Prices from the last").color(pal::DIM));
        ui.add(egui::DragValue::new(&mut p.abn_age_h).range(1.0..=720.0).suffix(" h"));
        pad_combo(ui, "abnpad", &mut p.min_pad);
        if crate::widgets::button(ui, &format!("{} Refresh", icon::ARROW_CLOCKWISE)).clicked() {
            st.abn_key = None;
        }
    });
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
    let mut table = TableBuilder::new(ui)
        .id_salt("abnormal")
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(24.0))
        .column(Column::initial(180.0).at_least(110.0).clip(true))
        .column(Column::initial(118.0));
    if wide {
        table = table.column(Column::initial(100.0)).column(Column::initial(70.0)).column(Column::initial(84.0));
    } else {
        table = table.column(Column::initial(64.0));
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
        .column(Column::remainder().at_least(140.0).clip(true))
        .column(Column::initial(76.0))
        .column(Column::initial(76.0))
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
            body.rows(34.0, deals.len(), |mut row| {
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
                        if nav_label(ui, RichText::new(name).color(pal::TEXT), tip) {
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
    TableBuilder::new(ui)
        .id_salt("pins")
        .striped(true)
        .resizable(false)
        .vscroll(true)
        .max_scroll_height(30.0 * 5.0)
        .auto_shrink([false, true])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(24.0))
        .column(Column::initial(44.0))
        .column(Column::initial(150.0).clip(true))
        .column(Column::initial(124.0))
        .column(Column::remainder().at_least(140.0).clip(true))
        .column(Column::initial(66.0))
        .column(Column::initial(118.0))
        .column(Column::initial(26.0))
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
    if let Some(i) = flip {
        st.prefs.pins[i].in_plan = !st.prefs.pins[i].in_plan;
    }
    if let Some(i) = remove {
        st.prefs.pins.remove(i);
    }
    ui.add_space(8.0);
    nav
}

/// The carriers the game listed in this system since you arrived, against what the database knows.
fn carriers_card(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) -> Option<Nav> {
    let listed = carriers_here(gs);
    let key = (gs.location.system_address, listed.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>());
    if st.carriers_key.as_ref() != Some(&key) && !st.carriers.busy() {
        st.carriers_key = Some(key.clone());
        let db = db.clone();
        let job = key.clone();
        st.carriers.run(ui.ctx(), move || {
            let mut known = FxHashMap::default();
            for c in &job.1 {
                if let Some(s) = db.find_stations(c, 4).map_err(|e| e.to_string())?.into_iter().find(|s| s.is_carrier && s.name.eq_ignore_ascii_case(c)) {
                    known.insert(c.clone(), s);
                }
            }
            let recorded = db.stations_in_system(job.0).map_err(|e| e.to_string())?.into_iter().filter(|s| s.is_carrier).collect();
            Ok((known, recorded))
        });
    }
    // Only this system's answer (right after a jump the last one is still the previous system's).
    if st.carriers.busy() || st.carriers_key.as_ref() != Some(&key) {
        return None;
    }
    let Some(Ok((known, recorded))) = &st.carriers.last else { return None };
    let gone: Vec<&nexus_galaxy::StationInfo> = recorded.iter().filter(|s| !listed.iter().any(|(_, c)| c.eq_ignore_ascii_case(&s.name))).collect();
    if listed.is_empty() && gone.is_empty() {
        return None;
    }
    let mut nav = None;
    let title = format!("{} Fleet carriers in {} · {} listed since you arrived", icon::ANCHOR, gs.location.system, listed.len());
    egui::CollapsingHeader::new(RichText::new(title).font(display(size::CALLOUT)).color(pal::TEXT)).id_salt("carriers-here").default_open(true).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("carriers-here-list").max_height(180.0).show(ui, |ui| {
            Grid::new("carriers-grid").num_columns(3).striped(true).spacing([14.0, 4.0]).max_col_width(ui.available_width()).show(ui, |ui| {
                for (name, cs) in &listed {
                    ui.label(RichText::new(format!("{} {name}", icon::CHECK_CIRCLE)).color(pal::TEXT));
                    match known.get(cs) {
                        Some(s) if s.market_updated_unix > 0 => {
                            if nav_label(ui, RichText::new(format!("{} goods on its market", s.commodity_count)).color(pal::ACCENT_SOFT), "Click for its whole market") {
                                nav = Some(Nav::Station(s.market_id));
                            }
                            let (a, c) = age(s.market_updated_unix);
                            ui.label(RichText::new(format!("reported {a} ago")).color(c));
                        }
                        _ => {
                            ui.label(RichText::new("market not in the database").color(pal::FAINT));
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
                for s in &gone {
                    // Missing from a list the game has made: probably gone. With no list yet, nothing is known.
                    let (mark, tip) = if listed.is_empty() {
                        (icon::QUESTION, "The database places this carrier here; the game hasn't listed any carrier yet this visit.")
                    } else {
                        (icon::X_CIRCLE, "The database places this carrier here, but the game hasn't listed it since you arrived: it has probably jumped away.")
                    };
                    ui.label(RichText::new(format!("{mark} {}", s.name)).color(pal::DIM)).on_hover_text(tip);
                    if nav_label(ui, RichText::new("database only").color(pal::DIM), "Click for its last known market") {
                        nav = Some(Nav::Station(s.market_id));
                    }
                    let (a, _) = age(s.updated_unix.max(s.market_updated_unix));
                    ui.label(RichText::new(format!("last reported here {a} ago")).color(pal::FAINT));
                    ui.end_row();
                }
            });
        });
        if listed.is_empty() {
            crate::widgets::footnote(ui, "The game lists a system's carriers as they show up in your navigation panel: on arrival, or when you drop out of supercruise. None has been listed yet this visit.");
        }
    });
    ui.add_space(6.0);
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
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui.label("Look for sellers and buyers within");
        ui.add(egui::DragValue::new(&mut p.plan_radius).range(5.0..=MAX_SEARCH_LY).suffix(" ly")).on_hover_text("Around each pinned order: sellers near a pinned buyer, buyers near a pinned seller.");
        ui.label("of each order, by");
        crate::widgets::segmented(ui, &mut p.plan_cheapest, &[(false, "Shortest detour"), (true, "Cheapest price")]);
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(14.0, 8.0);
        crate::widgets::toggle(ui, &mut p.plan_hold, "Sell what's aboard first").on_hover_text("Cargo you already carry fills a pinned buyer before anything is bought for it.");
        crate::widgets::toggle(ui, &mut p.plan_carriers, "Fleet carriers").on_hover_text("Let the planner pick fleet carriers as sellers or buyers (pinned carriers always count).");
        ui.label(RichText::new(format!("{} pad · hold {} t, {} t free · {} · prices up to {:.0} h old", pad.unwrap_or('L'), capacity, (capacity - cargo_used).max(0), short_credits(gs.cmdr.credits), p.max_age_h)).color(pal::DIM));
        again = crate::widgets::button(ui, &format!("{} Plan again", icon::ARROWS_CLOCKWISE)).clicked();
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

fn feed_header(ui: &mut Ui, svc: &mut MarketService) {
    ui.horizontal_wrapped(|ui| {
        match svc.feed_summary() {
            Some((connected, line)) => {
                badge(ui, if connected { "Live" } else { "Connecting" }, if connected { pal::GOOD } else { pal::WARN });
                ui.label(RichText::new(line).color(pal::DIM));
            }
            None => {
                badge(ui, "Feed off", pal::GRAY);
                ui.label(RichText::new("Turn on the live feed in Accounts & sync for real-time prices.").color(pal::DIM));
            }
        }
        if let Some(s) = svc.db_summary() {
            ui.label(RichText::new(s).small().color(pal::DIM));
        }
    });
    for l in svc.recent_headlines(4) {
        ui.label(RichText::new(format!("{} {l}", icon::BROADCAST)).small().color(pal::DIM));
    }
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

fn results_routes(ui: &mut Ui, slot: &Slot<Res<TradeRoute>>) -> Option<Nav> {
    if !status(ui, slot) {
        return None;
    }
    let Some(Ok(routes)) = &slot.last else { return None };
    if routes.is_empty() {
        ui.label(RichText::new("No profitable routes in range with the current data.").color(pal::DIM));
        return None;
    }
    let mut nav = None;
    const TABLE: &str = "trade-routes";
    let mut sort = TableSort::load(ui, TABLE);
    let table = TableBuilder::new(ui)
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(170.0))
        .column(Column::initial(90.0))
        .column(Column::initial(100.0))
        .column(Column::initial(260.0))
        .column(Column::initial(260.0))
        .column(Column::remainder())
        .header(30.0, |mut h| {
            // Buy at / Sell at sort by how far from you the station is.
            let heads: Heads = vec![("Commodity", Some((0, false))), ("Profit/t", Some((1, true))), ("Trip profit", Some((2, true))), ("Buy at", Some((3, false))), ("Sell at", Some((4, false))), ("Hop", Some((5, false)))];
            headings(&mut h, &heads, &mut sort, TABLE);
        });
    let order = sort.order(routes, |a, b, c| match c {
        0 => cmp_text(&a.commodity_name, &b.commodity_name),
        1 => a.profit_per_ton.cmp(&b.profit_per_ton),
        2 => a.total_profit.cmp(&b.total_profit),
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
                for (end, price) in [(&r.from, r.buy_price), (&r.to, r.sell_price)] {
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            if nav_label(ui, format!("{} — {} Cr", end.station, nexus_data::names::group_digits(price)), "Click for the station's whole market") {
                                nav = Some(Nav::Station(end.market_id));
                            }
                            let (a, c) = age(end.market_updated_unix);
                            ui.label(RichText::new(format!("{} · {:.1} ly · {}", end.system, end.distance_from_origin_ly, a)).small().color(c));
                        });
                    });
                }
                row.col(|ui| {
                    ui.label(RichText::new(format!("{:.1} ly", r.hop_ly)).color(pal::DIM));
                });
            });
        });
    nav
}

fn results_stations(ui: &mut Ui, slot: &Slot<Res<StationHit>>) {
    if !status(ui, slot) {
        return;
    }
    let Some(Ok(list)) = &slot.last else { return };
    const TABLE: &str = "services";
    let mut sort = TableSort::load(ui, TABLE);
    let table = TableBuilder::new(ui)
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(260.0))
        .column(Column::initial(220.0))
        .column(Column::initial(80.0))
        .column(Column::initial(80.0))
        .column(Column::initial(50.0))
        .column(Column::remainder())
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
                    ui.label(&s.station.name);
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
}

fn results_hotspots(ui: &mut Ui, slot: &Slot<Res<HotspotHit>>) {
    card(ui, "Rings", |ui| {
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

/// The Station tab: a market opened from a list, or the one you're docked at.
fn station_tab(ui: &mut Ui, gs: &GameState, db: &GalaxyDb, st: &mut MarketUi) -> Option<Nav> {
    let Some(id) = st.station else {
        let carriers = carriers_card(ui, gs, db, st);
        return docked_market(ui, gs).or(carriers);
    };
    if st.station_key != Some(id) && !st.station_market.busy() {
        st.station_key = Some(id);
        let db = db.clone();
        st.station_market.run(ui.ctx(), move || db.station_market(id).map_err(|e| e.to_string()));
    }
    if gs.market.is_some() && crate::widgets::button(ui, &format!("{} The station you're docked at", icon::ARROW_LEFT)).clicked() {
        st.station = None;
        return None;
    }
    let current = st.station_key == Some(id) && !st.station_market.busy();
    match (&st.station_market.last, current) {
        (Some(Ok(Some(m))), true) => {
            let (nav, pin) = db_market(ui, m, gs.location.star_pos, &st.prefs);
            if let Some(p) = pin {
                st.prefs.toggle_pin(p);
            }
            nav
        }
        (Some(Ok(None)), true) => {
            empty(ui, icon::STOREFRONT, "Station not in the database", "It may have been a fleet carrier that has since been decommissioned.");
            None
        }
        (Some(Err(e)), true) => {
            ui.label(RichText::new(e).color(pal::BAD));
            None
        }
        _ => {
            ui.spinner();
            None
        }
    }
}

/// A station's market from the database: every good it trades, against the galactic average.
fn db_market(ui: &mut Ui, m: &StationMarket, origin: Option<[f64; 3]>, prefs: &MarketPrefs) -> (Option<Nav>, Option<Pin>) {
    let s = &m.station;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let name = if s.is_carrier { format!("{} {}", icon::ANCHOR, s.name) } else { s.name.clone() };
        ui.label(RichText::new(name).font(display(size::TITLE3)).color(pal::TEXT));
        if ui.link(&s.system).on_hover_text("Click to copy the system").clicked() {
            ui.ctx().copy_text(s.system.clone());
        }
    });
    let mut facts = Vec::new();
    if let Some(t) = &s.station_type {
        facts.push(t.clone());
    }
    if let Some(p) = s.max_pad {
        facts.push(format!("{p} pad"));
    }
    if let Some(ls) = s.dist_ls {
        facts.push(format!("{ls:.0} ls from arrival"));
    }
    if let (Some(o), Some(p)) = (origin, s.pos) {
        facts.push(format!("{:.1} ly from you", ((o[0] - p[0]).powi(2) + (o[1] - p[1]).powi(2) + (o[2] - p[2]).powi(2)).sqrt()));
    }
    if s.market_updated_unix > 0 {
        facts.push(format!("market updated {} ago", age(s.market_updated_unix).0));
    }
    if s.is_carrier {
        facts.push(format!("docking: {}", s.carrier_docking_access.as_deref().map(carrier_access).unwrap_or("unknown")));
    }
    ui.label(RichText::new(facts.join(" · ")).color(pal::DIM));
    if !m.prohibited.is_empty() {
        ui.add(egui::Label::new(RichText::new(format!("Prohibited: {}", m.prohibited.join(", "))).size(size::FOOTNOTE).color(pal::DIM)).wrap());
    }
    ui.add_space(8.0);
    let items: Vec<_> = m.items.iter().filter(|i| (i.buy > 0 && i.stock > 0) || (i.sell > 0 && i.demand > 0)).collect();
    if items.is_empty() {
        empty(ui, icon::STOREFRONT, "No market data", "The database has no prices for this station yet.");
        return (None, None);
    }
    let mut nav = None;
    let mut pin = None;
    const TABLE: &str = "db-market";
    let mut sort = TableSort::load(ui, TABLE);
    let table = TableBuilder::new(ui)
        .id_salt("db-market")
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(24.0))
        .column(Column::remainder().at_least(160.0).clip(true))
        .column(Column::initial(140.0).clip(true))
        .column(Column::initial(96.0))
        .column(Column::initial(76.0))
        .column(Column::initial(96.0))
        .column(Column::initial(76.0))
        .column(Column::initial(60.0))
        .header(30.0, |mut h| {
            let heads: Heads = vec![
                ("", None),
                ("Commodity", Some((0, false))),
                ("Category", Some((1, false))),
                ("Buy for", Some((2, false))),
                ("Stock", Some((3, true))),
                ("Sell for", Some((4, true))),
                ("Demand", Some((5, true))),
                ("vs avg", Some((6, true))),
            ];
            headings(&mut h, &heads, &mut sort, TABLE);
        });
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
    let order = sort.order(&items, |a, b, c| match c {
        0 => cmp_text(&a.name, &b.name),
        1 => cmp_text(&a.category, &b.category).then_with(|| cmp_text(&a.name, &b.name)),
        2 => cmp_opt(price(a.buy, a.stock), price(b.buy, b.stock)),
        3 => a.stock.cmp(&b.stock),
        4 => cmp_opt(price(a.sell, a.demand), price(b.sell, b.demand)),
        5 => a.demand.cmp(&b.demand),
        _ => cmp_opt(deal(a).or(if desc { Some(f64::NEG_INFINITY) } else { None }), deal(b).or(if desc { Some(f64::NEG_INFINITY) } else { None })),
    });
    table
        .body(|body| {
            body.rows(32.0, items.len(), |mut row| {
                let i = items[order[row.index()]];
                let sells = i.buy > 0 && i.stock > 0;
                let buys = i.sell > 0 && i.demand > 0;
                row.col(|ui| {
                    // Pins the side it trades (where it buys, when it does both).
                    if pin_button(ui, prefs.is_pinned(s.market_id, &i.symbol, buys)) {
                        let (price, qty) = if buys { (i.sell, i.demand) } else { (i.buy, i.stock) };
                        pin = Some(Pin {
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
                        });
                    }
                });
                row.col(|ui| {
                    if nav_label(ui, RichText::new(&i.name).color(pal::TEXT), "Where to buy and sell it, latest prices") {
                        nav = Some(Nav::Commodity { name: i.symbol.clone(), side: None });
                    }
                });
                row.col(|ui| {
                    ui.label(RichText::new(&i.category).color(pal::DIM));
                });
                row.col(|ui| {
                    if sells {
                        ui.label(RichText::new(cr(i.buy)).color(pal::CYAN));
                    }
                });
                row.col(|ui| {
                    if sells {
                        ui.label(RichText::new(nexus_data::names::group_digits(i.stock)).color(pal::DIM));
                    }
                });
                row.col(|ui| {
                    if buys {
                        ui.label(RichText::new(cr(i.sell)).color(pal::GOOD));
                    }
                });
                row.col(|ui| {
                    if buys {
                        ui.label(RichText::new(nexus_data::names::group_digits(i.demand)).color(pal::DIM));
                    }
                });
                row.col(|ui| {
                    // Against the side the station trades: what it sells is judged as a purchase.
                    if sells {
                        vs_average(ui, i.buy, Some(i.mean), true);
                    } else if buys {
                        vs_average(ui, i.sell, Some(i.mean), false);
                    }
                });
            });
        });
    (nav, pin)
}

fn docked_market(ui: &mut Ui, gs: &GameState) -> Option<Nav> {
    let Some(m) = &gs.market else {
        empty(ui, icon::STOREFRONT, "No market opened yet", "Open a station's commodity market: its full price list appears here. Click a station anywhere on the Market page to see its market from the database.");
        return None;
    };
    ui.label(RichText::new(format!("{} — {} · {}", m.station_name, m.star_system, m.timestamp.to_local_string("%d %b %H:%M"))).font(display(16.0)));
    let mut items: Vec<_> = m.items.iter().filter(|i| i.buy_price > 0 || i.sell_price > 0).collect();
    items.sort_by(|a, b| a.category_localised.cmp(&b.category_localised).then(a.name_localised.cmp(&b.name_localised)));
    let mut nav = None;
    const TABLE: &str = "docked-market";
    let mut sort = TableSort::load(ui, TABLE);
    let table = TableBuilder::new(ui)
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(240.0))
        .column(Column::initial(160.0))
        .column(Column::initial(90.0))
        .column(Column::initial(90.0))
        .column(Column::initial(90.0))
        .column(Column::remainder())
        .header(30.0, |mut h| {
            let heads: Heads = vec![("Commodity", Some((0, false))), ("Category", Some((1, false))), ("Buy", Some((2, false))), ("Sell", Some((3, true))), ("Stock", Some((4, true))), ("Demand", Some((5, true)))];
            headings(&mut h, &heads, &mut sort, TABLE);
        });
    let desc = sort.descending;
    let price = move |v: i64| if v > 0 { Some(v as f64) } else if desc { Some(f64::NEG_INFINITY) } else { None };
    let name = |i: &&nexus_journal::companion::MarketItem| i.name_localised.clone().unwrap_or_else(|| i.symbol());
    let order = sort.order(&items, |a, b, c| match c {
        0 => cmp_text(&name(a), &name(b)),
        1 => cmp_text(a.category_localised.as_deref().unwrap_or_default(), b.category_localised.as_deref().unwrap_or_default()),
        2 => cmp_opt(price(a.buy_price), price(b.buy_price)),
        3 => cmp_opt(price(a.sell_price), price(b.sell_price)),
        4 => a.stock.cmp(&b.stock),
        _ => a.demand.cmp(&b.demand),
    });
    table
        .body(|body| {
            body.rows(32.0, items.len(), |mut row| {
                let i = items[order[row.index()]];
                row.col(|ui| {
                    if nav_label(ui, RichText::new(i.name_localised.clone().unwrap_or_else(|| i.symbol())).color(pal::TEXT), "Where to buy and sell it, latest prices") {
                        nav = Some(Nav::Commodity { name: i.symbol(), side: None });
                    }
                });
                row.col(|ui| {
                    ui.label(RichText::new(i.category_localised.clone().unwrap_or_default()).color(pal::DIM));
                });
                row.col(|ui| {
                    if i.buy_price > 0 {
                        ui.label(RichText::new(nexus_data::names::group_digits(i.buy_price)).color(pal::ACCENT_SOFT));
                    }
                });
                row.col(|ui| {
                    if i.sell_price > 0 {
                        ui.label(RichText::new(nexus_data::names::group_digits(i.sell_price)).color(pal::GOLD));
                    }
                });
                row.col(|ui| {
                    ui.label(RichText::new(if i.stock > 0 { nexus_data::names::group_digits(i.stock) } else { String::new() }).color(pal::DIM));
                });
                row.col(|ui| {
                    // Demand bracket 0 is the game's "no demand" (it lists its own goods with demand 1).
                    ui.label(RichText::new(if i.demand > 0 && i.demand_bracket != 0 { nexus_data::names::group_digits(i.demand) } else { String::new() }).color(pal::DIM));
                });
            });
        });
    nav
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
                        digits(f.ingested),
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
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{} {} t of {} commodities", icon::PACKAGE, nexus_data::names::group_digits(tons), hold.len())).font(display(15.0)));
        ui.add_space(12.0);
        ui.label("buyers within");
        search |= ui.add(egui::DragValue::new(&mut st.prefs.sell_radius).range(5.0..=MAX_SEARCH_LY).suffix(" ly")).changed();
        ui.label(RichText::new(format!("· {} pad · prices up to {:.0} h old", pad.unwrap_or('L'), st.prefs.max_age_h)).color(pal::DIM));
        search |= ui.button(RichText::new(format!("{} Refresh", icon::ARROWS_CLOCKWISE)).color(pal::ACCENT)).clicked();
    });
    let key = (gs.inventory.cargo.updated.0, tons, gs.location.system_address);
    if (search || key != st.sell_key) && !st.sell.busy() {
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
    if let Some(t) = st.sell.took {
        ui.label(RichText::new(format!("planned in {:.1} ms", t.as_secs_f64() * 1e3)).small().color(pal::DIM));
    }
    if plan.stops.is_empty() {
        ui.label(RichText::new("No known buyers in range. Widen the radius, or fill the database (Market → Database & feed).").color(pal::DIM));
        return None;
    }
    let mut nav = None;
    card(ui, "Best single stop", |ui| {
        const TABLE: &str = "sell-stops";
        let mut sort = TableSort::load(ui, TABLE);
        let table = TableBuilder::new(ui)
            .id_salt("sellstops")
            .striped(true)
            .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .vscroll(false)
            .column(Column::initial(250.0))
            .column(Column::initial(190.0))
            .column(Column::initial(70.0))
            .column(Column::initial(110.0))
            .column(Column::initial(90.0))
            .column(Column::remainder())
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
                            if nav_label(ui, RichText::new(&h.station).color(pal::TEXT), "Click for the station's whole market") {
                                nav = Some(Nav::Station(h.market_id));
                            }
                            if let Some(ls) = h.dist_ls {
                                ui.label(RichText::new(format!("{ls:.0} ls")).small().color(pal::DIM));
                            }
                        });
                    });
                    row.col(|ui| {
                        if ui.link(&h.system).on_hover_text("Click to copy").clicked() {
                            ui.ctx().copy_text(h.system.clone());
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
    ui.add_space(8.0);
    card(ui, "Best buyer per commodity", |ui| {
        Grid::new("sellitems").num_columns(4).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
            for t in ["Commodity", "Tonnes", "Best price", "Where"] {
                ui.label(RichText::new(t).color(pal::DIM));
            }
            ui.end_row();
            for (name, count, hits) in &plan.per_item {
                if nav_label(ui, RichText::new(name).color(pal::TEXT), "Every buyer and the latest prices") {
                    nav = Some(Nav::Commodity { name: name.clone(), side: Some(Side::Sell) });
                }
                ui.label(RichText::new(nexus_data::names::group_digits(*count)).color(pal::DIM));
                match hits.first() {
                    Some(h) => {
                        ui.label(RichText::new(cr(h.price)).font(display(14.0)).color(pal::GOLD));
                        let others = hits.iter().skip(1).map(|o| format!("{} — {} / {}", cr(o.price), o.station, o.system)).collect::<Vec<_>>().join("\n");
                        let r = ui.label(
                            RichText::new(format!("{} — {} · {:.1} ly · demand {}", h.station, h.system, h.distance_ly.unwrap_or(0.0), nexus_data::names::group_digits(h.quantity)))
                                .color(pal::DIM),
                        );
                        if !others.is_empty() {
                            r.on_hover_text(others);
                        }
                    }
                    None => {
                        ui.label(RichText::new("no buyer in range").color(pal::FAINT));
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    });
    nav
}

fn describe_watch(w: &PriceWatch) -> String {
    let digits = nexus_data::names::group_digits;
    let mut s = format!("{} {} {} Cr", w.commodity, if w.sell { "sells for at least" } else { "can be bought for at most" }, digits(w.price));
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

/// Rules checked against every live market update. Returns true when the rules changed.
fn alerts_tab(ui: &mut Ui, svc: &MarketService, db: &GalaxyDb, st: &mut MarketUi, watches: &mut Vec<PriceWatch>) -> bool {
    let mut changed = false;
    if st.commodities.is_empty() {
        st.commodities = db.commodities();
    }
    ui.label(
        RichText::new(
            "Every market update from the live feed (and every market you open) is checked against these rules the moment it arrives, \
             usually seconds after a commander docks. A rule fires at most once per station per hour, as a toast and, with voice on, spoken.",
        )
        .color(pal::DIM),
    );
    ui.add_space(6.0);
    card(ui, "New alert", |ui| {
        let w = &mut st.new_watch;
        let (all, recent) = (&st.commodities, &st.prefs.recent);
        ui.horizontal_wrapped(|ui| {
            crate::widgets::autocomplete(ui, "alert-commodity", &mut w.commodity, "Commodity", 210.0, |t| commodity_suggestions(all, recent, t));
            crate::widgets::segmented(ui, &mut w.sell, &[(true, "sells for at least"), (false, "can be bought for at most")]);
            ui.add(egui::DragValue::new(&mut w.price).range(1..=100_000_000).speed(100.0).suffix(" Cr"));
            ui.label("within");
            ui.add(egui::DragValue::new(&mut w.max_ly).range(0.0..=20_000.0).suffix(" ly")).on_hover_text("0 = anywhere");
            pad_combo(ui, "wpad", &mut w.min_pad);
            ui.label(if w.sell { "min demand" } else { "min stock" });
            ui.add(egui::DragValue::new(&mut w.min_qty).range(1..=1_000_000).suffix(" t"));
            crate::widgets::toggle(ui, &mut w.carriers, "Fleet carriers");
        });
        let known = !w.commodity.trim().is_empty() && svc.resolves(w.commodity.trim());
        ui.horizontal(|ui| {
            if ui.add_enabled(known, egui::Button::new(RichText::new(format!("{} Add alert", icon::BELL_RINGING)).color(pal::ACCENT))).clicked() {
                let mut rule = w.clone();
                rule.commodity = rule.commodity.trim().to_string();
                rule.enabled = true;
                watches.push(rule);
                changed = true;
            }
            if !known && !w.commodity.trim().is_empty() {
                ui.label(RichText::new("Unknown commodity: pick one from the suggestions").color(pal::BAD));
            }
        });
    });
    ui.add_space(8.0);
    card(ui, "Your alerts", |ui| {
        if watches.is_empty() {
            ui.label(RichText::new("No alerts yet. Example: Tritium sells for at least 60,000 Cr within 50 ly.").color(pal::FAINT));
            return;
        }
        let mut remove = None;
        for (i, w) in watches.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= crate::widgets::switch(ui, &mut w.enabled).on_hover_text("Enabled").changed();
                let col = if w.enabled { pal::TEXT } else { pal::FAINT };
                ui.label(RichText::new(describe_watch(w)).color(col));
                if !svc.resolves(&w.commodity) {
                    ui.label(RichText::new("unknown commodity").small().color(pal::BAD));
                }
                if ui.small_button(icon::TRASH).on_hover_text("Delete").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            watches.remove(i);
            changed = true;
        }
    });
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
        };
        let r = Relax::for_hit(&hit, &p, true);
        assert_eq!(r.reasons(&p), vec!["farther than 60 ly", "fewer than 100 t", "fleet carrier (switched off)"]);
        let mut wider = p.clone();
        r.apply(&mut wider);
        assert_eq!((wider.max_dist, wider.min_qty, wider.carriers, wider.min_pad), (820.0, 45, true, Some('L')));
        assert!(wider.show_options, "the filters that changed are shown");
    }
}
