//! "Ask": the built-in assistant. One question box; setup is a single pasted key (other providers
//! and the model choice are tucked away under a disclosure).

use crate::assistant::AssistantService;
use crate::secrets;
use crate::settings::AiSettings;
use crate::theme::{bold, display, pal};
use crate::widgets;
use egui::text::LayoutJob;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Frame, Label, Layout, Margin, RichText, ScrollArea, Sense, Stroke, TextEdit, TextFormat, Ui, Vec2};
use egui_phosphor::regular as icon;
use nexus_ai::ProviderKind;

#[derive(Default)]
pub struct AskUi {
    pub input: String,
    key_input: String,
    show_setup: bool,
    show_other: bool,
    /// Screenshot runs (`--demo`): show the conversation without a configured provider.
    pub preview: bool,
}

impl AskUi {
    /// `preview`: show the conversation without a configured provider (screenshot runs).
    pub fn new(preview: bool) -> Self {
        AskUi { preview, ..Default::default() }
    }
}

pub enum AskAction {
    None,
    Ask(String),
    SettingsChanged,
}

const SUGGESTIONS: &[&str] = &[
    "Where can I make the most money in the next hour?",
    "Where should I sell my cargo?",
    "What's the best deal near me right now?",
    "Plan an exobiology trip near me worth a billion credits",
    "Where's the nearest raw material trader for my ship?",
];

/// Readable column width for the conversation.
const COLUMN: f32 = 820.0;

pub fn show(ui: &mut Ui, svc: &mut AssistantService, st: &mut AskUi, ai: &mut AiSettings) -> AskAction {
    if (!ai.is_ready() && !st.preview) || st.show_setup {
        return setup(ui, st, ai);
    }
    chat(ui, svc, st)
}

fn short_name(k: ProviderKind) -> &'static str {
    match k {
        ProviderKind::Gemini => "Gemini",
        ProviderKind::Claude => "Claude",
        ProviderKind::Local => "local",
        ProviderKind::Custom => "API",
    }
}

/// A centred column of at most `width`.
fn centered<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let w = ui.available_width().min(width);
    let side = ((ui.available_width() - w) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(side);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui)
        })
        .inner
    })
    .inner
}

fn hero(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(68.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 34.0, pal::ACCENT_BG);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon::SPARKLE, FontId::proportional(32.0), pal::ACCENT);
        ui.add_space(12.0);
        ui.label(RichText::new(title).font(bold(26.0)).color(pal::TEXT));
        ui.add_space(2.0);
        ui.add(Label::new(RichText::new(subtitle).size(15.0).color(pal::DIM)).wrap());
    });
}

fn setup(ui: &mut Ui, st: &mut AskUi, ai: &mut AiSettings) -> AskAction {
    let mut action = AskAction::None;
    ScrollArea::vertical().id_salt("ask-setup").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(36.0);
        centered(ui, 520.0, |ui| {
            hero(ui, "Ask Elite Nexus", "An AI co-pilot that knows your ship, cargo, journal and the live galaxy market. Ask things like “Where can I make the most money in the next hour?”");
            ui.add_space(22.0);
            widgets::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                let kind = ai.provider;
                if kind.needs_key() {
                    ui.label(RichText::new("Connect once").font(display(15.5)).color(pal::TEXT));
                    ui.add_space(4.0);
                    if let Some(url) = kind.key_url() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("1").font(display(13.0)).color(pal::ACCENT));
                            ui.hyperlink_to(format!("Get a free {} key {}", short_name(kind), icon::ARROW_SQUARE_OUT), url);
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("2").font(display(13.0)).color(pal::ACCENT));
                        ui.label(RichText::new("Paste it here").color(pal::TEXT));
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let w = ui.available_width() - 110.0;
                        let field = ui.add_sized([w, 34.0], TextEdit::singleline(&mut st.key_input).password(true).hint_text("API key").vertical_align(Align::Center));
                        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (widgets::primary_button(ui, "Connect").clicked() || enter) && !st.key_input.trim().is_empty() {
                            ai.api_key = secrets::seal(st.key_input.trim());
                            st.key_input.clear();
                            st.show_setup = false;
                            action = AskAction::SettingsChanged;
                        }
                    });
                    if !ai.api_key.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{} A key is saved, {}", icon::LOCK_KEY, secrets::KEPT)).size(13.0).color(pal::GOOD));
                            if ui.small_button("Remove").clicked() {
                                ai.api_key.clear();
                                action = AskAction::SettingsChanged;
                            }
                        });
                    }
                } else {
                    ui.add(Label::new(RichText::new("Uses a model running on this computer: start Ollama or LM Studio with a model that supports tools.").color(pal::DIM)).wrap());
                    ui.add_space(6.0);
                    if widgets::primary_button(ui, "Connect").clicked() {
                        st.show_setup = false;
                        action = AskAction::SettingsChanged;
                    }
                }
                ui.add_space(8.0);
                let kept = format!("Your key stays {}.", secrets::KEPT);
                let note = match kind {
                    ProviderKind::Gemini => format!("{kept} On Google's free tier, questions may be used to improve Google's models; paid use is not."),
                    ProviderKind::Claude => format!("{kept} Claude Haiku costs about a tenth of a cent per question."),
                    ProviderKind::Local => "Nothing leaves this computer.".to_string(),
                    ProviderKind::Custom => kept,
                };
                ui.add(Label::new(RichText::new(note).size(12.5).color(pal::FAINT)).wrap());
            });
            ui.add_space(12.0);
            // Hidden depth: other providers, model, address.
            if widgets::disclosure(ui, &mut st.show_other, "Other providers and model") {
                widgets::card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::Grid::new("ai-advanced").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                        ui.label(RichText::new("Provider").color(pal::DIM));
                        egui::ComboBox::from_id_salt("ai-provider").selected_text(ai.provider.label()).width(300.0).show_ui(ui, |ui| {
                            for k in ProviderKind::ALL {
                                if ui.selectable_value(&mut ai.provider, k, k.label()).changed() {
                                    // The address and model belonged to the previous provider.
                                    ai.base_url.clear();
                                    ai.model.clear();
                                    action = AskAction::SettingsChanged;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label(RichText::new("Model").color(pal::DIM));
                        let auto = ai.provider.preferred_models().first().map(|m| format!("Automatic ({m})")).unwrap_or_else(|| "Automatic".into());
                        if ui.add(TextEdit::singleline(&mut ai.model).hint_text(auto).desired_width(300.0)).changed() {
                            action = AskAction::SettingsChanged;
                        }
                        ui.end_row();
                        if ai.provider.takes_address() {
                            ui.label(RichText::new("Address").color(pal::DIM));
                            let hint = match ai.provider.default_base_url() {
                                "" => "https://…/v1/",
                                u => u,
                            };
                            if ui.add(TextEdit::singleline(&mut ai.base_url).hint_text(hint).desired_width(300.0)).changed() {
                                action = AskAction::SettingsChanged;
                            }
                            ui.end_row();
                        }
                    });
                });
            }
            if ai.is_ready() && st.show_setup {
                ui.add_space(12.0);
                if ui.button("Done").clicked() {
                    st.show_setup = false;
                }
            }
        });
    });
    action
}

fn chat(ui: &mut Ui, svc: &mut AssistantService, st: &mut AskUi) -> AskAction {
    let mut action = AskAction::None;
    // Toolbar: the model in use, a new conversation, settings.
    ui.horizontal(|ui| {
        if let Some(m) = &svc.model {
            widgets::badge(ui, m, pal::DIM);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icon::GEAR_SIX, "Assistant settings").clicked() {
                st.show_setup = true;
            }
            if !svc.exchanges.is_empty() && widgets::icon_button(ui, icon::NOTE_PENCIL, "New conversation").clicked() {
                svc.clear();
            }
        });
    });
    let input_h = 70.0;
    ScrollArea::vertical()
        .id_salt("ask-chat")
        .max_height((ui.available_height() - input_h).max(120.0))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            centered(ui, COLUMN, |ui| {
                if svc.exchanges.is_empty() {
                    ui.add_space(40.0);
                    hero(ui, "What do you want to do, Commander?", "I can see your ship, cargo and journal, the live market and Spansh's route planners.");
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        for s in SUGGESTIONS {
                            if widgets::chip(ui, s).clicked() {
                                action = AskAction::Ask((*s).to_string());
                            }
                            ui.add_space(2.0);
                        }
                    });
                }
                let width = ui.available_width();
                for ex in &svc.exchanges {
                    ui.add_space(14.0);
                    question_bubble(ui, &ex.question, width);
                    ui.add_space(10.0);
                    ui.horizontal_top(|ui| {
                        avatar(ui);
                        ui.vertical(|ui| {
                            ui.set_width(width - 44.0);
                            match &ex.answer {
                                None if ex.stopped => {
                                    ui.label(RichText::new(format!("{} Stopped", icon::STOP_CIRCLE)).color(pal::FAINT));
                                }
                                None => {
                                    ui.horizontal(|ui| {
                                        typing(ui);
                                        ui.label(RichText::new(svc.status.clone().unwrap_or_else(|| "Thinking…".into())).color(pal::DIM));
                                    });
                                }
                                Some(Ok(text)) => {
                                    markdown(ui, text);
                                    if !ex.tools.is_empty() {
                                        let mut nouns: Vec<&str> = ex.tools.iter().map(|t| crate::ai_tools::noun(t)).collect();
                                        nouns.dedup();
                                        ui.add_space(2.0);
                                        ui.label(RichText::new(format!("{} {}", icon::CHECK, nouns.join(" · "))).size(12.0).color(pal::FAINT)).on_hover_text(format!(
                                            "{} input tokens ({} cached), {} output tokens",
                                            ex.usage.input_tokens, ex.usage.cached_tokens, ex.usage.output_tokens
                                        ));
                                    }
                                }
                                Some(Err(e)) => {
                                    ui.add(Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
                                }
                            }
                        });
                    });
                }
                ui.add_space(12.0);
            });
        });
    ui.add_space(8.0);
    centered(ui, COLUMN, |ui| {
        Frame::new()
            .fill(pal::CARD)
            .stroke(Stroke::new(1.0, pal::BORDER))
            .corner_radius(CornerRadius::same(24))
            .inner_margin(Margin { left: 18, right: 6, top: 6, bottom: 6 })
            .shadow(egui::Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha(80) })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 44.0).max(120.0);
                    let field = ui.add_sized(
                        [w, 34.0],
                        TextEdit::singleline(&mut st.input).frame(Frame::new().inner_margin(Margin::symmetric(2, 4))).hint_text("Ask anything").font(FontId::proportional(15.5)).vertical_align(Align::Center),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let busy = svc.busy();
                    let ready = !busy && !st.input.trim().is_empty();
                    // Send, or Stop while the assistant works.
                    let (rect, send) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
                    let (fill, glyph, col) = match (busy, ready) {
                        (true, _) => (pal::ACCENT, icon::STOP, pal::ON_ACCENT),
                        (false, true) => (pal::ACCENT, icon::ARROW_UP, pal::ON_ACCENT),
                        (false, false) => (pal::CARD_HI, icon::ARROW_UP, pal::FAINT),
                    };
                    ui.painter().circle_filled(rect.center(), 17.0, fill);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(18.0), col);
                    if busy {
                        if send.on_hover_text("Stop").clicked() {
                            svc.stop();
                        }
                    } else if (enter || send.clicked()) && ready {
                        action = AskAction::Ask(std::mem::take(&mut st.input));
                        field.request_focus();
                    }
                });
            });
    });
    action
}

fn avatar(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 15.0, pal::ACCENT_BG);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon::SPARKLE, FontId::proportional(15.0), pal::ACCENT);
    ui.add_space(4.0);
}

/// The commander's question: an accent bubble on the right, wrapping at 75 % of the column.
fn question_bubble(ui: &mut Ui, text: &str, width: f32) {
    let max_w = (width * 0.75).max(220.0);
    let galley = ui.painter().layout(text.to_owned(), FontId::proportional(15.0), pal::ON_ACCENT, max_w - 30.0);
    let size = galley.size() + Vec2::new(30.0, 20.0);
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(19), pal::ACCENT);
        ui.painter().galley(rect.min + Vec2::new(15.0, 10.0), galley, pal::ON_ACCENT);
    });
}

/// Three pulsing dots while the assistant works, as in Messages.
fn typing(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(44.0, 24.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    for i in 0..3 {
        let phase = (t * 4.0 - i as f32 * 0.7).sin() * 0.5 + 0.5;
        let c = egui::pos2(rect.left() + 10.0 + i as f32 * 12.0, rect.center().y);
        ui.painter().circle_filled(c, 3.5, pal::DIM.gamma_multiply(0.35 + 0.65 * phase));
    }
    ui.ctx().request_repaint();
}

/// Simple Markdown: headings, bullet and numbered lists (hanging indent), **bold**.
fn markdown(ui: &mut Ui, text: &str) {
    for line in text.lines() {
        let t = line.trim_end();
        if t.trim().is_empty() {
            ui.add_space(6.0);
            continue;
        }
        let trimmed = t.trim_start();
        let level = ((t.len() - trimmed.len()) / 2).min(3) as f32;
        let (heading, body, marker) = if let Some(h) = trimmed.strip_prefix("### ").or_else(|| trimmed.strip_prefix("## ")).or_else(|| trimmed.strip_prefix("# ")) {
            (true, h, None)
        } else if let Some(b) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("• ")) {
            (false, b, Some("•".to_string()))
        } else if let Some((n, rest)) = trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit())) {
            (false, rest, Some(format!("{n}.")))
        } else {
            (false, trimmed, None)
        };
        let size = if heading { 17.0 } else { 15.0 };
        let mut job = LayoutJob::default();
        let normal = TextFormat { font_id: if heading { display(size) } else { FontId::proportional(size) }, color: pal::TEXT, line_height: Some(size * 1.42), ..Default::default() };
        let strong = TextFormat { font_id: display(size), color: pal::TEXT, line_height: Some(size * 1.42), ..Default::default() };
        for (i, part) in body.split("**").enumerate() {
            if !part.is_empty() {
                // Single *emphasis* markers are dropped rather than shown.
                let part = part.replace('*', "");
                job.append(&part, 0.0, if i % 2 == 1 || heading { strong.clone() } else { normal.clone() });
            }
        }
        if heading {
            ui.add_space(4.0);
        }
        match marker {
            Some(m) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(level * 16.0 + 2.0);
                    ui.add_sized([16.0, size * 1.42], Label::new(RichText::new(m).size(size).color(pal::ACCENT)));
                    ui.add(Label::new(job).wrap());
                });
            }
            None => {
                ui.add(Label::new(job).wrap());
            }
        }
    }
}
