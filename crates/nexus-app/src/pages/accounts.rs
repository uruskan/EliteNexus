//! Website/service connections (EDDN, EDSM, Inara, EDAstro, Frontier CAPI). All opt-in. Laid out
//! like System Settings: grouped rows, switches on the right, details folded away.

use crate::secrets;
use crate::settings::Accounts;
use crate::theme::{pal, size};
use crate::widgets::{self, Group, ago, badge, cards, footnote, group, section};
use egui::{RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_uplink::{ServiceStatus, UplinkStatus};

/// What the services report, refreshed every frame while the page is open.
#[derive(Debug, Clone, Default)]
pub struct StatusView {
    /// One line about the incoming EDDN feed (market service).
    pub eddn_feed: Option<String>,
    pub uplink: UplinkStatus,
}

#[derive(Default)]
pub struct AccountsUi {
    edsm_key_input: String,
    inara_key_input: String,
    show_inara_options: bool,
    /// "Check again" was clicked (the app asks the uplink).
    pub check_inara: bool,
}

fn has_status(s: &ServiceStatus) -> bool {
    s.enabled || s.queued > 0 || s.sent > 0 || s.failed > 0 || s.paused.is_some() || s.last_error.is_some()
}

/// Sent / queued / failed counts, pause reason and last error, as a row of the group.
fn status_row(g: &mut Group, s: &ServiceStatus) {
    if !has_status(s) {
        return;
    }
    g.content(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            badge(ui, &format!("{} sent", s.sent), pal::GOOD);
            if s.queued > 0 {
                badge(ui, &format!("{} queued", s.queued), pal::CYAN);
            }
            if s.failed > 0 {
                badge(ui, &format!("{} failed", s.failed), pal::BAD);
            }
            if let Some(t) = s.last_success {
                ui.label(RichText::new(format!("Last accepted {}", ago(t))).size(size::FOOTNOTE).color(pal::DIM));
            }
        });
        if let Some(p) = &s.paused {
            ui.add(egui::Label::new(RichText::new(format!("{} Paused: {p}", icon::PAUSE_CIRCLE)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
        }
        if let Some(e) = &s.last_error {
            // An error the service has recovered from since is history: shown dimmed, with its age.
            let when = s.last_error_at.map(|t| format!(" ({})", ago(t))).unwrap_or_default();
            let (text, col) = if s.recovered { (format!("Earlier{when}, since recovered: {e}"), pal::DIM) } else { (format!("{e}{when}"), pal::BAD) };
            ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(col)).wrap());
        }
    });
}

/// An API key row: a masked field and Save (and Remove once a key is stored). Returns true when
/// the stored (sealed) value changed.
fn key_row(g: &mut Group, stored: &mut String, input: &mut String, hint: &str) -> bool {
    let mut changed = false;
    let detail = if stored.is_empty() { "Not set".to_string() } else { format!("Saved, {}", secrets::KEPT) };
    g.row("API key", &detail, |ui| {
        // Right to left: the buttons, then the field.
        if !stored.is_empty() && widgets::button(ui, "Remove").clicked() {
            stored.clear();
            changed = true;
        }
        let save = ui.add_enabled_ui(!input.trim().is_empty(), |ui| widgets::button(ui, "Save")).inner;
        let field = ui.add(egui::TextEdit::singleline(input).password(true).hint_text(if stored.is_empty() { hint } else { "Paste a new key to replace it" }).desired_width(200.0).margin(egui::Margin::symmetric(10, 6)));
        if (save.clicked() || field.lost_focus()) && !input.trim().is_empty() {
            *stored = secrets::seal(input.trim());
            input.clear();
            changed = true;
        }
    });
    changed
}

fn link_row(g: &mut Group, text: &str, url: &str) {
    g.content(|ui| {
        ui.hyperlink_to(format!("{text} {}", icon::ARROW_SQUARE_OUT), url);
    });
}

/// Returns true if settings changed.
pub fn show(ui: &mut Ui, acc: &mut Accounts, st: &mut AccountsUi, cmdr_name: &str, status: &StatusView) -> bool {
    let mut changed = false;
    footnote(ui, &format!("Connect your commander to the community services. Everything is off until you switch it on, keys are {}, and history is never uploaded: only what happens while Elite Nexus is running.", secrets::KEPT));
    ui.add_space(4.0);
    if acc.edsm_commander.is_empty() && !cmdr_name.is_empty() {
        acc.edsm_commander = cmdr_name.to_string();
        changed = true;
    }
    cards(ui, 2, 420.0, |c| {
        c.add(|ui| {
            section(ui, "EDDN · Elite Dangerous Data Network");
            group(ui, |g| {
                changed |= g.switch(&mut acc.eddn_listen, "Receive the live market feed", "Every commander's station data, in real time.");
                if let Some(f) = &status.eddn_feed {
                    g.content(|ui| {
                        ui.label(RichText::new(format!("{} {f}", icon::BROADCAST)).size(size::FOOTNOTE).color(pal::CYAN));
                    });
                }
                changed |= g.switch(
                    &mut acc.eddn_upload,
                    "Share my market, outfitting, shipyard and exploration data",
                    "The same data EDMC and EDDiscovery send: station prices and services, system and body scans, routes. No credits, cargo or personal data, and no name. It keeps Inara, Spansh, EDSM and everyone else up to date.",
                );
                if acc.eddn_upload {
                    changed |= g.switch(
                        &mut acc.eddn_paused,
                        "Pause sharing",
                        "Send nothing until you resume (also in the toolbar). What happens while paused is dropped, not sent later. Prices are only sent when you undock, so pausing while docked keeps a market you just found to yourself.",
                    );
                }
                changed |= g.switch(&mut acc.eddn_test_mode, "Test mode", "Send to EDDN's test schemas only.");
                status_row(g, &status.uplink.eddn);
            });
            section(ui, "EDAstro");
            group(ui, |g| {
                changed |= g.switch(&mut acc.edastro_enabled, "Send the events EDAstro asks for", "Carrier jumps and stats, docking and organic scans. Feeds EDAstro's galaxy maps and statistics. No key needed.");
                if acc.edastro_enabled {
                    changed |= g.switch(&mut acc.edastro_paused, "Pause sharing", "Send nothing until you resume (also in the toolbar). What happens while paused is dropped, not sent later.");
                }
                status_row(g, &status.uplink.edastro);
            });
            section(ui, "Frontier Companion API");
            group(ui, |g| {
                g.row(
                    "Fleet-carrier market, crew and remote shipyard data",
                    "Needs a client ID that Frontier approves for this app (Developer Zone at user.frontierstore.net). The journal already covers almost everything.",
                    |ui| {
                        badge(ui, "Not configured", pal::GRAY);
                    },
                );
            });
        });
        c.add(|ui| {
            section(ui, "EDSM · flight log and star map");
            group(ui, |g| {
                changed |= g.switch(&mut acc.edsm_enabled, "Sync my journal to EDSM", "");
                if acc.edsm_enabled {
                    changed |= g.switch(
                        &mut acc.edsm_paused,
                        "Pause sync",
                        "Send nothing until you resume (also in the toolbar). What happens while paused is dropped, not sent later, so your EDSM flight log has a gap for that time.",
                    );
                }
                g.row("Commander name", "", |ui| {
                    changed |= ui.add(egui::TextEdit::singleline(&mut acc.edsm_commander).desired_width(200.0).margin(egui::Margin::symmetric(10, 6))).changed();
                });
                changed |= key_row(g, &mut acc.edsm_api_key, &mut st.edsm_key_input, "EDSM API key");
                link_row(g, "Get your EDSM API key", "https://www.edsm.net/en/settings/api");
                status_row(g, &status.uplink.edsm);
            });
            section(ui, "Inara · commander profile");
            group(ui, |g| {
                changed |= g.switch(&mut acc.inara_enabled, "Sync my commander profile to Inara", "");
                if acc.inara_enabled {
                    changed |= g.switch(
                        &mut acc.inara_paused,
                        "Pause sync",
                        "Send nothing until you resume (also in the toolbar). What happens while paused is dropped, not sent later; when you resume, your profile is brought up to date as it is then.",
                    );
                }
                changed |= key_row(g, &mut acc.inara_api_key, &mut st.inara_key_input, "Inara API key");
                link_row(g, "Get your Inara API key", "https://inara.cz/elite/cmdr-settings-api/");
                // Proof that it works: who Inara says the key belongs to, with a link to the profile.
                let s = &status.uplink.inara;
                // Paused, nothing goes to Inara: not even the question of who the key belongs to.
                let paused = acc.inara_paused;
                if acc.inara_enabled && !acc.inara_api_key.is_empty() {
                    g.content(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            match (&s.account, s.checking) {
                                (Some(a), _) => {
                                    ui.label(RichText::new(format!("{} Connected as CMDR {}", icon::CHECK_CIRCLE, a.name)).color(pal::GOOD));
                                    if let Some(url) = &a.url {
                                        ui.hyperlink_to(format!("View your Inara profile {}", icon::ARROW_SQUARE_OUT), url);
                                    }
                                }
                                (None, true) => {
                                    ui.spinner();
                                    ui.label(RichText::new("Checking the key with Inara…").color(pal::DIM));
                                }
                                (None, false) => {
                                    ui.label(RichText::new(if paused { "Not confirmed yet: sync is paused" } else { "Not confirmed yet" }).color(pal::DIM));
                                }
                            }
                            if !s.checking
                                && !paused
                                && widgets::button(ui, "Check again").on_hover_text("Asks Inara who this key belongs to (counts towards its limit of about 2 requests a minute)").clicked()
                            {
                                st.check_inara = true;
                            }
                        });
                        ui.add(
                            egui::Label::new(
                                RichText::new("Your profile (credits, ranks, reputation, ship and loadout, materials, cargo, location) is sent when sync starts, then updated live as you play.")
                                    .size(size::FOOTNOTE)
                                    .color(pal::DIM),
                            )
                            .wrap(),
                        );
                    });
                }
                // Inara approved "Elite Nexus" on 2026-10-13; should it ever refuse the app again, say so plainly.
                let refused = s.last_error.as_deref().is_some_and(|e| e.to_ascii_lowercase().contains("no access allowed"));
                if refused {
                    g.content(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{} Inara answered \"no access allowed\": it isn't accepting requests from \"Elite Nexus\" right now, so sync is paused. Inara approved the app in October 2026, so this is unexpected: check Inara for news, or ask its author.",
                                    icon::WARNING
                                ))
                                .color(pal::WARN),
                            )
                            .wrap(),
                        );
                        ui.hyperlink_to(format!("Contact Inara {}", icon::ARROW_SQUARE_OUT), "https://inara.cz/elite/cmdr/1/");
                    });
                }
                status_row(g, s);
            });
            if !matches!(status.uplink.inara.last_error.as_deref(), Some(e) if e.to_ascii_lowercase().contains("no access allowed")) {
                footnote(ui, "Inara accepts uploads only from apps it has approved by name; it approved \"Elite Nexus\" in October 2026.");
            }
            // Hidden depth.
            ui.add_space(6.0);
            if widgets::disclosure(ui, &mut st.show_inara_options, "Inara options") {
                group(ui, |g| {
                    changed |= g.switch(
                        &mut acc.inara_dev_mode,
                        "Developer mode",
                        "Sent to Inara as isBeingDeveloped: Inara then treats your uploads as an app being built and ignores the data they share with everyone, such as community goals. Leave it off unless you're testing with a test account. Updates are batched every 35+ seconds, so the key is never locked out.",
                    );
                });
            }
        });
    });
    changed
}
