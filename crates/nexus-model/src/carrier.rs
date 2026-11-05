//! The commander's fleet carriers: their own, and their squadron's.

use nexus_journal::events as ev;
use nexus_journal::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which of the commander's carriers something is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CarrierKind {
    /// Their own fleet carrier (`CarrierType` "FleetCarrier").
    #[default]
    Personal,
    /// Their squadron's carrier (`CarrierType` "SquadronCarrier", since Vanguards in August 2025).
    Squadron,
}

impl CarrierKind {
    /// The carrier a journal event is about, from its `CarrierType` and `CarrierID`. Since Vanguards
    /// every carrier event names its type. Events without one come from older journals, when only
    /// personal carriers existed, unless the id is the squadron carrier's. None for a type we don't
    /// know.
    pub fn of(carrier_type: Option<&str>, id: u64, squadron_id: Option<u64>) -> Option<CarrierKind> {
        match carrier_type {
            Some("FleetCarrier") => Some(CarrierKind::Personal),
            Some("SquadronCarrier") => Some(CarrierKind::Squadron),
            Some(_) => None,
            None if squadron_id == Some(id) => Some(CarrierKind::Squadron),
            None => Some(CarrierKind::Personal),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CarrierKind::Personal => "Fleet carrier",
            CarrierKind::Squadron => "Squadron carrier",
        }
    }
}

/// A decommission the owner asked for (`CarrierDecommission`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decommission {
    pub requested: Timestamp,
    /// When the carrier is scrapped and the refund paid.
    pub scrap_time: Timestamp,
    pub refund: i64,
}

#[derive(Debug, Clone, Default)]
pub struct CarrierJumpRecord {
    pub ts: Timestamp,
    pub system: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PendingJump {
    pub system: String,
    pub system_address: u64,
    pub body: Option<String>,
    pub departure: Option<Timestamp>,
    pub requested: Timestamp,
}

/// Fleet carrier jump timings the journal doesn't carry. The countdown itself comes from the
/// journal (`CarrierJumpRequest.DepartureTime`, which already includes any wait for busy servers);
/// these fill in the rest. Sources are in `docs/research/journal.md` (§5.4).
pub mod timing {
    /// After a jump the carrier can't jump again for 5 minutes. Frontier's patch notes for update
    /// 1.43 (June 2020, when carriers launched): "Fleet Carriers take 15 minutes to prepare for a
    /// jump, and have a 5 minute cool-down after." No later change found.
    pub const COOLDOWN_SECS: i64 = 5 * 60;
    /// The usual countdown from request to departure (same notes). Busy servers set a later
    /// departure, so only route estimates use it, as a minimum.
    pub const PREPARE_SECS: i64 = 15 * 60;
    /// Pre-flight lockdown (pads close, services stop) starts 3 min 20 s before departure. From the
    /// community wiki, not Frontier; one player reports about 2 minutes.
    pub const LOCKDOWN_SECS: i64 = 3 * 60 + 20;
    /// The owner can cancel a jump until 10 minutes before departure (community wiki).
    pub const CANCEL_CUTOFF_SECS: i64 = 10 * 60;
}

/// The jump the carrier made last, kept for the cooldown that follows it.
#[derive(Debug, Clone, Default)]
pub struct LastJump {
    pub system: String,
    pub body: Option<String>,
    /// When the journal saw the carrier arrive: `CarrierJump` (written to a commander aboard) or a
    /// `CarrierLocation` in the new system. None when only the departure time is known.
    pub arrived: Option<Timestamp>,
    pub departed: Option<Timestamp>,
}

/// What the carrier's jump drive is doing at a given moment.
#[derive(Debug, Clone, PartialEq)]
pub enum JumpTimer {
    /// A jump is scheduled. `departs` is None if the journal left the departure time out.
    Countdown { to: String, body: Option<String>, departs: Option<Timestamp> },
    /// The carrier jumped and can't jump again until `ready`. `estimated` when the journal didn't
    /// record the arrival (the commander wasn't aboard): then it's counted from the departure,
    /// and the real end comes a little later, by however long the jump itself took.
    Cooldown { at: String, body: Option<String>, ready: Timestamp, estimated: bool },
}

impl JumpTimer {
    /// Seconds until departure (countdown) or until the carrier can jump again (cooldown).
    pub fn left(&self, now: Timestamp) -> Option<i64> {
        match self {
            JumpTimer::Countdown { departs, .. } => departs.map(|d| (d.0 - now.0).max(0)),
            JumpTimer::Cooldown { ready, .. } => Some((ready.0 - now.0).max(0)),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TradeOrder {
    pub commodity: String,
    pub purchase: Option<i64>,
    pub sale: Option<i64>,
    pub price: Option<i64>,
    pub black_market: bool,
    pub updated: Timestamp,
}

#[derive(Debug, Clone, Default)]
pub struct Carrier {
    pub id: Option<u64>,
    pub carrier_type: Option<String>,
    pub callsign: String,
    pub name: String,
    pub fuel: i64,
    pub jump_range_curr: f64,
    pub jump_range_max: f64,
    pub docking_access: String,
    pub allow_notorious: bool,
    /// The last `CarrierStats` said a decommission is pending.
    pub pending_decommission: bool,
    /// From `CarrierDecommission`, until it's cancelled. Past its scrap time the carrier is gone.
    pub decommission: Option<Decommission>,
    pub space: ev::CarrierSpaceUsage,
    pub finance: ev::CarrierFinanceInfo,
    pub crew: Vec<ev::CarrierCrewMember>,
    pub system: Option<String>,
    pub system_address: Option<u64>,
    pub body_id: Option<u32>,
    /// When the journal last placed the carrier in `system` (zero when it never has); after a jump nobody saw
    /// arrive, the departure time.
    pub located: Timestamp,
    pub pending_jump: Option<PendingJump>,
    pub last_jump: Option<LastJump>,
    pub jumps: Vec<CarrierJumpRecord>,
    pub trade_orders: BTreeMap<String, TradeOrder>,
    pub stats_updated: Timestamp,
}

impl Carrier {
    /// The journal has shown this carrier (it may since have been decommissioned: see [`Carrier::active`]).
    pub fn owned(&self) -> bool {
        self.id.is_some()
    }

    /// The carrier was scrapped: its decommission's scrap time has passed.
    pub fn decommissioned(&self, now: Timestamp) -> bool {
        self.decommission.is_some_and(|d| now.0 >= d.scrap_time.0)
    }

    /// Known and still there at `now`.
    pub fn active(&self, now: Timestamp) -> bool {
        self.owned() && !self.decommissioned(now)
    }

    fn mine(&self, id: u64) -> bool {
        self.id.is_none_or(|own| own == id)
    }

    pub fn on_stats(&mut self, s: &ev::CarrierStats, ts: Timestamp) {
        // Another carrier of this kind: the old one was scrapped, or the commander changed squadrons.
        if self.id.is_some_and(|id| id != s.carrier_id) {
            *self = Carrier::default();
        }
        self.id = Some(s.carrier_id);
        self.carrier_type = s.carrier_type.clone();
        self.callsign = s.callsign.clone();
        self.name = s.name.clone();
        self.fuel = s.fuel_level;
        self.jump_range_curr = s.jump_range_curr;
        self.jump_range_max = s.jump_range_max;
        self.docking_access = s.docking_access.clone();
        self.allow_notorious = s.allow_notorious;
        self.pending_decommission = s.pending_decommission;
        if !s.pending_decommission {
            self.decommission = None;
        }
        self.space = s.space_usage.clone();
        self.finance = s.finance.clone();
        self.crew = s.crew.clone();
        self.stats_updated = ts;
    }

    /// A new carrier: nothing of an earlier one carries over.
    pub fn on_buy(&mut self, b: &ev::CarrierBuy, ts: Timestamp) {
        *self = Carrier {
            id: Some(b.carrier_id),
            carrier_type: b.carrier_type.clone(),
            callsign: b.callsign.clone(),
            system: Some(b.location.clone()),
            system_address: Some(b.system_address),
            located: ts,
            stats_updated: ts,
            ..Carrier::default()
        };
    }

    pub fn on_decommission(&mut self, d: &ev::CarrierDecommission, ts: Timestamp) {
        if !self.mine(d.carrier_id) {
            return;
        }
        self.id.get_or_insert(d.carrier_id);
        self.pending_decommission = true;
        self.decommission = Some(Decommission { requested: ts, scrap_time: Timestamp(d.scrap_time), refund: d.scrap_refund });
    }

    pub fn on_cancel_decommission(&mut self, c: &ev::CarrierLocation) {
        if self.mine(c.carrier_id) {
            self.pending_decommission = false;
            self.decommission = None;
        }
    }

    pub fn on_jump_request(&mut self, r: &ev::CarrierJumpRequest, ts: Timestamp) {
        if !self.mine(r.carrier_id) {
            return;
        }
        self.id.get_or_insert(r.carrier_id);
        self.pending_jump = Some(PendingJump {
            system: r.system_name.clone(),
            system_address: r.system_address,
            body: r.body.clone(),
            departure: r.departure_time.as_deref().and_then(Timestamp::parse),
            requested: ts,
        });
    }

    pub fn on_jump_cancelled(&mut self, c: &ev::CarrierLocation) {
        if self.mine(c.carrier_id) {
            self.pending_jump = None;
        }
    }

    pub fn on_location(&mut self, l: &ev::CarrierLocation, ts: Timestamp) {
        if !self.mine(l.carrier_id) {
            return;
        }
        self.id.get_or_insert(l.carrier_id);
        let moved = self.system_address.is_some() && self.system_address != l.system_address;
        if moved
            && let Some(sys) = &l.star_system {
                self.jumps.push(CarrierJumpRecord { ts, system: sys.clone(), body: None });
            }
        if l.star_system.is_some() {
            self.system = l.star_system.clone();
            self.located = ts;
        }
        self.system_address = l.system_address.or(self.system_address);
        self.body_id = l.body_id;
        if self.pending_jump.as_ref().is_some_and(|p| Some(p.system_address) == l.system_address)
            && let Some(p) = self.pending_jump.take()
        {
            self.last_jump = Some(LastJump { system: p.system, body: p.body, arrived: Some(ts), departed: p.departure });
        }
    }

    /// The commander is docked on a carrier that jumped (CarrierJump event).
    pub fn on_carrier_jump(&mut self, a: &ev::SystemArrival, ts: Timestamp) {
        if a.market_id.is_some() && a.market_id == self.id {
            self.system = Some(a.star_system.clone());
            self.system_address = Some(a.system_address);
            self.body_id = a.body_id;
            self.located = ts;
            self.jumps.push(CarrierJumpRecord { ts, system: a.star_system.clone(), body: a.body.clone() });
            let departed = self.pending_jump.take().and_then(|p| p.departure);
            self.last_jump = Some(LastJump { system: a.star_system.clone(), body: a.body.clone(), arrived: Some(ts), departed });
        }
    }

    /// Our own carrier jump has departed long enough ago that it must have arrived.
    pub fn settle_pending(&mut self, now: Timestamp) {
        let arrived = self.pending_jump.as_ref().and_then(|p| p.departure).is_some_and(|d| now.0 > d.0 + 60 * 5);
        if arrived
            && let Some(p) = self.pending_jump.take() {
                self.system = Some(p.system.clone());
                self.system_address = Some(p.system_address);
                self.located = p.departure.unwrap_or(now);
                self.jumps.push(CarrierJumpRecord { ts: p.departure.unwrap_or(now), system: p.system.clone(), body: p.body.clone() });
                self.last_jump = Some(LastJump { system: p.system, body: p.body, arrived: None, departed: p.departure });
            }
    }

    /// The jump countdown or cooldown running at `now`; None when the carrier is free to jump.
    /// Once the departure time has passed the cooldown starts, counted from the departure until
    /// the journal shows the arrival.
    pub fn jump_timer(&self, now: Timestamp) -> Option<JumpTimer> {
        use timing::COOLDOWN_SECS;
        if let Some(p) = &self.pending_jump {
            return match p.departure {
                Some(d) if now.0 >= d.0 => {
                    let ready = Timestamp(d.0 + COOLDOWN_SECS);
                    (now.0 < ready.0).then(|| JumpTimer::Cooldown { at: p.system.clone(), body: p.body.clone(), ready, estimated: true })
                }
                departs => Some(JumpTimer::Countdown { to: p.system.clone(), body: p.body.clone(), departs }),
            };
        }
        let j = self.last_jump.as_ref()?;
        let from = j.arrived.or(j.departed)?;
        let ready = Timestamp(from.0 + COOLDOWN_SECS);
        (now.0 < ready.0).then(|| JumpTimer::Cooldown { at: j.system.clone(), body: j.body.clone(), ready, estimated: j.arrived.is_none() })
    }

    /// A rough time, in seconds, until `jumps` more jumps have arrived: whatever countdown or
    /// cooldown is running (a running countdown counts as the first of those jumps), then the
    /// usual 15-minute countdown for each jump and a 5-minute cooldown between them. Busy servers
    /// and the jumps themselves add time, so treat it as a minimum.
    pub fn route_estimate(&self, now: Timestamp, jumps: u32) -> i64 {
        use timing::{COOLDOWN_SECS, PREPARE_SECS};
        let n = i64::from(jumps);
        if n == 0 {
            return 0;
        }
        let rest = |legs: i64| legs * PREPARE_SECS + (legs - 1).max(0) * COOLDOWN_SECS;
        match self.jump_timer(now) {
            Some(t @ JumpTimer::Countdown { departs: Some(_), .. }) => t.left(now).unwrap_or(0) + (n - 1) * (COOLDOWN_SECS + PREPARE_SECS),
            Some(t @ JumpTimer::Cooldown { .. }) => t.left(now).unwrap_or(0) + rest(n),
            _ => rest(n),
        }
    }

    pub fn on_finance(&mut self, f: &ev::CarrierFinance) {
        if !self.mine(f.carrier_id) {
            return;
        }
        self.finance.carrier_balance = f.carrier_balance;
        self.finance.reserve_balance = f.reserve_balance;
        self.finance.available_balance = f.available_balance;
        if f.reserve_percent.is_some() {
            self.finance.reserve_percent = f.reserve_percent;
        }
    }

    pub fn on_bank_transfer(&mut self, t: &ev::CarrierBankTransfer) {
        if self.mine(t.carrier_id) {
            self.finance.carrier_balance = t.carrier_balance;
        }
    }

    pub fn on_deposit_fuel(&mut self, d: &ev::CarrierDepositFuel) {
        if self.mine(d.carrier_id) {
            self.fuel = d.total;
        }
    }

    pub fn on_trade_order(&mut self, o: &ev::CarrierTradeOrder, ts: Timestamp) {
        if !self.mine(o.carrier_id) {
            return;
        }
        let name = o.commodity_localised.clone().unwrap_or_else(|| o.commodity.clone());
        if o.cancel_trade.unwrap_or(false) {
            self.trade_orders.remove(&name);
            return;
        }
        self.trade_orders.insert(
            name.clone(),
            TradeOrder {
                commodity: name,
                purchase: o.purchase_order,
                sale: o.sale_order,
                price: o.price,
                black_market: o.black_market,
                updated: ts,
            },
        );
    }

    pub fn on_name_change(&mut self, n: &ev::CarrierNameChange) {
        if self.mine(n.carrier_id) {
            self.name = n.name.clone();
            self.callsign = n.callsign.clone();
        }
    }

    pub fn on_crew_services(&mut self, c: &ev::CarrierCrewServices) {
        if !self.mine(c.carrier_id) {
            return;
        }
        self.id.get_or_insert(c.carrier_id);
        let entry = match self.crew.iter_mut().position(|m| m.crew_role == c.crew_role) {
            Some(i) => &mut self.crew[i],
            None => {
                self.crew.push(ev::CarrierCrewMember { crew_role: c.crew_role.clone(), ..Default::default() });
                self.crew.last_mut().expect("just pushed")
            }
        };
        match c.operation.as_str() {
            "Activate" => {
                entry.activated = true;
                entry.enabled = Some(true);
            }
            "Deactivate" => entry.activated = false,
            "Pause" => entry.enabled = Some(false),
            "Resume" => entry.enabled = Some(true),
            _ => {}
        }
        if c.crew_name.is_some() {
            entry.crew_name = c.crew_name.clone();
        }
    }

    /// Tritium burned per jump is roughly 5 t + 1 t per ~7.6 kt of mass… the game reports the
    /// range; for planning we use Spansh. Here: how many max-range jumps the tank allows (~ 90 t
    /// for a laden carrier is a common rule of thumb, so stay conservative).
    pub fn rough_jumps_on_fuel(&self) -> i64 {
        if self.fuel <= 0 { 0 } else { self.fuel / 90 }
    }
}

#[cfg(test)]
mod tests {
    use super::timing::{COOLDOWN_SECS, PREPARE_SECS};
    use super::*;

    const ID: u64 = 3_716_323_840;
    const T0: i64 = 1_790_000_000;

    fn at(secs: i64) -> Timestamp {
        Timestamp(T0 + secs)
    }

    fn own() -> Carrier {
        Carrier { id: Some(ID), system: Some("Alpha".into()), system_address: Some(100), ..Default::default() }
    }

    /// The owner schedules a jump to Beta at `t`, departing 15 minutes later.
    fn request(c: &mut Carrier, t: i64) {
        let departs = Timestamp(T0 + t + PREPARE_SECS).to_iso();
        let r = ev::CarrierJumpRequest {
            carrier_type: Some("FleetCarrier".into()),
            carrier_id: ID,
            system_name: "Beta".into(),
            body: Some("Beta A 1".into()),
            system_address: 200,
            body_id: Some(3),
            departure_time: Some(departs),
        };
        c.on_jump_request(&r, at(t));
    }

    #[test]
    fn request_then_cancel_clears_the_countdown() {
        let mut c = own();
        assert_eq!(c.jump_timer(at(0)), None, "no jump scheduled");
        request(&mut c, 0);
        let t = c.jump_timer(at(60)).expect("countdown");
        assert!(matches!(&t, JumpTimer::Countdown { to, departs: Some(d), .. } if to == "Beta" && *d == at(PREPARE_SECS)));
        assert_eq!(t.left(at(60)), Some(PREPARE_SECS - 60));
        c.on_jump_cancelled(&ev::CarrierLocation { carrier_id: ID, ..Default::default() });
        assert_eq!(c.jump_timer(at(61)), None, "a cancelled jump leaves nothing to count");
        assert!(c.last_jump.is_none());
    }

    #[test]
    fn request_jump_cooldown_ready() {
        let mut c = own();
        request(&mut c, 0);
        // Docked aboard: the journal writes CarrierJump on arrival, 40 s after departure.
        let arrival = PREPARE_SECS + 40;
        let a = ev::SystemArrival { star_system: "Beta".into(), system_address: 200, body: Some("Beta A 1".into()), market_id: Some(ID), ..Default::default() };
        c.on_carrier_jump(&a, at(arrival));
        assert!(c.pending_jump.is_none());
        assert_eq!(c.system.as_deref(), Some("Beta"));
        let t = c.jump_timer(at(arrival + 10)).expect("cooldown");
        assert_eq!(t, JumpTimer::Cooldown { at: "Beta".into(), body: Some("Beta A 1".into()), ready: at(arrival + COOLDOWN_SECS), estimated: false });
        assert_eq!(t.left(at(arrival + 10)), Some(COOLDOWN_SECS - 10));
        assert_eq!(c.jump_timer(at(arrival + COOLDOWN_SECS)), None, "ready to jump again");
    }

    #[test]
    fn departure_passed_without_arrival_starts_an_estimated_cooldown() {
        let mut c = own();
        request(&mut c, 0);
        // Not aboard: no CarrierJump. Past the departure time the cooldown runs from it.
        let t = c.jump_timer(at(PREPARE_SECS + 30)).expect("cooldown");
        assert_eq!(t, JumpTimer::Cooldown { at: "Beta".into(), body: Some("Beta A 1".into()), ready: at(PREPARE_SECS + COOLDOWN_SECS), estimated: true });
        // A later journal event settles the jump; the estimate stays the same.
        c.settle_pending(at(PREPARE_SECS + COOLDOWN_SECS + 1));
        assert_eq!(c.system.as_deref(), Some("Beta"));
        assert_eq!(c.jump_timer(at(PREPARE_SECS + COOLDOWN_SECS - 1)).and_then(|t| t.left(at(PREPARE_SECS + COOLDOWN_SECS - 1))), Some(1));
        assert_eq!(c.jump_timer(at(PREPARE_SECS + COOLDOWN_SECS)), None);
    }

    #[test]
    fn carrier_location_in_the_new_system_marks_the_arrival() {
        let mut c = own();
        request(&mut c, 0);
        let arrival = PREPARE_SECS + 50;
        c.on_location(&ev::CarrierLocation { carrier_type: Some("FleetCarrier".into()), carrier_id: ID, star_system: Some("Beta".into()), system_address: Some(200), body_id: Some(3) }, at(arrival));
        let t = c.jump_timer(at(arrival)).expect("cooldown");
        assert!(matches!(t, JumpTimer::Cooldown { ready, estimated: false, .. } if ready == at(arrival + COOLDOWN_SECS)));
        assert_eq!((c.system.as_deref(), c.located), (Some("Beta"), at(arrival)), "placed, and when");
    }

    #[test]
    fn a_jump_nobody_saw_arrive_is_placed_at_its_departure() {
        let mut c = own();
        request(&mut c, 0);
        c.settle_pending(at(PREPARE_SECS + COOLDOWN_SECS + 60));
        assert_eq!((c.system.as_deref(), c.located), (Some("Beta"), at(PREPARE_SECS)));
    }

    #[test]
    fn someone_elses_carrier_is_ignored() {
        let mut c = own();
        let r = ev::CarrierJumpRequest { carrier_id: ID + 1, system_name: "Gamma".into(), system_address: 300, departure_time: Some("2026-10-18T12:00:00Z".into()), ..Default::default() };
        c.on_jump_request(&r, at(0));
        assert_eq!(c.jump_timer(at(1)), None);
    }

    #[test]
    fn carrier_type_picks_the_carrier() {
        use CarrierKind::*;
        assert_eq!(CarrierKind::of(Some("FleetCarrier"), 1, Some(1)), Some(Personal), "the type wins over the id");
        assert_eq!(CarrierKind::of(Some("SquadronCarrier"), 1, None), Some(Squadron));
        assert_eq!(CarrierKind::of(None, 1, None), Some(Personal), "older journals only had personal carriers");
        assert_eq!(CarrierKind::of(None, 2, Some(2)), Some(Squadron));
        assert_eq!(CarrierKind::of(Some("SomethingNew"), 1, None), None);
    }

    fn decommission(c: &mut Carrier, t: i64, scrap: i64) {
        c.on_decommission(&ev::CarrierDecommission { carrier_id: ID, carrier_type: Some("FleetCarrier".into()), scrap_refund: 4_500_000_000, scrap_time: T0 + scrap }, at(t));
    }

    #[test]
    fn decommission_completes_at_the_scrap_time() {
        const WEEK: i64 = 7 * 24 * 3600;
        let mut c = own();
        decommission(&mut c, 0, WEEK);
        assert!(c.pending_decommission);
        assert!(c.active(at(WEEK - 1)), "still there until the scrap time");
        assert!(c.decommissioned(at(WEEK)));
        assert!(!c.active(at(WEEK)) && c.owned(), "gone, but remembered");
        assert_eq!(c.decommission.map(|d| d.refund), Some(4_500_000_000));
    }

    #[test]
    fn cancelled_decommission_keeps_the_carrier() {
        let mut c = own();
        decommission(&mut c, 0, 100);
        c.on_cancel_decommission(&ev::CarrierLocation { carrier_type: Some("FleetCarrier".into()), carrier_id: ID, ..Default::default() });
        assert!(c.active(at(1_000)) && !c.pending_decommission);
        // Carrier management showing no decommission cancels one we missed, too.
        decommission(&mut c, 0, 100);
        c.on_stats(&ev::CarrierStats { carrier_id: ID, ..Default::default() }, at(50));
        assert!(c.active(at(1_000)));
    }

    #[test]
    fn a_new_carrier_starts_clean() {
        let mut c = own();
        request(&mut c, 0);
        decommission(&mut c, 0, 100);
        c.jumps.push(CarrierJumpRecord { ts: at(0), system: "Alpha".into(), body: None });
        // The old one is scrapped; carrier management now shows another carrier.
        c.on_stats(&ev::CarrierStats { carrier_id: ID + 1, callsign: "NEW-001".into(), fuel_level: 800, ..Default::default() }, at(200));
        assert_eq!((c.id, c.callsign.as_str(), c.fuel), (Some(ID + 1), "NEW-001", 800));
        assert!(c.active(at(300)) && c.jumps.is_empty() && c.pending_jump.is_none() && c.decommission.is_none());
        // Same after a purchase.
        c.on_buy(&ev::CarrierBuy { carrier_id: ID + 2, callsign: "NEW-002".into(), location: "Gamma".into(), system_address: 300, ..Default::default() }, at(400));
        assert_eq!((c.id, c.fuel, c.system.as_deref()), (Some(ID + 2), 0, Some("Gamma")));
    }

    #[test]
    fn route_estimate_counts_the_running_timer_first() {
        let cycle = PREPARE_SECS + COOLDOWN_SECS;
        let mut c = own();
        assert_eq!(c.route_estimate(at(0), 0), 0);
        // Idle: three countdowns and the two cooldowns between them.
        assert_eq!(c.route_estimate(at(0), 3), 3 * PREPARE_SECS + 2 * COOLDOWN_SECS);
        // Counting down to the first of three jumps.
        request(&mut c, 0);
        assert_eq!(c.route_estimate(at(100), 3), PREPARE_SECS - 100 + 2 * cycle);
        // Cooling down after it: two jumps left.
        let a = ev::SystemArrival { star_system: "Beta".into(), system_address: 200, market_id: Some(ID), ..Default::default() };
        c.on_carrier_jump(&a, at(PREPARE_SECS));
        assert_eq!(c.route_estimate(at(PREPARE_SECS + 60), 2), COOLDOWN_SECS - 60 + 2 * PREPARE_SECS + COOLDOWN_SECS);
    }
}
