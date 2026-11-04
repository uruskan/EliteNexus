//! Where to get a material or a commodity: the nearest material trader of its kind, the nearest markets that sell it,
//! rings with hotspots for it, and, for a raw material, the planets near you richest in it. A button on a row opens a
//! panel under it (Engineering's needs and shopping list, Missions' goods); each answer says where it comes from and
//! what it can't know, and can open the Market page on the full list.

use crate::jobs::Slot;
use crate::theme::{pal, size};
use egui::{RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_data::materials::{Material, MaterialKind};
use nexus_galaxy::{CommodityHit, CommodityQuery, GalaxyDb, HotspotHit, HotspotQuery, MaterialTrader, Sort, StationHit, StationQuery, TradeMode};
use nexus_model::GameState;
use nexus_spansh::{Coords, Reference, SearchRequest, SortDir, Spansh, filter, sort};
use serde_json::json;

/// How far around you Spansh is asked for planets rich in a raw material, ly.
const PLANETS_LY: f64 = 100.0;
/// Market prices older than this aren't offered, hours (the Market page's own default).
const MARKET_AGE_H: f64 = 72.0;

/// What a panel asks the Market page to open.
#[derive(Debug, Clone, PartialEq)]
pub enum Goto {
    /// Services, on material traders of this kind.
    Trader(MaterialKind),
    /// Find commodity, on where to buy this commodity (its name).
    Commodity(String),
    /// Mining hotspots, for this signal (a name the Mining hotspots tab lists).
    Hotspots(String),
}

/// What the panels search with.
pub struct Sources<'a> {
    pub db: Option<&'a GalaxyDb>,
    pub spansh: &'a Spansh,
    pub rt: &'a tokio::runtime::Handle,
}

/// What's wanted, as Elite Nexus knows it.
enum Wanted {
    Material(&'static Material),
    Commodity(&'static nexus_data::commodities::Commodity),
    /// An on-foot resource, or something unknown: no source to offer.
    Other(String),
}

impl Wanted {
    fn of(symbol: &str) -> Wanted {
        if let Some(m) = nexus_data::materials::material(symbol) {
            Wanted::Material(m)
        } else if let Some(c) = nexus_data::commodities::commodity(symbol) {
            Wanted::Commodity(c)
        } else if let Some(r) = nexus_data::materials::micro_resource(symbol) {
            Wanted::Other(r.name.clone())
        } else {
            Wanted::Other(nexus_data::commodities::prettify(symbol))
        }
    }
}

/// What the market database knows near you.
#[derive(Default)]
struct Local {
    traders: Vec<StationHit>,
    markets: Vec<CommodityHit>,
    hotspots: Vec<HotspotHit>,
}

/// A planet Spansh knows, with the share of the material it's made of.
struct Rich {
    body: String,
    system: String,
    distance_ly: f64,
    share: f64,
    gravity_g: Option<f64>,
}

/// Landable planets near you richest in a raw material, and how many Spansh found in all.
struct Planets {
    list: Vec<Rich>,
    matched: u64,
}

/// One list's "where to get it" panels: which one is open, and its answers.
#[derive(Default)]
pub struct WhereToGet {
    /// The symbol whose panel is open.
    open: Option<String>,
    /// What the answers were found for: the symbol and the system you were in (a jump looks again).
    key: Option<(String, u64)>,
    local: Slot<Result<Local, String>>,
    planets: Slot<Result<Planets, String>>,
}

impl WhereToGet {
    /// The button at the end of a row: opens its panel, or closes it.
    pub fn button(&mut self, ui: &mut Ui, symbol: &str) {
        let open = self.open.as_deref() == Some(symbol);
        let (glyph, tip) = if open { (icon::X, "Close") } else { (icon::MAP_PIN, "Where to get it") };
        if crate::widgets::icon_button(ui, glyph, tip).clicked() {
            self.open = if open { None } else { Some(symbol.to_string()) };
        }
    }

    /// Whether `symbol`'s panel is open.
    pub fn is_open(&self, symbol: &str) -> bool {
        self.open.as_deref() == Some(symbol)
    }

    /// `symbol`'s panel, when it's open (put it right under its row). Returns what it asks the Market page to open.
    pub fn panel(&mut self, ui: &mut Ui, gs: &GameState, src: &Sources, symbol: &str) -> Option<Goto> {
        if !self.is_open(symbol) {
            return None;
        }
        self.local.tick();
        self.planets.tick();
        let wanted = Wanted::of(symbol);
        let Some(origin) = gs.location.star_pos else {
            crate::widgets::footnote(ui, "Elite Nexus doesn't know where you are yet: jump or relog once.");
            return None;
        };
        let key = (symbol.to_string(), gs.location.system_address);
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.search(ui.ctx(), gs, src, &wanted, origin);
        }
        match &wanted {
            Wanted::Material(m) => self.material(ui, m),
            Wanted::Commodity(c) => self.commodity(ui, c),
            Wanted::Other(name) => {
                crate::widgets::footnote(ui, &format!("Elite Nexus has no record of where {name} comes from."));
                None
            }
        }
    }

    /// Starts the searches for what's wanted, around `origin`.
    fn search(&mut self, ctx: &egui::Context, gs: &GameState, src: &Sources, wanted: &Wanted, origin: [f64; 3]) {
        self.local = Slot::default();
        self.planets = Slot::default();
        if let Some(db) = src.db {
            let db = db.clone();
            let pad = crate::finder::ship_pad(gs);
            let trader = match wanted {
                Wanted::Material(m) if m.tradeable => Some(match m.kind {
                    MaterialKind::Raw => MaterialTrader::Raw,
                    MaterialKind::Manufactured => MaterialTrader::Manufactured,
                    MaterialKind::Encoded => MaterialTrader::Encoded,
                }),
                _ => None,
            };
            let goods = match wanted {
                Wanted::Commodity(c) => Some(c.symbol.clone()),
                _ => None,
            };
            if trader.is_some() || goods.is_some() {
                self.local.run(ctx, move || {
                    let mut found = Local::default();
                    if let Some(t) = trader {
                        let q = StationQuery { origin, material_trader: Some(t), min_pad: pad, limit: 2, ..Default::default() };
                        found.traders = db.stations_near(&q).map_err(|e| e.to_string())?;
                    }
                    if let Some(symbol) = goods {
                        let q = CommodityQuery {
                            commodity: symbol.clone(),
                            mode: TradeMode::BuyFromStation,
                            origin: Some(origin),
                            min_pad: pad,
                            max_age_hours: Some(MARKET_AGE_H),
                            sort: Sort::Nearest,
                            limit: 3,
                            ..Default::default()
                        };
                        found.markets = db.search_commodity(&q).map_err(|e| e.to_string())?;
                        let h = HotspotQuery { origin, signal: symbol, limit: 3, ..Default::default() };
                        found.hotspots = db.hotspots_near(&h).map_err(|e| e.to_string())?;
                    }
                    Ok(found)
                });
            }
        }
        if let Wanted::Material(m) = wanted
            && m.kind == MaterialKind::Raw
        {
            let spansh = src.spansh.clone();
            let req = rich_planets(&m.name, origin);
            let name = m.name.clone();
            self.planets.run_async(ctx, src.rt, async move {
                let res = spansh.search_bodies(&req).await.map_err(|e| e.to_string())?;
                let list = res
                    .results
                    .into_iter()
                    .map(|b| Rich {
                        share: b.materials.iter().find(|s| s.name.eq_ignore_ascii_case(&name)).map_or(0.0, |s| s.share),
                        distance_ly: b.distance.unwrap_or(0.0),
                        gravity_g: b.gravity,
                        system: b.system_name,
                        body: b.name,
                    })
                    .collect();
                Ok(Planets { list, matched: res.count })
            });
        }
    }

    /// A material: its trader, and for a raw one the planets richest in it.
    fn material(&mut self, ui: &mut Ui, m: &Material) -> Option<Goto> {
        let mut goto = None;
        let kind = m.kind.label();
        if m.tradeable {
            crate::widgets::section(ui, &format!("{kind} material trader"));
            match &self.local.last {
                _ if self.local.busy() => searching(ui),
                Some(Ok(l)) if !l.traders.is_empty() => {
                    for t in &l.traders {
                        let s = &t.station;
                        let pad = s.max_pad.map(|p| format!(" · {p} pad")).unwrap_or_default();
                        place(ui, &s.name, &s.system, &format!("{:.0} ly{pad}", t.distance_ly), None);
                    }
                }
                Some(Ok(_)) => quiet(ui, &format!("No {} material trader Elite Nexus knows of has a pad your ship fits.", kind.to_lowercase())),
                Some(Err(e)) => quiet(ui, e),
                None => quiet(ui, "The market database isn't open."),
            }
            ui.horizontal_wrapped(|ui| {
                if crate::widgets::button(ui, &format!("{} All {} traders", icon::STOREFRONT, kind.to_lowercase())).clicked() {
                    goto = Some(Goto::Trader(m.kind));
                }
            });
            crate::widgets::footnote(
                ui,
                &format!(
                    "{} The nearest Elite Nexus's market database knows (from the live feed, your journal and the Spansh import): a trader it hasn't heard of isn't here.",
                    trade_words(m)
                ),
            );
        } else {
            crate::widgets::section(ui, "Material traders");
            quiet(ui, &format!("No material trader takes {} materials.", m.category));
        }
        if m.kind == MaterialKind::Raw {
            crate::widgets::section(ui, &format!("Planets within {PLANETS_LY:.0} ly richest in it"));
            match &self.planets.last {
                _ if self.planets.busy() => searching(ui),
                Some(Ok(p)) if !p.list.is_empty() => {
                    for b in &p.list {
                        let g = b.gravity_g.map(|g| format!(" · {g:.2} g")).unwrap_or_default();
                        place(ui, &b.body, &b.system, &format!("{:.0} ly · {:.1} % {}{g}", b.distance_ly, b.share, m.name), None);
                    }
                    if p.matched > p.list.len() as u64 {
                        quiet(ui, &format!("{} of {} landable planets with it.", p.list.len(), group(p.matched)));
                    }
                }
                Some(Ok(_)) => quiet(ui, &format!("Spansh knows no landable planet with {} within {PLANETS_LY:.0} ly.", m.name)),
                Some(Err(e)) => quiet(ui, &format!("Spansh didn't answer: {e}")),
                None => {}
            }
            crate::widgets::footnote(
                ui,
                "From Spansh, among the planets commanders scanned and shared. The share is the planet's own mix, as its scan shows it: what you find on the ground varies from spot to spot.",
            );
        }
        crate::widgets::footnote(ui, &format!("{} Elite Nexus has no list of where each material drops.", other_sources(m)));
        goto
    }

    /// A commodity: the nearest markets that sell it, and rings with hotspots for it.
    fn commodity(&mut self, ui: &mut Ui, c: &nexus_data::commodities::Commodity) -> Option<Goto> {
        let mut goto = None;
        crate::widgets::section(ui, "Markets that sell it");
        let local = match &self.local.last {
            _ if self.local.busy() => {
                searching(ui);
                None
            }
            Some(Ok(l)) => Some(l),
            Some(Err(e)) => {
                quiet(ui, e);
                None
            }
            None => {
                quiet(ui, "The market database isn't open.");
                None
            }
        };
        if let Some(l) = local {
            if l.markets.is_empty() {
                quiet(ui, &format!("No market Elite Nexus knows sold {} in the last {MARKET_AGE_H:.0} hours (with a pad your ship fits).", c.name));
            }
            for h in &l.markets {
                let (age, _) = crate::pages::market::age(h.updated_unix);
                let d = h.distance_ly.map(|d| format!("{d:.0} ly · ")).unwrap_or_default();
                let figures = format!("{d}{} Cr · {} t · {age} ago", nexus_data::names::group_digits(h.price), nexus_data::names::group_digits(h.quantity));
                place(ui, &h.station, &h.system, &figures, h.is_carrier.then_some(h.carrier_access.as_deref()));
            }
            if !l.hotspots.is_empty() {
                crate::widgets::section(ui, "Rings with hotspots for it");
                for h in &l.hotspots {
                    place(ui, &h.body_name, &h.system, &format!("{:.0} ly · {} hotspot{}", h.distance_ly, h.count, if h.count == 1 { "" } else { "s" }), None);
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            if crate::widgets::button(ui, &format!("{} Where to buy it", icon::STOREFRONT)).clicked() {
                goto = Some(Goto::Commodity(c.name.clone()));
            }
            let signal = crate::pages::market::hotspot_signal(&c.name);
            if let Some(s) = signal.filter(|_| local.is_some_and(|l| !l.hotspots.is_empty()))
                && crate::widgets::button(ui, &format!("{} Mining hotspots", icon::DIAMONDS_FOUR)).clicked()
            {
                goto = Some(Goto::Hotspots(s.to_string()));
            }
        });
        crate::widgets::footnote(
            ui,
            &format!(
                "The nearest Elite Nexus's market database knows: markets as someone last reported them, and rings someone scanned. A market or ring nobody reported isn't here. A fleet carrier ({}) says who may dock, as last reported.",
                icon::ANCHOR
            ),
        );
        goto
    }
}

/// Spansh: landable planets within [`PLANETS_LY`] of `origin` that have the material, richest first.
fn rich_planets(material: &str, origin: [f64; 3]) -> SearchRequest {
    SearchRequest::new()
        .near(Reference::Coords(Coords::new(origin[0], origin[1], origin[2])))
        .within(PLANETS_LY)
        .filter("is_landable", filter::eq(true))
        .filter("materials", json!([{ "name": material, "value": [0, 100], "comparison": "<=>" }]))
        .sort(sort::named("materials", material, SortDir::Desc))
        .size(3)
}

/// What a trader takes for this material: any other of its kind, those of its own category (or family) at the usual
/// rate and the rest at six times the cost, as the shopping list's "Trade across categories" counts it.
fn trade_words(m: &Material) -> String {
    let (own, others) = match m.raw_category() {
        Some(c) => (format!("category {c}"), "the other categories"),
        None => (format!("the {} family", m.category), "the other families"),
    };
    format!("There, any other {} material trades for it: from {own} at the usual rate, from {others} at six times the cost.", m.kind.label().to_lowercase())
}

/// Where materials of its kind come from, in general.
fn other_sources(m: &Material) -> &'static str {
    match m.kind {
        MaterialKind::Raw => "Raw materials are also mined: on planet surfaces with the SRV, and from asteroids.",
        MaterialKind::Manufactured => "Manufactured materials are also salvaged: from signal sources, settlements and the ships you destroy.",
        MaterialKind::Encoded => "Encoded materials are also scanned: from ships, data points, signal sources and settlements.",
    }
}

/// A place as one line: its name, its system (click to copy), and the figures after them. A fleet carrier (`carrier`:
/// who may dock, as last reported; `Some(None)` when that was never reported) is marked as the Market lists mark it.
fn place(ui: &mut Ui, name: &str, system: &str, figures: &str, carrier: Option<Option<&str>>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        match carrier {
            Some(access) => {
                ui.label(RichText::new(format!("{} {name}", icon::ANCHOR)).color(pal::TEXT)).on_hover_text("Fleet carrier: its owner sets the prices and who may dock");
                crate::pages::market::access_badge(ui, access);
            }
            None => {
                ui.label(RichText::new(name).color(pal::TEXT));
            }
        }
        if ui.add(egui::Label::new(RichText::new(system).color(pal::ACCENT_SOFT)).sense(egui::Sense::click())).on_hover_text("Click to copy the system").clicked() {
            ui.ctx().copy_text(system.to_string());
        }
        ui.label(RichText::new(figures).size(size::FOOTNOTE).color(pal::DIM));
    });
}

fn searching(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(RichText::new("Looking…").color(pal::DIM));
    });
}

fn quiet(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(pal::DIM)).wrap());
}

fn group(n: u64) -> String {
    nexus_data::names::group_digits(i64::try_from(n).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row's symbol finds what it is: a material, a commodity, or something without a known source.
    #[test]
    fn what_is_wanted() {
        assert!(matches!(Wanted::of("polonium"), Wanted::Material(m) if m.kind == MaterialKind::Raw && m.tradeable));
        assert!(matches!(Wanted::of("Gold"), Wanted::Commodity(c) if c.symbol == "gold"));
        assert!(matches!(Wanted::of("$gold_name;"), Wanted::Commodity(_)));
        assert!(matches!(Wanted::of("opticallens"), Wanted::Other(_)), "an on-foot resource");
        assert!(matches!(Wanted::of("guardian_sentinel_wreckagecomponents"), Wanted::Material(m) if !m.tradeable));
    }

    /// The Spansh request for planets rich in a raw material: landable, with it, richest first, near you.
    #[test]
    fn rich_planets_request() {
        let j = rich_planets("Polonium", [1.0, 2.0, 3.0]).to_json();
        assert_eq!(j["filters"]["is_landable"], json!({"value": true}));
        assert_eq!(j["filters"]["materials"], json!([{"name": "Polonium", "value": [0, 100], "comparison": "<=>"}]));
        assert_eq!(j["sort"], json!([{"materials": [{"name": "Polonium", "direction": "desc"}]}]));
        assert_eq!(j["filters"]["distance"], json!({"min": "0", "max": "100"}));
        assert_eq!(j["size"], json!(3));
    }

    #[test]
    fn trades_and_sources_in_words() {
        let po = nexus_data::materials::material("polonium").expect("polonium");
        assert_eq!(trade_words(po), "There, any other raw material trades for it: from category 6 at the usual rate, from the other categories at six times the cost.");
        let m = nexus_data::materials::material("chemicalprocessors").expect("chemical processors");
        assert!(trade_words(m).contains("any other manufactured material") && trade_words(m).contains("from the Chemical family at the usual rate, from the other families"), "{}", trade_words(m));
        assert!(other_sources(m).starts_with("Manufactured"));
    }
}
