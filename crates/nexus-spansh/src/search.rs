//! Systems / bodies / stations search: `POST /api/{kind}/search` with a JSON body
//! `{"filters":{…},"sort":[…],"size":n,"page":p,"reference_system":"Sol"}`.
//!
//! [`SearchRequest`] builds the body; [`filter`] and [`sort`] build the per-field shapes (see
//! the field catalogue, [`Spansh::searchable_fields`]). [`MarketQuery`], [`OutfittingQuery`],
//! [`ServiceQuery`] and [`HotspotQuery`] cover the common questions ("who sells Tritium near X",
//! "who sells all of these modules", "nearest raw material trader", "Low Temperature Diamond
//! hotspots ×2 near X").

use std::fmt;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::de;
use crate::records::{BodyRecord, StationRecord, SystemPoint, SystemRecord};
use crate::util::{Coords, json_num, num};
use crate::{Error, Result, Spansh};

/// Spansh caps a page at 500 rows.
pub const MAX_PAGE_SIZE: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    Systems,
    Bodies,
    Stations,
}

impl SearchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchKind::Systems => "systems",
            SearchKind::Bodies => "bodies",
            SearchKind::Stations => "stations",
        }
    }
}

impl fmt::Display for SearchKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where distances are measured from (needed for the `distance` filter and sort).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reference {
    System(String),
    Coords(Coords),
    /// Distance to the straight line between two systems.
    Route {
        source: String,
        destination: String,
    },
}

impl From<&str> for Reference {
    fn from(system: &str) -> Self {
        Reference::System(system.to_owned())
    }
}

impl From<String> for Reference {
    fn from(system: String) -> Self {
        Reference::System(system)
    }
}

impl From<&String> for Reference {
    fn from(system: &String) -> Self {
        Reference::System(system.clone())
    }
}

impl From<Coords> for Reference {
    fn from(c: Coords) -> Self {
        Reference::Coords(c)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    pub fn as_str(self) -> &'static str {
        match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        }
    }
}

/// Field-filter JSON shapes (see `docs/research/spansh.md` §4.3).
pub mod filter {
    use super::*;

    /// `{"value": v}` — string fields (`*` wildcards allowed) and booleans.
    pub fn eq(value: impl Into<Value>) -> Value {
        json!({ "value": value.into() })
    }

    /// `{"value": [..]}` — group/enum fields, any of.
    pub fn any_of<I, S>(values: I) -> Value
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        json!({ "value": values.into_iter().map(Into::into).collect::<Vec<String>>() })
    }

    /// `{"value": [..], "logic": "AND"}` — must have all.
    pub fn all_of<I, S>(values: I) -> Value
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut v = any_of(values);
        v["logic"] = json!("AND");
        v
    }

    /// `{"value": [..], "logic": "NOT"}` — none of.
    pub fn none_of<I, S>(values: I) -> Value
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut v = any_of(values);
        v["logic"] = json!("NOT");
        v
    }

    /// `{"comparison": "<=>", "value": [min, max]}` — numeric range.
    pub fn between(min: f64, max: f64) -> Value {
        json!({ "comparison": "<=>", "value": [json_num(min), json_num(max)] })
    }

    /// `{"min": "0", "max": "100"}` — the `distance` filter (strings, as the website sends).
    pub fn distance(min: f64, max: f64) -> Value {
        json!({ "min": num(min), "max": num(max) })
    }

    /// `{"comparison": "<=>", "value": ["now-2d", "now"]}` — timestamp no older than `age`.
    pub fn updated_within(age: Duration) -> Value {
        json!({ "comparison": "<=>", "value": [format!("now-{}", date_math(age)), "now"] })
    }

    /// One `value_struct` item: `{"name": "Painite", "count": [2, 999], "comparison": "<=>"}`
    /// (for `signals` / `ring_signals`; use it inside an array).
    pub fn named_count(name: &str, min: u32, max: u32) -> Value {
        json!({ "comparison": "<=>", "count": [min, max], "name": name })
    }

    /// One `market` item for the stations search. Bounds are sent as strings, like the website.
    pub fn market(commodity: &str, bounds: &MarketBounds) -> Value {
        let mut item = Map::new();
        item.insert("name".into(), json!(commodity));
        let range = |(lo, hi): (u64, u64)| json!({ "value": [lo.to_string(), hi.to_string()], "comparison": "<=>" });
        for (key, b) in [
            ("supply", bounds.supply),
            ("demand", bounds.demand),
            ("buy_price", bounds.buy_price),
            ("sell_price", bounds.sell_price),
        ] {
            if let Some(b) = b {
                item.insert(key.into(), range(b));
            }
        }
        Value::Object(item)
    }

    /// A `combined` field item with one group subfield, e.g. services:
    /// `{"name": {"value": ["Interstellar Factors Contact"]}}`.
    pub fn combined_names<I, S>(values: I) -> Value
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        json!({ "name": any_of(values) })
    }

    /// Elasticsearch date-math amount: whole days/hours/minutes when exact, else seconds.
    pub(crate) fn date_math(age: Duration) -> String {
        let s = age.as_secs().max(1);
        if s.is_multiple_of(86_400) {
            format!("{}d", s / 86_400)
        } else if s.is_multiple_of(3_600) {
            format!("{}h", s / 3_600)
        } else if s.is_multiple_of(60) {
            format!("{}m", s / 60)
        } else {
            format!("{s}s")
        }
    }
}

/// Bounds for a `market` filter item (inclusive).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketBounds {
    pub supply: Option<(u64, u64)>,
    pub demand: Option<(u64, u64)>,
    pub buy_price: Option<(u64, u64)>,
    pub sell_price: Option<(u64, u64)>,
}

/// Upper bound used for "at least" filters (as the website does).
pub const UNBOUNDED: u64 = 999_999_999;

/// Sort-entry JSON shapes.
pub mod sort {
    use super::*;

    /// `{"distance": {"direction": "asc"}}`.
    pub fn field(name: &str, dir: SortDir) -> Value {
        json!({ name: { "direction": dir.as_str() } })
    }

    /// `value_struct` / `combined` sort: `{"ring_signals": [{"name": "Painite", "direction": "desc"}]}`.
    pub fn named(field: &str, name: &str, dir: SortDir) -> Value {
        json!({ field: [{ "name": name, "direction": dir.as_str() }] })
    }

    /// Station market column: `{"market_buy_price": [{"name": "Tritium", "direction": "asc"}]}`;
    /// `column` is `buy_price`, `sell_price`, `supply` or `demand`.
    pub fn market(column: &str, commodity: &str, dir: SortDir) -> Value {
        named(&format!("market_{column}"), commodity, dir)
    }
}

/// Body of a search request.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRequest {
    pub filters: Map<String, Value>,
    pub sort: Vec<Value>,
    /// Rows per page, 1–500.
    pub size: u32,
    /// 0-based.
    pub page: u32,
    pub reference: Option<Reference>,
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            filters: Map::new(),
            sort: Vec::new(),
            size: 10,
            page: 0,
            reference: None,
        }
    }
}

impl SearchRequest {
    pub fn new() -> Self {
        Self::default()
    }

    /// Measure distances from a system, coordinates or a route.
    pub fn near(mut self, reference: impl Into<Reference>) -> Self {
        self.reference = Some(reference.into());
        self
    }

    /// Add (or replace) a field filter.
    pub fn filter(mut self, field: &str, value: Value) -> Self {
        self.filters.insert(field.to_owned(), value);
        self
    }

    /// Only results within `max_ly` of the reference.
    pub fn within(self, max_ly: f64) -> Self {
        self.filter("distance", filter::distance(0.0, max_ly))
    }

    /// Append a sort key (`sort::field("distance", SortDir::Asc)` is the usual first one).
    pub fn sort(mut self, entry: Value) -> Self {
        self.sort.push(entry);
        self
    }

    /// Append `{"<field>": {"direction": …}}`.
    pub fn sort_by(self, field: &str, dir: SortDir) -> Self {
        self.sort(sort::field(field, dir))
    }

    pub fn size(mut self, rows: u32) -> Self {
        self.size = rows.clamp(1, MAX_PAGE_SIZE);
        self
    }

    pub fn page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    /// The JSON body Spansh expects.
    pub fn to_json(&self) -> Value {
        let mut body = Map::new();
        body.insert("filters".into(), Value::Object(self.filters.clone()));
        body.insert("sort".into(), Value::Array(self.sort.clone()));
        body.insert("size".into(), json!(self.size.clamp(1, MAX_PAGE_SIZE)));
        body.insert("page".into(), json!(self.page));
        match &self.reference {
            Some(Reference::System(name)) => {
                body.insert("reference_system".into(), json!(name.trim()));
            }
            Some(Reference::Coords(c)) => {
                body.insert(
                    "reference_coords".into(),
                    json!({ "x": c.x, "y": c.y, "z": c.z }),
                );
            }
            Some(Reference::Route {
                source,
                destination,
            }) => {
                body.insert(
                    "reference_route".into(),
                    json!({ "source": source, "destination": destination }),
                );
            }
            None => {}
        }
        Value::Object(body)
    }
}

/// A page of search results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    default,
    bound(deserialize = "T: DeserializeOwned", serialize = "T: Serialize")
)]
pub struct SearchResponse<T> {
    /// Total hits (Spansh stops counting around 10 000).
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub count: u64,
    /// Offset of the first row.
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub from: u64,
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub size: u64,
    /// The resolved reference system.
    pub reference: Option<SystemPoint>,
    /// GUID for sharing: `https://spansh.co.uk/{kind}/search/{guid}/1`.
    #[serde(deserialize_with = "de::opt_string")]
    pub search_reference: Option<String>,
    #[serde(deserialize_with = "de::lenient_vec")]
    pub results: Vec<T>,
}

impl<T> Default for SearchResponse<T> {
    fn default() -> Self {
        Self {
            count: 0,
            from: 0,
            size: 0,
            reference: None,
            search_reference: None,
            results: Vec::new(),
        }
    }
}

impl<T> SearchResponse<T> {
    /// More hits beyond this page.
    pub fn has_more(&self) -> bool {
        self.from + (self.results.len() as u64) < self.count
    }
}

/// Website link to a saved search (`search_reference` of a [`SearchResponse`]).
pub fn search_url(kind: SearchKind, search_reference: &str) -> String {
    format!(
        "{}/{kind}/search/{search_reference}/1",
        crate::util::WEBSITE
    )
}

// ---------------------------------------------------------------------------------------------
// typed queries

/// Which side of a market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MarketSide {
    /// Stations selling the commodity (we buy; filters on supply).
    #[default]
    Buy,
    /// Stations buying the commodity (we sell; filters on demand).
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MarketSort {
    #[default]
    Distance,
    /// Cheapest buy price (side `Buy`) or best sell price (side `Sell`) first.
    Price,
}

/// "Who sells/buys X near Y": stations search on one commodity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketQuery {
    /// Commodity display name (`Tritium`, `Low Temperature Diamonds`).
    pub commodity: String,
    pub near: Reference,
    pub side: MarketSide,
    /// Minimum supply (`Buy`) or demand (`Sell`), t.
    pub min_quantity: u64,
    pub max_distance: Option<f64>,
    /// Ignore markets not updated within this window.
    pub max_age: Option<Duration>,
    pub large_pad: bool,
    pub include_carriers: bool,
    pub sort: MarketSort,
    pub size: u32,
}

impl MarketQuery {
    /// Stations selling `commodity` (supply ≥ 1), nearest first.
    pub fn buy(commodity: impl Into<String>, near: impl Into<Reference>) -> Self {
        Self {
            commodity: commodity.into(),
            near: near.into(),
            side: MarketSide::Buy,
            min_quantity: 1,
            max_distance: None,
            max_age: None,
            large_pad: false,
            include_carriers: true,
            sort: MarketSort::Distance,
            size: 10,
        }
    }

    /// Stations buying `commodity` (demand ≥ 1), nearest first.
    pub fn sell(commodity: impl Into<String>, near: impl Into<Reference>) -> Self {
        Self {
            side: MarketSide::Sell,
            ..Self::buy(commodity, near)
        }
    }

    pub fn min_quantity(mut self, tonnes: u64) -> Self {
        self.min_quantity = tonnes;
        self
    }

    pub fn within(mut self, ly: f64) -> Self {
        self.max_distance = Some(ly);
        self
    }

    pub fn max_age(mut self, age: Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    pub fn large_pad(mut self, required: bool) -> Self {
        self.large_pad = required;
        self
    }

    pub fn include_carriers(mut self, on: bool) -> Self {
        self.include_carriers = on;
        self
    }

    pub fn sort_by(mut self, sort: MarketSort) -> Self {
        self.sort = sort;
        self
    }

    pub fn size(mut self, rows: u32) -> Self {
        self.size = rows;
        self
    }

    pub fn to_request(&self) -> SearchRequest {
        let quantity = Some((self.min_quantity, UNBOUNDED.max(self.min_quantity)));
        let bounds = match self.side {
            MarketSide::Buy => MarketBounds {
                supply: quantity,
                ..MarketBounds::default()
            },
            MarketSide::Sell => MarketBounds {
                demand: quantity,
                ..MarketBounds::default()
            },
        };
        let mut req = SearchRequest::new()
            .near(self.near.clone())
            .filter("market", json!([filter::market(&self.commodity, &bounds)]))
            .size(self.size);
        if self.large_pad {
            req = req.filter("has_large_pad", filter::eq(true));
        }
        if !self.include_carriers {
            req = req.filter("type", filter::none_of(CARRIER_TYPES));
        }
        if let Some(age) = self.max_age {
            req = req.filter("market_updated_at", filter::updated_within(age));
        }
        if let Some(ly) = self.max_distance {
            req = req.within(ly);
        }
        match (self.sort, self.side) {
            (MarketSort::Distance, _) => req.sort_by("distance", SortDir::Asc),
            (MarketSort::Price, MarketSide::Buy) => req
                .sort(sort::market("buy_price", &self.commodity, SortDir::Asc))
                .sort_by("distance", SortDir::Asc),
            (MarketSort::Price, MarketSide::Sell) => req
                .sort(sort::market("sell_price", &self.commodity, SortDir::Desc))
                .sort_by("distance", SortDir::Asc),
        }
    }
}

/// Station `type` values of fleet carriers.
pub const CARRIER_TYPES: [&str; 1] = ["Drake-Class Carrier"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MaterialTrader {
    Raw,
    Manufactured,
    Encoded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TechnologyBroker {
    Guardian,
    Human,
}

/// A station facility to look for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StationService {
    MaterialTrader(MaterialTrader),
    TechnologyBroker(TechnologyBroker),
    /// Any `services` entry, e.g. `Interstellar Factors Contact`, `Universal Cartographics`,
    /// `Vista Genomics`, `Shipyard`.
    Named(String),
}

/// "Nearest X": stations search on a facility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceQuery {
    pub service: StationService,
    pub near: Reference,
    pub max_distance: Option<f64>,
    pub large_pad: bool,
    pub include_carriers: bool,
    pub size: u32,
}

impl ServiceQuery {
    pub fn new(service: StationService, near: impl Into<Reference>) -> Self {
        Self {
            service,
            near: near.into(),
            max_distance: None,
            large_pad: false,
            include_carriers: true,
            size: 10,
        }
    }

    pub fn material_trader(kind: MaterialTrader, near: impl Into<Reference>) -> Self {
        Self::new(StationService::MaterialTrader(kind), near)
    }

    pub fn technology_broker(kind: TechnologyBroker, near: impl Into<Reference>) -> Self {
        Self::new(StationService::TechnologyBroker(kind), near)
    }

    pub fn named(service: impl Into<String>, near: impl Into<Reference>) -> Self {
        Self::new(StationService::Named(service.into()), near)
    }

    pub fn within(mut self, ly: f64) -> Self {
        self.max_distance = Some(ly);
        self
    }

    pub fn large_pad(mut self, required: bool) -> Self {
        self.large_pad = required;
        self
    }

    pub fn include_carriers(mut self, on: bool) -> Self {
        self.include_carriers = on;
        self
    }

    pub fn size(mut self, rows: u32) -> Self {
        self.size = rows;
        self
    }

    pub fn to_request(&self) -> SearchRequest {
        let (field, value) = match &self.service {
            StationService::MaterialTrader(kind) => {
                ("material_trader", filter::any_of([format!("{kind:?}")]))
            }
            StationService::TechnologyBroker(kind) => {
                ("technology_broker", filter::any_of([format!("{kind:?}")]))
            }
            StationService::Named(name) => {
                ("services", json!([filter::combined_names([name.as_str()])]))
            }
        };
        let mut req = SearchRequest::new()
            .near(self.near.clone())
            .filter(field, value)
            .size(self.size);
        if self.large_pad {
            req = req.filter("has_large_pad", filter::eq(true));
        }
        if !self.include_carriers {
            req = req.filter("type", filter::none_of(CARRIER_TYPES));
        }
        if let Some(ly) = self.max_distance {
            req = req.within(ly);
        }
        req.sort_by("distance", SortDir::Asc)
    }
}

/// "Ring hotspots of X (×N or more) near Y": bodies search on `ring_signals`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotspotQuery {
    /// Commodity display name (`Low Temperature Diamonds`, `Painite`, `Platinum`, …).
    pub commodity: String,
    /// Minimum hotspots in one ring.
    pub min_count: u32,
    pub near: Reference,
    pub max_distance: Option<f64>,
    pub size: u32,
}

impl HotspotQuery {
    pub fn new(commodity: impl Into<String>, min_count: u32, near: impl Into<Reference>) -> Self {
        Self {
            commodity: commodity.into(),
            min_count: min_count.max(1),
            near: near.into(),
            max_distance: None,
            size: 10,
        }
    }

    pub fn within(mut self, ly: f64) -> Self {
        self.max_distance = Some(ly);
        self
    }

    pub fn size(mut self, rows: u32) -> Self {
        self.size = rows;
        self
    }

    pub fn to_request(&self) -> SearchRequest {
        let mut req = SearchRequest::new()
            .near(self.near.clone())
            .filter(
                "ring_signals",
                json!([filter::named_count(&self.commodity, self.min_count, 999)]),
            )
            .size(self.size);
        if let Some(ly) = self.max_distance {
            req = req.within(ly);
        }
        req.sort_by("distance", SortDir::Asc)
    }
}

/// One module to look for in station outfitting, as Spansh spells it (the FDevIDs `name`:
/// `Multi-Cannon`, `Frame Shift Drive`, `Military Grade Composite`), with its class and rating.
///
/// A station's module goes by one name, and MercGear rows share their base module's symbol:
/// Spansh lists most 5A power distributors as `Balanced Power Distributor`, so give every name
/// the symbol has (any of them matches). Spansh can't filter on the mount or on the ship a set
/// of bulkheads is for either: `Multi-Cannon` 4A matches fixed and gimballed huge multi-cannons
/// alike. Check each result with [`StationRecord::module`] for the exact symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleFilter {
    /// Any of these names.
    pub names: Vec<String>,
    /// 0 for utilities, 1–4 for hardpoints, 1–8 for internals.
    pub class: Option<u8>,
    /// `A`…`I`.
    pub rating: Option<char>,
}

impl ModuleFilter {
    pub fn new(names: Vec<String>, class: Option<u8>, rating: Option<char>) -> Self {
        Self {
            names,
            class,
            rating,
        }
    }

    /// One `modules` item: `{"name": {"value": [..]}, "class": {"value": ["5"]}, "rating": {"value": ["A"]}}`.
    pub fn to_json(&self) -> Value {
        let mut item = Map::new();
        item.insert("name".into(), filter::any_of(self.names.iter().map(String::as_str)));
        if let Some(c) = self.class {
            item.insert("class".into(), filter::any_of([c.to_string()]));
        }
        if let Some(r) = self.rating {
            item.insert("rating".into(), filter::any_of([r.to_string()]));
        }
        Value::Object(item)
    }
}

/// "Stations that sell all of these modules (and these ships) near X": a stations search on
/// `modules` and `ships`. Several items must all match (tested live 2026-11-02: within 25 ly of
/// Sol, 130 stations sold a 6A FSD, 108 a 7A shield generator, 56 both). With no modules, any
/// station with outfitting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutfittingQuery {
    pub modules: Vec<ModuleFilter>,
    /// Ship names as Spansh spells them (the FDevIDs `name`: `Anaconda`, `Krait MkII`).
    pub ships: Vec<String>,
    pub near: Reference,
    pub max_distance: Option<f64>,
    /// Ignore stations whose outfitting wasn't updated within this window.
    pub max_age: Option<Duration>,
    pub large_pad: bool,
    pub include_carriers: bool,
    pub size: u32,
    pub page: u32,
}

impl OutfittingQuery {
    /// Stations that sell every one of `modules`, nearest first.
    pub fn new(modules: Vec<ModuleFilter>, near: impl Into<Reference>) -> Self {
        Self {
            modules,
            ships: Vec::new(),
            near: near.into(),
            max_distance: None,
            max_age: None,
            large_pad: false,
            include_carriers: true,
            size: 10,
            page: 0,
        }
    }

    /// The shipyard must sell these ships too.
    pub fn ships(mut self, names: Vec<String>) -> Self {
        self.ships = names;
        self
    }

    pub fn within(mut self, ly: f64) -> Self {
        self.max_distance = Some(ly);
        self
    }

    pub fn max_age(mut self, age: Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    pub fn large_pad(mut self, required: bool) -> Self {
        self.large_pad = required;
        self
    }

    pub fn include_carriers(mut self, on: bool) -> Self {
        self.include_carriers = on;
        self
    }

    pub fn size(mut self, rows: u32) -> Self {
        self.size = rows;
        self
    }

    pub fn page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    pub fn to_request(&self) -> SearchRequest {
        let mut req = SearchRequest::new()
            .near(self.near.clone())
            .size(self.size)
            .page(self.page);
        if self.modules.is_empty() {
            req = req.filter("has_outfitting", filter::eq(true));
        } else {
            req = req.filter(
                "modules",
                Value::Array(self.modules.iter().map(ModuleFilter::to_json).collect()),
            );
        }
        if !self.ships.is_empty() {
            req = req.filter(
                "ships",
                Value::Array(
                    self.ships
                        .iter()
                        .map(|s| filter::combined_names([s.as_str()]))
                        .collect(),
                ),
            );
        }
        if self.large_pad {
            req = req.filter("has_large_pad", filter::eq(true));
        }
        if !self.include_carriers {
            req = req.filter("type", filter::none_of(CARRIER_TYPES));
        }
        if let Some(age) = self.max_age {
            req = req.filter("outfitting_updated_at", filter::updated_within(age));
        }
        if let Some(ly) = self.max_distance {
            req = req.within(ly);
        }
        req.sort_by("distance", SortDir::Asc)
    }
}

// ---------------------------------------------------------------------------------------------
// client methods

impl Spansh {
    /// Raw search: `POST /api/{kind}/search` with any JSON body; returns the raw answer.
    pub async fn search_raw(&self, kind: SearchKind, body: Value) -> Result<Value> {
        self.post_json(&format!("{kind}/search"), &body, &format!("{kind} search"))
            .await
    }

    /// Typed search; `T` is [`SystemRecord`], [`BodyRecord`] or [`StationRecord`] for the
    /// matching `kind` (or your own type).
    pub async fn search<T: DeserializeOwned>(
        &self,
        kind: SearchKind,
        req: &SearchRequest,
    ) -> Result<SearchResponse<T>> {
        self.post_json(
            &format!("{kind}/search"),
            &req.to_json(),
            &format!("{kind} search"),
        )
        .await
    }

    pub async fn search_systems(
        &self,
        req: &SearchRequest,
    ) -> Result<SearchResponse<SystemRecord>> {
        self.search(SearchKind::Systems, req).await
    }

    pub async fn search_bodies(&self, req: &SearchRequest) -> Result<SearchResponse<BodyRecord>> {
        self.search(SearchKind::Bodies, req).await
    }

    pub async fn search_stations(
        &self,
        req: &SearchRequest,
    ) -> Result<SearchResponse<StationRecord>> {
        self.search(SearchKind::Stations, req).await
    }

    /// Stations selling/buying a commodity (e.g. Tritium sellers near a carrier).
    pub async fn market_search(
        &self,
        query: &MarketQuery,
    ) -> Result<SearchResponse<StationRecord>> {
        if query.commodity.trim().is_empty() {
            return Err(Error::InvalidRequest("commodity must not be empty".into()));
        }
        self.search_stations(&query.to_request()).await
    }

    /// Nearest material traders, technology brokers or other station services.
    pub async fn service_search(
        &self,
        query: &ServiceQuery,
    ) -> Result<SearchResponse<StationRecord>> {
        self.search_stations(&query.to_request()).await
    }

    /// Stations that sell all of a list of modules (and ships), nearest first.
    pub async fn outfitting_search(
        &self,
        query: &OutfittingQuery,
    ) -> Result<SearchResponse<StationRecord>> {
        if query
            .modules
            .iter()
            .any(|m| m.names.is_empty() || m.names.iter().any(|n| n.trim().is_empty()))
        {
            return Err(Error::InvalidRequest("module names must not be empty".into()));
        }
        self.search_stations(&query.to_request()).await
    }

    /// Bodies with ring hotspots of a commodity, nearest first.
    pub async fn hotspot_search(&self, query: &HotspotQuery) -> Result<SearchResponse<BodyRecord>> {
        if query.commodity.trim().is_empty() {
            return Err(Error::InvalidRequest("commodity must not be empty".into()));
        }
        self.search_bodies(&query.to_request()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_math_units() {
        assert_eq!(filter::date_math(Duration::from_secs(2 * 86_400)), "2d");
        assert_eq!(filter::date_math(Duration::from_secs(3 * 3_600)), "3h");
        assert_eq!(filter::date_math(Duration::from_secs(90 * 60)), "90m");
        assert_eq!(filter::date_math(Duration::from_secs(61)), "61s");
        assert_eq!(filter::date_math(Duration::ZERO), "1s");
    }

    #[test]
    fn reference_forms() {
        let body = SearchRequest::new()
            .near(Coords::new(1.0, 2.5, -3.0))
            .to_json();
        assert_eq!(
            body["reference_coords"],
            json!({"x": 1.0, "y": 2.5, "z": -3.0})
        );
        let body = SearchRequest::new()
            .near(Reference::Route {
                source: "Sol".into(),
                destination: "Colonia".into(),
            })
            .to_json();
        assert_eq!(
            body["reference_route"],
            json!({"source": "Sol", "destination": "Colonia"})
        );
        assert!(
            SearchRequest::new()
                .to_json()
                .get("reference_system")
                .is_none()
        );
    }

    #[test]
    fn service_queries() {
        let raw = ServiceQuery::material_trader(MaterialTrader::Raw, "Sol")
            .within(200.0)
            .size(5)
            .to_request()
            .to_json();
        assert_eq!(raw["filters"]["material_trader"], json!({"value": ["Raw"]}));
        assert_eq!(
            raw["filters"]["distance"],
            json!({"min": "0", "max": "200"})
        );
        let factors = ServiceQuery::named("Interstellar Factors Contact", "Sol")
            .include_carriers(false)
            .to_request()
            .to_json();
        assert_eq!(
            factors["filters"]["services"],
            json!([{"name": {"value": ["Interstellar Factors Contact"]}}])
        );
        assert_eq!(
            factors["filters"]["type"],
            json!({"value": ["Drake-Class Carrier"], "logic": "NOT"})
        );
        let broker = ServiceQuery::technology_broker(TechnologyBroker::Guardian, "Sol")
            .to_request()
            .to_json();
        assert_eq!(
            broker["filters"]["technology_broker"],
            json!({"value": ["Guardian"]})
        );
    }

    #[test]
    fn outfitting_queries() {
        let body = OutfittingQuery::new(
            vec![
                ModuleFilter::new(vec!["Frame Shift Drive".into()], Some(6), Some('A')),
                ModuleFilter::new(vec!["Shield Booster".into()], Some(0), Some('A')),
                ModuleFilter::new(
                    vec!["Power Distributor".into(), "Balanced Power Distributor".into()],
                    None,
                    None,
                ),
            ],
            "Sol",
        )
        .ships(vec!["Anaconda".into()])
        .large_pad(true)
        .include_carriers(false)
        .max_age(Duration::from_secs(30 * 86_400))
        .size(20)
        .page(1)
        .to_request()
        .to_json();
        let f = &body["filters"];
        assert_eq!(
            f["modules"],
            json!([
                {"name": {"value": ["Frame Shift Drive"]}, "class": {"value": ["6"]}, "rating": {"value": ["A"]}},
                {"name": {"value": ["Shield Booster"]}, "class": {"value": ["0"]}, "rating": {"value": ["A"]}},
                {"name": {"value": ["Power Distributor", "Balanced Power Distributor"]}}
            ])
        );
        assert_eq!(f["ships"], json!([{"name": {"value": ["Anaconda"]}}]));
        assert_eq!(f["has_large_pad"], json!({"value": true}));
        assert_eq!(
            f["type"],
            json!({"value": ["Drake-Class Carrier"], "logic": "NOT"})
        );
        assert_eq!(
            f["outfitting_updated_at"],
            json!({"comparison": "<=>", "value": ["now-30d", "now"]})
        );
        assert_eq!(body["sort"][0], json!({"distance": {"direction": "asc"}}));
        assert_eq!((body["size"].clone(), body["page"].clone()), (json!(20), json!(1)));
        // No modules: any station with outfitting.
        let any = OutfittingQuery::new(Vec::new(), "Sol").to_request().to_json();
        assert_eq!(any["filters"]["has_outfitting"], json!({"value": true}));
        assert!(any["filters"].get("modules").is_none());
    }

    #[test]
    fn market_price_sort() {
        let body = MarketQuery::sell("Gold", "Sol")
            .sort_by(MarketSort::Price)
            .max_age(Duration::from_secs(86_400))
            .to_request()
            .to_json();
        assert_eq!(
            body["filters"]["market"][0]["demand"],
            json!({"value": ["1", "999999999"], "comparison": "<=>"})
        );
        assert_eq!(
            body["filters"]["market_updated_at"],
            json!({"comparison": "<=>", "value": ["now-1d", "now"]})
        );
        assert_eq!(
            body["sort"][0],
            json!({"market_sell_price": [{"name": "Gold", "direction": "desc"}]})
        );
    }
}
