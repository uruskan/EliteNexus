use crate::theme::{display, pal, size};
use crate::widgets::{GAP, badge, bar, card_tinted, cards, credits, duration, kv, short_credits};
use egui::{Grid, RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_data::ranks::RankKind;
use nexus_journal::Timestamp;
use nexus_model::carrier::CarrierKind;
use nexus_model::GameState;

/// `hidden_depots`: construction depots hidden on the Colonisation page.
pub fn show(ui: &mut Ui, gs: &GameState, hidden_depots: &[u64]) {
    cards(ui, 3, 300.0, |c| {
        c.add(|ui| commander(ui, gs));
        c.add(|ui| ship(ui, gs));
        c.add(|ui| location(ui, gs));
        c.add(|ui| session(ui, gs));
        c.add(|ui| system(ui, gs));
        c.add(|ui| holdings(ui, gs, hidden_depots));
    });
    ui.add_space(GAP);
    feed(ui, gs);
}

/// Headline figure of a card.
fn hero(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(display(size::TITLE)).color(pal::TEXT)).wrap());
}

fn sub(ui: &mut Ui, text: impl Into<String>, color: egui::Color32) {
    ui.add(egui::Label::new(RichText::new(text.into()).size(size::CALLOUT).color(color)).wrap());
}

fn commander(ui: &mut Ui, gs: &GameState) {
    let c = &gs.cmdr;
    card_tinted(ui, icon::USER_CIRCLE, "Commander", pal::ACCENT, |ui| {
        hero(ui, &format!("CMDR {}", c.name));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(credits(c.credits)).font(display(size::HEADLINE)).color(pal::GOLD));
            if let Some(l) = &gs.legal_state {
                badge(ui, l, if l == "Clean" { pal::GOOD } else { pal::BAD });
            }
            if !c.game_mode.is_empty() {
                badge(ui, &c.game_mode, pal::CYAN);
            }
        });
        ui.add_space(8.0);
        Grid::new("ranks").max_col_width(ui.available_width()).num_columns(3).spacing([12.0, 7.0]).show(ui, |ui| {
            for k in [RankKind::Combat, RankKind::Trade, RankKind::Explore, RankKind::Soldier, RankKind::Exobiologist, RankKind::Empire, RankKind::Federation] {
                ui.label(RichText::new(k.label()).color(pal::DIM));
                ui.label(RichText::new(c.rank_name(k)).color(pal::TEXT));
                ui.scope(|ui| {
                    ui.set_width(72.0);
                    ui.add_space(6.0);
                    match c.rank_progress(k) {
                        Some(p) => bar(ui, p as f32 / 100.0, pal::ACCENT, 5.0, None).on_hover_text(format!("{p}% to the next rank")),
                        // Promoted this session: the game reports progress again at the next login.
                        None => bar(ui, 0.0, pal::FAINT, 5.0, None).on_hover_text("Promoted since the game last reported your progress: it does again when you next log in."),
                    };
                });
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        let unlocked = c.unlocked_engineers().count();
        sub(ui, format!("{} {} engineers unlocked", icon::WRENCH, unlocked), pal::DIM);
        if let Some(sq) = &c.squadron {
            sub(ui, format!("{} Squadron: {}", icon::USERS_THREE, sq.name), pal::DIM);
        } else if !c.squadron_applications.is_empty() {
            sub(ui, format!("{} Applied to {}", icon::USERS_THREE, c.squadron_applications.join(", ")), pal::DIM);
        }
        if let Some(pp) = &c.powerplay {
            sub(ui, format!("{} {} · rank {} · {} merits", icon::CROWN, pp.power, pp.rank, pp.merits), pal::PURPLE);
        }
    });
}

fn ship(ui: &mut Ui, gs: &GameState) {
    card_tinted(ui, icon::ROCKET_LAUNCH, "Ship", pal::BLUE, |ui| {
        let Some(s) = gs.fleet.current() else {
            sub(ui, "No ship information yet", pal::DIM);
            return;
        };
        let name = if s.name.trim().is_empty() { s.type_name.clone() } else { s.name.clone() };
        hero(ui, &name);
        sub(ui, format!("{} · {}", s.type_name, s.ident), pal::DIM);
        ui.add_space(8.0);
        let fuel_cap = s.fuel_main_capacity.max(0.001);
        let fuel = gs.fleet.fuel_main;
        let fuel_frac = (fuel / fuel_cap) as f32;
        let fuel_col = if fuel_frac < 0.25 { pal::BAD } else if fuel_frac < 0.5 { pal::WARN } else { pal::CYAN };
        let used = gs.inventory.cargo.total();
        let cap = s.cargo_capacity.max(1) as f32;
        let hc = if s.hull_health < 0.5 { pal::BAD } else { pal::GOOD };
        for (label, frac, col, text) in [
            ("Fuel", fuel_frac, fuel_col, format!("{fuel:.1} / {:.0} t", s.fuel_main_capacity)),
            ("Hull", s.hull_health as f32, hc, format!("{:.0}%", s.hull_health * 100.0)),
            ("Cargo", used as f32 / cap, pal::ACCENT, format!("{used} / {} t", s.cargo_capacity)),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).color(pal::DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(text).size(size::FOOTNOTE).color(pal::TEXT));
                });
            });
            ui.add_space(-4.0);
            bar(ui, frac, col, 6.0, None);
            ui.add_space(4.0);
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            crate::widgets::stat(ui, "max jump", format!("{:.1} ly", s.max_jump_range), pal::ACCENT);
            crate::widgets::stat(ui, "rebuy", short_credits(s.rebuy), pal::TEXT);
            crate::widgets::stat(ui, "value", short_credits(s.total_value()), pal::TEXT);
        });
        sub(ui, format!("{} engineered modules", s.engineered_modules()), pal::DIM);
    });
}

fn location(ui: &mut Ui, gs: &GameState) {
    let l = &gs.location;
    card_tinted(ui, icon::MAP_PIN, "Location", pal::TEAL, |ui| {
        hero(ui, if l.system.is_empty() { "Unknown" } else { &l.system });
        sub(ui, l.situation(), pal::ACCENT_SOFT);
        if let Some(b) = &l.body
            && !b.is_empty()
            && l.station.as_ref().is_none_or(|s| !l.docked || (s.name != *b && s.journal_name != *b))
        {
            sub(ui, format!("{} {}", icon::PLANET, b), pal::DIM);
        }
        ui.add_space(8.0);
        let i = &l.info;
        Grid::new("locgrid").max_col_width(ui.available_width()).num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            if i.population > 0 {
                kv(ui, "Population", nexus_data::names::compact_credits(i.population as i64));
                kv(ui, "Allegiance", i.allegiance.clone());
                kv(ui, "Government", i.government.clone());
                kv(ui, "Economy", if i.second_economy.is_empty() || i.second_economy == "None" { i.economy.clone() } else { format!("{} / {}", i.economy, i.second_economy) });
                kv(ui, "Security", i.security.clone());
                if let Some(f) = &i.controlling_faction {
                    kv(ui, "Controlled by", f.clone());
                }
            } else if !l.system.is_empty() {
                kv(ui, "Population", "Unpopulated");
            }
            if let Some(p) = &i.controlling_power {
                kv(ui, "Power", format!("{} ({})", p, i.powerplay_state.clone().unwrap_or_default()));
            }
            if let Some(pos) = l.star_pos {
                kv(ui, "From Sol", format!("{:.1} ly", nexus_model::location::distance(pos, [0.0, 0.0, 0.0])));
            }
        });
        if let Some(st) = l.station.as_ref().filter(|_| l.docked) {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                badge(ui, &st.station_type, pal::CYAN);
                if let Some(p) = &st.pads {
                    badge(ui, &format!("Pads L{} M{} S{}", p.large, p.medium, p.small), pal::GRAY);
                }
                for (svc, label) in [("shipyard", "Shipyard"), ("outfitting", "Outfitting"), ("commodities", "Market"), ("engineer", "Engineer"), ("vistagenomics", "Vista Genomics"), ("exploration", "Cartographics")] {
                    if st.has_service(svc) {
                        badge(ui, label, pal::GOOD);
                    }
                }
            });
        }
        if let Some(t) = &l.fsd_target {
            ui.add_space(6.0);
            sub(ui, format!("{} Target: {} ({}){}", icon::CROSSHAIR, t.name, t.star_class, t.remaining_jumps.map(|j| format!(" · {j} jumps left")).unwrap_or_default()), pal::CYAN);
        }
        let route = gs.route_remaining();
        if let Some(dest) = route.last() {
            let jumps = if route.len() == 1 { "1 jump".to_string() } else { format!("{} jumps", route.len()) };
            sub(ui, format!("{} Route: {jumps} to {}", icon::PATH, dest.star_system), pal::DIM);
        }
    });
}

fn session(ui: &mut Ui, gs: &GameState) {
    let s = &gs.session;
    card_tinted(ui, icon::TIMER, "This session", pal::PURPLE, |ui| {
        let now = Timestamp::now();
        let dur = if s.active() { s.duration_secs(now) } else { s.duration_secs(s.end.unwrap_or(gs.last_event)) };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            crate::widgets::stat(ui, "time", duration(dur), pal::TEXT);
            crate::widgets::stat(ui, "jumps", s.jumps.to_string(), pal::TEXT);
            crate::widgets::stat(ui, "light years", format!("{:.0}", s.distance_ly), pal::TEXT);
        });
        ui.add_space(8.0);
        Grid::new("sessgrid").max_col_width(ui.available_width()).num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            kv(ui, "Bodies scanned / mapped", format!("{} / {}", s.bodies_scanned, s.bodies_mapped));
            kv(ui, "Organics analysed", s.organics_analysed.to_string());
            kv(ui, "Missions completed", s.missions_completed.to_string());
            kv(ui, "Income", RichText::new(short_credits(s.income)).color(pal::GOOD));
            kv(ui, "Spent", RichText::new(short_credits(-s.expenses)).color(pal::BAD));
            let delta = gs.cmdr.credits - s.start_credits;
            kv(ui, "Balance change", RichText::new(short_credits(delta)).color(if delta >= 0 { pal::GOOD } else { pal::BAD }));
        });
    });
}

fn system(ui: &mut Ui, gs: &GameState) {
    card_tinted(ui, icon::PLANET, "Current system", pal::MINT, |ui| {
        let Some(sys) = gs.exploration.current_system() else {
            sub(ui, "No scan data for this system yet.", pal::DIM);
            return;
        };
        let scanned = sys.scanned_bodies();
        let total = sys.fss_body_count.unwrap_or(0) as usize;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            crate::widgets::stat(ui, "bodies scanned", if total > 0 { format!("{scanned}/{total}") } else { scanned.to_string() }, pal::TEXT);
            crate::widgets::stat(ui, "value now", short_credits(sys.value()), pal::GOLD);
            crate::widgets::stat(ui, "if mapped", short_credits(sys.potential_value()), pal::ACCENT);
        });
        if total > 0 {
            ui.add_space(4.0);
            bar(ui, scanned as f32 / total as f32, pal::MINT, 6.0, None);
        }
        let bio = sys.bio_signal_total();
        if bio > 0 {
            ui.add_space(4.0);
            sub(ui, format!("{} {} biological signals", icon::PLANT, bio), pal::GOOD);
        }
        let mut notable: Vec<_> = sys.bodies.values().filter(|b| b.has_scan).collect();
        notable.sort_by_key(|b| std::cmp::Reverse(b.value.mapped));
        for b in notable.iter().take(4) {
            let tags = nexus_model::exploration::interesting(b, sys);
            if tags.is_empty() && b.value.mapped < 300_000 {
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(b.short_name(&sys.name)).color(pal::TEXT));
                ui.label(RichText::new(short_credits(b.value.mapped)).color(pal::GOLD));
                for t in tags.iter().take(2) {
                    badge(ui, t.label(), pal::ACCENT);
                }
            });
        }
        ui.add_space(6.0);
        sub(ui, format!("Unsold cartographic data: {}", short_credits(gs.exploration.unsold_value())), pal::DIM);
        sub(ui, format!("Unsold exobiology: {}", short_credits(gs.exobio.unsold_value())), pal::DIM);
    });
}

fn holdings(ui: &mut Ui, gs: &GameState, hidden_depots: &[u64]) {
    card_tinted(ui, icon::VAULT, "Holdings", pal::GOLD, |ui| {
        Grid::new("holdgrid").max_col_width(ui.available_width()).num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            kv(ui, "Ships owned", format!("{} ({})", gs.fleet.ships.len(), short_credits(gs.fleet.total_value())));
            let m = &gs.inventory.materials;
            kv(
                ui,
                "Materials",
                format!(
                    "{} raw · {} mfd · {} enc",
                    m.total(nexus_model::inventory::MaterialCategory::Raw),
                    m.total(nexus_model::inventory::MaterialCategory::Manufactured),
                    m.total(nexus_model::inventory::MaterialCategory::Encoded)
                ),
            );
            kv(ui, "Ship locker", format!("{} items", gs.inventory.ship_locker.total()));
            kv(ui, "Active missions", gs.missions.active.len().to_string());
        });
        // The commander's own carrier and their squadron's; a scrapped one isn't a holding any more.
        let now = Timestamp::now();
        for (kind, c) in gs.carriers(now) {
            ui.add_space(10.0);
            let glyph = if kind == CarrierKind::Squadron { icon::USERS_THREE } else { icon::ANCHOR };
            ui.label(RichText::new(format!("{glyph} {}", crate::pages::carrier::carrier_name(kind, c))).font(display(size::HEADLINE)).color(pal::TEXT));
            let what = if kind == CarrierKind::Squadron { "Squadron carrier · " } else { "" };
            sub(ui, format!("{what}{} · {} · {} t tritium · {}", c.callsign, c.system.clone().unwrap_or_else(|| "?".into()), c.fuel, short_credits(c.finance.carrier_balance)), pal::DIM);
            if let Some(line) = crate::pages::carrier::decommission_line(c) {
                sub(ui, line, pal::BAD);
            }
            if let Some((line, colr)) = crate::pages::carrier::jump_timer_line(c, now) {
                sub(ui, line, colr);
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            }
        }
        for d in gs.colonisation.active_depots(nexus_journal::Timestamp::now()).filter(|d| !hidden_depots.contains(&d.market_id)).take(2) {
            ui.add_space(6.0);
            sub(ui, format!("{} {} · {:.0}%", icon::BUILDINGS, d.station.clone().unwrap_or_else(|| d.system.clone()), d.progress * 100.0), pal::TEXT);
            bar(ui, d.progress as f32, pal::GOOD, 6.0, None);
        }
    });
}

fn feed(ui: &mut Ui, gs: &GameState) {
    card_tinted(ui, icon::PULSE, "Activity", pal::PINK, |ui| {
        if gs.feed.is_empty() {
            sub(ui, "Nothing yet. Events appear here as you play.", pal::DIM);
            return;
        }
        ui.add_space(4.0);
        for (i, f) in gs.feed.iter().rev().take(40).enumerate() {
            if i > 0 {
                let r = ui.available_rect_before_wrap();
                ui.painter().hline(r.x_range(), r.top() - 3.0, egui::Stroke::new(1.0, pal::SEPARATOR));
            }
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                ui.add_sized([104.0, 20.0], egui::Label::new(RichText::new(f.ts.to_local_string("%d %b %H:%M")).size(size::FOOTNOTE).color(pal::DIM)));
                ui.add_sized([150.0, 20.0], egui::Label::new(RichText::new(&f.event).size(size::CALLOUT).color(pal::ACCENT_SOFT)).truncate());
                ui.add(egui::Label::new(RichText::new(&f.text).color(pal::TEXT)).wrap());
            });
        }
    });
}
