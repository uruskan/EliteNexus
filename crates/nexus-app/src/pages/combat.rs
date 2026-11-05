//! Combat: where a commander can fight legally near them, and how.
//!
//! "Fights near you" lists the wars and civil wars (elections on request) around the commander from the galaxy
//! database, nearest first: the game's own conflict list where a system's last report carried it (who fights whom,
//! what's at stake, the days each side has won), otherwise what the faction states say. "How to fight legally"
//! explains, in plain words, what the game counts as a crime and where killing isn't one. Nothing here goes online.

use crate::jobs::Slot;
use crate::theme::{display, pal, size};
use crate::widgets::{self, badge, card, card_tinted, cards, empty, footnote, icon_button, primary_button, section};
use egui::{Color32, RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_galaxy::{ConflictHit, ConflictKind, ConflictQuery, ConflictScan, ConflictSide, ConflictStatus, GalaxyDb};
use nexus_journal::Timestamp;
use nexus_model::GameState;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Fights,
    Guide,
}

/// How far to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Range {
    Ly25,
    Ly50,
    #[default]
    Ly100,
    Ly250,
    Anywhere,
}

impl Range {
    fn ly(self) -> Option<f64> {
        match self {
            Range::Ly25 => Some(25.0),
            Range::Ly50 => Some(50.0),
            Range::Ly100 => Some(100.0),
            Range::Ly250 => Some(250.0),
            Range::Anywhere => None,
        }
    }
}

/// How old a system's last report may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fresh {
    Day,
    #[default]
    ThreeDays,
    Week,
}

impl Fresh {
    fn secs(self) -> i64 {
        match self {
            Fresh::Day => 86_400,
            Fresh::ThreeDays => 3 * 86_400,
            Fresh::Week => 7 * 86_400,
        }
    }

    fn words(self) -> &'static str {
        match self {
            Fresh::Day => "the last 24 hours",
            Fresh::ThreeDays => "the last 3 days",
            Fresh::Week => "the last 7 days",
        }
    }
}

/// The search choices, remembered with the window's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CombatPrefs {
    pub range: Range,
    pub fresh: Fresh,
    pub elections: bool,
}

impl CombatPrefs {
    fn id() -> egui::Id {
        egui::Id::new("combat-prefs")
    }

    fn load(ui: &Ui) -> CombatPrefs {
        ui.data_mut(|d| *d.get_persisted_mut_or_default::<CombatPrefs>(Self::id()))
    }

    fn store(self, ui: &Ui) {
        ui.data_mut(|d| d.insert_persisted(Self::id(), self));
    }
}

/// What the shown list answers.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    system: String,
    prefs: CombatPrefs,
}

pub struct CombatUi {
    pub tab: Tab,
    scan: Slot<Result<ConflictScan, String>>,
    asked: Option<(Asked, Instant)>,
}

impl CombatUi {
    /// `tab` from the command line (`guide` opens How to fight legally).
    pub fn new(tab: Option<&str>) -> CombatUi {
        CombatUi { tab: if tab == Some("guide") { Tab::Guide } else { Tab::Fights }, scan: Slot::default(), asked: None }
    }
}

/// What the page asks the app to do.
pub enum CombatAction {
    /// Plot a route to this system (Navigation).
    PlotTo(String),
}

/// How often the list is asked again while the page is open: the live feed keeps adding reports.
const REFRESH: Duration = Duration::from_secs(60);

/// `feed`: the live EDDN feed is switched on (Accounts & sync), so the database fills while the app runs.
pub fn show(ui: &mut Ui, gs: &GameState, db: Option<&GalaxyDb>, feed: bool, st: &mut CombatUi) -> Option<CombatAction> {
    st.scan.tick();
    if let Some(t) = widgets::tabs(ui, st.tab, &[(Tab::Fights, "Fights near you"), (Tab::Guide, "How to fight legally")]) {
        st.tab = t;
    }
    ui.add_space(10.0);
    match st.tab {
        Tab::Fights => fights(ui, gs, db, feed, st),
        Tab::Guide => {
            guide(ui, gs);
            None
        }
    }
}

fn fights(ui: &mut Ui, gs: &GameState, db: Option<&GalaxyDb>, feed: bool, st: &mut CombatUi) -> Option<CombatAction> {
    let mut action = None;
    here(ui, gs);

    let mut prefs = CombatPrefs::load(ui);
    let before = prefs;
    filters(ui, &mut prefs);
    if prefs != before {
        prefs.store(ui);
    }

    let Some(db) = db else {
        empty(ui, icon::DATABASE, "The galaxy database isn't open", "Wars are looked up in Elite Nexus's galaxy database, which couldn't be opened. Market › Database says why.");
        return action;
    };
    let asked = Asked { system: gs.location.system.clone(), prefs };
    let stale = st.asked.as_ref().is_none_or(|(a, at)| *a != asked || at.elapsed() >= REFRESH);
    if stale && !st.scan.busy() && !asked.system.is_empty() {
        let (db, system, pos) = (db.clone(), asked.system.clone(), gs.location.star_pos);
        st.scan.run(ui.ctx(), move || {
            let origin = match pos {
                Some(p) => p,
                None => db.system_by_name(&system).map_err(|e| e.to_string())?.and_then(|s| s.pos).ok_or_else(|| format!("Elite Nexus doesn't know where {system} is yet."))?,
            };
            let q = ConflictQuery { origin, max_distance_ly: prefs.range.ly(), since_unix: Timestamp::now().0 - prefs.fresh.secs(), elections: prefs.elections, limit: 60 };
            db.conflicts_near(&q).map_err(|e| e.to_string())
        });
        st.asked = Some((asked, Instant::now()));
    }
    ui.ctx().request_repaint_after(REFRESH);

    ui.add_space(6.0);
    match &st.scan.last {
        None if gs.location.system.is_empty() => empty(ui, icon::CROSSHAIR, "Where are you?", "Elite Nexus finds the fights nearest to you once the game says where you are: start the game, or jump once."),
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Looking for wars near you…").color(pal::DIM));
            });
        }
        Some(Err(e)) => empty(ui, icon::WARNING, "Couldn't look for wars", e),
        Some(Ok(scan)) => {
            if let Some(a) = list(ui, gs, scan, prefs, feed) {
                action = Some(a);
            }
        }
    }
    action
}

/// The bar of search choices.
fn filters(ui: &mut Ui, prefs: &mut CombatPrefs) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui.label(RichText::new("Within").color(pal::DIM));
        widgets::segmented(ui, &mut prefs.range, &[(Range::Ly25, "25 ly"), (Range::Ly50, "50 ly"), (Range::Ly100, "100 ly"), (Range::Ly250, "250 ly"), (Range::Anywhere, "Anywhere")]);
        ui.add_space(8.0);
        ui.label(RichText::new("Reported in").color(pal::DIM));
        widgets::segmented(ui, &mut prefs.fresh, &[(Fresh::Day, "24 h"), (Fresh::ThreeDays, "3 days"), (Fresh::Week, "7 days")]);
        ui.add_space(8.0);
        widgets::toggle(ui, &mut prefs.elections, "Elections")
            .on_hover_text("Elections have no conflict zones: they're won with missions, trade and other peaceful work for a faction. Off, the list shows only wars and civil wars, the fights.");
    });
}

/// The list of fights, then what it's based on.
fn list(ui: &mut Ui, gs: &GameState, scan: &ConflictScan, prefs: CombatPrefs, feed: bool) -> Option<CombatAction> {
    let mut action = None;
    // The current system is shown above, from the journal, unless someone has reported it since you arrived.
    let journal = gs.location.info.updated.0;
    let hits: Vec<&ConflictHit> = scan.hits.iter().filter(|h| h.system_id64 != gs.location.system_address || h.updated_unix > journal).collect();
    let here = scan.hits.len() > hits.len();
    let within = match prefs.range.ly() {
        Some(ly) => format!("within {ly:.0} ly"),
        None => "anywhere".to_string(),
    };
    if hits.is_empty() {
        let other = if here { " other" } else { "" };
        let what = if prefs.elections { format!("No{other} wars, civil wars or elections") } else { format!("No{other} wars or civil wars") };
        // The current system, at war, is among those reported.
        let reported = scan.systems_reported.saturating_sub(usize::from(here));
        let hint = if reported == 0 && !feed {
            "Elite Nexus fills its galaxy database from the live EDDN feed, which is switched off: turn on \"Receive the live market feed\" in Accounts & sync. \
             Until then only the systems you visit yourself are reported."
                .to_string()
        } else if reported == 0 {
            format!(
                "No{other} populated system {within} has been reported in {}. Elite Nexus fills its galaxy database from the live EDDN feed while it runs: \
                 leave it open for a while (a few hours cover the populated bubble), or look further.",
                prefs.fresh.words()
            )
        } else {
            format!(
                "None of the{other} {} populated systems reported {within} in {} has {}. Look further, or allow older reports: a war lasts days, so a report from yesterday is still worth a look.",
                nexus_data::names::group_digits(reported as i64),
                prefs.fresh.words(),
                if prefs.elections { "a war or an election" } else { "a war" }
            )
        };
        empty(ui, icon::CROSSHAIR, &format!("{what} reported {within}"), &hint);
    } else {
        let systems = {
            let mut ids: Vec<u64> = hits.iter().map(|h| h.system_id64).collect();
            ids.dedup();
            ids.len()
        };
        let (one, many) = if prefs.elections { ("conflict", "conflicts") } else { ("fight", "fights") };
        // Found in all, without the current system's (shown above).
        let total = scan.total - (scan.hits.len() - hits.len());
        let title = if total > hits.len() {
            format!("The {} nearest of {} {many} {within}", hits.len(), nexus_data::names::group_digits(total as i64))
        } else {
            format!("{} {} in {} {} {within}, nearest first", hits.len(), if hits.len() == 1 { one } else { many }, systems, if systems == 1 { "system" } else { "systems" })
        };
        section(ui, &title);
        cards(ui, 2, 430.0, |c| {
            for hit in &hits {
                c.add(|ui| {
                    if let Some(a) = fight_card(ui, hit, false) {
                        action = Some(a);
                    }
                });
            }
        });
    }
    ui.add_space(10.0);
    let mut note = format!(
        "From the reports of players' game clients that Elite Nexus receives from EDDN while it runs, and your own journal: {} populated systems {within} reported in {}. \
         A system nobody has visited since a war began, or since it ended, shows its last report, so check the system in the game's galaxy map before a long trip. \
         Days won, sides and stakes come from the game's own list of a system's conflicts; where Elite Nexus has only the factions' states (reports stored by an \
         earlier version, a Spansh dump), the card says the score isn't reported.",
        nexus_data::names::group_digits(scan.systems_reported as i64),
        prefs.fresh.words()
    );
    if !feed {
        note.push_str(" The live feed is switched off (Accounts & sync › Receive the live market feed), so only the systems you visit yourself are reported.");
    }
    footnote(ui, &note);
    action
}

fn kind_tip(k: ConflictKind) -> &'static str {
    match k {
        ConflictKind::War => "Two factions fight over the system's assets (at least one of them came from another system), in conflict zones in space and at settlements.",
        ConflictKind::CivilWar => "Two of the system's own factions fight over its assets, in conflict zones in space and at settlements.",
        ConflictKind::Election => "No fighting and no conflict zones: an election is won with missions, trade, exploration data and other peaceful work.",
    }
}

/// The colour of a kind of conflict.
fn kind_color(k: ConflictKind) -> Color32 {
    match k {
        ConflictKind::War => pal::BAD,
        ConflictKind::CivilWar => pal::ACCENT,
        ConflictKind::Election => pal::BLUE,
    }
}

/// A conflict's status badge: words, colour and what it means.
fn status_badge(status: ConflictStatus) -> (&'static str, Color32, &'static str) {
    match status {
        ConflictStatus::Active => ("Active", pal::GOOD, "Being fought now: each day goes to the side that did more for itself."),
        ConflictStatus::Pending => (
            "Pending",
            pal::WARN,
            "Declared, not started: the fighting begins after a short wait. Frontier doesn't say how long; players report one to three days.",
        ),
        ConflictStatus::Ended => ("Ended", pal::FAINT, "Over: the factions are recovering from it, and its conflict zones are gone."),
    }
}

/// How a conflict is decided, for the days-won dots.
const DAYS_TIP: &str = "A conflict lasts up to seven days. Each day goes to the side that did more for itself (conflict zones won, bonds, missions), and it ends early once the other side can no longer catch up: usually when one side has won four days. Even days won at the end is a draw.";

/// One fight: what, where, who, the score, and a route there.
fn fight_card(ui: &mut Ui, hit: &ConflictHit, here: bool) -> Option<CombatAction> {
    let mut action = None;
    widgets::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            badge(ui, hit.kind.label(), kind_color(hit.kind)).on_hover_text(kind_tip(hit.kind));
            let (text, color, tip) = status_badge(hit.status);
            badge(ui, text, color).on_hover_text(tip);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if here {
                    ui.label(RichText::new("You're here").font(display(size::CALLOUT)).color(pal::ACCENT));
                } else {
                    ui.label(RichText::new(format!("{:.1} ly", hit.distance_ly)).font(display(size::CALLOUT)).color(pal::TEXT));
                }
            });
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new(&hit.system).font(display(size::TITLE3)).color(pal::TEXT));
            if icon_button(ui, icon::COPY, "Copy the system's name").clicked() {
                ui.ctx().copy_text(hit.system.clone());
            }
        });
        let mut about: Vec<String> = Vec::new();
        if let Some(g) = &hit.government {
            about.push(g.clone());
        }
        if let Some(s) = hit.security.as_ref().filter(|s| s.as_str() != "Anarchy") {
            about.push(format!("{s} security"));
        }
        let when = widgets::ago(Timestamp(hit.updated_unix));
        about.push(if here { format!("as of your arrival, {when}") } else { format!("reported {when}") });
        ui.label(RichText::new(about.join(" · ")).size(size::FOOTNOTE).color(pal::DIM));
        ui.add_space(6.0);
        sides(ui, hit);
        if !here {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if primary_button(ui, &format!("{}  Plot route", icon::NAVIGATION_ARROW)).on_hover_text("Plan the route on the Navigation page").clicked() {
                    action = Some(CombatAction::PlotTo(hit.system.clone()));
                }
            });
        }
    });
    action
}

/// Who fights whom: each side with its days won as dots (four usually decide it), and what it stands to lose.
fn sides(ui: &mut Ui, hit: &ConflictHit) {
    if !hit.detailed {
        let names: Vec<&str> = hit.sides.iter().map(|s| s.faction.as_str()).collect();
        let who = match names.as_slice() {
            [] => "Factions not reported".to_string(),
            [one] => format!("{one} (the other side wasn't reported)"),
            [a, b] => format!("{a}  vs  {b}"),
            many => format!("{} (who fights whom wasn't reported)", many.join(", ")),
        };
        ui.add(egui::Label::new(RichText::new(who).size(size::BODY).color(pal::TEXT)).wrap());
        ui.label(RichText::new("Score not reported").size(size::FOOTNOTE).color(pal::FAINT)).on_hover_text(
            "Elite Nexus has only the factions' states for this system (a report stored by an earlier version, a Spansh dump, or a sender that left \
             the game's list of conflicts out): which factions are at war is known, not the days each has won or what's at stake. The next report \
             from someone there fills it in.",
        );
        return;
    }
    let lead = hit.sides.iter().filter_map(|s| s.won_days).max().unwrap_or(0);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        // Rows as tall as their text (egui makes a row at least as tall as a button).
        ui.spacing_mut().interact_size.y = 16.0;
        for (i, s) in hit.sides.iter().enumerate() {
            if i > 0 {
                ui.add_space(6.0);
            }
            side(ui, hit.kind, s, lead);
        }
    });
}

/// Width of the days-won dots, and the indent that lines the stake up under the faction's name.
const DOTS: f32 = 4.0 * 10.0 + 3.0 * 3.0;
const INDENT: f32 = DOTS + 10.0;

fn side(ui: &mut Ui, kind: ConflictKind, s: &ConflictSide, lead: u32) {
    let won = s.won_days.unwrap_or(0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let tint = if won > 0 && won == lead { kind_color(kind) } else { pal::DIM };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(DOTS, 14.0), egui::Sense::hover());
        for d in 0..4u32 {
            let c = egui::pos2(rect.left() + 5.0 + d as f32 * 13.0, rect.center().y);
            if d < won {
                ui.painter().circle_filled(c, 4.5, tint);
            } else {
                ui.painter().circle_stroke(c, 4.0, egui::Stroke::new(1.0, pal::FAINT));
            }
        }
        resp.on_hover_text(format!("{won} {} won. {DAYS_TIP}", if won == 1 { "day" } else { "days" }));
        ui.add(egui::Label::new(RichText::new(&s.faction).size(size::BODY).color(pal::TEXT)).truncate()).on_hover_text(faction_tip(s));
    });
    if let Some(stake) = &s.stake {
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            ui.add(egui::Label::new(RichText::new(format!("stands to lose {stake}")).size(size::FOOTNOTE).color(pal::DIM)).truncate());
        });
    }
}

fn faction_tip(s: &ConflictSide) -> String {
    let mut lines = vec![s.faction.clone()];
    let about: Vec<&str> = [s.allegiance.as_deref(), s.government.as_deref()].into_iter().flatten().collect();
    if !about.is_empty() {
        lines.push(about.join(" · "));
    }
    if let Some(i) = s.influence {
        lines.push(format!("{:.1} % influence", i * 100.0));
    }
    if let Some(w) = s.won_days {
        lines.push(format!("{w} {} won", if w == 1 { "day" } else { "days" }));
    }
    lines.join("\n")
}

/// The current system, from the journal: its conflicts, the combat signals logged since arriving, and what kind of
/// place it is.
fn here(ui: &mut Ui, gs: &GameState) {
    let loc = &gs.location;
    if loc.system.is_empty() {
        return;
    }
    let info = &loc.info;
    let conflicts: Vec<ConflictHit> = info
        .conflicts
        .iter()
        .filter_map(|c| {
            let kind = ConflictKind::parse(&c.war_type)?;
            let side = |s: &nexus_journal::events::ConflictSide| {
                let f = info.factions.iter().find(|f| f.name == s.name);
                ConflictSide {
                    faction: s.name.clone(),
                    stake: Some(nexus_data::names::clean_station_name(&s.stake)).filter(|x| !x.is_empty()),
                    won_days: Some(s.won_days),
                    influence: f.map(|f| f.influence),
                    allegiance: f.map(|f| f.allegiance.clone()).filter(|a| !a.is_empty()),
                    government: f.map(|f| f.government.clone()).filter(|g| !g.is_empty()),
                }
            };
            Some(ConflictHit {
                system: loc.system.clone(),
                system_id64: loc.system_address,
                pos: loc.star_pos.unwrap_or_default(),
                distance_ly: 0.0,
                kind,
                status: ConflictStatus::parse(&c.status),
                sides: vec![side(&c.faction1), side(&c.faction2)],
                detailed: true,
                updated_unix: info.updated.0,
                government: Some(info.government.clone()).filter(|g| !g.is_empty()),
                // The journal's security is a whole localised phrase ("Medium Security"), unlike the database's level.
                security: None,
                allegiance: Some(info.allegiance.clone()).filter(|a| !a.is_empty()),
                population: Some(info.population),
                controlling_faction: info.controlling_faction.clone(),
            })
        })
        .collect();
    let signals = combat_signals(gs);
    // The controlling faction's government is an English word in every client language; the system's is localised.
    let controlling = info.controlling_faction.as_ref().and_then(|c| info.factions.iter().find(|f| &f.name == c));
    let anarchy = controlling.is_some_and(|f| f.government.eq_ignore_ascii_case("anarchy")) || info.government.to_lowercase().contains("anarch");
    if conflicts.is_empty() && signals.is_empty() && !anarchy && gs.legal_state.as_deref().is_none_or(|l| l == "Clean") {
        return;
    }
    let fighting = conflicts.iter().any(|c| c.kind.is_fight() && c.status == ConflictStatus::Active);
    let coming = !fighting && conflicts.iter().any(|c| c.kind.is_fight() && c.status == ConflictStatus::Pending);
    let (tint, glyph, title) = if fighting {
        (pal::BAD, icon::SWORD, format!("{} is at war", loc.system))
    } else if coming {
        (pal::WARN, icon::SWORD, format!("War is coming to {}", loc.system))
    } else if anarchy {
        (pal::WARN, icon::SKULL, format!("{} is an anarchy", loc.system))
    } else {
        (pal::ACCENT, icon::MAP_PIN, format!("Here: {}", loc.system))
    };
    card_tinted(ui, glyph, &title, tint, |ui| {
        if let Some(l) = gs.legal_state.as_deref().filter(|l| *l != "Clean") {
            ui.horizontal_wrapped(|ui| {
                badge(ui, &legal_state_label(l), pal::BAD);
                ui.label(RichText::new(legal_state_meaning(l)).size(size::CALLOUT).color(pal::TEXT));
            });
            ui.add_space(4.0);
        }
        if anarchy {
            ui.add(egui::Label::new(RichText::new(ANARCHY_HERE).size(size::CALLOUT).color(pal::TEXT)).wrap());
            ui.add_space(4.0);
        }
        if !signals.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                ui.label(RichText::new("Seen in your scans here:").size(size::CALLOUT).color(pal::DIM));
                for label in &signals {
                    badge(ui, label, pal::ACCENT);
                }
            });
            ui.add_space(4.0);
        }
        let say = if fighting {
            Some((CONFLICT_HERE, pal::TEXT))
        } else if coming {
            Some((COMING_HERE, pal::TEXT))
        } else if !conflicts.is_empty() {
            Some(("No fighting here now: see below.", pal::DIM))
        } else {
            None
        };
        if let Some((text, color)) = say {
            ui.add(egui::Label::new(RichText::new(text).size(size::CALLOUT).color(color)).wrap());
        }
    });
    if !conflicts.is_empty() {
        ui.add_space(widgets::GAP);
        cards(ui, 2, 430.0, |c| {
            for hit in &conflicts {
                c.add(|ui| {
                    fight_card(ui, hit, true);
                });
            }
        });
    }
    ui.add_space(10.0);
}

const CONFLICT_HERE: &str = "Conflict zones show in your navigation panel while you're in supercruise. When you drop into one, choose \
     a side in the Comms panel before you fire; then shoot only the other side's ships.";
const COMING_HERE: &str = "A war here is declared but hasn't started: its conflict zones appear once it does. Frontier doesn't say how \
     long that takes; players report one to three days.";
const ANARCHY_HERE: &str = "No law here: no fines, no bounties and no police, so destroying a clean ship costs you nothing, and other \
     pilots may attack you just as freely. On foot, a settlement of a faction that isn't an anarchy keeps its law.";

/// The kinds of combat site logged here since the last arrival. (The exploration record keeps one signal per name,
/// and zones of one intensity share theirs, so how many there are isn't known.)
fn combat_signals(gs: &GameState) -> Vec<String> {
    let Some(sys) = gs.exploration.current_system() else { return Vec::new() };
    if sys.address != gs.location.system_address {
        return Vec::new();
    }
    // Odyssey writes a system's signals just before the jump that reaches it.
    let since = Timestamp(gs.location.last_arrival.0 - 120);
    let mut out: Vec<String> = Vec::new();
    for s in sys.signals.iter().filter(|s| s.ts >= since) {
        if let Some(label) = combat_site(&s.name, s.kind.as_deref())
            && !out.contains(&label)
        {
            out.push(label);
        }
    }
    out
}

/// What a signal is, for a fighter: conflict zones, extraction sites, nav beacons. `name` is the name the journal
/// gave (localised when it could), `kind` its `SignalType`.
pub(crate) fn combat_site(name: &str, kind: Option<&str>) -> Option<String> {
    let n = name.to_ascii_lowercase();
    let kind = kind.unwrap_or_default();
    if n.contains("warzone_powerplay") || n.contains("power conflict zone") {
        return Some("Powerplay conflict zone".into());
    }
    if n.contains("warzone_tg") || n.contains("ax conflict zone") {
        return Some("AX conflict zone".into());
    }
    if n.contains("warzone") || n.contains("conflict zone") {
        let intensity = if n.contains("high") { " (high)" } else if n.contains("med") { " (medium)" } else if n.contains("low") { " (low)" } else { "" };
        return Some(format!("Conflict zone{intensity}"));
    }
    // A game in another language names it in that language; its type is still Combat.
    if kind == "Combat" {
        return Some("Conflict zone".into());
    }
    if kind == "ResourceExtraction" || n.contains("resource extraction") || n.contains("scenario14_") || n.contains("scenario77_") || n.contains("scenario78_") || n.contains("scenario79_") {
        let grade = if n.contains("hazardous") || n.contains("scenario79_") {
            " (hazardous)"
        } else if n.contains("[high]") || n.contains("scenario78_") {
            " (high)"
        } else if n.contains("[low]") || n.contains("scenario77_") {
            " (low)"
        } else {
            ""
        };
        return Some(format!("Extraction site{grade}"));
    }
    if n.contains("compromised") || n.contains("scenario80_") {
        return Some("Compromised nav beacon".into());
    }
    if kind == "NavBeacon" || n.contains("nav beacon") || n.contains("scenario42_") {
        return Some("Nav beacon".into());
    }
    None
}

/// Status.json's `LegalState`, in words.
fn legal_state_label(l: &str) -> String {
    match l {
        "IllegalCargo" => "Illegal cargo".into(),
        "PassengerWanted" => "Wanted passenger".into(),
        other => other.into(),
    }
}

fn legal_state_meaning(l: &str) -> &'static str {
    match l {
        "Wanted" => "You have a bounty in this system's jurisdiction: police and bounty hunters may attack you.",
        "Warrant" => "You have a bounty elsewhere, not here.",
        "Hostile" => "The local authorities are hostile to you.",
        "Speeding" => "You're speeding near a station.",
        "IllegalCargo" => "You carry cargo that's illegal here: a scan means a fine.",
        "PassengerWanted" => "A passenger aboard is wanted here.",
        "Allied" => "The local authorities are allied with you.",
        _ => "",
    }
}

// ---------------------------------------------------------------------------------------------
// How to fight legally: the game's rules as of version 4.4 (November 2026), checked in docs/research/combat.md.

/// One topic of the guide: a tinted heading, what it comes down to, and the points that matter.
struct Topic {
    icon: &'static str,
    tint: Color32,
    title: &'static str,
    lead: &'static str,
    points: &'static [&'static str],
}

const SHORT: &[&str] = &[
    "You may destroy a ship when the game shows it as Wanted here (target it and let the scan finish), when it fights for the other side in a conflict zone you've joined, or when it's a Thargoid.",
    "Anything else is a crime: hitting a clean ship, a station or the police gets you a fine or a bounty, and destroying a clean ship is murder, which also adds to your notoriety.",
    "Destroyed where you have a bounty, you're sent to a detention centre and pay the bounty with your rebuy. In an anarchy there's no law at all.",
];

const WHERE: &[Topic] = &[
    Topic {
        icon: icon::SWORD,
        tint: pal::BAD,
        title: "Conflict zones",
        lead: "Wars and civil wars are fought in them: Fights near you lists the nearest. Join a side, and the other side's ships are fair game.",
        points: &[
            "They show in your navigation panel while you're in supercruise within about 1,000 ls: Conflict Zone, at low, medium or high intensity. High intensity has the toughest ships, sometimes a capital ship.",
            "When you drop in, choose a side in the Comms panel. Don't fire before that: until you've joined, the ships there aren't legal targets.",
            "Once you've joined, nothing you destroy there adds notoriety. Keep off your own side's ships: they're allies, not targets.",
            "Each kill pays a combat bond from your side. Hand bonds in at a station your side controls (Interstellar Factors takes them anywhere, for a 25 % cut), and before you risk the ship: bonds are lost with it.",
            "On foot: sign up at the Frontline Solutions desk in a port of that system (pick the settlement and the side, then ride the dropship), or land near the settlement and accept the prompt in the Comms panel.",
        ],
    },
    Topic {
        icon: icon::CROSSHAIR,
        tint: pal::ACCENT,
        title: "Wanted ships",
        lead: "Bounty hunting: pirates gather at resource extraction sites and nav beacons, and only ships Wanted here are legal targets.",
        points: &[
            "Extraction sites come as low, normal, high and hazardous; the higher, the more ships. System security helps, except at hazardous sites and compromised nav beacons.",
            "Target a ship and let the scan finish: shoot only when it says Wanted. A ship whose bounties are all elsewhere isn't a legal target; a Kill Warrant Scanner reveals those bounties and pays them on the kill.",
            "Bounties are paid at a station of the faction that issued them, through its security contact, or at Interstellar Factors for any faction's, for a 25 % cut.",
        ],
    },
    Topic {
        icon: icon::SKULL,
        tint: pal::WARN,
        title: "Anarchy systems",
        lead: "A system run by an anarchy faction has no law: no fines, no bounties and no police.",
        points: &[
            "Destroying a clean ship there earns no bounty and no notoriety, and even the Pilots' Federation bounty for killing another player doesn't apply.",
            "The same goes for everyone else: anyone may attack you there.",
            "On foot, a settlement of a faction that isn't an anarchy keeps its law, and killing there still adds notoriety.",
        ],
    },
    Topic {
        icon: icon::ALIEN,
        tint: pal::TEAL,
        title: "Thargoids",
        lead: "Thargoids belong to no faction: each kill pays a combat bond.",
        points: &[
            "You need anti-xeno weapons and a ship built for it; ordinary weapons barely scratch them.",
            "The Thargoid war ended when the last Titan fell in December 3310 (2024). In 2025 Thargoids still held parts of the Pleiades, Witch Head, Coalsack and California nebulae.",
        ],
    },
    Topic {
        icon: icon::CROWN,
        tint: pal::PURPLE,
        title: "Powerplay",
        lead: "Pledged to a Power? Its rivals' ships earn you merits.",
        points: &[
            "Killing ships of another Power pays merits in acquisition, reinforcement and undermining systems. Frontier still calls it technically a crime until December; players report small bounties for it, and no notoriety.",
            "Crimes between pledged commanders bring a Power bounty instead: no police come, only that Power's pledges can claim it, and it never sends you to a detention centre.",
            "Power conflict zones form in contested systems (Power Conflict Zone in the navigation panel): merits for every kill, and bonds cashed only at the Power's stronghold carrier.",
        ],
    },
    Topic {
        icon: icon::TARGET,
        tint: pal::BLUE,
        title: "Combat missions",
        lead: "Massacre and assassination missions name your targets. Read the details before you accept.",
        points: &[
            "Pirate massacre targets are Wanted, so killing them is legal. Massacre missions from several factions against the same target faction all count each kill: the Missions page counts the stack.",
            "Some missions ask for illegal kills; the mission details usually say so, though not always. A clean target is murder, whoever asked for it.",
            "Massacre missions for a side in a war are done in that war's conflict zones.",
        ],
    },
    Topic {
        icon: icon::USERS_THREE,
        tint: pal::INDIGO,
        title: "Operations",
        lead: "Squad fights for up to four, started from your ship's panel while docked: Mercenary or Powerplay operations.",
        points: &[
            "Your bounties, fines and bonds are set aside before you go in, and nothing you do there adds notoriety.",
            "They pay credits, materials and Merc Coins, and in Powerplay mode merits for pledged commanders.",
        ],
    },
];

const FIRST: &[&str] = &[
    "Pick an active war in Fights near you and press Plot route; low intensity zones are the place to start.",
    "In the system, open the navigation panel in supercruise and drop into a Conflict Zone.",
    "Choose a side in the Comms panel as soon as you arrive. Before each kill, check the target's faction: only the other side.",
    "Cash your bonds at a station of your side before you take the ship back out. If your shields fail, leave: bonds go down with the ship.",
];

const TROUBLE: &[Topic] = &[
    Topic {
        icon: icon::GAVEL,
        tint: pal::BAD,
        title: "What makes you wanted",
        lead: "Every crime costs a fine or a bounty from the faction whose space you're in.",
        points: &[
            "Fines: illegal or stolen cargo, loitering at a station, hitting ships at speed near a station, abandoned missions, light hits on a clean ship (reckless weapons fire).",
            "Bounties: damaging or destroying clean ships or SRVs, killing people, trespassing, destroying property. Destroying a clean ship is murder.",
            "Bounties never expire, and an unpaid fine can turn into one. Bounties from five or more factions of one superpower add a superpower bounty.",
        ],
    },
    Topic {
        icon: icon::SIREN,
        tint: pal::WARN,
        title: "Notoriety",
        lead: "From 0 to 10: each murder adds 1 (less on foot), and it drops by 1 for every two hours you're logged in, docked or not.",
        points: &[
            "While it's above 0, Interstellar Factors won't clear your bounties, and every point makes a murder's bounty bigger.",
            "Kills in a conflict zone you've joined, and clean kills in an anarchy, add none.",
        ],
    },
    Topic {
        icon: icon::LOCK,
        tint: pal::BAD,
        title: "The detention centre",
        lead: "Get destroyed, or killed on foot, where you have a bounty, and whoever claims it sends you there.",
        points: &[
            "You pay your rebuy plus the fines and bounties of that jurisdiction; the others stay on record. A ship you committed crimes in is impounded and costs a release fee. If you can't pay, you start over in a Sidewinder.",
            "To stay out: pay your bounties first, at Interstellar Factors (in low-security and anarchy systems) once notoriety is 0, but not in a system where the faction that issued them is present. Waiting doesn't clear a bounty.",
            "Handing yourself in at a station's security contact also pays them, and sends you to the nearest detention centre.",
        ],
    },
    Topic {
        icon: icon::USERS,
        tint: pal::PINK,
        title: "Other players",
        lead: "Attacking another commander is a crime like any other, unless they're Wanted here, you're in an anarchy, or you're both pledged to rival Powers.",
        points: &[
            "Murdering a player also brings a Pilots' Federation bounty, valid everywhere but in anarchies.",
            "A player can switch off reporting crimes against them; switched back on, it takes 15 seconds to count.",
        ],
    },
];

fn bullets(ui: &mut Ui, points: &[&str], color: Color32) {
    for p in points {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("•").size(size::CALLOUT).color(pal::FAINT));
            ui.add(egui::Label::new(RichText::new(*p).size(size::CALLOUT).color(color)).wrap());
        });
    }
}

fn topic(ui: &mut Ui, t: &Topic) {
    card_tinted(ui, t.icon, t.title, t.tint, |ui| {
        ui.add(egui::Label::new(RichText::new(t.lead).font(display(size::BODY)).color(pal::TEXT)).wrap());
        ui.add_space(4.0);
        bullets(ui, t.points, pal::DIM);
    });
}

fn guide(ui: &mut Ui, gs: &GameState) {
    card_tinted(ui, icon::CALENDAR_DOTS, "These rules change in December 2026", pal::WARN, |ui| {
        ui.add(
            egui::Label::new(
                RichText::new(
                    "This is how the game works now (version 4.4). Frontier's Law & Disorder update, due in December 2026, rewrites crime and punishment: \
                     crimes will count only once the authorities confirm them, the Kill Warrant Scanner and Interstellar Factors go, inhabited anarchies \
                     get enforcer ships, and Powerplay kills stop being crimes. After it, trust the game's own Pilot's Handbook over this page.",
                )
                .size(size::CALLOUT)
                .color(pal::TEXT),
            )
            .wrap(),
        );
    });
    ui.add_space(widgets::GAP);
    card_tinted(ui, icon::SHIELD_CHECK, "The short version", pal::GOOD, |ui| bullets(ui, SHORT, pal::TEXT));
    if let Some(l) = gs.legal_state.as_deref() {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Your status where you are:").size(size::CALLOUT).color(pal::DIM));
            badge(ui, &legal_state_label(l), if l == "Clean" { pal::GOOD } else { pal::BAD });
            let meaning = if l == "Clean" { "not wanted here." } else { legal_state_meaning(l) };
            ui.label(RichText::new(meaning).size(size::CALLOUT).color(pal::DIM));
        });
    }

    section(ui, "Where you can fight");
    cards(ui, 2, 400.0, |c| {
        for t in WHERE {
            c.add(|ui| topic(ui, t));
        }
    });

    section(ui, "Your first conflict zone");
    card(ui, "", |ui| {
        for (i, step) in FIRST.iter().enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 11.0, pal::ACCENT_BG);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, (i + 1).to_string(), display(size::FOOTNOTE), pal::ACCENT);
                ui.add(egui::Label::new(RichText::new(*step).size(size::CALLOUT).color(pal::TEXT)).wrap());
            });
            ui.add_space(2.0);
        }
    });

    section(ui, "Staying out of trouble");
    cards(ui, 2, 400.0, |c| {
        for t in TROUBLE {
            c.add(|ui| topic(ui, t));
        }
    });
    ui.add_space(10.0);
    footnote(
        ui,
        "As the game works in November 2026. Frontier doesn't document every detail: how long a war stays pending, and exactly which stations take a side's bonds, vary between reports, so the advice above takes the safe side.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combat_sites_from_signal_names() {
        assert_eq!(combat_site("Conflict Zone [High Intensity]", Some("Combat")).as_deref(), Some("Conflict zone (high)"));
        assert_eq!(combat_site("$Warzone_PointRace_Low:#index=1;", Some("Combat")).as_deref(), Some("Conflict zone (low)"));
        assert_eq!(combat_site("Power Conflict Zone [Medium Intensity]", Some("Combat")).as_deref(), Some("Powerplay conflict zone"));
        assert_eq!(combat_site("Konfliktzone [Hohe Intensität]", Some("Combat")).as_deref(), Some("Conflict zone"), "another language");
        assert_eq!(combat_site("Resource Extraction Site [Hazardous]", Some("ResourceExtraction")).as_deref(), Some("Extraction site (hazardous)"));
        assert_eq!(combat_site("$MULTIPLAYER_SCENARIO77_TITLE;", Some("ResourceExtraction")).as_deref(), Some("Extraction site (low)"));
        assert_eq!(combat_site("Resource Extraction Site", Some("ResourceExtraction")).as_deref(), Some("Extraction site"));
        assert_eq!(combat_site("Nav Beacon", Some("NavBeacon")).as_deref(), Some("Nav beacon"));
        assert_eq!(combat_site("$MULTIPLAYER_SCENARIO80_TITLE;", Some("NavBeacon")).as_deref(), Some("Compromised nav beacon"));
        assert_eq!(combat_site("Leesti's History", Some("TouristBeacon")), None);
        assert_eq!(combat_site("TCG BARBAROS KFK-L3V", Some("FleetCarrier")), None);
    }
}
