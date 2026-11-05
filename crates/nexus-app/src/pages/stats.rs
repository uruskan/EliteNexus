//! Statistics, in four tabs. **Career**: the game's own lifetime figures (its statistics, written at each login) in
//! plain words: where the credits came from and went, each activity with its rank, records and the round numbers coming
//! up. **Activity**: what the commander did over a period they pick, from the journals Elite Nexus has read, against
//! the period before. **Ships**: the same period ship by ship. **All statistics**: every figure the game keeps,
//! searchable, with the zeros put away. Every chart fits the period shown, and amounts read as "19.1 B".

use crate::theme::{display, pal, size};
use crate::widgets::{self, GAP, bar, card_tinted, card_with, cards, footnote, section, short_credits};
use egui::{Align, Color32, Layout, RichText, Ui};
use egui_phosphor::regular as icon;
use egui_plot::{Bar, BarChart, GridInput, GridMark, Line, Plot, PlotPoints};
use nexus_data::names::group_digits;
use nexus_data::ranks::RankKind;
use nexus_journal::time::LocalDays;
use nexus_journal::Timestamp;
use nexus_model::career::{self, Career, Earning, Expense, Record, Unit};
use nexus_model::history::LedgerCategory;
use nexus_model::period::{Day, Records, Summary, Window};
use nexus_model::ships::ShipSummary;
use nexus_model::GameState;
use std::sync::Arc;

const CAREER: u8 = 0;
const ACTIVITY: u8 = 1;
const ALL: u8 = 2;
const SHIPS: u8 = 3;
const TABS: [(u8, &str); 4] = [(CAREER, "Career"), (ACTIVITY, "Activity"), (SHIPS, "Ships"), (ALL, "All statistics")];

/// Where the tab is remembered (between runs, like the period).
fn tab_id() -> egui::Id {
    egui::Id::new("stats-tab")
}

/// Opens a tab by name (`career`, `activity`, `ships`, `all`), for screenshot runs.
pub fn open_tab(ctx: &egui::Context, name: Option<&str>) {
    let tab = match name {
        Some("career") => CAREER,
        Some("activity") => ACTIVITY,
        Some("ships") => SHIPS,
        Some("all") => ALL,
        _ => return,
    };
    ctx.data_mut(|d| d.insert_persisted(tab_id(), tab));
}

pub fn show(ui: &mut Ui, gs: &GameState) {
    let mut tab: u8 = ui.data_mut(|d| *d.get_persisted_mut_or(tab_id(), CAREER));
    if let Some(t) = widgets::tabs(ui, tab, &TABS)
        && t != tab
    {
        tab = t;
        ui.data_mut(|d| d.insert_persisted(tab_id(), tab));
    }
    ui.add_space(GAP);
    match tab {
        ACTIVITY => activity_tab(ui, gs),
        SHIPS => ships_tab(ui, gs),
        ALL => all_tab(ui, gs),
        _ => career_tab(ui, gs),
    }
}

// ---------------------------------------------------------------------------------------------
// Building blocks

/// Work the page does once rather than every frame: kept under `slot` while `key` stays the same.
fn cached<T: Send + Sync + 'static>(ui: &Ui, slot: &str, key: impl std::hash::Hash + std::fmt::Debug, make: impl FnOnce() -> T) -> Arc<T> {
    let id = egui::Id::new(("stats-cache", slot));
    let key = egui::Id::new(key);
    if let Some((k, v)) = ui.data(|d| d.get_temp::<(egui::Id, Arc<T>)>(id))
        && k == key
    {
        return v;
    }
    let v = Arc::new(make());
    ui.data_mut(|d| d.insert_temp(id, (key, v.clone())));
    v
}

/// A figure at the top of a tab: a tinted caption, the figure, and lines under it.
fn tile(ui: &mut Ui, glyph: &str, caption: &str, tint: Color32, value: &str, value_colour: Color32, lines: &[(String, Color32)]) {
    card_tinted(ui, glyph, caption, tint, |ui| {
        ui.add(egui::Label::new(RichText::new(value).font(display(size::TITLE)).color(value_colour)).truncate());
        for (text, colour) in lines {
            ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(*colour)).wrap());
        }
    });
}

/// A label on the left and its value on the right, on one line. The value keeps up to two thirds of the row and the
/// label the rest; past that, either ends in "…" and shows whole on hover.
fn fact(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(21.0);
        let room = ui.available_width();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.scope(|ui| {
                ui.set_max_width(room * 0.66);
                ui.add(egui::Label::new(RichText::new(value).color(pal::TEXT)).truncate());
            });
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(label).color(pal::DIM)).truncate());
            });
        });
    });
}

/// A headline figure and what it is.
fn headline(ui: &mut Ui, value: &str, caption: &str) {
    ui.add(egui::Label::new(RichText::new(value).font(display(size::TITLE)).color(pal::TEXT)).truncate());
    if !caption.is_empty() {
        ui.add(egui::Label::new(RichText::new(caption).size(size::FOOTNOTE).color(pal::DIM)).wrap());
    }
}

/// A rank and the way to the next one.
fn rank(ui: &mut Ui, gs: &GameState, kind: RankKind, tint: Color32) {
    let r = gs.cmdr.ranks.get(kind);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} rank", kind.label())).size(size::FOOTNOTE).color(pal::DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(kind.name_of(r)).font(display(size::FOOTNOTE)).color(tint));
        });
    });
    ui.add_space(-4.0);
    if r >= kind.max() {
        bar(ui, 1.0, tint, 5.0, None).on_hover_text("The highest rank there is");
    } else {
        let next = kind.name_of(r + 1);
        match gs.cmdr.rank_progress(kind) {
            Some(p) => bar(ui, p as f32 / 100.0, tint, 5.0, None).on_hover_text(format!("{p}% of the way to {next}")),
            None => bar(ui, 0.0, pal::FAINT, 5.0, None).on_hover_text("Promoted since the game last reported your progress: it does again when you next log in."),
        };
    }
    ui.add_space(4.0);
}

/// One part of a whole: its name, amount and colour.
struct Part {
    label: String,
    amount: i64,
    colour: Color32,
}

/// "37%", or "<1%" for a sliver.
fn share(amount: i64, total: i64) -> String {
    let p = amount as f64 * 100.0 / total.max(1) as f64;
    if p > 0.0 && p < 1.0 { "<1%".into() } else { format!("{p:.0}%") }
}

/// Parts of a whole: a bar of their shares (hover a part for its figures), then one line per part, biggest first; past
/// `rows`, the rest share a line.
fn breakdown(ui: &mut Ui, parts: &[Part], rows: usize) {
    let total: i64 = parts.iter().map(|p| p.amount).sum();
    if total <= 0 {
        return;
    }
    let shares: Vec<(f64, Color32)> = parts.iter().map(|p| (p.amount as f64, p.colour)).collect();
    let (resp, hovered) = widgets::share_bar(ui, &shares, 12.0);
    if let Some(p) = hovered.and_then(|i| parts.get(i)) {
        resp.on_hover_text(format!("{}: {} ({})", p.label, widgets::credits(p.amount), share(p.amount, total)));
    }
    ui.add_space(6.0);
    let line = |ui: &mut Ui, colour: Color32, label: &str, amount: i64| {
        ui.horizontal(|ui| {
            ui.set_min_height(22.0);
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.5, colour);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_sized([40.0, 18.0], egui::Label::new(RichText::new(share(amount, total)).size(size::FOOTNOTE).color(pal::DIM)));
                ui.label(RichText::new(short_credits(amount)).font(display(size::CALLOUT)).color(pal::TEXT));
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(label).color(pal::TEXT)).truncate());
                });
            });
        });
    };
    for p in parts.iter().take(rows) {
        line(ui, p.colour, &p.label, p.amount);
    }
    if parts.len() > rows {
        let rest: i64 = parts[rows..].iter().map(|p| p.amount).sum();
        line(ui, pal::FAINT, &format!("{} more", parts.len() - rows), rest);
    }
}

/// A small bar per item against the largest: (label, value shown, amount).
fn bars(ui: &mut Ui, items: &[(String, String, i64)], tint: Color32) {
    let max = items.iter().map(|(_, _, v)| *v).max().unwrap_or(1).max(1);
    for (label, value, amount) in items {
        ui.horizontal(|ui| {
            ui.set_min_height(20.0);
            ui.allocate_ui_with_layout(egui::vec2(118.0, 18.0), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(118.0);
                ui.add(egui::Label::new(RichText::new(label).size(size::FOOTNOTE).color(pal::DIM)).truncate());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_sized([52.0, 18.0], egui::Label::new(RichText::new(value).size(size::FOOTNOTE).color(pal::TEXT)));
                ui.add_space(4.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    bar(ui, *amount as f32 / max as f32, tint, 6.0, None);
                });
            });
        });
    }
}

/// "▲ 12% on the 30 days before", or why there's nothing to compare with.
fn compared(now: f64, before: f64) -> Option<(String, Color32)> {
    if before == 0.0 && now == 0.0 {
        return None;
    }
    if before == 0.0 {
        return Some((format!("{} none in the period before", icon::TREND_UP), pal::DIM));
    }
    let pct = (now - before) / before.abs() * 100.0;
    let (glyph, colour) = if pct >= 0.0 { (icon::TREND_UP, pal::GOOD) } else { (icon::TREND_DOWN, pal::BAD) };
    let pct = if pct.abs() >= 1_000.0 { format!("{:.0}×", now / before) } else { format!("{:.0}%", pct.abs()) };
    Some((format!("{glyph} {pct} on the period before"), colour))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", group_digits(n as i64), if n == 1 { one } else { many })
}

// ---------------------------------------------------------------------------------------------
// Career

/// The colour each earning keeps on the page.
fn earning_colour(e: Earning) -> Color32 {
    match e {
        Earning::Trade => pal::GOLD,
        Earning::Mining => pal::ACCENT,
        Earning::Exploration => pal::CYAN,
        Earning::Exobiology => pal::GOOD,
        Earning::Bounties => pal::PINK,
        Earning::CombatBonds => pal::BAD,
        Earning::Assassinations => pal::PURPLE,
        Earning::BlackMarkets => pal::GRAY,
        Earning::SearchAndRescue => pal::MINT,
        Earning::Multicrew => pal::BLUE,
        Earning::Cqc => pal::INDIGO,
    }
}

fn expense_colour(e: Expense) -> Color32 {
    match e {
        Expense::Ships => pal::BLUE,
        Expense::Outfitting => pal::TEAL,
        Expense::CrewWages => pal::PURPLE,
        Expense::Rebuys => pal::BAD,
        Expense::Repairs => pal::WARN,
        Expense::Ammo => pal::ACCENT,
        Expense::Fuel => pal::CYAN,
        Expense::OnFoot => pal::MINT,
        Expense::Engineering => pal::INDIGO,
        Expense::PremiumStock => pal::GOLD,
        Expense::Shuttles => pal::GRAY,
    }
}

/// When the game wrote its statistics, as the page says it.
fn as_of(gs: &GameState) -> String {
    let at = gs.cmdr.statistics_at;
    if at.is_zero() { String::new() } else { at.to_local_string("%d %b %Y, %H:%M") }
}

fn career_tab(ui: &mut Ui, gs: &GameState) {
    let Some(stats) = gs.cmdr.statistics.as_deref() else {
        widgets::empty(
            ui,
            icon::CHART_BAR,
            "No career figures yet",
            "The game writes your statistics each time you log in, and they show here: start Elite Dangerous and load your commander. The Activity tab already counts what your journals hold.",
        );
        return;
    };
    let c = Career::new(stats);
    let get = |g: &str, k: &str| c.get(g, k);
    let hours = get("Exploration", "Time_Played") / 3_600.0;
    let earned: i64 = c.earnings().iter().map(|(_, v)| v).sum();
    let jumps = get("Exploration", "Total_Hyperspace_Jumps");
    let ly = get("Exploration", "Total_Hyperspace_Distance");
    tile_row(ui, |cs| {
        cs.add(|ui| {
            let mut lines = Vec::new();
            if hours >= 1.0 && earned > 0 {
                lines.push((format!("about {} an hour, by the game's totals", short_credits((earned as f64 / hours) as i64)), pal::DIM));
            }
            tile(ui, icon::CLOCK, "Time in the game", pal::PURPLE, &career::duration(get("Exploration", "Time_Played") as i64), pal::TEXT, &lines);
        });
        cs.add(|ui| {
            let wealth = c.int("Bank_Account", "Current_Wealth");
            tile(ui, icon::VAULT, "Wealth", pal::GOLD, &short_credits(wealth), pal::GOLD, &[("your credits and what you own, as the game counts it".into(), pal::DIM)]);
        });
        cs.add(|ui| {
            let per = if jumps > 0.0 { format!("{} jumps · {:.1} ly a jump", group_digits(jumps as i64), ly / jumps) } else { String::new() };
            tile(ui, icon::PATH, "Distance jumped", pal::CYAN, &career::show(ly, Unit::Ly), pal::TEXT, &[(per, pal::DIM)]);
        });
        cs.add(|ui| {
            let far = get("Exploration", "Greatest_Distance_From_Start");
            let line = if far > 0.0 { format!("farthest {} from where you started", career::show(far, Unit::Ly)) } else { String::new() };
            tile(ui, icon::GLOBE_HEMISPHERE_WEST, "Systems visited", pal::TEAL, &group_digits(get("Exploration", "Systems_Visited") as i64), pal::TEXT, &[(line, pal::DIM)]);
        });
    });
    let when = as_of(gs);
    footnote(ui, &format!("From the game's own statistics{}: your whole career, as the game counts it.", if when.is_empty() { String::new() } else { format!(", written when you last logged in ({when})") }));
    ui.add_space(GAP);
    career_money(ui, &c);
    section(ui, "Activities");
    career_activities(ui, gs, &c);
    section(ui, "Records and milestones");
    // Worked out again once a minute at most: the records walk every body scanned.
    let records = cached(ui, "records", (&gs.cmdr.fid, &gs.cmdr.name, Timestamp::now().0 / 60), || Records::compute(gs, Timestamp::now()));
    widgets::split(ui, 0.62, 340.0, |ui| records_grid(ui, &c, &records, gs.history.first()), |ui| milestones(ui, &c));
}

fn career_money(ui: &mut Ui, c: &Career) {
    let earned: Vec<Part> = c.earnings().into_iter().map(|(e, v)| Part { label: e.label().into(), amount: v, colour: earning_colour(e) }).collect();
    let spent: Vec<Part> = c.spending().into_iter().map(|(e, v)| Part { label: e.label().into(), amount: v, colour: expense_colour(e) }).collect();
    cards(ui, 2, 340.0, |cs| {
        cs.add(|ui| {
            card_tinted(ui, icon::HAND_COINS, "Where your credits came from", pal::GOOD, |ui| {
                let total: i64 = earned.iter().map(|p| p.amount).sum();
                if total <= 0 {
                    ui.label(RichText::new("The game hasn't counted any earnings yet.").color(pal::DIM));
                    return;
                }
                headline(ui, &short_credits(total), "earned in your career, by the game's totals");
                ui.add_space(8.0);
                breakdown(ui, &earned, 6);
                footnote(ui, "The game doesn't total mission rewards or what ships and modules sold for, so they aren't here.");
            });
        });
        cs.add(|ui| {
            card_tinted(ui, icon::RECEIPT, "Where they went", pal::ACCENT, |ui| {
                let total: i64 = spent.iter().map(|p| p.amount).sum();
                if total <= 0 {
                    ui.label(RichText::new("The game hasn't counted any spending yet.").color(pal::DIM));
                    return;
                }
                headline(ui, &short_credits(total), "spent in your career, by the game's totals");
                ui.add_space(8.0);
                breakdown(ui, &spent, 6);
                footnote(ui, "Fines aren't here (the game counts them received, not paid), nor your fleet carrier's upkeep.");
            });
        });
    });
}

/// An activity's card on the Career tab.
struct Activity<'a> {
    glyph: &'static str,
    title: &'static str,
    tint: Color32,
    rank: Option<RankKind>,
    headline: Option<(String, String)>,
    facts: Vec<(String, String)>,
    extra: Option<Extra<'a>>,
}

/// More for a card to draw under its figures (a small chart).
type Extra<'a> = Box<dyn Fn(&mut Ui) + 'a>;

impl Activity<'_> {
    fn new(glyph: &'static str, title: &'static str, tint: Color32) -> Self {
        Activity { glyph, title, tint, rank: None, headline: None, facts: Vec::new(), extra: None }
    }

    /// Anything to show beyond the rank.
    fn any(&self) -> bool {
        self.headline.is_some() || !self.facts.is_empty()
    }
}

fn career_activities(ui: &mut Ui, gs: &GameState, c: &Career) {
    let get = |g: &str, k: &str| c.get(g, k);
    let cr = |v: f64| short_credits(v.round() as i64);
    let n = |v: f64| group_digits(v.round() as i64);
    // A figure from the table, when it isn't zero.
    let fig = |a: &mut Activity, g: &str, k: &str| {
        let v = get(g, k);
        if v != 0.0 {
            let (label, unit) = career::describe(g, k);
            a.facts.push((label, career::show(v, unit)));
        }
    };
    // A count and what it earned, on one line: "17 · 1.6 M Cr".
    let pair = |a: &mut Activity, label: &str, count: f64, credits: f64| {
        if count != 0.0 || credits != 0.0 {
            a.facts.push((label.to_string(), if credits != 0.0 { format!("{} · {}", n(count), cr(credits)) } else { n(count) }));
        }
    };
    let head = |a: &mut Activity, value: f64, unit: Unit, caption: &str| {
        if value != 0.0 {
            a.headline = Some((career::show(value, unit), caption.to_string()));
        }
    };
    let mut list: Vec<Activity> = Vec::new();

    let mut a = Activity::new(icon::SCALES, "Trade", pal::GOLD);
    a.rank = Some(RankKind::Trade);
    head(&mut a, get("Trading", "Market_Profits"), Unit::Credits, "profit from trade");
    fig(&mut a, "Trading", "Resources_Traded");
    fig(&mut a, "Trading", "Markets_Traded_With");
    fig(&mut a, "Trading", "Highest_Single_Transaction");
    fig(&mut a, "Smuggling", "Black_Markets_Profits");
    fig(&mut a, "Smuggling", "Resources_Smuggled");
    list.push(a);

    let mut a = Activity::new(icon::CROSSHAIR, "Combat", pal::PINK);
    a.rank = Some(RankKind::Combat);
    let combat = get("Combat", "Bounty_Hunting_Profit") + get("Combat", "Combat_Bond_Profits") + get("Combat", "Assassination_Profits");
    head(&mut a, combat, Unit::Credits, "earned from bounties, combat bonds and assassinations");
    pair(&mut a, "Bounties claimed", get("Combat", "Bounties_Claimed"), get("Combat", "Bounty_Hunting_Profit"));
    pair(&mut a, "Combat bonds", get("Combat", "Combat_Bonds"), get("Combat", "Combat_Bond_Profits"));
    pair(&mut a, "Assassinations", get("Combat", "Assassinations"), get("Combat", "Assassination_Profits"));
    fig(&mut a, "Combat", "Highest_Single_Reward");
    let (zones, won) = (get("Combat", "ConflictZone_Total"), get("Combat", "ConflictZone_Total_Wins"));
    if zones > 0.0 {
        a.facts.push(("Conflict zones won".into(), format!("{} of {}", n(won), n(zones))));
    }
    fig(&mut a, "TG_ENCOUNTERS", "TG_ENCOUNTER_KILLED");
    fig(&mut a, "TG_ENCOUNTERS", "TG_SCOUT_COUNT");
    fig(&mut a, "Combat", "Skimmers_Killed");
    fig(&mut a, "Combat", "Settlement_Defended");
    fig(&mut a, "Combat", "Settlement_Conquered");
    list.push(a);

    let mut a = Activity::new(icon::COMPASS, "Exploration", pal::CYAN);
    a.rank = Some(RankKind::Explore);
    head(&mut a, get("Exploration", "Exploration_Profits"), Unit::Credits, "earned from exploration data");
    fig(&mut a, "Exploration", "Planets_Scanned_To_Level_3");
    fig(&mut a, "Exploration", "Efficient_Scans");
    fig(&mut a, "Exploration", "Highest_Payout");
    fig(&mut a, "Exploration", "Fuel_Scooped");
    list.push(a);

    let mut a = Activity::new(icon::PLANT, "Exobiology", pal::GOOD);
    a.rank = Some(RankKind::Exobiologist);
    head(&mut a, get("Exobiology", "Organic_Data_Profits"), Unit::Credits, "earned from organic data");
    pair(&mut a, "Species logged first", get("Exobiology", "First_Logged"), get("Exobiology", "First_Logged_Profits"));
    fig(&mut a, "Exobiology", "Organic_Data");
    let found = [("genera", "Organic_Genus_Encountered"), ("species", "Organic_Species_Encountered"), ("variants", "Organic_Variant_Encountered")];
    let found: Vec<String> = found.iter().filter(|(_, k)| get("Exobiology", k) > 0.0).map(|(w, k)| format!("{} {w}", n(get("Exobiology", k)))).collect();
    if !found.is_empty() {
        a.facts.push(("Found".into(), found.join(" · ")));
    }
    fig(&mut a, "Exobiology", "Organic_Planets");
    list.push(a);

    let mut a = Activity::new(icon::DIAMONDS_FOUR, "Mining", pal::ACCENT);
    head(&mut a, get("Mining", "Mining_Profits"), Unit::Credits, "profit from mining");
    fig(&mut a, "Mining", "Quantity_Mined");
    list.push(a);

    let mut a = Activity::new(icon::USERS_THREE, "Passengers", pal::PURPLE);
    head(&mut a, get("Passengers", "Passengers_Missions_Delivered"), Unit::Count, "passengers delivered");
    fig(&mut a, "Passengers", "Passengers_Missions_Accepted");
    fig(&mut a, "Passengers", "Passengers_Missions_Ejected");
    fig(&mut a, "Passengers", "Passengers_Missions_Disgruntled");
    pair(&mut a, "Search and rescue hand-ins", get("Search_And_Rescue", "SearchRescue_Count"), get("Search_And_Rescue", "SearchRescue_Profit"));
    let kinds = c.passengers();
    if kinds.iter().any(|(_, d, _)| *d > 0) {
        let items: Vec<(String, String, i64)> = kinds.iter().filter(|(_, d, _)| *d > 0).take(4).map(|(k, d, _)| (k.to_string(), group_digits(*d), *d)).collect();
        a.extra = Some(Box::new(move |ui: &mut Ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("Who you carried most").size(size::FOOTNOTE).color(pal::DIM));
            bars(ui, &items, pal::PURPLE);
        }));
    }
    list.push(a);

    let mut a = Activity::new(icon::PERSON_SIMPLE_WALK, "On foot", pal::MINT);
    a.rank = Some(RankKind::Soldier);
    head(&mut a, get("Exploration", "OnFoot_Distance_Travelled"), Unit::Metres, "walked");
    fig(&mut a, "Exploration", "Planet_Footfalls");
    fig(&mut a, "Exploration", "First_Footfalls");
    fig(&mut a, "Exploration", "Settlements_Visited");
    pair(&mut a, "Combat bonds on foot", get("Combat", "OnFoot_Combat_Bonds"), get("Combat", "OnFoot_Combat_Bonds_Profits"));
    fig(&mut a, "Combat", "OnFoot_Scavs_Killed");
    fig(&mut a, "Combat", "Dropships_Taken");
    let (suits, weapons) = (get("Bank_Account", "Suits_Owned"), get("Bank_Account", "Weapons_Owned"));
    if suits > 0.0 || weapons > 0.0 {
        let count = |v: f64, one: &str, many: &str| plural(v.round() as usize, one, many);
        a.facts.push(("Owned".into(), format!("{} · {}", count(suits, "suit", "suits"), count(weapons, "hand weapon", "hand weapons"))));
    }
    fig(&mut a, "Exploration", "Shuttle_Journeys");
    list.push(a);

    let mut a = Activity::new(icon::WRENCH, "Engineering", pal::BLUE);
    head(&mut a, get("Crafting", "Recipes_Generated"), Unit::Count, "engineering rolls");
    fig(&mut a, "Crafting", "Count_Of_Used_Engineers");
    let (trades, traded) = (get("Material_Trader_Stats", "Trades_Completed"), get("Material_Trader_Stats", "Materials_Traded"));
    if trades > 0.0 {
        a.facts.push(("Material trades".into(), format!("{} · {} materials", n(trades), n(traded))));
    }
    fig(&mut a, "Mining", "Materials_Collected");
    fig(&mut a, "Crafting", "Suits_Upgraded");
    fig(&mut a, "Crafting", "Weapons_Upgraded");
    fig(&mut a, "Crafting", "Suit_Mods_Applied");
    fig(&mut a, "Crafting", "Weapon_Mods_Applied");
    fig(&mut a, "Bank_Account", "MercCoins_Total_Earned");
    let grades = c.rolls_by_grade();
    if grades.iter().any(|g| *g > 0) {
        let items: Vec<(String, String, i64)> = grades.iter().enumerate().map(|(i, g)| (format!("Grade {}", i + 1), group_digits(*g), *g)).collect();
        a.extra = Some(Box::new(move |ui: &mut Ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("Rolls by grade").size(size::FOOTNOTE).color(pal::DIM));
            bars(ui, &items, pal::BLUE);
        }));
    }
    list.push(a);

    let mut a = Activity::new(icon::ROCKET_LAUNCH, "Fleet and crew", pal::INDIGO);
    head(&mut a, get("Bank_Account", "Owned_Ship_Count"), Unit::Count, "ships owned");
    pair(&mut a, "Ships lost and bought back", get("Bank_Account", "Insurance_Claims"), get("Bank_Account", "Spent_On_Insurance"));
    fig(&mut a, "Crew", "NpcCrew_TotalWages");
    fig(&mut a, "Crew", "NpcCrew_Hired");
    fig(&mut a, "Crew", "NpcCrew_Died");
    fig(&mut a, "Multicrew", "Multicrew_Time_Total");
    fig(&mut a, "Multicrew", "Multicrew_Credits_Total");
    list.push(a);

    let mut a = Activity::new(icon::ANCHOR, "Fleet carrier", pal::TEAL);
    head(&mut a, get("FLEETCARRIER", "FLEETCARRIER_DISTANCE_TRAVELLED"), Unit::Ly, "the carrier jumped");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_TOTAL_JUMPS");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_TRADEPROFIT_TOTAL");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_TRADESPEND_TOTAL");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_REFUEL_TOTAL");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_REPAIRS_TOTAL");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_REARM_TOTAL");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_OUTFITTING_PROFIT");
    fig(&mut a, "FLEETCARRIER", "FLEETCARRIER_SHIPYARD_PROFIT");
    list.push(a);

    let mut a = Activity::new(icon::ALIEN, "Thargoids", pal::MINT);
    head(&mut a, get("TG_ENCOUNTERS", "TG_ENCOUNTER_TOTAL"), Unit::Count, "encounters");
    fig(&mut a, "TG_ENCOUNTERS", "TG_ENCOUNTER_WAKES");
    let last = [c.text("TG_ENCOUNTERS", "TG_ENCOUNTER_TOTAL_LAST_SYSTEM"), c.text("TG_ENCOUNTERS", "TG_ENCOUNTER_TOTAL_LAST_SHIP")];
    if !last[0].is_empty() {
        let ship = if last[1].is_empty() { String::new() } else { format!(", in your {}", last[1]) };
        a.facts.push(("Last encounter".into(), format!("{}{ship}", last[0])));
    }
    list.push(a);

    let mut a = Activity::new(icon::SIREN, "Crime", pal::BAD);
    head(&mut a, get("Crime", "Total_Fines"), Unit::Credits, "in fines");
    fig(&mut a, "Crime", "Fines");
    pair(&mut a, "Bounties put on you", get("Crime", "Bounties_Received"), get("Crime", "Total_Bounties"));
    fig(&mut a, "Crime", "Highest_Bounty");
    fig(&mut a, "Crime", "Notoriety");
    fig(&mut a, "Crime", "Total_Murders");
    fig(&mut a, "Crime", "Total_Stolen");
    list.push(a);

    let mut a = Activity::new(icon::GAME_CONTROLLER, "CQC", pal::INDIGO);
    a.rank = (gs.cmdr.ranks.get(RankKind::Cqc) > 0).then_some(RankKind::Cqc);
    head(&mut a, get("CQC", "CQC_Credits_Earned"), Unit::Credits, "earned in CQC");
    fig(&mut a, "CQC", "CQC_Time_Played");
    fig(&mut a, "CQC", "CQC_Kills");
    fig(&mut a, "CQC", "CQC_KD");
    list.push(a);

    let mut a = Activity::new(icon::FLAG_BANNER, "Squadron", pal::BLUE);
    for f in c.figures("Squadron").into_iter().filter(|f| !f.is_zero()) {
        a.facts.push((f.label.clone(), f.shown()));
    }
    list.push(a);

    let (shown, quiet): (Vec<Activity>, Vec<Activity>) = list.into_iter().partition(Activity::any);
    widgets::masonry(ui, 3, 290.0, &shown, |ui, a| activity_card(ui, gs, a));
    if !quiet.is_empty() {
        let names: Vec<&str> = quiet.iter().map(|a| a.title).collect();
        footnote(ui, &format!("Nothing counted yet for {}.", names.join(", ")));
    }
}

/// An activity's card: its headline figure, the rank that goes with it, its figures, and anything more it draws.
fn activity_card(ui: &mut Ui, gs: &GameState, a: &Activity) {
    card_tinted(ui, a.glyph, a.title, a.tint, |ui| {
        if let Some((value, caption)) = &a.headline {
            headline(ui, value, caption);
            ui.add_space(6.0);
        }
        if let Some(k) = a.rank {
            rank(ui, gs, k, a.tint);
        }
        for (label, value) in &a.facts {
            fact(ui, label, value);
        }
        if let Some(extra) = &a.extra {
            extra(ui);
        }
    });
}

/// The four figures at the top of a tab: four in a row, or two and two when narrow (never three and one).
fn tile_row(ui: &mut Ui, add: impl FnOnce(&mut widgets::Cards)) {
    let cols = if ui.available_width() >= 4.0 * 190.0 + 3.0 * GAP { 4 } else { 2 };
    cards(ui, cols, 150.0, add);
}

/// The records: the game's own (career bests) and those your journals hold (with their dates).
fn records_grid(ui: &mut Ui, c: &Career, r: &Records, since: Option<Timestamp>) {
    struct Tile {
        glyph: &'static str,
        label: &'static str,
        tint: Color32,
        value: String,
        detail: String,
    }
    let mut tiles: Vec<Tile> = Vec::new();
    for (rec, v) in c.records() {
        let (glyph, tint) = match rec {
            Record::BestTrade => (icon::SCALES, pal::GOLD),
            Record::BestExplorationPayout => (icon::COMPASS, pal::CYAN),
            Record::BestCombatReward => (icon::CROSSHAIR, pal::PINK),
            Record::FarthestFromStart => (icon::GLOBE_HEMISPHERE_WEST, pal::TEAL),
            Record::BestBlackMarketSale => (icon::SCALES, pal::GRAY),
            Record::HighestBounty => (icon::SIREN, pal::BAD),
        };
        tiles.push(Tile { glyph, label: rec.label(), tint, value: career::show(v, rec.unit()), detail: "career best".into() });
    }
    let date = |ts: Timestamp| ts.to_local_string("%d %b %Y");
    let mut feat = |f: &Option<nexus_model::period::Feat>, glyph, label, tint, value: &dyn Fn(f64) -> String| {
        if let Some(f) = f {
            let detail = if f.what.is_empty() { date(f.ts) } else { format!("{} · {}", f.what, date(f.ts)) };
            tiles.push(Tile { glyph, label, tint, value: value(f.value), detail });
        }
    };
    let money = |v: f64| short_credits(v.round() as i64);
    feat(&r.best_day, icon::CALENDAR_CHECK, "Best day", pal::GOOD, &money);
    feat(&r.biggest_payment, icon::HAND_COINS, "Biggest single payment", pal::GOLD, &money);
    feat(&r.best_mission, icon::TARGET, "Best mission reward", pal::PURPLE, &money);
    feat(&r.best_find, icon::PLANET, "Most valuable body scanned", pal::CYAN, &money);
    feat(&r.longest_jump, icon::LIGHTNING, "Longest jump", pal::CYAN, &|v| format!("{v:.2} ly"));
    feat(&r.farthest_from_sol, icon::MAP_PIN, "Farthest from Sol", pal::TEAL, &|v| career::show(v, Unit::Ly));
    feat(&r.busiest_day, icon::PATH, "Most jumps in a day", pal::CYAN, &|v| format!("{v:.0}"));
    feat(&r.longest_session, icon::CLOCK, "Longest session", pal::PURPLE, &|v| career::duration(v as i64));
    if tiles.is_empty() {
        return;
    }
    cards(ui, 3, 200.0, |cs| {
        for t in &tiles {
            cs.add(|ui| {
                card_tinted(ui, t.glyph, t.label, t.tint, |ui| {
                    ui.add(egui::Label::new(RichText::new(&t.value).font(display(size::TITLE3)).color(pal::TEXT)).truncate());
                    ui.add(egui::Label::new(RichText::new(&t.detail).size(size::FOOTNOTE).color(pal::DIM)).truncate());
                });
            });
        }
    });
    let since = since.map(|t| format!(" ({})", t.to_local_string("%d %b %Y"))).unwrap_or_default();
    footnote(ui, &format!("Career bests are the game's own. The others come from your journals, so they count only what you did since the oldest one Elite Nexus has read{since}."));
}

fn milestones(ui: &mut Ui, c: &Career) {
    card_tinted(ui, icon::FLAG_CHECKERED, "Milestones within reach", pal::ACCENT, |ui| {
        let list = c.milestones();
        if list.is_empty() {
            ui.label(RichText::new("Your career counters are all at zero so far.").color(pal::DIM));
            return;
        }
        for m in list.iter().take(5) {
            // Time counts in hours here: "326 h of 500 h".
            let show = |v: f64| if m.unit == Unit::Seconds { format!("{} h", group_digits((v / 3_600.0).round() as i64)) } else { career::show(v, m.unit) };
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(m.label).color(pal::TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} to go", show(m.target - m.value))).size(size::FOOTNOTE).color(pal::DIM));
                });
            });
            bar(ui, m.progress() as f32, pal::ACCENT, 6.0, None);
            ui.label(RichText::new(format!("{} of {}", show(m.value), show(m.target))).size(size::FOOTNOTE).color(pal::DIM));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Activity

/// The periods the Activity tab can show: (days, label); 0 = all time.
const RANGES: [(u32, &str); 4] = [(7, "7 days"), (30, "30 days"), (90, "90 days"), (0, "All time")];

/// Where the period starts: the start of the calendar day `days - 1` days before today, so "7 days" is today and the six
/// days before, each a whole day on the chart; for all time, the first record.
fn range_start(days: u32, now: Timestamp, first: Option<Timestamp>) -> Timestamp {
    if days == 0 {
        return first.unwrap_or(now);
    }
    let today = LocalDays::default().day(now);
    LocalDays::instant((today - i64::from(days) + 1) as f64)
}

/// What the Activity tab shows for one period, worked out once.
struct Period {
    window: Window,
    now: Summary,
    before: Option<Summary>,
}

/// Why a period holds none of something: the journals don't reach it, or the commander didn't play in it. None when
/// they did play in it, and there was simply none of that.
fn why_empty(gs: &GameState, w: Window) -> Option<String> {
    let h = &gs.history;
    let played = h.play.iter().any(|p| w.overlap(p.start, p.end) > 0 || w.contains(p.start));
    match h.first() {
        None => Some("Elite Nexus hasn't read a journal of yours yet.".into()),
        Some(f) if f >= w.end => Some("Your journals start after it.".into()),
        _ if played => None,
        Some(f) if f > w.start => Some(format!("Your journals start on {}: earlier days aren't covered.", f.to_local_string("%d %b %Y"))),
        _ => Some(match h.play.iter().rev().find(|p| p.start < w.start) {
            Some(p) => format!("You didn't play in it: your last session before it was on {}.", p.start.to_local_string("%d %b %Y")),
            None => "You didn't play in it.".into(),
        }),
    }
}

/// "Nothing of that in the last 7 days.", with the reason when there is one.
fn none_in(gs: &GameState, w: Window, what: &str) -> String {
    match why_empty(gs, w) {
        Some(why) => format!("{what} {why}"),
        None => what.to_string(),
    }
}

/// The period picker the Activity and Ships tabs share (remembered between runs): the days picked (0 for all time) and
/// the stretch of time they cover, up to now.
fn pick_period(ui: &mut Ui, gs: &GameState, now: Timestamp) -> (u32, Window) {
    let id = egui::Id::new("stats-range");
    let mut days: u32 = ui.data_mut(|d| *d.get_persisted_mut_or(id, 30u32));
    if widgets::segmented(ui, &mut days, &RANGES) {
        ui.data_mut(|d| d.insert_persisted(id, days));
    }
    (days, Window { start: range_start(days, now, gs.history.first()), end: Timestamp(now.0 + 1) })
}

fn activity_tab(ui: &mut Ui, gs: &GameState) {
    let h = &gs.history;
    let now = Timestamp::now();
    let (days, window) = pick_period(ui, gs, now);
    let first = h.first();
    let start = window.start;
    // Worked out again every ten seconds at most (journal lines arrive faster than that during a scan), and at once for
    // another period or commander.
    let p = cached(ui, "period", (&gs.cmdr.fid, &gs.cmdr.name, days, start, now.0 / 10), || {
        // Compared only with a period the journals cover whole.
        let before = (days > 0 && first.is_some_and(|f| f <= window.before().start)).then(|| Summary::compute(gs, window.before()));
        Period { window, now: Summary::compute(gs, window), before }
    });
    let s = &p.now;
    let span = if days == 0 { "all time".to_string() } else { format!("the last {days} days") };
    let coverage = match first {
        Some(f) => format!("From your journals: Elite Nexus has read them back to {}.", f.to_local_string("%d %b %Y")),
        None => "From your journals, as Elite Nexus reads them.".into(),
    };
    let compare = if p.before.is_some() { " Arrows compare with the period before: as long a stretch of time, just before it." } else { "" };
    footnote(ui, &format!("{coverage}{compare}"));
    ui.add_space(6.0);

    let before = |f: fn(&Summary) -> f64| p.before.as_ref().and_then(|b| compared(f(s), f(b)));
    tile_row(ui, |cs| {
        cs.add(|ui| {
            let earned = s.money.earned();
            let mut lines = Vec::new();
            if s.play_secs >= 600 && earned > 0 {
                lines.push((format!("{} an hour in the game", short_credits((earned as f64 / (s.play_secs as f64 / 3_600.0)) as i64)), pal::DIM));
            }
            lines.extend(before(|x| x.money.earned() as f64));
            if earned == 0 && lines.is_empty() {
                lines.push(("nothing sold, cashed in or handed in".into(), pal::DIM));
            }
            let sign = if earned > 0 { "+" } else { "" };
            tile(ui, icon::HAND_COINS, "Earned", pal::GOOD, &format!("{sign}{}", short_credits(earned)), if earned >= 0 { pal::GOOD } else { pal::BAD }, &lines);
        });
        cs.add(|ui| {
            let mut lines = Vec::new();
            if s.sessions > 0 {
                lines.push((format!("{} over {}", plural(s.sessions, "session", "sessions"), plural(s.days_played, "day", "days")), pal::DIM));
            }
            lines.extend(before(|x| x.play_secs as f64));
            tile(ui, icon::CLOCK, "Time in the game", pal::PURPLE, &career::duration(s.play_secs), pal::TEXT, &lines);
        });
        cs.add(|ui| {
            let mut lines = vec![(format!("{:.0} ly · {} · {} new", s.ly, plural(s.systems, "system", "systems"), group_digits(s.first_visits as i64)), pal::DIM)];
            lines.extend(before(|x| x.jumps as f64));
            tile(ui, icon::PATH, "Jumps", pal::CYAN, &group_digits(s.jumps as i64), pal::TEXT, &lines);
        });
        cs.add(|ui| {
            let steps = credit_steps(&h.credit_samples, start.0, now.0, gs.cmdr.credits);
            let line = match (steps.first(), steps.last()) {
                (Some(a), Some(b)) => {
                    let c = b.1 - a.1;
                    (format!("{}{} in {span}", if c > 0 { "+" } else { "" }, short_credits(c)), if c >= 0 { pal::GOOD } else { pal::BAD })
                }
                _ => ("no balance recorded in this period".into(), pal::DIM),
            };
            tile(ui, icon::WALLET, "Balance", pal::GOLD, &short_credits(gs.cmdr.credits), pal::GOLD, &[line]);
        });
    });
    ui.add_space(GAP);
    cards(ui, 2, 360.0, |cs| {
        cs.add(|ui| credits_chart(ui, gs, days, start.0, now.0));
        cs.add(|ui| period_money(ui, gs, &p, &span));
    });
    ui.add_space(GAP);
    by_day(ui, gs, &p, days);
    section(ui, "What you did");
    period_activities(ui, gs, &p, &span);
}

fn credits_chart(ui: &mut Ui, gs: &GameState, days: u32, start: i64, now: i64) {
    let steps = credit_steps(&gs.history.credit_samples, start, now, gs.cmdr.credits);
    // A log scale shows the hundreds of millions next to the billions; remembered between runs.
    let log_id = egui::Id::new("stats-credits-log");
    // Both the title row (the switch) and the chart read it: a Cell lets the switch change it first.
    let log = std::cell::Cell::new(ui.data_mut(|d| *d.get_persisted_mut_or(log_id, false)));
    card_with(
        ui,
        "Balance",
        |ui| {
            let mut on = log.get();
            if widgets::toggle(ui, &mut on, "Log scale").on_hover_text("Each step up the axis is ten times more: small and large balances both show").changed() {
                log.set(on);
                ui.data_mut(|d| d.insert_persisted(log_id, on));
            }
        },
        |ui| {
            if steps.len() < 2 {
                ui.label(RichText::new("Your journals hold no balance for this period.").color(pal::DIM));
                return;
            }
            let log = log.get();
            // On the log scale the line holds log10 of the balance (below 1 Cr counts as 1 Cr).
            let y = |v: i64| if log { (v.max(1) as f64).log10() } else { v as f64 };
            // Along the commander's calendar days, as the chart by day: each midnight falls under its date. (Unix time over
            // 86,400 put UTC midnight there, which west of UTC is the evening before that date.)
            let mut calendar = LocalDays::default();
            let shown: Vec<[f64; 2]> = steps.iter().map(|&(t, v)| [calendar.at(Timestamp(t)), y(v)]).collect();
            let (lo, hi) = shown.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
            let (bottom, top) = credit_y_range(lo, hi, log);
            let credits = move |v: f64| if log { 10f64.powf(v) } else { v };
            Plot::new(("credits", days, log))
                .show_background(false)
                .show_grid([false, true])
                .height(240.0)
                .allow_scroll(false)
                .allow_drag(false)
                .allow_zoom(false)
                .include_x(calendar.at(Timestamp(start)))
                .include_x(calendar.at(Timestamp(now)))
                .include_y(bottom)
                .include_y(top)
                .x_axis_formatter(widgets::day_axis)
                .y_grid_spacer(credit_marks((bottom, top), log))
                .y_axis_formatter(move |m, _| nexus_data::names::compact_credits(credits(m.value).round() as i64))
                .label_formatter(move |h| {
                    let (x, v) = widgets::hover_xy(h);
                    Some(format!("{}\n{} Cr", LocalDays::instant(x).to_local_string("%d %b %H:%M"), group_digits(credits(v).round() as i64)))
                })
                .show(ui, |p| {
                    p.line(Line::new("Credits", PlotPoints::from(shown)).color(pal::GOLD).width(2.0));
                });
        },
    );
}

/// The colour each ledger category keeps on the page.
fn ledger_colour(c: LedgerCategory) -> Color32 {
    match c {
        LedgerCategory::Trade => pal::GOLD,
        LedgerCategory::Mining => pal::ACCENT,
        LedgerCategory::Exploration => pal::CYAN,
        LedgerCategory::Exobiology => pal::GOOD,
        LedgerCategory::Bounties => pal::PINK,
        LedgerCategory::CombatBonds => pal::BAD,
        LedgerCategory::Missions => pal::PURPLE,
        LedgerCategory::Ships => pal::BLUE,
        LedgerCategory::Outfitting => pal::TEAL,
        LedgerCategory::Repairs => pal::WARN,
        LedgerCategory::Fuel => pal::MINT,
        LedgerCategory::Ammo => pal::ACCENT_SOFT,
        LedgerCategory::Fines => pal::BAD,
        LedgerCategory::Rebuy => pal::PINK,
        LedgerCategory::Carrier => pal::INDIGO,
        LedgerCategory::Colonisation => pal::MINT,
        LedgerCategory::Materials => pal::GRAY,
        LedgerCategory::Taxi => pal::GRAY,
        LedgerCategory::Transfer | LedgerCategory::Other => pal::FAINT,
    }
}

/// What a category is called on the money-in side and on the money-out side.
fn ledger_label(c: LedgerCategory, earned: bool) -> &'static str {
    match (c, earned) {
        (LedgerCategory::Trade, true) => "Trade profit",
        (LedgerCategory::Trade, false) => "Trade at a loss",
        // Cargo that cost nothing: mined, but also salvaged, stolen or given for a mission.
        (LedgerCategory::Mining, true) => "Mined and found cargo",
        (LedgerCategory::Ships, true) => "Ships sold",
        (LedgerCategory::Ships, false) => "Ships and transfers",
        (LedgerCategory::Outfitting, true) => "Modules and gear sold",
        (LedgerCategory::Outfitting, false) => "Modules and gear",
        (LedgerCategory::Ammo, true) => "Limpets sold",
        (LedgerCategory::Ammo, false) => "Ammunition, limpets and restocking",
        (LedgerCategory::Materials, true) => "Micro-resources sold",
        (LedgerCategory::Materials, false) => "Micro-resources",
        (LedgerCategory::Missions, false) => "Mission donations",
        (LedgerCategory::Fines, _) => "Fines and bounties paid",
        (LedgerCategory::Rebuy, _) => "Rebuys",
        (LedgerCategory::Carrier, true) => "From the squadron carrier",
        (LedgerCategory::Carrier, false) => "Fleet carrier",
        (LedgerCategory::Taxi, _) => "Taxis and dropships",
        (LedgerCategory::Exploration, true) => "Exploration data",
        (LedgerCategory::Exobiology, true) => "Exobiology data",
        (c, _) => c.label(),
    }
}

fn period_money(ui: &mut Ui, gs: &GameState, p: &Period, span: &str) {
    let m = &p.now.money;
    card_tinted(ui, icon::COINS, "Money in and out", pal::GOLD, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 26.0;
            widgets::stat(ui, "in", format!("+{}", short_credits(m.income_total())), pal::GOOD);
            widgets::stat(ui, "out", format!("-{}", short_credits(m.spending_total())), pal::BAD);
            let net = m.net();
            widgets::stat(ui, "net", format!("{}{}", if net > 0 { "+" } else { "" }, short_credits(net)), if net >= 0 { pal::GOOD } else { pal::BAD });
        });
        ui.add_space(8.0);
        let parts = |flows: &[nexus_model::period::Flow], earned: bool| -> Vec<Part> {
            flows.iter().map(|f| Part { label: ledger_label(f.category, earned).into(), amount: f.amount, colour: ledger_colour(f.category) }).collect()
        };
        let mut out = parts(&m.spending, false);
        if m.unsold_cargo > 0 {
            out.push(Part { label: "Cargo bought, not sold".into(), amount: m.unsold_cargo, colour: pal::GOLD });
            out.sort_by_key(|p| std::cmp::Reverse(p.amount));
        }
        if m.income.is_empty() && out.is_empty() {
            ui.label(RichText::new(none_in(gs, p.window, &format!("Nothing earned or spent in {span}."))).color(pal::DIM));
        } else {
            for (title, list, earned) in [("In", parts(&m.income, true), true), ("Out", out, false)] {
                ui.label(RichText::new(title).font(display(size::FOOTNOTE)).color(pal::DIM));
                if list.is_empty() {
                    ui.label(RichText::new(if earned { "Nothing sold, cashed in or handed in." } else { "Nothing spent." }).color(pal::DIM));
                } else {
                    breakdown(ui, &list, 5);
                }
                ui.add_space(6.0);
            }
        }
        if m.moved != 0 {
            let what = if m.moved < 0 { format!("{} moved to your fleet carrier", short_credits(-m.moved)) } else { format!("{} taken from your fleet carrier", short_credits(m.moved)) };
            ui.add(egui::Label::new(RichText::new(format!("{what} isn't counted: it stays yours.")).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        }
        footnote(
            ui,
            "Trade counts by its profit: what a sale fetched over what the cargo cost. Cargo bought and not sold in the period (delivered to a construction or a mission, lost, or still aboard) counts as spent, at cost. From your journal: a mission a wingmate shared and handed in isn't here (the game writes no journal line for it), but its credits are in your balance.",
        );
    });
}

/// What the chart by day shows.
const METRICS: [(u8, &str); 4] = [(0, "Jumps"), (1, "Light years"), (2, "Earned"), (3, "Time")];

fn by_day(ui: &mut Ui, gs: &GameState, p: &Period, days: u32) {
    let id = egui::Id::new("stats-day-metric");
    let metric = std::cell::Cell::new(ui.data_mut(|d| *d.get_persisted_mut_or(id, 0u8)));
    card_with(
        ui,
        "By day",
        |ui| {
            ui.scope(|ui| {
                ui.set_max_width(360.0);
                let mut m = metric.get();
                if widgets::segmented(ui, &mut m, &METRICS) {
                    metric.set(m);
                    ui.data_mut(|d| d.insert_persisted(id, m));
                }
            });
        },
        |ui| {
            let metric = metric.get();
            let value = |d: &Day| match metric {
                0 => f64::from(d.jumps),
                1 => d.ly,
                2 => d.earned as f64,
                _ => d.play_secs as f64 / 3_600.0,
            };
            let what = ["No jumps", "No jumps", "Nothing earned", "No time in the game"][usize::from(metric.min(3))];
            let days_of = &p.now.days;
            if !days_of.values().any(|d| value(d) != 0.0) {
                ui.label(RichText::new(none_in(gs, p.window, &format!("{what} in this period."))).color(pal::DIM));
                return;
            }
            // Days are the commander's calendar days (local time), for the bars, the axis and the tooltip alike.
            let mut calendar = LocalDays::default();
            let colour = [pal::CYAN, pal::TEAL, pal::GOOD, pal::PURPLE][usize::from(metric.min(3))];
            let bars: Vec<Bar> = days_of
                .iter()
                .filter(|(_, d)| value(d) != 0.0)
                .map(|(day, d)| {
                    let v = value(d);
                    Bar::new(*day as f64 + 0.5, v).width(0.8).fill(if v < 0.0 { pal::BAD } else { colour })
                })
                .collect();
            let unit = match metric {
                2 => 1.0,
                3 => 0.1,
                _ => 1.0,
            };
            let plot = Plot::new(("byday", days, metric))
                .show_background(false)
                // The period's days edge to edge, without a day before or after.
                .set_margin_fraction(egui::vec2(0.0, 0.05))
                .show_grid([false, true])
                .height(200.0)
                .allow_scroll(false)
                .allow_drag(false)
                .allow_zoom(false)
                .include_x(calendar.day(p.window.start) as f64)
                .include_x(calendar.day(Timestamp(p.window.end.0 - 1)) as f64 + 1.0)
                .include_y(0.0)
                .x_axis_formatter(widgets::day_axis)
                .y_grid_spacer(widgets::round_marks(unit))
                .y_axis_formatter(move |m, _| match metric {
                    2 => nexus_data::names::compact_credits(m.value.round() as i64),
                    3 => format!("{} h", (m.value * 10.0).round() / 10.0),
                    _ if m.value.fract() == 0.0 => format!("{}", m.value as i64),
                    _ => String::new(),
                })
                .show(ui, |pl| pl.bar_chart(BarChart::new("By day", bars).element_formatter(Box::new(|_, _| String::new()))));
            widgets::bar_tooltip(&plot, |day| {
                let date = LocalDays::label(day, "%d %b");
                Some(match days_of.get(&day) {
                    Some(d) => {
                        let mut parts = vec![plural(d.jumps as usize, "jump", "jumps")];
                        if d.ly > 0.0 {
                            parts.push(format!("{:.0} ly", d.ly));
                        }
                        if d.earned != 0 {
                            parts.push(format!("{}{} earned", if d.earned > 0 { "+" } else { "" }, short_credits(d.earned)));
                        }
                        if d.play_secs > 0 {
                            parts.push(format!("{} in the game", career::duration(d.play_secs)));
                        }
                        format!("{date}\n{}", parts.join("\n"))
                    }
                    None => format!("{date}\nnothing recorded"),
                })
            });
        },
    );
}

fn period_activities(ui: &mut Ui, gs: &GameState, p: &Period, span: &str) {
    let s = &p.now;
    let n = |v: usize| group_digits(v as i64);
    let mut list: Vec<Activity> = Vec::new();

    let mut a = Activity::new(icon::PATH, "Travel", pal::CYAN);
    if s.jumps > 0 || s.carrier_jumps > 0 || s.docks > 0 {
        a.headline = Some((format!("{:.0} ly", s.ly), plural(s.jumps, "jump in your own ships", "jumps in your own ships")));
        if s.systems > 0 {
            a.facts.push(("Systems jumped to".into(), format!("{} · {} new", n(s.systems), n(s.first_visits))));
        }
        if s.docks > 0 {
            a.facts.push(("Docked".into(), format!("{} at {}", plural(s.docks, "time", "times"), plural(s.stations, "station", "stations"))));
        }
        if s.carrier_jumps > 0 {
            a.facts.push(("Carrier jumps you rode".into(), n(s.carrier_jumps)));
        }
    }
    list.push(a);

    let mut a = Activity::new(icon::SCALES, "Trade", pal::GOLD);
    if s.sales > 0 {
        a.headline = Some((format!("{}{}", if s.trade_profit > 0 { "+" } else { "" }, short_credits(s.trade_profit)), "profit on cargo sold".into()));
        a.facts.push(("Sales".into(), format!("{} · {} t", n(s.sales), group_digits(s.tonnes_sold))));
        if s.tonnes_sold > 0 {
            a.facts.push(("Profit per tonne".into(), short_credits(s.trade_profit / s.tonnes_sold)));
        }
    } else if s.money.cargo_bought > 0 {
        a.headline = Some((short_credits(s.money.cargo_bought), "of cargo bought, none of it sold".into()));
    }
    if s.sales > 0 && s.money.cargo_bought > 0 {
        a.facts.push(("Cargo bought".into(), short_credits(s.money.cargo_bought)));
    }
    if s.money.unsold_cargo > 0 && s.sales > 0 {
        a.facts.push(("Bought and not sold".into(), short_credits(s.money.unsold_cargo)));
    }
    list.push(a);

    let mut a = Activity::new(icon::CROSSHAIR, "Combat", pal::PINK);
    if s.kills > 0 || s.deaths > 0 {
        a.headline = Some((n(s.kills), if s.kills == 1 { "kill for a bounty or a combat bond" } else { "kills for bounties and combat bonds" }.into()));
        if s.kill_rewards > 0 {
            a.facts.push(("Worth".into(), short_credits(s.kill_rewards)));
        }
        if s.bond_kills > 0 {
            a.facts.push(("For combat bonds".into(), n(s.bond_kills)));
        }
        let cashed: i64 = s.money.income.iter().filter(|f| matches!(f.category, LedgerCategory::Bounties | LedgerCategory::CombatBonds)).map(|f| f.amount).sum();
        if cashed > 0 {
            a.facts.push(("Handed in".into(), short_credits(cashed)));
        }
        a.facts.push(("Deaths".into(), n(s.deaths)));
    }
    list.push(a);

    let mut a = Activity::new(icon::COMPASS, "Exploration", pal::CYAN);
    if s.scanned > 0 || s.mapped > 0 || s.data_sold > 0 {
        a.headline = Some((n(s.scanned), "stars and planets scanned".into()));
        if s.first_discoveries > 0 {
            a.facts.push(("First discovered by you".into(), n(s.first_discoveries)));
        }
        if s.mapped > 0 {
            a.facts.push(("Planets mapped".into(), format!("{} · {} within target", n(s.mapped), n(s.efficient))));
        }
        if s.first_mapped > 0 {
            a.facts.push(("First mapped by you".into(), n(s.first_mapped)));
        }
        if s.codex_new > 0 {
            a.facts.push(("New codex entries".into(), n(s.codex_new)));
        }
        if s.data_sold > 0 {
            a.facts.push(("Data sold".into(), short_credits(s.data_sold)));
        }
    }
    list.push(a);

    let mut a = Activity::new(icon::PLANT, "Exobiology", pal::GOOD);
    if s.organics > 0 || s.organic_sold > 0 {
        a.headline = Some((n(s.organics), "species analysed".into()));
        if s.organic_value > 0 {
            a.facts.push(("Worth at Vista Genomics".into(), short_credits(s.organic_value)));
        }
        if s.organic_first > 0 {
            a.facts.push(("Likely logged first".into(), n(s.organic_first)));
        }
        if s.organic_sold > 0 {
            a.facts.push(("Data sold".into(), short_credits(s.organic_sold)));
        }
    }
    list.push(a);

    let mut a = Activity::new(icon::DIAMONDS_FOUR, "Mining", pal::ACCENT);
    if s.refined > 0 || s.prospected > 0 || s.mined_sold > 0 {
        a.headline = Some((format!("{} t", group_digits(s.refined)), "refined".into()));
        if s.mining_sessions > 0 {
            a.facts.push(("Mining sessions".into(), n(s.mining_sessions)));
        }
        if s.prospected > 0 {
            a.facts.push(("Asteroids prospected".into(), group_digits(i64::from(s.prospected))));
        }
        if s.cracked > 0 {
            a.facts.push(("Asteroids cracked".into(), group_digits(i64::from(s.cracked))));
        }
        if s.mined_sold > 0 {
            a.facts.push(("Mined and found cargo sold".into(), format!("{} t", group_digits(s.mined_sold))));
        }
    }
    list.push(a);

    let mut a = Activity::new(icon::TARGET, "Missions", pal::PURPLE);
    if s.missions_completed + s.missions_failed + s.missions_abandoned > 0 {
        a.headline = Some((n(s.missions_completed), "completed".into()));
        if s.mission_rewards > 0 {
            a.facts.push(("Rewards".into(), short_credits(s.mission_rewards)));
        }
        if s.missions_failed > 0 {
            a.facts.push(("Failed".into(), n(s.missions_failed)));
        }
        if s.missions_abandoned > 0 {
            a.facts.push(("Abandoned".into(), n(s.missions_abandoned)));
        }
    }
    list.push(a);

    let (shown, quiet): (Vec<Activity>, Vec<Activity>) = list.into_iter().partition(Activity::any);
    if shown.is_empty() {
        let why = why_empty(gs, p.window).unwrap_or_else(|| "You played, but none of what these cards count: no jumps, docking, trade, kills, scans, samples, mining or missions.".into());
        widgets::empty(ui, icon::CALENDAR_BLANK, &format!("Nothing in {span}"), &why);
        return;
    }
    widgets::masonry(ui, 3, 290.0, &shown, |ui, a| activity_card(ui, gs, a));
    if !quiet.is_empty() {
        let names: Vec<String> = quiet.iter().map(|a| a.title.to_lowercase()).collect();
        footnote(ui, &format!("No {} in {span}. Ranks and career totals are on the Career tab.", names.join(", ")));
    }
}

// ---------------------------------------------------------------------------------------------
// Ships

/// A ship's row: its summary and what it's called.
struct ShipRow {
    s: ShipSummary,
    /// The name it shows under: its own name, or its type.
    title: String,
    /// Its type and ident, or what the row stands for when it isn't a ship.
    detail: String,
    /// Still in the fleet (not sold).
    owned: bool,
    /// When it was bought and sold, as far as the journals go.
    bought: Option<Timestamp>,
    sold: Option<Timestamp>,
}

fn ship_row(gs: &GameState, s: ShipSummary) -> ShipRow {
    let Some(id) = s.ship else {
        return ShipRow { s, title: "Out of your ships".into(), detail: "on foot, in taxis and dropships, crewing for others".into(), owned: true, bought: None, sold: None };
    };
    let (ty, name, ident) = nexus_model::ships::ship_names(gs, id);
    let ty = if ty.is_empty() { "A ship".to_string() } else { ty };
    let title = if name.trim().is_empty() { ty.clone() } else { name.trim().to_string() };
    let detail = match (title == ty, ident.is_empty()) {
        (true, true) => String::new(),
        (true, false) => ident,
        (false, true) => ty,
        (false, false) => format!("{ty} · {ident}"),
    };
    let rec = gs.history.ships.get(&id);
    ShipRow { s, title, detail, owned: gs.fleet.ships.contains_key(&id), bought: rec.and_then(|r| r.bought).map(|b| b.0), sold: rec.and_then(|r| r.sold).map(|x| x.0) }
}

fn ships_tab(ui: &mut Ui, gs: &GameState) {
    let now = Timestamp::now();
    let (days, window) = pick_period(ui, gs, now);
    let span = if days == 0 { "all time".to_string() } else { format!("the last {days} days") };
    let coverage = match gs.history.first() {
        Some(f) => format!("From your journals, back to {}.", f.to_local_string("%d %b %Y")),
        None => "From your journals, as Elite Nexus reads them.".into(),
    };
    footnote(ui, &format!("{coverage} Time counts for the ship you were in, and what you did on foot beside it (sampling on its planet, selling where it's docked) counts for it too. Taxis, dropships, crewing for others, kills on foot and walks elsewhere have a row of their own."));
    ui.add_space(6.0);
    let summaries = cached(ui, "ships", (&gs.cmdr.fid, &gs.cmdr.name, days, window.start, now.0 / 10), || nexus_model::ships::per_ship(gs, window));
    let rows: Vec<ShipRow> = summaries.iter().cloned().map(|s| ship_row(gs, s)).collect();
    if rows.iter().all(|r| r.s.ship.is_none()) {
        let why = why_empty(gs, window).unwrap_or_else(|| "Your journals don't show you in one of your ships in it.".into());
        widgets::empty(ui, icon::ROCKET_LAUNCH, &format!("No ship flown in {span}"), &why);
        return;
    }
    let ships: Vec<&ShipRow> = rows.iter().filter(|r| r.s.ship.is_some()).collect();
    // The ship that did most by a measure, and how much.
    let top = |f: &dyn Fn(&ShipSummary) -> f64| ships.iter().copied().filter(|r| f(&r.s) > 0.0).max_by(|a, b| f(&a.s).total_cmp(&f(&b.s)));
    tile_row(ui, |cs| {
        cs.add(|ui| match top(&|s| s.play_secs as f64) {
            Some(r) => tile(ui, icon::CLOCK, "Most time in", pal::PURPLE, &r.title, pal::TEXT, &[(career::duration(r.s.play_secs), pal::DIM)]),
            None => tile(ui, icon::CLOCK, "Most time in", pal::PURPLE, "—", pal::DIM, &[]),
        });
        cs.add(|ui| match top(&|s| s.earned() as f64) {
            Some(r) => tile(ui, icon::HAND_COINS, "Earned most", pal::GOOD, &r.title, pal::TEXT, &[(format!("+{}", short_credits(r.s.earned())), pal::GOOD)]),
            None => tile(ui, icon::HAND_COINS, "Earned most", pal::GOOD, "—", pal::DIM, &[("nothing earned in a ship".into(), pal::DIM)]),
        });
        cs.add(|ui| match top(&|s| s.ly) {
            Some(r) => tile(ui, icon::PATH, "Jumped farthest", pal::CYAN, &r.title, pal::TEXT, &[(format!("{:.0} ly in {}", r.s.ly, plural(r.s.jumps, "jump", "jumps")), pal::DIM)]),
            None => tile(ui, icon::PATH, "Jumped farthest", pal::CYAN, "—", pal::DIM, &[("no jumps in your ships".into(), pal::DIM)]),
        });
        cs.add(|ui| {
            let gone = ships.iter().filter(|r| !r.owned).count();
            let line = if gone > 0 { format!("{} no longer in your fleet", group_digits(gone as i64)) } else { format!("of the {} in your fleet", group_digits(gs.fleet.ships.len() as i64)) };
            tile(ui, icon::ROCKET_LAUNCH, "Ships flown", pal::INDIGO, &group_digits(ships.len() as i64), pal::TEXT, &[(line, pal::DIM)]);
        });
    });
    ui.add_space(GAP);
    // The ship picked in the table, or the one with the most time.
    let sel_id = egui::Id::new("stats-ship");
    let picked: Option<Option<u64>> = ui.data(|d| d.get_temp::<Option<u64>>(sel_id));
    let selected = picked.filter(|p| rows.iter().any(|r| r.s.ship == *p)).unwrap_or(rows[0].s.ship);
    let mut clicked = None;
    widgets::card(ui, "Ship by ship", |ui| clicked = ships_table(ui, &rows, selected));
    if let Some(c) = clicked {
        ui.data_mut(|d| d.insert_temp(sel_id, c));
    }
    ui.add_space(GAP);
    if let Some(r) = rows.iter().find(|r| r.s.ship == clicked.unwrap_or(selected)) {
        ship_detail(ui, gs, r);
    }
    footnote(
        ui,
        "A sale's profit counts for the ship that sold the cargo, and a bounty or a combat bond for the ship that made the kill, when it was made (a voucher can still be lost before it's handed in). Data, mission rewards and the rest count for the ship you were paid with, wherever the work was done. Time counts while it was the ship you were in, docked or flying.",
    );
}

/// The table of ships; returns the row clicked.
fn ships_table(ui: &mut Ui, rows: &[ShipRow], selected: Option<u64>) -> Option<Option<u64>> {
    use egui_extras::Column;
    const TABLE: &str = "stats-ships";
    // Ten columns need about 960 points (the ship's at its narrowest): below that, the dates go.
    let wide = ui.available_width() >= 980.0;
    let mut sort = widgets::TableSort::load(ui, TABLE);
    let mut clicked = None;
    {
        // Columns of fixed widths, the ship's taking the rest; the page scrolls, so the table doesn't scroll inside it.
        let mut table = egui_extras::TableBuilder::new(ui)
            .id_salt(if wide { "stats-ships/wide" } else { "stats-ships/narrow" })
            .striped(true)
            .resizable(false)
            .vscroll(false)
            .sense(egui::Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::remainder().at_least(170.0).clip(true))
            .column(Column::exact(84.0).clip(true))
            .column(Column::exact(60.0).clip(true))
            .column(Column::exact(80.0).clip(true))
            .column(Column::exact(92.0).clip(true))
            .column(Column::exact(56.0).clip(true))
            .column(Column::exact(62.0).clip(true))
            .column(Column::exact(84.0).clip(true));
        if wide {
            table = table.column(Column::exact(100.0).clip(true)).column(Column::exact(100.0).clip(true));
        }
        // Sortable columns: 0 ship, 1 time, 2 jumps, 3 distance, 4 earned, 5 kills, 6 deaths, 7 costs, 8 bought, 9 sold.
        let mut heads: Vec<(&str, u8, bool)> =
            vec![("Ship", 0, false), ("Time", 1, true), ("Jumps", 2, true), ("Distance", 3, true), ("Earned", 4, true), ("Kills", 5, true), ("Deaths", 6, true), ("Costs", 7, true)];
        if wide {
            heads.extend([("Bought", 8, true), ("Sold", 9, true)]);
        }
        let table = table.header(28.0, |mut h| {
            for (text, col, desc) in &heads {
                h.col(|ui| widgets::th_sort(ui, text, *col, &mut sort, TABLE, *desc));
            }
        });
        let order = sort.order(rows, |a, b, c| match c {
            0 => widgets::cmp_text(&a.title, &b.title),
            1 => a.s.play_secs.cmp(&b.s.play_secs),
            2 => a.s.jumps.cmp(&b.s.jumps),
            3 => a.s.ly.total_cmp(&b.s.ly),
            4 => a.s.earned().cmp(&b.s.earned()),
            5 => a.s.kills.cmp(&b.s.kills),
            6 => a.s.deaths.cmp(&b.s.deaths),
            7 => a.s.costs().cmp(&b.s.costs()),
            8 => a.bought.cmp(&b.bought),
            _ => a.sold.cmp(&b.sold),
        });
        table.body(|body| {
            body.rows(44.0, rows.len(), |mut row| {
                let r = &rows[order[row.index()]];
                row.set_selected(r.s.ship == selected);
                let out = r.s.ship.is_none();
                row.col(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&r.title).color(if out { pal::DIM } else { pal::TEXT })).truncate());
                            if r.sold.is_some() {
                                widgets::badge(ui, "sold", pal::GRAY);
                            } else if !r.owned {
                                widgets::badge(ui, "gone", pal::GRAY).on_hover_text("No longer in your fleet, and your journals don't show it sold");
                            }
                        });
                        if !r.detail.is_empty() {
                            ui.add(egui::Label::new(RichText::new(&r.detail).size(size::FOOTNOTE).color(pal::DIM)).truncate());
                        }
                    });
                });
                // A dash, in grey, for nothing.
                let num = |ui: &mut Ui, text: String, colour: Color32| {
                    let colour = if text == "—" { pal::FAINT } else { colour };
                    ui.label(RichText::new(text).color(colour));
                };
                let blank_or = |v: bool, text: String| if v { text } else { "—".into() };
                row.col(|ui| num(ui, blank_or(r.s.play_secs > 0, career::duration(r.s.play_secs)), pal::TEXT));
                row.col(|ui| num(ui, blank_or(r.s.jumps > 0, group_digits(r.s.jumps as i64)), pal::TEXT));
                row.col(|ui| num(ui, blank_or(r.s.ly > 0.0, format!("{:.0} ly", r.s.ly)), pal::TEXT));
                row.col(|ui| num(ui, blank_or(r.s.earned() != 0, short_credits(r.s.earned())), if r.s.earned() >= 0 { pal::GOOD } else { pal::BAD }));
                row.col(|ui| num(ui, blank_or(r.s.kills > 0, group_digits(r.s.kills as i64)), pal::TEXT));
                row.col(|ui| num(ui, blank_or(r.s.deaths > 0, group_digits(r.s.deaths as i64)), if r.s.deaths > 0 { pal::BAD } else { pal::TEXT }));
                row.col(|ui| num(ui, blank_or(r.s.costs() > 0, short_credits(r.s.costs())), pal::DIM));
                if wide {
                    // Dates the journals hold; a ship bought before them says so on hover.
                    let date = |t: Option<Timestamp>| t.map(|t| t.to_local_string("%d %b %Y"));
                    row.col(|ui| match (date(r.bought), out) {
                        (Some(d), _) => num(ui, d, pal::DIM),
                        (None, true) => num(ui, String::new(), pal::DIM),
                        (None, false) => {
                            ui.label(RichText::new("—").color(pal::FAINT))
                                .on_hover_text("No purchase in your journals: bought before them, or a ship you didn't buy (the one you started with, a free one after a rebuy you couldn't pay)");
                        }
                    });
                    row.col(|ui| num(ui, date(r.sold).unwrap_or_default(), pal::DIM));
                }
                if row.response().clicked() {
                    clicked = Some(r.s.ship);
                }
            });
        });
    }
    clicked
}

/// One ship in full: what it did, where its credits came from, what it cost, and when it joined and left the fleet.
fn ship_detail(ui: &mut Ui, gs: &GameState, r: &ShipRow) {
    let s = &r.s;
    let title = if r.detail.is_empty() { r.title.clone() } else { format!("{} · {}", r.title, r.detail) };
    card_tinted(ui, if s.ship.is_some() { icon::ROCKET_LAUNCH } else { icon::PERSON_SIMPLE_WALK }, &title, pal::INDIGO, |ui| {
        if let Some(id) = s.ship {
            let rec = gs.history.ships.get(&id);
            let mut life = Vec::new();
            match rec.and_then(|x| x.bought) {
                Some((t, price)) => life.push(format!("Bought on {} for {}", t.to_local_string("%d %b %Y"), short_credits(price))),
                None => life.push("No purchase in your journals (bought before them, or not bought at all)".into()),
            }
            match rec.and_then(|x| x.sold) {
                Some((t, Some(price))) => life.push(format!("sold on {} for {}", t.to_local_string("%d %b %Y"), short_credits(price))),
                Some((t, None)) => life.push(format!("sold on {} (the journal gives no price)", t.to_local_string("%d %b %Y"))),
                None if r.owned => life.push("in your fleet now".into()),
                None => life.push("no longer in your fleet".into()),
            }
            ui.label(RichText::new(life.join(", ")).color(pal::DIM));
            ui.add_space(6.0);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 26.0;
            widgets::stat(ui, "in the game", career::duration(s.play_secs), pal::TEXT);
            widgets::stat(ui, "jumps", group_digits(s.jumps as i64), pal::TEXT);
            widgets::stat(ui, "light years", format!("{:.0}", s.ly), pal::CYAN);
            let (earned, costs) = (s.earned(), s.costs());
            widgets::stat(ui, "earned", if earned > 0 { format!("+{}", short_credits(earned)) } else { short_credits(earned) }, if earned >= 0 { pal::GOOD } else { pal::BAD });
            widgets::stat(ui, "costs", if costs > 0 { format!("-{}", short_credits(costs)) } else { short_credits(0) }, if costs > 0 { pal::BAD } else { pal::DIM });
        });
        ui.add_space(10.0);
        // The parts above zero, biggest first.
        let parts = |list: &[(&str, i64, Color32)]| -> Vec<Part> {
            let mut v: Vec<Part> = list.iter().filter(|(_, a, _)| *a > 0).map(|(l, a, c)| Part { label: (*l).into(), amount: *a, colour: *c }).collect();
            v.sort_by_key(|p| std::cmp::Reverse(p.amount));
            v
        };
        let earned = parts(&[
            ("Trade profit", s.trade, pal::GOLD),
            ("Bounties and bonds, at the kill", s.combat, pal::PINK),
            ("Exploration data", s.exploration, pal::CYAN),
            ("Exobiology data", s.exobiology, pal::GOOD),
            ("Mined and found cargo", s.mining, pal::ACCENT),
            ("Mission rewards", s.missions, pal::PURPLE),
        ]);
        let costs = parts(&[("Rebuys", s.rebuys, pal::BAD), ("Repairs", s.repairs, pal::WARN), ("Fuel", s.fuel, pal::MINT), ("Ammunition, limpets and restocking", s.ammo, pal::ACCENT_SOFT), ("Trade at a loss", -s.trade, pal::GOLD)]);
        let mut did: Vec<(String, String)> = Vec::new();
        let count = |n: usize| group_digits(n as i64);
        if s.kills > 0 {
            did.push(("Kills for bounties and combat bonds".into(), count(s.kills)));
        }
        if s.deaths > 0 {
            did.push(("Deaths".into(), count(s.deaths)));
        }
        if s.scanned > 0 {
            did.push(("Stars and planets scanned".into(), count(s.scanned)));
        }
        if s.mapped > 0 {
            did.push(("Planets mapped".into(), count(s.mapped)));
        }
        if s.organics > 0 {
            did.push(("Species analysed".into(), count(s.organics)));
        }
        if s.refined > 0 {
            did.push(("Tonnes refined".into(), group_digits(s.refined)));
        }
        widgets::split(
            ui,
            0.5,
            300.0,
            |ui| {
                ui.label(RichText::new("Where its credits came from").font(display(size::FOOTNOTE)).color(pal::DIM));
                if earned.is_empty() {
                    ui.label(RichText::new("Nothing earned in it in this period.").color(pal::DIM));
                } else {
                    breakdown(ui, &earned, 6);
                }
            },
            |ui| {
                ui.label(RichText::new("What it cost").font(display(size::FOOTNOTE)).color(pal::DIM));
                if costs.is_empty() {
                    ui.label(RichText::new("Nothing paid for it in this period.").color(pal::DIM));
                } else {
                    breakdown(ui, &costs, 4);
                }
                if !did.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("What it did").font(display(size::FOOTNOTE)).color(pal::DIM));
                    for (label, value) in &did {
                        fact(ui, label, value);
                    }
                }
            },
        );
    });
}

// ---------------------------------------------------------------------------------------------
// All statistics

fn all_tab(ui: &mut Ui, gs: &GameState) {
    let Some(stats) = gs.cmdr.statistics.as_deref() else {
        widgets::empty(ui, icon::LIST_BULLETS, "No statistics yet", "The game writes them each time you log in: start Elite Dangerous and load your commander.");
        return;
    };
    let c = Career::new(stats);
    let zeros_id = egui::Id::new("stats-show-zeros");
    let search_id = egui::Id::new("stats-search");
    let mut zeros: bool = ui.data_mut(|d| *d.get_persisted_mut_or(zeros_id, false));
    let mut search: String = ui.data(|d| d.get_temp::<String>(search_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        if widgets::search_field(ui, &mut search, "Find a statistic", 280.0).changed() {
            ui.data_mut(|d| d.insert_temp(search_id, search.clone()));
        }
        ui.add_space(12.0);
        if widgets::toggle(ui, &mut zeros, "Show zeros").changed() {
            ui.data_mut(|d| d.insert_persisted(zeros_id, zeros));
        }
    });
    let when = as_of(gs);
    footnote(
        ui,
        &format!(
            "Every figure the game keeps about your career{}, in plain words. The game updates them only at login, so today's play shows from your next one.",
            if when.is_empty() { String::new() } else { format!(", as it wrote them at your last login ({when})") }
        ),
    );
    ui.add_space(6.0);
    let needle = search.trim().to_lowercase();
    let mut quiet: Vec<&str> = Vec::new();
    let mut groups: Vec<(&str, Vec<career::Figure>, usize)> = Vec::new();
    for g in career::GROUPS {
        let figures = c.figures(g.key);
        if figures.is_empty() {
            continue;
        }
        let matching: Vec<career::Figure> = figures.into_iter().filter(|f| needle.is_empty() || f.label.to_lowercase().contains(&needle) || g.title.to_lowercase().contains(&needle)).collect();
        let zero = matching.iter().filter(|f| f.is_zero()).count();
        if !matching.is_empty() && zero == matching.len() && !zeros {
            quiet.push(g.title);
            continue;
        }
        if !matching.is_empty() {
            groups.push((g.title, matching, zero));
        }
    }
    if groups.is_empty() {
        let hint = match (needle.is_empty(), quiet.is_empty()) {
            (true, _) => "Every figure the game wrote is zero so far: turn on Show zeros to list them.".to_string(),
            (false, false) => format!("Only figures at zero match \u{201c}{}\u{201d}: turn on Show zeros to see them.", search.trim()),
            (false, true) => format!("No statistic matches \u{201c}{}\u{201d}.", search.trim()),
        };
        widgets::empty(ui, icon::MAGNIFYING_GLASS, "Nothing to show", &hint);
        return;
    }
    widgets::masonry(ui, 3, 300.0, &groups, |ui, (title, figures, zero)| {
        widgets::card(ui, title, |ui| {
            for f in figures.iter().filter(|f| zeros || !f.is_zero()) {
                fact(ui, &f.label, &f.shown());
            }
            if !zeros && *zero > 0 {
                ui.add_space(2.0);
                ui.label(RichText::new(format!("and {zero} at zero")).size(size::FOOTNOTE).color(pal::FAINT));
            }
        });
    });
    if !quiet.is_empty() {
        let what = if needle.is_empty() { "All at zero" } else { "Matching, but all at zero" };
        footnote(ui, &format!("{what}: {}. Show zeros lists them.", quiet.join(", ")));
    }
}

// ---------------------------------------------------------------------------------------------
// The balance chart's axis

/// The balance as a step line over [`start`, `now`], as (unix seconds, credits): it starts at the balance held when
/// the period began (the last sample before it) and runs to today at `current`.
pub(crate) fn credit_steps(samples: &[(Timestamp, i64)], start: i64, now: i64, current: i64) -> Vec<(i64, i64)> {
    let mut pts: Vec<(i64, i64)> = Vec::new();
    let before = samples.iter().rev().find(|(t, _)| t.0 < start).map(|(_, v)| *v);
    let mut last = before;
    if let Some(v) = before {
        pts.push((start, v));
    }
    for (t, v) in samples.iter().filter(|(t, _)| t.0 >= start && t.0 <= now) {
        if let Some(prev) = last {
            pts.push((t.0, prev));
        }
        pts.push((t.0, *v));
        last = Some(*v);
    }
    if let Some(prev) = last {
        pts.push((now, prev));
        if prev != current {
            pts.push((now, current));
        }
    }
    pts
}

/// The y range the balance chart shows: its line with a little room above and below, so it isn't drawn on the edge.
/// The labels keep three figures ("1.23 B"): a balance that didn't move spanned a tenth of a credit, and every label
/// read "1.23 B". So a flat or nearly flat line gets a band of a tenth of its value above and below it, which labels of
/// two figures ("1.2 M", "12 k", the coarsest they get) still tell apart. On the log scale the line holds powers of
/// ten, and a tenth of one is room enough.
fn credit_y_range(lo: f64, hi: f64, log: bool) -> (f64, f64) {
    if log {
        return (lo - 0.1, hi + 0.1);
    }
    let span = ((hi - lo) * 1.1).max(lo.abs().max(hi.abs()) * 0.2).max(20.0);
    let mid = (lo + hi) / 2.0;
    (mid - span / 2.0, mid + span / 2.0)
}

/// The finest step the balance chart's labels (`compact_credits`) tell apart up to `v`: 0.01 B among billions, 0.1 M
/// among millions, 1 k from 10 k, single credits below that.
fn credit_resolution(v: f64) -> f64 {
    match v.abs() {
        a if a >= 1e9 => 1e7,
        a if a >= 1e6 => 1e5,
        a if a >= 1e4 => 1e3,
        _ => 1.0,
    }
}

/// The marks up the balance chart's side, only within `band` (its y range: a label in the room egui_plot adds around
/// it sits on the chart's edge and runs into the dates), at steps its labels tell apart.
fn credit_marks(band: (f64, f64), log: bool) -> impl Fn(GridInput) -> Vec<GridMark> {
    move |mut input| {
        input.bounds = (input.bounds.0.max(band.0), input.bounds.1.min(band.1));
        if log { log_credit_marks(&input) } else { widgets::round_marks(credit_resolution(band.0.abs().max(band.1.abs())))(input) }
    }
}

/// Marks on the log scale, where the chart holds log10 of the balance (labelling whole powers of ten only left a
/// balance between 2 and 5 billion with no label at all): powers of ten, then 2 and 5 times one, then 3, 4, 6, 7, 8
/// and 9 times one, each where it keeps its gap from those already there. Within less than a power of ten, a round
/// step in credits takes over where it gives as many: the finest whose top two marks, the closest on this scale, keep
/// the gap.
fn log_credit_marks(input: &GridInput) -> Vec<GridMark> {
    use crate::widgets::{marks_every, round_step, value_per_point, LABEL_GAP};
    let gap = value_per_point(input) * LABEL_GAP;
    let (lo, hi) = input.bounds;
    if !(gap > 0.0 && lo <= hi && hi - lo < 1_000.0) {
        return Vec::new();
    }
    let mut values: Vec<f64> = Vec::new();
    for tier in [&[1.0][..], &[2.0, 5.0], &[3.0, 4.0, 6.0, 7.0, 8.0, 9.0]] {
        // From 1 Cr up: the line holds nothing below.
        for power in (lo.floor() as i64).max(0)..=hi.ceil() as i64 {
            for m in tier {
                let v = power as f64 + f64::log10(*m);
                if (lo..=hi).contains(&v) && values.iter().all(|c| (c - v).abs() >= gap) {
                    values.push(v);
                }
            }
        }
    }
    if hi - lo < 1.0 {
        let top = 10f64.powf(hi);
        let step = round_step(top * (1.0 - 10f64.powf(-gap)), credit_resolution(top));
        let even: Vec<f64> = marks_every((10f64.powf(lo), top), step).into_iter().filter(|m| m.value >= 1.0).map(|m| m.value.log10()).collect();
        if even.len() >= values.len() {
            values = even;
        }
    }
    values.sort_by(f64::total_cmp);
    values.into_iter().map(|value| GridMark { value, step_size: gap }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_credit_line_steps_through_the_period() {
        let d = 86_400;
        let samples = [(Timestamp(10 * d), 100), (Timestamp(20 * d), 300), (Timestamp(25 * d), 200)];
        // From day 15: starts at the balance held then (100), steps at days 20 and 25, runs to today.
        let pts = credit_steps(&samples, 15 * d, 30 * d, 250);
        let days: Vec<(i64, i64)> = pts.iter().map(|&(t, v)| (t / d, v)).collect();
        assert_eq!(days, vec![(15, 100), (20, 100), (20, 300), (25, 300), (25, 200), (30, 200), (30, 250)]);
        // All time: from the first sample.
        let pts = credit_steps(&samples, 10 * d, 30 * d, 200);
        assert_eq!(pts.first(), Some(&(10 * d, 100)));
        assert_eq!(pts.last(), Some(&(30 * d, 200)));
        // Nothing before or in the period: nothing to draw.
        assert!(credit_steps(&[], 0, 30 * d, 5).is_empty());
    }

    #[test]
    fn a_period_is_whole_calendar_days() {
        // "7 days" is today and the six days before, from midnight on the commander's own clock; all time starts at the
        // first record.
        let now = Timestamp::parse("2026-11-05T15:30:00Z").expect("time");
        let mut cal = LocalDays::default();
        let start = range_start(7, now, Some(Timestamp(3)));
        assert_eq!(cal.day(start), cal.day(now) - 6);
        assert_eq!(cal.day(Timestamp(start.0 - 1)), cal.day(now) - 7, "it starts at a midnight");
        assert_eq!(range_start(0, now, Some(Timestamp(3))), Timestamp(3));
        assert_eq!(range_start(0, now, None), now);
    }

    /// The labels up the Credits chart, `height` points tall, for a line from `lo` to `hi` (log10 of the balance on the
    /// log scale): egui_plot adds 5 % of room above and below the chart's range, and labels the marks it's given. The
    /// chart is 240 points tall with its dates, about 223 without.
    fn labels((lo, hi): (f64, f64), log: bool, height: f64) -> Vec<String> {
        let band = credit_y_range(lo, hi, log);
        let room = (band.1 - band.0) * 0.05;
        let input = || crate::widgets::grid_input((band.0 - room, band.1 + room), height);
        let credits = |v: f64| if log { 10f64.powf(v) } else { v };
        crate::widgets::labelled(credit_marks(band, log)(input()), &input()).into_iter().map(|v| nexus_data::names::compact_credits(credits(v).round() as i64)).collect()
    }

    #[test]
    fn a_flat_balance_gets_labels_that_differ() {
        // What a flat balance used to get: a tenth of a credit, where egui_plot's own marks wrote "1.23 B" on every line.
        let input = || crate::widgets::grid_input((1_234_567_889.945, 1_234_567_890.055), 223.0);
        let old: Vec<String> =
            crate::widgets::labelled(egui_plot::log_grid_spacer(10)(input()), &input()).into_iter().filter(|v| (1_234_567_889.95..=1_234_567_890.05).contains(v)).map(|v| nexus_data::names::compact_credits(v.round() as i64)).collect();
        assert!(old.len() > 2 && old.iter().all(|l| l == "1.23 B"), "{old:?}");
        // Balances from nothing to a trillion, flat and moving by less than the labels show: two labels at least, all
        // different.
        let mut v = 0.0;
        while v < 1e12 {
            for (lo, hi) in [(v, v), (v, v * 1.001 + 1.0)] {
                for height in [200.0, 223.0, 300.0] {
                    let l = labels((lo, hi), false, height);
                    assert!(l.len() >= 2 && l.windows(2).all(|w| w[0] != w[1]), "{lo}..{hi} at {height} pt: {l:?}");
                }
            }
            v = (v * 1.07 + 1.0).round();
        }
        // A flat line sits in the middle of a band a fifth of its value tall, one that moves keeps 5 % of its move
        // above and below, and on the log scale a tenth of a power of ten.
        let near = |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-3 && (b - d).abs() < 1e-3;
        assert!(near(credit_y_range(2e9, 2e9, false), (1.8e9, 2.2e9)));
        assert!(near(credit_y_range(100e6, 300e6, false), (90e6, 310e6)));
        assert!(near(credit_y_range(8.0, 9.0, true), (7.9, 9.1)));
    }

    #[test]
    fn the_log_scale_always_has_labels() {
        // A balance between 2.1 and 4.6 billion reaches no power of ten: labelled at whole powers only, it had none.
        assert_eq!(labels((2.1e9f64.log10(), 4.6e9f64.log10()), true, 223.0), ["2.00 B", "3.00 B", "4.00 B", "5.00 B"]);
        // A climb over powers of ten is labelled at each one, or at 1, 2 and 5 times one where there's room.
        assert_eq!(labels((3.0, 9.0), true, 223.0), ["1000", "10 k", "100 k", "1.0 M", "10.0 M", "100.0 M", "1.00 B"]);
        assert_eq!(labels((6.0, 3e7f64.log10()), true, 223.0), ["1.0 M", "2.0 M", "5.0 M", "10.0 M", "20.0 M"]);
        // From 100 Cr to a trillion, flat or climbing up to ten thousandfold: two labels at least, all different.
        let mut v = 100.0;
        while v < 1e12 {
            for k in [1.0, 1.001, 2.5, 30.0, 1e4] {
                for height in [200.0, 223.0, 300.0] {
                    let l = labels((f64::log10(v), f64::log10(v * k)), true, height);
                    assert!(l.len() >= 2 && l.windows(2).all(|w| w[0] != w[1]), "{v} x{k} at {height} pt: {l:?}");
                }
            }
            v = (v * 1.07).round();
        }
    }
}
