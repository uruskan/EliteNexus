use crate::theme::{display, pal, size};
use crate::widgets::{ago, badge, bar, card, duration, empty, kv, short_credits, GAP};
use egui::{Color32, Grid, RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::carrier::{timing, Carrier, CarrierKind, JumpTimer};
use nexus_model::GameState;
use std::time::Duration;

/// Where the timings come from, for the timer's tooltip.
const TIMING_SOURCES: &str = "The departure time comes from the game's journal (CarrierJumpRequest) and already includes any wait for busy servers.\n\nThe 5-minute cooldown after a jump is from Frontier's patch notes when carriers launched (update 1.43, June 2020). The journal doesn't record it, and no later change was found.\n\nThe pre-flight lockdown (3 min 20 s before departure) and the cancel cutoff (10 minutes before) come from the community wiki, not Frontier, so take them as a guide.";

/// Why a squadron carrier's jumps can be missing: the journal only has the commander's own requests.
const SQUADRON_JUMPS: &str = "The journal records only the jumps you schedule, not other squadron members': the carrier's place updates when the game next reports it (CarrierLocation), or when you're aboard as it jumps.";

/// A personal carrier holds up to 1,000 t of tritium in its tank.
const PERSONAL_TANK_T: f32 = 1000.0;

/// The carrier to show, of those the commander has now: the one picked, else the first.
pub fn pick(gs: &GameState, choice: CarrierKind, now: Timestamp) -> Option<CarrierKind> {
    let live = gs.carriers(now);
    live.iter().find(|(k, _)| *k == choice).or(live.first()).map(|(k, _)| *k)
}

/// The carrier's name, else its callsign, else what it is.
pub fn carrier_name(kind: CarrierKind, c: &Carrier) -> String {
    [&c.name, &c.callsign].into_iter().find(|s| !s.is_empty()).cloned().unwrap_or_else(|| kind.label().to_string())
}

/// Picks one of the carriers when the commander has both (a squadron carrier and their own).
/// Returns true when the choice changed.
pub fn carrier_switch(ui: &mut Ui, gs: &GameState, choice: &mut CarrierKind, now: Timestamp) -> bool {
    let live = gs.carriers(now);
    if live.len() < 2 {
        return false;
    }
    let labels: Vec<(CarrierKind, String)> = live
        .iter()
        .map(|(k, c)| (*k, format!("{} {}", if *k == CarrierKind::Squadron { icon::USERS_THREE } else { icon::ANCHOR }, carrier_name(*k, c))))
        .collect();
    let options: Vec<(CarrierKind, &str)> = labels.iter().map(|(k, l)| (*k, l.as_str())).collect();
    let mut shown = pick(gs, *choice, now).unwrap_or_default();
    let changed = crate::widgets::segmented(ui, &mut shown, &options);
    if changed {
        *choice = shown;
    }
    changed
}

/// The jump countdown or cooldown: what's happening, time left, where, and a progress bar.
/// Repaints every second while a timer runs.
pub fn jump_timer(ui: &mut Ui, kind: CarrierKind, c: &Carrier) {
    let now = Timestamp::now();
    let squadron = kind == CarrierKind::Squadron;
    let Some(t) = c.jump_timer(now) else {
        badge(ui, "READY", pal::GOOD);
        ui.label(RichText::new(if squadron { "No jump you scheduled" } else { "No jump scheduled" }).font(display(size::TITLE)).color(pal::TEXT));
        if squadron {
            note(ui, SQUADRON_JUMPS);
        } else {
            note(ui, "Schedule a jump in the carrier management screen: the countdown, and the cooldown after the jump, show here.");
        }
        return;
    };
    ui.ctx().request_repaint_after(Duration::from_secs(1));
    let left = t.left(now);
    match &t {
        JumpTimer::Countdown { to, body, departs } => {
            let lockdown = left.is_some_and(|l| l <= timing::LOCKDOWN_SECS);
            let (phase, colr) = if lockdown { ("LOCKDOWN", pal::BAD) } else { ("JUMP COUNTDOWN", pal::WARN) };
            badge(ui, phase, colr);
            let big = left.map(duration).unwrap_or_else(|| "Scheduled".into());
            ui.label(RichText::new(big).font(display(size::TITLE)).color(colr)).on_hover_text(TIMING_SOURCES);
            destination(ui, icon::ARROW_FAT_RIGHT, to, body.as_deref());
            let detail = match (departs, left) {
                (Some(d), Some(l)) if l > timing::CANCEL_CUTOFF_SECS => {
                    format!("Departs {} · can be cancelled until about {}", local(*d), local(Timestamp(d.0 - timing::CANCEL_CUTOFF_SECS)))
                }
                (Some(d), Some(l)) if l > timing::LOCKDOWN_SECS => {
                    format!("Departs {} · too late to cancel · lockdown from about {}", local(*d), local(Timestamp(d.0 - timing::LOCKDOWN_SECS)))
                }
                (Some(d), _) => format!("Departs {} · pre-flight lockdown: pads closed, services stopped", local(*d)),
                (None, _) => "The journal didn't give a departure time.".into(),
            };
            note(ui, &detail);
            if squadron {
                note(ui, "You scheduled this jump: it's in your journal.");
            }
            if let (Some(d), Some(p)) = (departs, &c.pending_jump) {
                let total = (d.0 - p.requested.0).max(1) as f32;
                bar(ui, (now.0 - p.requested.0) as f32 / total, colr, 6.0, None);
            }
        }
        JumpTimer::Cooldown { at, body, ready, estimated } => {
            badge(ui, "COOLDOWN", pal::CYAN);
            let big = left.map(duration).unwrap_or_default();
            ui.label(RichText::new(if *estimated { format!("≈ {big}") } else { big }).font(display(size::TITLE)).color(pal::CYAN)).on_hover_text(TIMING_SOURCES);
            destination(ui, icon::MAP_PIN, at, body.as_deref());
            note(ui, &format!("Can jump again from about {}", local(*ready)));
            if *estimated {
                note(ui, "Counted from the departure: the journal records the arrival only when you're aboard, so the cooldown ends a little later, by however long the jump took.");
            }
            bar(ui, 1.0 - left.unwrap_or(0) as f32 / timing::COOLDOWN_SECS as f32, pal::CYAN, 6.0, None);
        }
    }
}

/// One line for compact places (the Dashboard): the timer's phase and time left.
pub fn jump_timer_line(c: &Carrier, now: Timestamp) -> Option<(String, Color32)> {
    let t = c.jump_timer(now)?;
    let left = t.left(now);
    Some(match &t {
        JumpTimer::Countdown { to, .. } => match left {
            Some(l) if l <= timing::LOCKDOWN_SECS => (format!("{} Jumping to {to} in {} (lockdown)", icon::ARROW_FAT_RIGHT, duration(l)), pal::BAD),
            Some(l) => (format!("{} Jumping to {to} in {}", icon::ARROW_FAT_RIGHT, duration(l)), pal::WARN),
            None => (format!("{} Jump to {to} scheduled", icon::ARROW_FAT_RIGHT), pal::WARN),
        },
        JumpTimer::Cooldown { at, estimated, .. } => {
            let approx = if *estimated { "≈ " } else { "" };
            (format!("{} Cooldown in {at}: can jump again in {approx}{}", icon::TIMER, duration(left.unwrap_or(0))), pal::CYAN)
        }
    })
}

/// A decommission under way, for compact places: when the carrier is scrapped.
pub fn decommission_line(c: &Carrier) -> Option<String> {
    match c.decommission {
        Some(d) => Some(format!("{} Decommissioning: scrapped {} unless cancelled", icon::WARNING, date(d.scrap_time))),
        None => c.pending_decommission.then(|| format!("{} Decommission pending", icon::WARNING)),
    }
}

fn destination(ui: &mut Ui, glyph: &str, system: &str, body: Option<&str>) {
    let text = match body.filter(|b| !b.is_empty() && !b.eq_ignore_ascii_case(system)) {
        Some(b) => format!("{glyph} {system} · {b}"),
        None => format!("{glyph} {system}"),
    };
    if ui.link(RichText::new(text).color(pal::TEXT)).on_hover_text("Click to copy the system name").clicked() {
        ui.ctx().copy_text(system.to_string());
    }
}

fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(pal::DIM)).wrap());
}

fn local(t: Timestamp) -> String {
    t.to_local_string("%H:%M:%S")
}

fn date(t: Timestamp) -> String {
    t.to_local_string("%d %b %Y %H:%M")
}

/// The commander's own carrier was scrapped: say so, and what it paid back.
fn scrapped(ui: &mut Ui, c: &Carrier, alone: bool) {
    let name = carrier_name(CarrierKind::Personal, c);
    let callsign = if c.callsign.is_empty() || c.callsign == name { String::new() } else { format!(" ({})", c.callsign) };
    let (when, refund) = c.decommission.map(|d| (date(d.scrap_time), short_credits(d.refund))).unwrap_or_default();
    let text = format!("Your fleet carrier {name}{callsign} was decommissioned on {when}, with a refund of {refund}.");
    if alone {
        empty(
            ui,
            icon::ANCHOR,
            "Fleet carrier decommissioned",
            &format!("{text} If your squadron has a carrier, it shows here once the game writes its data: open its carrier management screen."),
        );
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(icon::INFO).color(pal::DIM));
            ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        });
        ui.add_space(GAP);
    }
}

/// What the Fleet carrier page asks of the Navigation page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierAction {
    /// A route for the ship to this system.
    RouteShipTo(String),
    /// Navigation's fleet carrier planner: the carrier's jumps and the nearest Fleet Carrier Administration.
    Plan,
}

/// Returns true when the carrier picked changed (it's kept in the settings), and what was asked of Navigation.
pub fn show(ui: &mut Ui, gs: &GameState, choice: &mut CarrierKind) -> (bool, Option<CarrierAction>) {
    let now = Timestamp::now();
    let live = gs.carriers(now);
    let gone = gs.carrier.owned() && gs.carrier.decommissioned(now);
    if gone {
        scrapped(ui, &gs.carrier, live.is_empty());
    }
    let Some(kind) = pick(gs, *choice, now) else {
        if !gone {
            empty(
                ui,
                icon::ANCHOR,
                "No fleet carrier",
                "Carrier data appears when the game writes CarrierStats: open the carrier management screen. That goes for your squadron's carrier too.",
            );
        }
        return (false, None);
    };
    let changed = carrier_switch(ui, gs, choice, now);
    if live.len() > 1 {
        ui.add_space(GAP);
    }
    let action = carrier_view(ui, gs, kind, gs.carrier_of(kind));
    (changed, action)
}

fn carrier_view(ui: &mut Ui, gs: &GameState, kind: CarrierKind, c: &Carrier) -> Option<CarrierAction> {
    let squadron = kind == CarrierKind::Squadron;
    let stats = !c.stats_updated.is_zero();
    let mut action = None;
    crate::widgets::cards(ui, 2, 300.0, |col| {
        col.add(|ui| {
            card(ui, kind.label(), |ui| {
                ui.label(RichText::new(carrier_name(kind, c)).font(display(24.0)));
                if !c.callsign.is_empty() && c.callsign != c.name {
                    ui.label(RichText::new(&c.callsign).font(display(16.0)).color(pal::ACCENT));
                }
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{} {}", icon::MAP_PIN, c.system.clone().unwrap_or_else(|| "Unknown".into()))).color(pal::TEXT));
                ui.horizontal_wrapped(|ui| {
                    if let Some(sq) = gs.cmdr.squadron.as_ref().filter(|_| squadron) {
                        badge(ui, &sq.name, pal::PURPLE);
                    }
                    if !c.docking_access.is_empty() {
                        badge(ui, &format!("Docking: {}", c.docking_access), pal::CYAN);
                    }
                    if c.allow_notorious {
                        badge(ui, "Notorious allowed", pal::WARN);
                    }
                    if c.pending_decommission || c.decommission.is_some() {
                        badge(ui, "DECOMMISSIONING", pal::BAD);
                    }
                });
                match c.decommission {
                    Some(d) => note(ui, &format!("Scrapped on {} unless the decommission is cancelled first · refund {}", date(d.scrap_time), short_credits(d.refund))),
                    None if c.pending_decommission => note(ui, "Carrier management reported a decommission, but the journal didn't record when it completes."),
                    None => {}
                }
                ui.add_space(2.0);
                if stats {
                    let more = if squadron { " Changes other members make show the next time you open it." } else { "" };
                    note(ui, &format!("From carrier management, {}.{more}", ago(c.stats_updated)));
                }
                // Getting back to it, or to where carriers are administered.
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    if let Some(sys) = &c.system {
                        let here = c.system_address.map_or(sys.eq_ignore_ascii_case(&gs.location.system), |a| a == gs.location.system_address);
                        if !here
                            && crate::widgets::primary_button(ui, &format!("{} Route my ship here", icon::NAVIGATION_ARROW))
                                .on_hover_text(format!("Navigation plots a route from {} to {sys} for your ship: every jump with the exact plotter, or with Neutron while your ship's loadout isn't known.", gs.location.system))
                                .clicked()
                        {
                            action = Some(CarrierAction::RouteShipTo(sys.clone()));
                        }
                    }
                    if crate::widgets::button(ui, &format!("{} Nearest carrier administration", icon::BUILDINGS))
                        .on_hover_text("Navigation › Fleet carrier lists the stations with Fleet Carrier Administration nearest the carrier or you, with a route there.")
                        .clicked()
                    {
                        action = Some(CarrierAction::Plan);
                    }
                });
            });
        });
        col.add(|ui| {
            card(ui, "Jump", |ui| jump_timer(ui, kind, c));
        });
    });
    ui.add_space(8.0);
    if !stats {
        empty(
            ui,
            icon::GAUGE,
            "No carrier management data yet",
            "Tritium, finances, capacity and services come from CarrierStats, which the game writes when you open the carrier management screen.",
        );
        return action;
    }
    crate::widgets::cards(ui, 2, 300.0, |col| {
        col.add(|ui| {
            card(ui, "Tritium", |ui| {
                ui.label(RichText::new(format!("{} t", c.fuel)).font(display(24.0)).color(pal::CYAN));
                if !squadron {
                    bar(ui, c.fuel as f32 / PERSONAL_TANK_T, pal::CYAN, 10.0, None);
                }
                ui.label(RichText::new(format!("Jump range {:.0} / {:.0} ly", c.jump_range_curr, c.jump_range_max)).color(pal::DIM));
                if ui
                    .link(RichText::new(format!("{} Plan jumps with this tritium and cargo", icon::PATH)).size(size::FOOTNOTE))
                    .on_hover_text("Navigation › Fleet carrier plans the carrier's jumps (Spansh).")
                    .clicked()
                {
                    action = Some(CarrierAction::Plan);
                }
            });
        });
        col.add(|ui| {
            card(ui, "Finance", |ui| {
                ui.label(RichText::new(short_credits(c.finance.carrier_balance)).font(display(24.0)).color(pal::GOLD));
                Grid::new("cfin").max_col_width(ui.available_width()).num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
                    kv(ui, "Available", short_credits(c.finance.available_balance));
                    kv(ui, "Reserved", short_credits(c.finance.reserve_balance));
                    if let Some(p) = c.finance.reserve_percent {
                        kv(ui, "Reserve %", format!("{p:.0}%"));
                    }
                });
            });
        });
    });
    ui.add_space(8.0);
    crate::widgets::cards(ui, 2, 360.0, |col| {
        col.add(|ui| {
            card(ui, "Capacity", |ui| {
                let s = &c.space;
                let total = s.total_capacity.max(1) as f32;
                Grid::new("cspace").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    for (label, v, colr) in [
                        ("Cargo", s.cargo, pal::ACCENT),
                        ("Reserved for orders", s.cargo_space_reserved, pal::WARN),
                        ("Crew", s.crew, pal::CYAN),
                        ("Ship packs", s.ship_packs, pal::PURPLE),
                        ("Module packs", s.module_packs, pal::PURPLE),
                        ("Free", s.free_space, pal::GOOD),
                    ] {
                        ui.label(RichText::new(label).color(pal::DIM));
                        bar(ui, v as f32 / total, colr, 12.0, Some(&format!("{v} t")));
                        ui.end_row();
                    }
                });
            });
        });
        col.add(|ui| {
            card(ui, "Crew & services", |ui| {
                if c.crew.is_empty() {
                    ui.label(RichText::new("No services recorded.").color(pal::DIM));
                }
                Grid::new("ccrew").max_col_width(ui.available_width()).num_columns(3).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                    for m in &c.crew {
                        ui.label(nexus_data::names::split_camel(&m.crew_role));
                        let (t, colr) = if !m.activated {
                            ("inactive", pal::FAINT)
                        } else if m.enabled == Some(false) {
                            ("suspended", pal::WARN)
                        } else {
                            ("active", pal::GOOD)
                        };
                        badge(ui, t, colr);
                        ui.label(RichText::new(m.crew_name.clone().unwrap_or_default()).color(pal::DIM));
                        ui.end_row();
                    }
                });
            });
        });
    });
    ui.add_space(8.0);
    crate::widgets::cards(ui, 2, 360.0, |col| {
        col.add(|ui| {
            card(ui, "Market orders", |ui| {
                if c.trade_orders.is_empty() {
                    ui.label(RichText::new(if squadron { "No orders you set." } else { "No orders recorded." }).color(pal::DIM));
                }
                Grid::new("corders").num_columns(3).striped(true).show(ui, |ui| {
                    for o in c.trade_orders.values() {
                        ui.label(&o.commodity);
                        if let Some(p) = o.purchase {
                            ui.label(RichText::new(format!("BUY {p}")).color(pal::GOOD));
                        } else if let Some(s) = o.sale {
                            ui.label(RichText::new(format!("SELL {s}")).color(pal::ACCENT));
                        } else {
                            ui.label("");
                        }
                        ui.label(RichText::new(o.price.map(|p| format!("{} Cr", nexus_data::names::group_digits(p))).unwrap_or_default()).color(pal::GOLD));
                        ui.end_row();
                    }
                });
                if squadron {
                    note(ui, "Only orders you set are in your journal; other members' aren't.");
                }
            });
        });
        col.add(|ui| {
            card(ui, "Jump history", |ui| {
                if c.jumps.is_empty() {
                    ui.label(RichText::new("No jumps recorded yet.").color(pal::DIM));
                }
                Grid::new("cjumps").max_col_width(ui.available_width()).num_columns(2).striped(true).show(ui, |ui| {
                    for j in c.jumps.iter().rev().take(30) {
                        ui.label(RichText::new(j.ts.to_local_string("%d %b %H:%M")).color(pal::FAINT));
                        ui.label(&j.system);
                        ui.end_row();
                    }
                });
                if squadron {
                    note(ui, "Jumps seen in your journal: ones you scheduled or rode along, and moves the game reported.");
                }
            });
        });
    });
    action
}
