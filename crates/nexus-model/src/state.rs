//! `GameState` (one commander) and `Model` (all commanders found in the journals).

use crate::alerts::{Alert, AlertKind, Alerts, Priority};
use crate::bgs::{Bgs, BgsKind, Here};
use crate::carrier::{Carrier, CarrierKind};
use crate::colonisation::Colonisation;
use crate::commander::{Commander, PowerplayStatus, Squadron};
use crate::exobio::{ExoBio, ScanOutcome};
use crate::exploration::{interesting, Exploration, Interest};
use crate::fleet::Fleet;
use crate::history::{DeathRecord, DockRecord, History, JumpRecord, LedgerCategory};
use crate::inventory::{Inventory, MaterialCategory};
use crate::location::{ArrivalKind, Location};
use crate::mining::Mining;
use crate::missions::{MissionOutcome, Missions};
use crate::session::Session;
use nexus_data::names::{clean_station_name, compact_credits, group_digits};
use nexus_journal::companion::{
    flags, Companion, FCMaterialsFile, MarketFile, ModulesInfoFile, OutfittingFile, RouteHop, ShipyardFile, Status,
};
use nexus_journal::events::{self as ev, Event};
use nexus_journal::{EventCtx, Timestamp};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

/// One line in the activity feed.
#[derive(Debug, Clone)]
pub struct FeedItem {
    pub ts: Timestamp,
    pub event: String,
    pub text: String,
}

const FEED_LEN: usize = 300;

/// Everything Elite Nexus knows about one commander.
#[derive(Debug, Clone, Default)]
pub struct GameState {
    pub cmdr: Commander,
    pub location: Location,
    pub status: Option<Arc<Status>>,
    /// Status.json updates applied so far: a new value means a new Status.json.
    pub status_writes: u64,
    pub fleet: Fleet,
    pub inventory: Inventory,
    pub missions: Missions,
    pub exploration: Exploration,
    pub exobio: ExoBio,
    pub mining: Mining,
    /// The commander's own fleet carrier.
    pub carrier: Carrier,
    /// Their squadron's carrier, tracked apart from their own.
    pub squadron_carrier: Carrier,
    pub colonisation: Colonisation,
    pub history: History,
    pub bgs: Bgs,
    pub session: Session,
    pub nav_route: Vec<RouteHop>,
    pub market: Option<Arc<MarketFile>>,
    pub outfitting: Option<Arc<OutfittingFile>>,
    pub shipyard: Option<Arc<ShipyardFile>>,
    pub modules_info: Option<Arc<ModulesInfoFile>>,
    pub fc_materials: Option<Arc<FCMaterialsFile>>,
    pub feed: VecDeque<FeedItem>,
    pub last_event: Timestamp,
    pub events_applied: u64,
    /// The game wrote `Shutdown` after the last session start.
    pub game_closed: bool,
    /// The main menu's music is playing (the `Music` event), until the next `LoadGame`.
    pub at_main_menu: bool,
    pub legal_state: Option<String>,
    prev_flags: u32,
    pending_star_class: Option<(String, String)>,
}

impl GameState {
    pub fn apply_header(&mut self, h: &ev::Fileheader) {
        if !h.gameversion.is_empty() {
            self.cmdr.game_version = h.gameversion.clone();
        }
        if !h.build.is_empty() {
            self.cmdr.game_build = h.build.clone();
        }
        if let Some(o) = h.odyssey {
            self.cmdr.odyssey = o;
            self.exploration.set_odyssey(o);
        }
    }

    fn here_station(&self) -> (String, Option<String>, u64) {
        (
            self.location.system.clone(),
            self.location.station.as_ref().map(|s| s.name.clone()),
            self.location.station.as_ref().map(|s| s.market_id).unwrap_or_default(),
        )
    }

    fn money(&mut self, ts: Timestamp, cat: LedgerCategory, amount: i64, note: impl Into<String>) {
        if amount == 0 {
            return;
        }
        self.history.ledger(ts, cat, amount, note);
        self.cmdr.credits += amount;
        if cat != LedgerCategory::Transfer {
            self.session.record_money(amount);
        }
    }

    pub fn carrier_of(&self, kind: CarrierKind) -> &Carrier {
        match kind {
            CarrierKind::Personal => &self.carrier,
            CarrierKind::Squadron => &self.squadron_carrier,
        }
    }

    /// The plotted route still ahead. The system it was plotted from is dropped when the route arrives, and arrivals
    /// take the systems up to the one reached off it; read again mid-route (after a restart), the route still holds
    /// the systems behind, so everything after the current one is ahead.
    pub fn route_remaining(&self) -> &[RouteHop] {
        let start = self.nav_route.iter().position(|h| h.system_address == self.location.system_address).map_or(0, |i| i + 1);
        &self.nav_route[start..]
    }

    /// In hyperspace: the system being jumped to and its star class (`StartJump`).
    pub fn jumping_to(&self) -> Option<(&str, &str)> {
        self.pending_star_class.as_ref().filter(|_| self.location.hyperspace).map(|(s, c)| (s.as_str(), c.as_str()))
    }

    /// The carriers the commander has at `now`, their own first: known and not decommissioned.
    pub fn carriers(&self, now: Timestamp) -> Vec<(CarrierKind, &Carrier)> {
        [CarrierKind::Personal, CarrierKind::Squadron].into_iter().map(|k| (k, self.carrier_of(k))).filter(|(_, c)| c.active(now)).collect()
    }

    /// The carrier a journal event is about (see [`CarrierKind::of`]).
    fn carrier_for(&mut self, carrier_type: Option<&str>, id: u64) -> Option<&mut Carrier> {
        match CarrierKind::of(carrier_type, id, self.squadron_carrier.id)? {
            CarrierKind::Personal => Some(&mut self.carrier),
            CarrierKind::Squadron => Some(&mut self.squadron_carrier),
        }
    }

    fn feed(&mut self, ts: Timestamp, event: &str, text: String) {
        if self.feed.len() >= FEED_LEN {
            self.feed.pop_front();
        }
        self.feed.push_back(FeedItem { ts, event: event.to_string(), text });
    }

    /// Applies one journal event.
    pub fn apply(&mut self, ctx: &EventCtx<'_>, event: &Event, alerts: &mut Alerts) {
        let ts = ctx.ts;
        self.last_event = ts;
        self.events_applied += 1;
        match event {
            Event::Fileheader(h) => self.apply_header(h),
            Event::Commander(c) | Event::NewCommander(c) => {
                self.cmdr.name = c.name.clone();
                if !c.fid.is_empty() {
                    self.cmdr.fid = c.fid.clone();
                }
            }
            Event::LoadGame(l) => {
                self.cmdr.on_load_game(l, ts);
                self.exploration.set_odyssey(l.odyssey.unwrap_or(false));
                self.fleet.on_load_game(l);
                self.session = Session::start(ts, l.credits);
                self.history.credit_sample(ts, l.credits);
                self.game_closed = false;
                self.at_main_menu = false;
                let mode = l.game_mode.as_deref().map(|m| format!(" ({m})")).unwrap_or_default();
                self.feed(ts, "LoadGame", format!("Logged in{mode}, {} Cr", group_digits(l.credits)));
            }
            Event::Shutdown(_) => {
                self.session.end = Some(ts);
                self.game_closed = true;
                self.feed(ts, "Shutdown", "Game closed".into());
            }
            Event::Music(m) => self.at_main_menu = m.music_track == "MainMenu",
            Event::Rank(r) => self.cmdr.ranks.apply(r),
            Event::Progress(p) => self.cmdr.progress.apply(p),
            Event::Promotion(p) => {
                self.cmdr.ranks.apply(p);
                for k in nexus_data::ranks::RankKind::ALL {
                    let v = match k {
                        nexus_data::ranks::RankKind::Combat => p.combat,
                        nexus_data::ranks::RankKind::Trade => p.trade,
                        nexus_data::ranks::RankKind::Explore => p.explore,
                        nexus_data::ranks::RankKind::Soldier => p.soldier,
                        nexus_data::ranks::RankKind::Exobiologist => p.exobiologist,
                        nexus_data::ranks::RankKind::Empire => p.empire,
                        nexus_data::ranks::RankKind::Federation => p.federation,
                        nexus_data::ranks::RankKind::Cqc => p.cqc,
                    };
                    if let Some(v) = v {
                        let text = format!("{} rank: {}", k.label(), k.name_of(v));
                        self.feed(ts, "Promotion", text.clone());
                        alerts.push_with(|| {
                            Alert::new(ts, AlertKind::Commander, Priority::Normal, "Promotion!")
                                .detail(text.clone())
                                .speak(format!("Congratulations commander. {text}"))
                        });
                    }
                }
            }
            Event::Reputation(r) => self.cmdr.on_reputation(r),
            Event::EngineerProgress(e) => self.cmdr.on_engineer_progress(e),
            Event::Powerplay(p) => {
                let session = self.cmdr.powerplay.as_ref().map(|x| x.session_merits).unwrap_or(0);
                self.cmdr.powerplay = Some(PowerplayStatus {
                    power: p.power.clone(),
                    rank: p.rank,
                    merits: p.merits,
                    time_pledged_secs: p.time_pledged,
                    session_merits: session,
                });
            }
            Event::PowerplayJoin(p) => {
                self.cmdr.powerplay =
                    Some(PowerplayStatus { power: p.power.clone().unwrap_or_default(), ..Default::default() });
            }
            Event::PowerplayLeave(_) => self.cmdr.powerplay = None,
            Event::PowerplayDefect(p) => {
                self.cmdr.powerplay =
                    Some(PowerplayStatus { power: p.to_power.clone().unwrap_or_default(), ..Default::default() });
            }
            Event::PowerplayRank(p) => {
                if let (Some(pp), Some(r)) = (self.cmdr.powerplay.as_mut(), p.rank) {
                    pp.rank = r;
                }
            }
            Event::PowerplayMerits(m) => {
                self.bgs.on_merits(m, ts);
                let pp = self.cmdr.powerplay.get_or_insert_with(Default::default);
                pp.power = m.power.clone();
                pp.merits = m.total_merits;
                pp.session_merits += m.merits_gained;
            }
            Event::SquadronStartup(s) => {
                self.cmdr.squadron = Some(Squadron {
                    name: s.squadron_name.clone(),
                    id: s.squadron_id,
                    rank: Some(s.current_rank),
                    rank_name: s.current_rank_name.clone(),
                });
            }
            Event::JoinedSquadron(s) | Event::SquadronCreated(s) => {
                self.cmdr.squadron = Some(Squadron { name: s.squadron_name.clone(), id: s.squadron_id, ..Default::default() });
                self.cmdr.squadron_applications.clear();
            }
            Event::LeftSquadron(_) | Event::KickedFromSquadron(_) | Event::DisbandedSquadron(_) => {
                self.cmdr.squadron = None;
                // Its carrier isn't the commander's any more.
                self.squadron_carrier = Carrier::default();
            }
            Event::AppliedToSquadron(s) => {
                if !self.cmdr.squadron_applications.contains(&s.squadron_name) {
                    self.cmdr.squadron_applications.push(s.squadron_name.clone());
                }
            }
            Event::SquadronPromotion(s) | Event::SquadronDemotion(s) => {
                if let Some(sq) = self.cmdr.squadron.as_mut() {
                    sq.rank = s.new_rank;
                }
            }

            // ---------------------------------------------------------------- travel
            Event::Location(a) => {
                self.location.on_arrival(a, ts, ArrivalKind::Location);
                self.history.visit(a.system_address, &a.star_system, a.star_pos, ts);
                self.exobio.on_system(a.system_address);
                self.mining.at_body(a.system_address, a.body_id);
                self.exploration.enter_system(a.system_address, &a.star_system, a.star_pos, ts);
                self.feed(ts, "Location", format!("In {} — {}", a.star_system, self.location.situation()));
            }
            Event::FSDJump(a) => {
                self.location.on_arrival(a, ts, ArrivalKind::Jump);
                let dist = a.jump_dist.unwrap_or(0.0);
                self.history.jump(JumpRecord {
                    ts,
                    system: a.star_system.clone(),
                    address: a.system_address,
                    pos: a.star_pos,
                    dist,
                    fuel_used: a.fuel_used.unwrap_or(0.0),
                    carrier: false,
                });
                self.session.jumps += 1;
                self.session.distance_ly += dist;
                self.session.systems_visited += 1;
                if let Some(f) = a.fuel_level {
                    self.fleet.fuel_main = f;
                }
                self.exobio.on_system(a.system_address);
                self.exploration.enter_system(a.system_address, &a.star_system, a.star_pos, ts);
                self.mining.end_session();
                self.mining.leave_body();
                // Arriving on the route leaves everything up to here behind, the system it was plotted from too.
                if let Some(i) = self.nav_route.iter().position(|h| h.system_address == a.system_address) {
                    self.nav_route.drain(..=i);
                }
                let pop = if a.population > 0 { format!(", pop {}", compact_credits(a.population as i64)) } else { String::new() };
                self.feed(ts, "FSDJump", format!("Jumped {:.2} ly to {}{}", dist, a.star_system, pop));
                let star = self.pending_star_class.as_ref().filter(|(s, _)| *s == a.star_system).map(|(_, c)| nexus_data::exploration::star_description(c));
                alerts.push_with(|| {
                    Alert::new(ts, AlertKind::Navigation, Priority::Low, format!("Arrived in {}", a.star_system))
                        .detail(format!("{}{}", star.clone().unwrap_or_default(), if a.population > 0 { format!(" · population {}", compact_credits(a.population as i64)) } else { " · uninhabited".into() }))
                        .speak(match &star {
                            Some(s) => format!("Arrived in {}. {s}.", a.star_system),
                            None => format!("Arrived in {}.", a.star_system),
                        })
                });
                self.pending_star_class = None;
            }
            Event::CarrierJump(a) => {
                self.location.on_arrival(a, ts, ArrivalKind::CarrierJump);
                self.history.jump(JumpRecord {
                    ts,
                    system: a.star_system.clone(),
                    address: a.system_address,
                    pos: a.star_pos,
                    dist: 0.0,
                    fuel_used: 0.0,
                    carrier: true,
                });
                self.exobio.on_system(a.system_address);
                self.exploration.enter_system(a.system_address, &a.star_system, a.star_pos, ts);
                self.carrier.on_carrier_jump(a, ts);
                self.squadron_carrier.on_carrier_jump(a, ts);
                self.mining.leave_body();
                self.feed(ts, "CarrierJump", format!("Carrier jumped to {}", a.star_system));
            }
            Event::StartJump(s) => {
                if s.jump_type == "Hyperspace" {
                    self.location.hyperspace = true;
                    if let (Some(sys), Some(class)) = (&s.star_system, &s.star_class) {
                        self.pending_star_class = Some((sys.clone(), class.clone()));
                        self.star_class_alert(ts, sys, class, alerts);
                    }
                }
            }
            Event::FSDTarget(t) => {
                self.location.fsd_target = Some(crate::location::FsdTarget {
                    name: t.name.clone(),
                    system_address: t.system_address,
                    star_class: t.star_class.clone(),
                    remaining_jumps: t.remaining_jumps_in_route,
                });
            }
            Event::SupercruiseEntry(_) => {
                self.location.on_supercruise_entry();
                self.mining.leave_body();
            }
            Event::SupercruiseExit(s) => self.location.on_supercruise_exit(s),
            Event::Docked(d) => {
                self.location.on_docked(d);
                let station = clean_station_name(&d.station_name);
                self.history.docks.push(DockRecord {
                    ts,
                    station: station.clone(),
                    station_type: d.station_type.clone(),
                    system: d.star_system.clone(),
                    market_id: d.market_id,
                });
                self.mining.end_session();
                self.feed(ts, "Docked", format!("Docked at {station} ({})", d.star_system));
            }
            Event::Undocked(u) => {
                self.location.on_undocked();
                self.feed(ts, "Undocked", format!("Undocked from {}", clean_station_name(&u.station_name)));
            }
            Event::DockingDenied(d) => {
                let reason = d.reason.clone().unwrap_or_default();
                let station = clean_station_name(&d.station_name);
                self.feed(ts, "DockingDenied", format!("Docking denied at {station}: {reason}"));
                alerts.push_with(|| Alert::new(ts, AlertKind::Navigation, Priority::Normal, "Docking denied").detail(format!("{station}: {reason}")));
            }
            Event::ApproachBody(b) => {
                self.location.body = Some(b.body.clone());
                self.location.body_id = Some(b.body_id);
                self.location.body_type = Some("Planet".into());
                self.mining.at_body(b.system_address, Some(b.body_id));
            }
            Event::LeaveBody(_) => {
                self.location.lat_lon = None;
                self.mining.leave_body();
            }
            Event::Touchdown(t) => {
                self.location.landed = true;
                self.location.body = Some(t.body.clone());
                self.location.body_id = Some(t.body_id);
                if let (Some(lat), Some(lon)) = (t.latitude, t.longitude) {
                    self.location.lat_lon = Some((lat, lon));
                }
                self.mining.on_landing(t, true);
                // Back at a mining location the Rhino worked before (not when the ship lands itself, recalled).
                if t.player_controlled != Some(false) && let Some(site) = self.mining.returning() {
                    alerts.push_with(|| {
                        let n = site.key.location.unwrap_or_default();
                        let mined: Vec<String> = site.by_tonnes().iter().take(2).map(|(c, tonnes)| format!("{tonnes} t {c}")).collect();
                        let mined = mined.join(", ");
                        Alert::new(ts, AlertKind::Mining, Priority::Low, format!("Back at mining location {n}"))
                            .detail(format!("You refined {mined} here ({})", site.last.to_local_string("%d %b")))
                            .speak(format!("Mining location {n}. You refined {mined} here before."))
                    });
                }
            }
            Event::Liftoff(t) => {
                self.location.landed = false;
                self.mining.on_landing(t, false);
            }
            Event::ApproachSettlement(s) => {
                self.location.settlement = Some(s.name_localised.clone().unwrap_or_else(|| s.name.clone()));
                self.location.settlement_pos = s.latitude.zip(s.longitude);
            }
            Event::Embark(e) => {
                self.location.on_foot = false;
                self.location.in_srv = e.srv;
                self.location.in_taxi = e.taxi;
            }
            Event::Disembark(d) => {
                self.location.on_foot = true;
                self.location.in_srv = false;
                if d.on_planet && !d.on_station
                    && let Some(body_id) = d.body_id {
                        self.exploration.on_footfall(d.system_address, body_id);
                    }
            }
            Event::LaunchSRV(v) => {
                self.location.in_srv = true;
                self.location.srv_type = v.srv_type.clone();
            }
            Event::DockSRV(_) | Event::SRVDestroyed(_) => {
                self.location.in_srv = false;
                self.location.srv_type = None;
            }
            Event::LaunchFighter(f) if f.player_controlled.unwrap_or(false) => self.location.in_fighter = true,
            Event::DockFighter(_) | Event::FighterDestroyed(_) => self.location.in_fighter = false,
            Event::NavRouteClear(_) => self.nav_route.clear(),
            Event::FuelScoop(f) => self.fleet.fuel_main = f.total,
            Event::RefuelAll(r) | Event::RefuelPartial(r) => {
                self.money(ts, LedgerCategory::Fuel, -r.cost, format!("{:.1} t fuel", r.amount));
            }
            Event::ReservoirReplenished(r) => {
                self.fleet.fuel_main = r.fuel_main;
                self.fleet.fuel_reservoir = r.fuel_reservoir;
            }
            Event::Died(d) => {
                let killers: Vec<String> = if d.killers.is_empty() {
                    d.killer_name_localised.clone().or_else(|| d.killer_name.clone()).into_iter().collect()
                } else {
                    d.killers.iter().map(|k| k.name.clone()).collect()
                };
                let lost = self.exploration.unsold_value() + self.exobio.unsold_value();
                self.history.deaths.push(DeathRecord { ts, system: self.location.system.clone(), killers: killers.clone() });
                self.exploration.on_death();
                self.exobio.on_death();
                self.inventory.cargo.items.clear();
                self.feed(ts, "Died", format!("Ship destroyed{}", if killers.is_empty() { String::new() } else { format!(" by {}", killers.join(", ")) }));
                alerts.push_with(|| {
                    Alert::new(ts, AlertKind::Combat, Priority::Critical, "Ship destroyed")
                        .detail(format!("Unsold data lost: about {} Cr", compact_credits(lost)))
                });
            }
            Event::Resurrect(r) => self.money(ts, LedgerCategory::Rebuy, -r.cost, r.option.clone()),
            Event::Interdicted(i) => {
                let who = i.interdictor.clone().unwrap_or_else(|| "unknown".into());
                self.feed(ts, "Interdicted", format!("Interdicted by {who}"));
                alerts.push_with(|| {
                    Alert::new(ts, AlertKind::Combat, Priority::High, "Interdiction")
                        .detail(format!("By {who}{}", if i.is_player { " (player)" } else { "" }))
                });
            }
            Event::HullDamage(h) => {
                self.fleet.on_hull_damage(h);
                if h.health < 0.25 && h.player_pilot.unwrap_or(true) {
                    alerts.push_with(|| {
                        Alert::new(ts, AlertKind::Combat, Priority::Critical, "Hull critical")
                            .detail(format!("Hull at {:.0}%", h.health * 100.0))
                            .speak("Warning. Hull integrity critical.")
                    });
                }
            }
            Event::BookTaxi(t) | Event::BookDropship(t) => self.money(ts, LedgerCategory::Taxi, -t.cost, "Transport"),

            // ---------------------------------------------------------------- ships
            Event::Loadout(l) => self.fleet.on_loadout(l, ctx.line, ts),
            Event::ShipyardBuy(b) => {
                let here = self.here_station();
                self.fleet.on_shipyard_buy(b, (&here.0, here.1.as_deref(), here.2), ts);
                let name = b.ship_type_localised.clone().unwrap_or_else(|| nexus_data::ships::ship_name(&b.ship_type));
                self.money(ts, LedgerCategory::Ships, -b.ship_price, format!("Bought {name}"));
                if let Some(p) = b.sell_price {
                    self.money(ts, LedgerCategory::Ships, p, "Sold old ship");
                }
            }
            Event::ShipyardNew(n) => self.fleet.on_shipyard_new(n),
            Event::ShipyardSell(s) => {
                self.fleet.on_shipyard_sell(s);
                self.money(ts, LedgerCategory::Ships, s.ship_price, format!("Sold {}", nexus_data::ships::ship_name(&s.ship_type)));
            }
            Event::ShipyardSwap(s) => {
                let here = self.here_station();
                self.fleet.on_shipyard_swap(s, (&here.0, here.1.as_deref(), here.2), ts);
            }
            Event::ShipyardTransfer(t) => {
                let here = self.here_station();
                self.fleet.on_shipyard_transfer(t, (&here.0, here.1.as_deref(), here.2), ts);
                self.money(ts, LedgerCategory::Ships, -t.transfer_price, "Ship transfer");
            }
            Event::ShipyardRedeem(_) => {}
            Event::ShipRedeemed(r) => {
                let here = self.here_station();
                self.fleet.on_ship_redeemed(r, (&here.0, here.1.as_deref(), here.2), ts);
            }
            Event::StoredShips(s) => self.fleet.on_stored_ships(s, ts),
            Event::SetUserShipName(n) => self.fleet.on_set_name(n),
            Event::StoredModules(m) => self.fleet.on_stored_modules(m, ts),
            Event::ModuleBuy(m) => {
                self.money(ts, LedgerCategory::Outfitting, -m.buy_price.unwrap_or(0), "Module bought");
                self.money(ts, LedgerCategory::Outfitting, m.sell_price.unwrap_or(0), "Module sold");
            }
            Event::ModuleBuyAndStore(m) => {
                self.money(ts, LedgerCategory::Outfitting, -m.buy_price.unwrap_or(0), "Module bought");
            }
            Event::ModuleSell(m) | Event::ModuleSellRemote(m) => {
                self.money(ts, LedgerCategory::Outfitting, m.sell_price.unwrap_or(0), "Module sold");
            }
            Event::ModuleStore(m) | Event::ModuleRetrieve(m) | Event::ModuleSwap(m) => {
                self.money(ts, LedgerCategory::Outfitting, -m.cost.unwrap_or(0), "Module storage");
            }
            Event::Repair(r) | Event::RepairAll(r) => {
                if matches!(event, Event::RepairAll(_)) {
                    self.fleet.on_repair_all();
                }
                self.money(ts, LedgerCategory::Repairs, -r.cost.unwrap_or(0), "Repairs");
            }
            Event::BuyAmmo(c) | Event::RestockVehicle(c) => {
                self.money(ts, LedgerCategory::Ammo, -c.cost.unwrap_or(0), "Ammo/restock");
            }
            Event::BuyDrones(c) => self.money(ts, LedgerCategory::Ammo, -c.total_cost.unwrap_or(0), "Limpets"),
            Event::SellDrones(c) => self.money(ts, LedgerCategory::Ammo, c.total_sale.unwrap_or(0), "Limpets sold"),
            Event::PayFines(c) | Event::PayBounties(c) => {
                self.money(ts, LedgerCategory::Fines, -c.amount.unwrap_or(0), "Fines/bounties paid");
            }

            // ---------------------------------------------------------------- exploration
            Event::Scan(s) => self.on_scan(s, ts, alerts),
            Event::FSSDiscoveryScan(f) => {
                self.exploration.on_fss_discovery(f, ts);
                self.feed(ts, "FSSDiscoveryScan", format!("Honk: {} bodies, {} non-body signals", f.body_count, f.non_body_count));
                alerts.push_with(|| {
                    Alert::new(ts, AlertKind::Exploration, Priority::Low, format!("{} bodies detected", f.body_count))
                        .detail(format!("{} non-body signals", f.non_body_count))
                        .speak(format!("Discovery scan: {} bodies.", f.body_count))
                });
            }
            Event::FSSAllBodiesFound(f) => {
                self.exploration.on_all_bodies_found(f, ts);
                let value = self.exploration.systems.get(&f.system_address).map(|s| s.value()).unwrap_or(0);
                self.feed(ts, "FSSAllBodiesFound", format!("{}: all {} bodies found", f.system_name, f.count));
                alerts.push_with(|| {
                    Alert::new(ts, AlertKind::Exploration, Priority::Low, "System scan complete")
                        .detail(format!("{} bodies, ~{} Cr", f.count, compact_credits(value)))
                });
            }
            Event::FSSBodySignals(b) => self.on_body_signals(b, false, ts, alerts),
            Event::SAASignalsFound(b) => self.on_body_signals(b, true, ts, alerts),
            Event::SAAScanComplete(s) => {
                self.exploration.on_saa_complete(s, ts);
                self.session.bodies_mapped += 1;
                let efficient = s.probes_used <= s.efficiency_target;
                self.feed(ts, "SAAScanComplete", format!("Mapped {} with {} probes{}", s.body_name, s.probes_used, if efficient { " (efficient)" } else { "" }));
            }
            Event::FSSSignalDiscovered(f) => {
                let name = self.location.system.clone();
                self.exploration.on_fss_signal(f, &name, ts);
            }
            Event::NavBeaconScan(n) => {
                let name = self.location.system.clone();
                self.exploration.on_nav_beacon(n, &name, ts);
            }
            Event::CodexEntry(c) => {
                self.exploration.on_codex(c, ts);
                if c.is_new_entry.unwrap_or(false) {
                    let name = c.name_localised.clone().unwrap_or_else(|| c.name.clone());
                    self.feed(ts, "CodexEntry", format!("New codex entry: {name}"));
                    alerts.push_with(|| {
                        Alert::new(ts, AlertKind::Exploration, Priority::Normal, "New codex entry")
                            .detail(format!("{name} ({})", c.region_localised.clone().unwrap_or_default()))
                    });
                }
            }
            Event::ScanOrganic(o) => self.on_scan_organic(o, ts, alerts),
            Event::SellOrganicData(s) => {
                let total = self.exobio.on_sell(s, ts);
                {
                    let here = Here { system: &self.location.system, system_address: self.location.system_address, station_faction: self.location.station.as_ref().and_then(|s| s.faction.as_deref()) };
                    self.bgs.on_data_sale(BgsKind::Exobiology, total, ts, &here);
                }
                self.money(ts, LedgerCategory::Exobiology, total, format!("{} samples", s.bio_data.len()));
                self.feed(ts, "SellOrganicData", format!("Sold exobiology data: {} Cr", group_digits(total)));
            }
            Event::MultiSellExplorationData(s) => {
                let names: Vec<String> = s.discovered.iter().map(|d| d.system_name.clone()).collect();
                self.exploration.on_sell(&names, s.total_earnings, ts);
                {
                    let here = Here { system: &self.location.system, system_address: self.location.system_address, station_faction: self.location.station.as_ref().and_then(|s| s.faction.as_deref()) };
                    self.bgs.on_data_sale(BgsKind::Exploration, s.total_earnings, ts, &here);
                }
                self.money(ts, LedgerCategory::Exploration, s.total_earnings, format!("{} systems", names.len()));
                self.feed(ts, "MultiSellExplorationData", format!("Sold cartographic data: {} Cr", group_digits(s.total_earnings)));
            }
            Event::SellExplorationData(s) => {
                self.exploration.on_sell(&s.systems, s.total_earnings, ts);
                self.money(ts, LedgerCategory::Exploration, s.total_earnings, format!("{} systems", s.systems.len()));
            }

            // ---------------------------------------------------------------- trade & cargo
            Event::MarketBuy(b) => {
                self.inventory.cargo.adjust(&b.kind, b.kind_localised.as_deref(), b.count, None);
                let name = b.kind_localised.clone().unwrap_or_else(|| b.kind.clone());
                self.money(ts, LedgerCategory::Trade, -b.total_cost, format!("Bought {} {}", b.count, name));
            }
            Event::MarketSell(s) => {
                self.inventory.cargo.adjust(&s.kind, s.kind_localised.as_deref(), -s.count, None);
                {
                    let here = Here { system: &self.location.system, system_address: self.location.system_address, station_faction: self.location.station.as_ref().and_then(|s| s.faction.as_deref()) };
                    self.bgs.on_market_sell(s, ts, &here);
                }
                let name = s.kind_localised.clone().unwrap_or_else(|| s.kind.clone());
                let cat = if s.avg_price_paid == 0 { LedgerCategory::Mining } else { LedgerCategory::Trade };
                let profit = s.total_sale - s.avg_price_paid * s.count;
                self.money(ts, cat, s.total_sale, format!("Sold {} {} (profit {})", s.count, name, group_digits(profit)));
                self.feed(ts, "MarketSell", format!("Sold {} {} for {} Cr", s.count, name, group_digits(s.total_sale)));
            }
            Event::Cargo(c) => {
                if let Some(inv) = &c.inventory {
                    if c.vessel == "SRV" {
                        self.inventory.srv_cargo.snapshot(inv, ts);
                    } else {
                        self.inventory.cargo.snapshot(inv, ts);
                    }
                }
            }
            Event::CollectCargo(c) => self.inventory.cargo.adjust(&c.kind, c.kind_localised.as_deref(), 1, c.mission_id),
            Event::EjectCargo(c) => {
                self.inventory.cargo.adjust(&c.kind, c.kind_localised.as_deref(), -c.count.unwrap_or(1), c.mission_id)
            }
            Event::CargoDepot(d) => self.missions.on_cargo_depot(d),

            // ---------------------------------------------------------------- mining
            Event::ProspectedAsteroid(p) => {
                let (sys, body) = (self.location.system.clone(), self.location.body.clone());
                self.mining.on_prospected(p, ts, &sys, body.as_deref());
                if let Some(m) = p.motherlode_material_localised.as_ref().or(p.motherlode_material.as_ref()) {
                    let m = m.clone();
                    alerts.push_with(|| {
                        Alert::new(ts, AlertKind::Mining, Priority::High, "Motherlode!")
                            .detail(format!("{m} core ({})", p.content_localised.clone().unwrap_or_default()))
                            .speak(format!("Motherlode detected. {m}."))
                    });
                } else if let Some(best) = p.materials.iter().max_by(|a, b| a.proportion.total_cmp(&b.proportion))
                    && best.proportion >= 25.0 {
                        let name = best.name_localised.clone().unwrap_or_else(|| best.name.clone());
                        let prop = best.proportion;
                        alerts.push_with(|| {
                            Alert::new(ts, AlertKind::Mining, Priority::Normal, "Rich asteroid")
                                .detail(format!("{name} {prop:.0}%"))
                                .speak(format!("{name}, {prop:.0} percent"))
                        });
                    }
            }
            Event::MiningRefined(r) => {
                let (sys, body) = (self.location.system.clone(), self.location.body.clone());
                self.mining.on_refined(r, ts, &sys, body.as_deref());
                // The Rhino's refinery: the tonne goes to the mining location this landing named. Where it came out is
                // only known live (Status.json); a replayed journal has no positions.
                if self.location.in_srv && let Some(body_id) = self.location.body_id {
                    let fix = if ctx.live { self.surface_fix() } else { None };
                    let place = crate::mining::Place { system: &sys, system_address: self.location.system_address, body: body.as_deref().unwrap_or_default(), body_id };
                    self.mining.on_surface_refined(r, ts, place, fix);
                }
                self.inventory.cargo.adjust(&r.kind, r.kind_localised.as_deref(), 1, None);
            }
            Event::AsteroidCracked(_) => {
                let (sys, body) = (self.location.system.clone(), self.location.body.clone());
                self.mining.on_cracked(ts, &sys, body.as_deref());
            }
            Event::LaunchDrone(d) => self.mining.on_drone(d, ts),

            // ---------------------------------------------------------------- materials & engineering
            Event::Materials(m) => self.inventory.materials.snapshot(m, ts),
            Event::MaterialCollected(m) => {
                let cat = MaterialCategory::from_journal(&m.category);
                self.inventory.materials.adjust(&m.name, cat, m.count, m.name_localised.as_deref());
            }
            Event::MaterialDiscarded(m) => {
                let cat = MaterialCategory::from_journal(&m.category);
                self.inventory.materials.adjust(&m.name, cat, -m.count, m.name_localised.as_deref());
            }
            Event::MaterialTrade(t) => {
                let paid = MaterialCategory::from_journal(&t.paid.category);
                let recv = MaterialCategory::from_journal(&t.received.category);
                self.inventory.materials.adjust(&t.paid.material, paid, -t.paid.quantity, t.paid.material_localised.as_deref());
                self.inventory.materials.adjust(
                    &t.received.material,
                    recv,
                    t.received.quantity,
                    t.received.material_localised.as_deref(),
                );
            }
            Event::EngineerCraft(c) => {
                for i in &c.ingredients {
                    if self.inventory.materials.category_of(&i.name).is_some() {
                        self.inventory.materials.adjust(&i.name, None, -i.count, None);
                    } else {
                        self.inventory.cargo.adjust(&i.name, i.name_localised.as_deref(), -i.count, None);
                    }
                }
                let what = c.module.clone().or_else(|| c.slot.clone()).unwrap_or_default();
                self.feed(ts, "EngineerCraft", format!("Engineered {} → {} G{}", what, c.blueprint_name, c.level));
            }
            Event::EngineerContribution(c) => {
                if let Some(m) = &c.material {
                    self.inventory.materials.adjust(m, None, -c.quantity, None);
                }
                if let Some(cm) = &c.commodity {
                    self.inventory.cargo.adjust(cm, None, -c.quantity, None);
                }
            }
            Event::Synthesis(s) => {
                for m in &s.materials {
                    self.inventory.materials.adjust(&m.name, None, -m.count, None);
                }
            }
            Event::TechnologyBroker(t) => {
                for m in &t.materials {
                    let cat = m.category.as_deref().and_then(MaterialCategory::from_journal);
                    self.inventory.materials.adjust(&m.name, cat, -m.count, m.name_localised.as_deref());
                }
                for c in &t.commodities {
                    self.inventory.cargo.adjust(&c.name, c.name_localised.as_deref(), -c.count, None);
                }
            }
            Event::ScientificResearch(s) => self.inventory.materials.adjust(&s.name, None, -s.count, None),

            // ---------------------------------------------------------------- odyssey
            Event::ShipLocker(s) => {
                if s.has_content() {
                    self.inventory.ship_locker.snapshot(s, ts);
                }
            }
            Event::Backpack(b) => {
                if b.has_content() {
                    self.inventory.backpack.snapshot(b, ts);
                }
            }
            Event::BackpackChange(c) => {
                for a in &c.added {
                    self.inventory.backpack.adjust(a, 1);
                }
                for r in &c.removed {
                    self.inventory.backpack.adjust(r, -1);
                }
            }
            Event::BuyMicroResources(m) => {
                self.money(ts, LedgerCategory::Materials, -m.price.unwrap_or(0), "Micro-resources bought")
            }
            Event::SellMicroResources(m) => {
                self.money(ts, LedgerCategory::Materials, m.price.unwrap_or(0), "Micro-resources sold")
            }
            Event::SuitLoadout(s) | Event::SwitchSuitLoadout(s) => self.inventory.on_suit_loadout(s, true),
            Event::CreateSuitLoadout(s) => self.inventory.on_suit_loadout(s, false),
            Event::UpgradeSuit(u) | Event::UpgradeWeapon(u) => {
                self.money(ts, LedgerCategory::Outfitting, -u.cost, format!("Upgraded {} to G{}", u.name_localised.clone().unwrap_or_else(|| u.name.clone()), u.class));
            }
            Event::BuySuit(g) | Event::BuyWeapon(g) => {
                self.money(ts, LedgerCategory::Outfitting, -g.price, format!("Bought {}", g.name_localised.clone().unwrap_or_else(|| g.name.clone())));
            }
            Event::SellSuit(g) | Event::SellWeapon(g) => {
                self.money(ts, LedgerCategory::Outfitting, g.price, format!("Sold {}", g.name_localised.clone().unwrap_or_else(|| g.name.clone())));
            }

            // ---------------------------------------------------------------- missions & combat
            Event::Missions(m) => self.missions.on_snapshot(m, ts),
            Event::MissionAccepted(a) => {
                let (sys, station) = (self.location.system.clone(), self.location.station.as_ref().map(|s| s.name.clone()));
                self.missions.on_accepted(a, ts, &sys, station.as_deref());
                self.feed(ts, "MissionAccepted", format!("Accepted: {}", a.localised_name.clone().unwrap_or_else(|| a.name.clone())));
            }
            Event::MissionCompleted(c) => {
                {
                    let (cur_addr, cur_name, visited) = (self.location.system_address, &self.location.system, &self.history.visited);
                    self.bgs.on_mission_completed(c, ts, |addr| {
                        if addr == cur_addr { cur_name.clone() } else { visited.get(&addr).map(|v| v.name.clone()).unwrap_or_default() }
                    });
                }
                let reward = self.missions.on_completed(c, ts).unwrap_or(0);
                self.session.missions_completed += 1;
                let title = c.localised_name.clone().unwrap_or_else(|| c.name.clone());
                self.money(ts, LedgerCategory::Missions, reward, title.clone());
                if let Some(d) = c.donated {
                    self.money(ts, LedgerCategory::Missions, -d, "Donation");
                }
                self.feed(ts, "MissionCompleted", format!("Completed: {} (+{} Cr)", title, group_digits(reward)));
            }
            Event::MissionFailed(m) | Event::MissionAbandoned(m) => {
                if let Some(f) = self.missions.active.get(&m.mission_id).map(|x| x.faction.clone()) {
                    let here = Here { system: &self.location.system, system_address: self.location.system_address, station_faction: None };
                    self.bgs.on_mission_failed(&f, ts, &here);
                }
                let outcome = if matches!(event, Event::MissionFailed(_)) { MissionOutcome::Failed } else { MissionOutcome::Abandoned };
                self.missions.on_ended(m, outcome, ts);
            }
            Event::MissionRedirected(r) => {
                if let Some(m) = self.missions.on_redirected(r) {
                    let text = format!("{} — return to {}", m.title, m.destination_station.clone().unwrap_or_else(|| m.faction.clone()));
                    self.feed(ts, "MissionRedirected", text.clone());
                    alerts.push_with(|| {
                        Alert::new(ts, AlertKind::Missions, Priority::Normal, "Mission objective complete").detail(text)
                    });
                }
            }
            Event::Bounty(b) => {
                self.session.kills += 1;
                let done = self.missions.on_bounty(b);
                for id in done {
                    if let Some(m) = self.missions.active.get(&id) {
                        let text = format!("{} for {}", m.title, m.faction);
                        alerts.push_with(|| {
                            Alert::new(ts, AlertKind::Missions, Priority::High, "Massacre mission complete")
                                .detail(text.clone())
                                .speak("Massacre mission complete.")
                        });
                    }
                }
            }
            Event::FactionKillBond(_) | Event::CapShipBond(_) => self.session.kills += 1,
            Event::RedeemVoucher(r) => {
                {
                    let here = Here { system: &self.location.system, system_address: self.location.system_address, station_faction: None };
                    self.bgs.on_redeem_voucher(r, ts, &here);
                }
                let cat = match r.kind.to_ascii_lowercase().as_str() {
                    "bounty" => LedgerCategory::Bounties,
                    "combatbond" => LedgerCategory::CombatBonds,
                    "codex" | "scannable" => LedgerCategory::Exploration,
                    _ => LedgerCategory::Other,
                };
                self.money(ts, cat, r.amount, format!("{} voucher", r.kind));
            }

            // ---------------------------------------------------------------- carrier
            Event::CarrierStats(s) => {
                if let Some(c) = self.carrier_for(s.carrier_type.as_deref(), s.carrier_id) {
                    c.on_stats(s, ts);
                }
            }
            Event::CarrierBuy(b) => {
                let squadron = b.carrier_type.as_deref() == Some("SquadronCarrier");
                if let Some(c) = self.carrier_for(b.carrier_type.as_deref(), b.carrier_id) {
                    c.on_buy(b, ts);
                }
                self.money(ts, LedgerCategory::Carrier, -b.price, if squadron { "Squadron carrier purchased" } else { "Fleet carrier purchased" });
            }
            Event::CarrierJumpRequest(r) => {
                let squadron = CarrierKind::of(r.carrier_type.as_deref(), r.carrier_id, self.squadron_carrier.id) == Some(CarrierKind::Squadron);
                if let Some(c) = self.carrier_for(r.carrier_type.as_deref(), r.carrier_id) {
                    c.on_jump_request(r, ts);
                }
                let what = if squadron { "Squadron carrier" } else { "Carrier" };
                let when = r.departure_time.as_deref().and_then(Timestamp::parse).map(|t| t.to_local_string("%H:%M")).unwrap_or_default();
                self.feed(ts, "CarrierJumpRequest", format!("{what} jump to {} at {}", r.system_name, when));
                alerts.push_with(|| {
                    Alert::new(ts, AlertKind::Carrier, Priority::Normal, format!("{what} jump scheduled"))
                        .detail(format!("{} — departs {}", r.system_name, when))
                });
            }
            Event::CarrierJumpCancelled(c) => {
                if let Some(k) = self.carrier_for(c.carrier_type.as_deref(), c.carrier_id) {
                    k.on_jump_cancelled(c);
                }
            }
            Event::CarrierLocation(l) => {
                if let Some(c) = self.carrier_for(l.carrier_type.as_deref(), l.carrier_id) {
                    c.on_location(l, ts);
                }
            }
            Event::CarrierFinance(f) => {
                if let Some(c) = self.carrier_for(f.carrier_type.as_deref(), f.carrier_id) {
                    c.on_finance(f);
                }
            }
            Event::CarrierBankTransfer(t) => {
                let squadron = CarrierKind::of(t.carrier_type.as_deref(), t.carrier_id, self.squadron_carrier.id) == Some(CarrierKind::Squadron);
                if let Some(c) = self.carrier_for(t.carrier_type.as_deref(), t.carrier_id) {
                    c.on_bank_transfer(t);
                }
                // Credits on the commander's own carrier stay theirs; a squadron carrier's are the squadron's.
                let amount = t.withdraw.unwrap_or(0) - t.deposit.unwrap_or(0);
                let (cat, note) = match (squadron, amount < 0) {
                    (false, true) => (LedgerCategory::Transfer, "Deposited to carrier"),
                    (false, false) => (LedgerCategory::Transfer, "Withdrew from carrier"),
                    (true, true) => (LedgerCategory::Carrier, "Deposited to the squadron carrier"),
                    (true, false) => (LedgerCategory::Carrier, "Withdrew from the squadron carrier"),
                };
                self.money(ts, cat, amount, note);
            }
            Event::CarrierDepositFuel(d) => {
                if let Some(c) = self.carrier_for(d.carrier_type.as_deref(), d.carrier_id) {
                    c.on_deposit_fuel(d);
                }
            }
            Event::CarrierTradeOrder(o) => {
                if let Some(c) = self.carrier_for(o.carrier_type.as_deref(), o.carrier_id) {
                    c.on_trade_order(o, ts);
                }
            }
            Event::CarrierNameChange(n) => {
                if let Some(c) = self.carrier_for(n.carrier_type.as_deref(), n.carrier_id) {
                    c.on_name_change(n);
                }
            }
            Event::CarrierCrewServices(s) => {
                if let Some(c) = self.carrier_for(s.carrier_type.as_deref(), s.carrier_id) {
                    c.on_crew_services(s);
                }
            }
            Event::CarrierDecommission(d) => {
                let name = self.carrier_for(d.carrier_type.as_deref(), d.carrier_id).map(|c| {
                    c.on_decommission(d, ts);
                    if c.name.is_empty() { c.callsign.clone() } else { c.name.clone() }
                });
                if let Some(name) = name {
                    let when = Timestamp(d.scrap_time).to_local_string("%d %b %H:%M");
                    self.feed(ts, "CarrierDecommission", format!("Decommissioning {name}: scrapped {when}, refund {} Cr", group_digits(d.scrap_refund)));
                }
            }
            Event::CarrierCancelDecommission(d) => {
                if let Some(c) = self.carrier_for(d.carrier_type.as_deref(), d.carrier_id) {
                    c.on_cancel_decommission(d);
                }
                self.feed(ts, "CarrierCancelDecommission", "Carrier decommission cancelled".into());
            }

            // ---------------------------------------------------------------- colonisation
            Event::ColonisationConstructionDepot(d) => {
                // Only the station you're docked at says where a depot is: a snapshot is never pinned on wherever
                // you happen to be.
                let docked_at = self.location.station.as_ref().filter(|_| self.location.docked);
                self.colonisation.on_depot(d, ts, docked_at);
            }
            Event::ColonisationContribution(c) => {
                self.colonisation.on_contribution(c);
                let tons: i64 = c.contributions.iter().map(|x| x.amount).sum();
                self.feed(ts, "ColonisationContribution", format!("Delivered {tons} t to construction"));
            }
            Event::ColonisationSystemClaim(c) => self.colonisation.on_claim(c, ts),
            Event::ColonisationSystemClaimRelease(c) => self.colonisation.on_claim_release(c),
            Event::ColonisationBeaconDeployed(c) => self.colonisation.on_beacon(c, ts),

            Event::CommitCrime(c) => {
                let here = Here { system: &self.location.system, system_address: self.location.system_address, station_faction: None };
                self.bgs.on_crime(c, ts, &here);
            }
            Event::Statistics(s) => self.cmdr.statistics = Some(Arc::new((**s).clone())),
            Event::ReceiveText(_) | Event::Friends(_) => {}
            _ => {}
        }
        self.carrier.settle_pending(ts);
        self.squadron_carrier.settle_pending(ts);
    }

    fn star_class_alert(&mut self, ts: Timestamp, system: &str, class: &str, alerts: &mut Alerts) {
        let scoopable = nexus_data::exploration::is_scoopable(class);
        let desc = nexus_data::exploration::star_description(class);
        let fuel_frac = self
            .fleet
            .current()
            .filter(|s| s.fuel_main_capacity > 0.0)
            .map(|s| self.fleet.fuel_main / s.fuel_main_capacity)
            .unwrap_or(1.0);
        if matches!(class, "N" | "H") || class.starts_with('D') {
            alerts.push_with(|| {
                Alert::new(ts, AlertKind::Navigation, Priority::Normal, format!("Next: {desc}"))
                    .detail(system.to_string())
                    .speak(format!("Caution, {desc} ahead."))
            });
        } else if !scoopable && fuel_frac < 0.5 {
            alerts.push_with(|| {
                Alert::new(ts, AlertKind::Navigation, Priority::High, "Next star is not scoopable")
                    .detail(format!("{system} — {desc}; fuel {:.0}%", fuel_frac * 100.0))
                    .speak("Warning. Next star is not scoopable.")
            });
        }
    }

    fn on_scan(&mut self, s: &ev::Scan, ts: Timestamp, alerts: &mut Alerts) {
        let (addr, id) = self.exploration.on_scan(s, ts);
        self.session.bodies_scanned += 1;
        let Some(sys) = self.exploration.systems.get(&addr) else { return };
        let Some(body) = sys.bodies.get(&id) else { return };
        if !alerts.enabled {
            return;
        }
        let tags = interesting(body, sys);
        let notable: Vec<&Interest> = tags
            .iter()
            .filter(|t| {
                matches!(
                    t,
                    Interest::EarthLike
                        | Interest::WaterWorld
                        | Interest::AmmoniaWorld
                        | Interest::Terraformable
                        | Interest::GasGiantLife
                        | Interest::HighValue
                )
            })
            .collect();
        if !notable.is_empty() {
            let short = body.short_name(&sys.name).to_string();
            let label = notable.iter().map(|t| t.label()).collect::<Vec<_>>().join(", ");
            let value = body.value.mapped;
            alerts.push(
                Alert::new(ts, AlertKind::Exploration, Priority::High, format!("{short}: {label}"))
                    .detail(format!(
                        "Mapped value ~{} Cr{}",
                        compact_credits(value),
                        if body.first_discovery() { " · first discovery" } else { "" }
                    ))
                    .speak(format!("{label}. Worth about {} when mapped.", spoken_credits(value))),
            );
        }
    }

    fn on_body_signals(&mut self, b: &ev::BodySignals, dss: bool, ts: Timestamp, alerts: &mut Alerts) {
        self.exploration.on_body_signals(b, dss, ts);
        let bio = b.signals.iter().find(|s| s.kind.to_ascii_lowercase().contains("biological")).map(|s| s.count).unwrap_or(0);
        if bio == 0 {
            return;
        }
        let short = b.body_name.strip_prefix(&self.location.system).map(str::trim).unwrap_or(&b.body_name).to_string();
        // What the prediction engine expects there (computed by exploration.on_body_signals).
        let populated = self.location.system_address == b.system_address && self.location.info.population > 0;
        let (likely, expected) = self
            .exploration
            .systems
            .get(&b.system_address)
            .and_then(|s| s.bodies.get(&b.body_id))
            .map(|body| {
                let bonus = nexus_data::bio_predict::first_logged_bonus_likely(body.was_footfalled, populated, None);
                let names: Vec<String> = body.likely_species().iter().take(3).map(|p| p.species.clone()).collect();
                (names, body.predicted_value(bonus))
            })
            .unwrap_or_default();
        if dss && !b.genuses.is_empty() {
            let genera: Vec<String> =
                b.genuses.iter().map(|g| g.genus_localised.clone().unwrap_or_else(|| g.genus.clone())).collect();
            self.feed(ts, "SAASignalsFound", format!("{short}: {}", genera.join(", ")));
            alerts.push_with(|| {
                Alert::new(ts, AlertKind::Exobiology, if expected >= 20_000_000 { Priority::High } else { Priority::Normal }, format!("{short}: {bio} biological"))
                    .detail(format!("{}{}", genera.join(", "), if expected > 0 { format!(" · ≈{} Cr", compact_credits(expected)) } else { String::new() }))
                    .speak(if expected > 0 { format!("{} genera mapped. Worth about {}.", genera.len(), spoken_credits(expected)) } else { format!("{} genera mapped.", genera.len()) })
            });
        } else {
            self.feed(ts, "FSSBodySignals", format!("{short}: {bio} biological signals"));
            alerts.push_with(|| {
                Alert::new(ts, AlertKind::Exobiology, if expected >= 20_000_000 { Priority::High } else { Priority::Normal }, format!("{short}: {bio} biological signals"))
                    .detail(if likely.is_empty() { String::new() } else { format!("Likely {} · ≈{} Cr", likely.join(", "), compact_credits(expected)) })
                    .speak(if expected > 0 { format!("{bio} biological signals on {}. Likely worth {}.", spoken_body(&short), spoken_credits(expected)) } else { format!("{bio} biological signals on {}", spoken_body(&short)) })
            });
        }
    }

    fn on_scan_organic(&mut self, o: &ev::ScanOrganic, ts: Timestamp, alerts: &mut Alerts) {
        let pos = self.status.as_ref().and_then(|s| s.lat_lon());
        let populated = self.location.system_address == o.system_address && self.location.info.population > 0;
        let (system, body_name, unfootfalled) = {
            let sys = self.exploration.systems.get(&o.system_address);
            let body = sys.and_then(|s| s.bodies.get(&o.body));
            (
                sys.map(|s| s.name.clone()).unwrap_or_else(|| self.location.system.clone()),
                body.map(|b| b.name.clone()).or_else(|| self.location.body.clone()).unwrap_or_default(),
                body.and_then(|b| b.was_footfalled),
            )
        };
        // First-logged bonus (5x): unpopulated system, nobody on the body at our first scan, and the latest
        // ScanOrganic.WasLogged not true. WasLogged false alone is no proof (checked against Vista Genomics sales).
        let bonus = nexus_data::bio_predict::first_logged_bonus_likely(unfootfalled, populated, o.was_logged);
        let outcome = self.exobio.on_scan_organic(o, ts, pos, &system, &body_name, bonus);
        let species = o.species_localised.clone().unwrap_or_else(|| o.species.clone());
        // mark on the body
        if let Some(body) = self.exploration.systems.get_mut(&o.system_address).and_then(|s| s.bodies.get_mut(&o.body)) {
            let genus = o.genus_localised.clone().unwrap_or_default();
            let entry = match body.organics.iter().position(|x| x.species_symbol == o.species) {
                Some(i) => &mut body.organics[i],
                None => {
                    body.organics.push(crate::exploration::OrganicSpecies {
                        genus,
                        species: species.clone(),
                        variant: o.variant_localised.clone(),
                        species_symbol: o.species.clone(),
                        samples: 0,
                        analysed: false,
                    });
                    body.organics.last_mut().expect("just pushed")
                }
            };
            entry.samples = match o.scan_type.as_str() {
                "Log" => 1,
                "Sample" => 2,
                _ => 3,
            };
            entry.analysed = entry.samples >= 3;
        }
        match outcome {
            ScanOutcome::Started => {
                let range = self.exobio.current.as_ref().and_then(|c| c.colony_range_m);
                self.feed(ts, "ScanOrganic", format!("Sample 1/3: {species}"));
                alerts.push_with(|| {
                    let mut a = Alert::new(ts, AlertKind::Exobiology, Priority::Normal, format!("{species}: sample 1 of 3"));
                    if let Some(r) = range {
                        a = a.detail(format!("Move at least {r} m before the next sample")).speak(format!("Sample one logged. Move {r} metres."));
                    }
                    a
                });
            }
            ScanOutcome::Progress(n) => {
                self.feed(ts, "ScanOrganic", format!("Sample {n}/3: {species}"));
                alerts.push_with(|| Alert::new(ts, AlertKind::Exobiology, Priority::Low, format!("{species}: sample {n} of 3")));
            }
            ScanOutcome::Completed(rec) => {
                self.session.organics_analysed += 1;
                let payout = rec.expected_payout();
                self.feed(ts, "ScanOrganic", format!("Analysed {species}: ~{} Cr", compact_credits(payout)));
                alerts.push_with(|| {
                    Alert::new(ts, AlertKind::Exobiology, Priority::High, format!("Analysis complete: {species}"))
                        .detail(format!(
                            "{} Cr{}",
                            group_digits(rec.value),
                            if rec.bonus_expected { format!(" (+ first-logged bonus → {})", compact_credits(payout)) } else { String::new() }
                        ))
                        .speak(format!("Analysis complete. {species}. {}", spoken_credits(payout)))
                });
            }
        }
    }

    /// Applies a companion file update.
    pub fn apply_companion(&mut self, c: &Companion, live: bool, alerts: &mut Alerts) {
        match c {
            Companion::Status(st) => {
                self.on_status(st, live, alerts);
                self.status = Some(st.clone());
                self.status_writes += 1;
            }
            Companion::Market(m, _) => self.market = Some(m.clone()),
            Companion::Outfitting(o, _) => self.outfitting = Some(o.clone()),
            Companion::Shipyard(s, _) => self.shipyard = Some(s.clone()),
            Companion::NavRoute(r, _) => {
                if r.route.is_empty() || r.event == "NavRouteClear" {
                    self.nav_route.clear();
                } else {
                    self.nav_route = r.route.clone();
                    // The route starts with the system it was plotted from: while still there, that's already behind.
                    if self.nav_route.first().is_some_and(|h| h.system_address == self.location.system_address) {
                        self.nav_route.remove(0);
                    }
                }
            }
            Companion::ModulesInfo(m, _) => self.modules_info = Some(m.clone()),
            Companion::Cargo(cf, _) => {
                if let Some(inv) = &cf.cargo.inventory {
                    if cf.cargo.vessel == "SRV" {
                        self.inventory.srv_cargo.snapshot(inv, cf.timestamp);
                    } else {
                        self.inventory.cargo.snapshot(inv, cf.timestamp);
                    }
                }
            }
            Companion::ShipLocker(f, _) => {
                if f.inventory.has_content() {
                    self.inventory.ship_locker.snapshot(&f.inventory, f.timestamp);
                }
            }
            Companion::Backpack(f, _) => {
                if f.inventory.has_content() {
                    self.inventory.backpack.snapshot(&f.inventory, f.timestamp);
                }
            }
            Companion::FCMaterials(f, _) => self.fc_materials = Some(f.clone()),
        }
    }

    fn on_status(&mut self, st: &Status, live: bool, alerts: &mut Alerts) {
        if !st.is_in_game() {
            return;
        }
        self.location.on_status(st);
        if let Some(f) = st.fuel {
            self.fleet.fuel_main = f.main;
            self.fleet.fuel_reservoir = f.reservoir;
        }
        if let Some(b) = st.balance {
            self.cmdr.credits = b;
            self.history.credit_sample(st.timestamp, b);
        }
        if st.legal_state.is_some() {
            self.legal_state = st.legal_state.clone();
        }
        let prev = self.prev_flags;
        let rising = |flag: u32| st.flags & flag != 0 && prev & flag == 0;
        // The first status after start-up has no "previous" state, so don't fire edge alerts.
        if live && prev != 0 {
            let ts = st.timestamp;
            if rising(flags::LOW_FUEL) {
                alerts.push(
                    Alert::new(ts, AlertKind::Navigation, Priority::High, "Fuel low")
                        .detail(format!("{:.1} t remaining", self.fleet.fuel_main))
                        .speak("Warning. Fuel low."),
                );
            }
            if rising(flags::BEING_INTERDICTED) {
                alerts.push(Alert::new(ts, AlertKind::Combat, Priority::High, "Being interdicted").speak("Interdiction attempt."));
            }
            if rising(flags::OVER_HEATING) {
                alerts.push(Alert::new(ts, AlertKind::Combat, Priority::Normal, "Overheating"));
            }
        }
        self.prev_flags = st.flags;
    }

    // ---------------------------------------------------------------- convenience queries

    pub fn current_body_radius(&self) -> Option<f64> {
        self.status.as_ref().and_then(|s| s.planet_radius)
    }

    /// Where the commander is on the body right now (Status.json), with its radius; None away from a body, or when
    /// Status.json is about another body than the journal's.
    fn surface_fix(&self) -> Option<crate::mining::Fix> {
        let st = self.status.as_ref()?;
        let (lat, lon) = st.lat_lon()?;
        let radius_m = st.planet_radius.filter(|r| *r > 0.0)?;
        if let (Some(a), Some(b)) = (&st.body_name, &self.location.body) && a != b {
            return None;
        }
        Some(crate::mining::Fix { lat, lon, radius_m })
    }

    /// Distance from the current position to `pos` (ly), if we know where we are.
    pub fn distance_to(&self, pos: [f64; 3]) -> Option<f64> {
        self.location.star_pos.map(|p| crate::location::distance(p, pos))
    }

    pub fn materials_by_category(&self) -> BTreeMap<MaterialCategory, i64> {
        [MaterialCategory::Raw, MaterialCategory::Manufactured, MaterialCategory::Encoded]
            .into_iter()
            .map(|c| (c, self.inventory.materials.total(c)))
            .collect()
    }
}

fn spoken_credits(v: i64) -> String {
    let a = v as f64;
    if a >= 1e9 {
        format!("{:.1} billion credits", a / 1e9)
    } else if a >= 1e6 {
        format!("{:.1} million credits", a / 1e6)
    } else if a >= 1e3 {
        format!("{:.0} thousand credits", a / 1e3)
    } else {
        format!("{v} credits")
    }
}

fn spoken_body(short: &str) -> String {
    // "A 4 b" -> "A 4 b" read letter by letter is fine for TTS; keep as is.
    short.to_string()
}

/// All commanders seen in the journals, plus which one is active.
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub commanders: BTreeMap<String, GameState>,
    pub active: Option<String>,
    pending_header: Option<ev::Fileheader>,
    pub events_applied: u64,
    pub last_event: Timestamp,
}

impl Model {
    pub fn active(&self) -> Option<&GameState> {
        self.active.as_ref().and_then(|k| self.commanders.get(k))
    }

    pub fn active_mut(&mut self) -> Option<&mut GameState> {
        self.active.as_ref().and_then(|k| self.commanders.get_mut(k))
    }

    fn switch_to(&mut self, fid: &str, name: &str) {
        let key = if fid.is_empty() { name.to_string() } else { fid.to_string() };
        if key.is_empty() {
            return;
        }
        let gs = self.commanders.entry(key.clone()).or_default();
        if let Some(h) = self.pending_header.take() {
            gs.apply_header(&h);
        }
        self.active = Some(key);
    }

    pub fn apply(&mut self, ctx: &EventCtx<'_>, event: &Event, alerts: &mut Alerts) {
        self.events_applied += 1;
        self.last_event = ctx.ts;
        match event {
            Event::Fileheader(h) => {
                self.pending_header = Some((**h).clone());
            }
            Event::Commander(c) | Event::NewCommander(c) => self.switch_to(&c.fid, &c.name),
            Event::LoadGame(l) => self.switch_to(&l.fid, &l.commander),
            _ => {}
        }
        if let Some(gs) = self.active_mut() {
            gs.apply(ctx, event, alerts);
        }
    }

    pub fn apply_companion(&mut self, c: &Companion, live: bool, alerts: &mut Alerts) {
        if let Some(gs) = self.active_mut() {
            gs.apply_companion(c, live, alerts);
        }
    }
}
