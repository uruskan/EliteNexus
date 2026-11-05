use crate::pages::where_to_get::{Goto, Sources, WhereToGet};
use crate::theme::{display, pal, size};
use crate::widgets::{badge, badge_size, bar, button, card, card_with, duration, empty, footnote, short_credits};
use egui::{Color32, Grid, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::missions::{Mission, MissionOutcome, MissionRecord};
use nexus_model::GameState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The Missions page's state: "where to get it" for the goods your missions ask for, and how to take back a Clear just
/// made.
#[derive(Default)]
pub struct MissionsUi {
    get: WhereToGet,
    undo: Option<Undo>,
}

/// How to take back a Clear: whose history it was (as [`commander_key`] names them), and where it had been cleared up
/// to before (None: nowhere).
#[derive(Debug, Clone, PartialEq)]
struct Undo {
    cmdr: String,
    before: Option<Timestamp>,
}

/// What the Missions page keeps in settings.json.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MissionsPrefs {
    /// How far each commander cleared their recent history (by FID, or by name where the journals give none): when the
    /// last mission it held then ended. The list leaves out the missions that ended then or before. The history is read
    /// from the journals again at every start, so this is what keeps them out of it; the journals aren't touched.
    pub cleared: BTreeMap<String, Timestamp>,
}

impl MissionsPrefs {
    /// Where `cmdr`'s history was cleared up to (None: it never was, or it's shown again).
    pub fn cleared(&self, cmdr: &str) -> Option<Timestamp> {
        self.cleared.get(cmdr).copied()
    }

    /// Clears `cmdr`'s history: every mission in `history` is left out of the list from now on, and one that ends
    /// later shows. Returns how to take it back, or None when there was nothing to clear.
    fn clear(&mut self, cmdr: &str, history: &[MissionRecord]) -> Option<Undo> {
        let newest = history.iter().map(|r| r.ts).max()?;
        let before = self.cleared(cmdr);
        if before.is_some_and(|b| b >= newest) {
            return None;
        }
        self.cleared.insert(cmdr.to_string(), newest);
        Some(Undo { cmdr: cmdr.to_string(), before })
    }

    /// Takes back a Clear: the history is hidden up to where it was before.
    fn undo(&mut self, u: &Undo) {
        match u.before {
            Some(t) => {
                self.cleared.insert(u.cmdr.clone(), t);
            }
            None => {
                self.cleared.remove(&u.cmdr);
            }
        }
    }

    /// Shows all of `cmdr`'s history again.
    fn show_all(&mut self, cmdr: &str) {
        self.cleared.remove(cmdr);
    }
}

/// Whose settings these are, as the model tells its commanders apart: the FID, or the name in journals that give none.
fn commander_key(gs: &GameState) -> &str {
    if gs.cmdr.fid.is_empty() { &gs.cmdr.name } else { &gs.cmdr.fid }
}

/// The missions the recent history lists, oldest first: those that ended after it was last cleared.
fn shown(history: &[MissionRecord], cleared: Option<Timestamp>) -> impl Iterator<Item = &MissionRecord> {
    history.iter().filter(move |r| cleared.is_none_or(|c| r.ts > c))
}

/// Returns true when `prefs` changed (to be saved), and what a "where to get it" panel asks the Market page to open.
pub fn show(ui: &mut Ui, gs: &GameState, st: &mut MissionsUi, prefs: &mut MissionsPrefs, src: &Sources) -> (bool, Option<Goto>) {
    let stacks = gs.missions.massacre_stacks();
    if !stacks.is_empty() {
        for s in &stacks {
            card(ui, &format!("Massacre stack — {}", s.target_faction), |ui| {
                ui.horizontal(|ui| {
                    crate::widgets::stat(ui, "kills remaining", format!("{} / {}", s.kills_remaining, s.kills_total), pal::ACCENT);
                    ui.add_space(16.0);
                    crate::widgets::stat(ui, "missions", s.missions.to_string(), pal::TEXT);
                    ui.add_space(16.0);
                    crate::widgets::stat(ui, "total reward", short_credits(s.total_reward), pal::GOLD);
                    if let Some(sys) = &s.target_system {
                        ui.add_space(16.0);
                        crate::widgets::stat(ui, "target system", sys.clone(), pal::CYAN);
                    }
                });
                let done = (s.kills_total - s.kills_remaining) as f32 / s.kills_total.max(1) as f32;
                bar(ui, done, pal::ACCENT, 10.0, None);
                ui.add_space(4.0);
                Grid::new(("stack", &s.target_faction)).num_columns(4).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
                    for g in &s.givers {
                        ui.label(&g.faction);
                        ui.label(RichText::new(format!("{} missions", g.missions)).color(pal::DIM));
                        ui.label(RichText::new(format!("{}/{} kills", g.kills_done, g.kills_needed)).color(if g.kills_done >= g.kills_needed { pal::GOOD } else { pal::TEXT }));
                        ui.label(RichText::new(short_credits(g.reward)).color(pal::GOLD));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Each kill counts once for every issuing faction (its oldest open mission). The stack is done after the largest per-faction requirement.").small().color(pal::DIM));
            });
            ui.add_space(8.0);
        }
    }

    card(ui, &format!("Active missions ({})", gs.missions.active.len()), |ui| {
        if gs.missions.active.is_empty() {
            empty(ui, icon::TARGET, "No active missions", "");
            return;
        }
        let now = Timestamp::now();
        let mut list: Vec<_> = gs.missions.active.values().collect();
        list.sort_by_key(|m| m.expiry.map(|e| e.0).unwrap_or(i64::MAX));
        let destination = |m: &Mission| match (&m.destination_station, &m.destination_system) {
            (Some(st), Some(sys)) => format!("{st} ({sys})"),
            (None, Some(sys)) => sys.clone(),
            _ => m.destination_settlement.clone().unwrap_or_default(),
        };
        let reward = |m: &Mission| m.reward.map(short_credits).unwrap_or_default();
        let expires = |m: &Mission| {
            m.seconds_left(now).map(|left| {
                let col = if left < 3600 { pal::BAD } else if left < 6 * 3600 { pal::WARN } else { pal::DIM };
                (if left > 0 { duration(left) } else { "expired".into() }, col)
            })
        };
        // What Progress says, in its colour (None: the usual text colour); a redirected mission's is a badge.
        let progress = |m: &Mission| -> Option<(String, Option<Color32>)> {
            if m.redirected {
                Some(("Complete — hand in".into(), None))
            } else if let Some(k) = m.kill_count {
                Some((format!("{}/{} kills", m.kills.min(k), k), None))
            } else if m.cargo_total > 0 {
                Some((format!("{}/{} delivered", m.cargo_delivered, m.cargo_total), None))
            } else if let Some(c) = &m.commodity {
                Some((format!("{} {}", m.count.unwrap_or(0), c), Some(pal::DIM)))
            } else {
                m.passenger_count.map(|p| (format!("{p} passengers"), Some(pal::DIM)))
            }
        };
        // Every column measured, so the table fits the card at any window size: Reward, Expires and Progress as wide
        // as their widest entry, and the three text columns share the rest. A title, faction or destination too long
        // for its share ends in "…" and shows whole on hover; the wing badge always stays.
        let heads = ["Mission", "Faction", "Destination", "Reward", "Expires", "Progress"];
        let gap = ui.spacing().item_spacing.x;
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text_w = |t: &str| ui.painter().layout_no_wrap(t.to_owned(), font.clone(), pal::TEXT).size().x + 8.0;
        let wing_w = badge_size(ui, "wing").x;
        let mut want = heads.map(|t| ui.painter().layout_no_wrap(t.to_owned(), display(size::FOOTNOTE), pal::DIM).size().x + 8.0);
        let floor = want;
        for m in &list {
            let progress_w = match progress(m) {
                Some((t, _)) if m.redirected => badge_size(ui, &t).x + 8.0,
                Some((t, _)) => text_w(&t),
                None => 0.0,
            };
            let row = [
                text_w(&m.title) + if m.wing { gap + wing_w } else { 0.0 },
                text_w(&m.faction),
                text_w(&destination(m)),
                text_w(&reward(m)),
                expires(m).map_or(0.0, |(t, _)| text_w(&t)),
                progress_w,
            ];
            for (w, r) in want.iter_mut().zip(row) {
                *w = w.max(r);
            }
        }
        let room = ui.available_width() - want[3..].iter().sum::<f32>() - gap * (heads.len() - 1) as f32;
        let text = share([want[0], want[1], want[2]], [floor[0], floor[1], floor[2]], room);
        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(text[0]))
            .column(Column::exact(text[1]))
            .column(Column::exact(text[2]))
            .column(Column::exact(want[3]))
            .column(Column::exact(want[4]))
            .column(Column::remainder().clip(true))
            .header(30.0, |mut h| {
                for t in heads {
                    h.col(|ui| {
                        crate::widgets::th(ui, t);
                    });
                }
            })
            .body(|body| {
                body.rows(32.0, list.len(), |mut row| {
                    let m = list[row.index()];
                    row.col(|ui| {
                        let badge_room = if m.wing { gap + wing_w } else { 0.0 };
                        ui.scope(|ui| {
                            ui.set_max_width((ui.available_width() - badge_room).max(0.0));
                            ui.add(egui::Label::new(&m.title).truncate());
                        });
                        if m.wing {
                            badge(ui, "wing", pal::CYAN);
                        }
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(RichText::new(&m.faction).color(pal::DIM)).truncate());
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(destination(m)).truncate());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(reward(m)).color(pal::GOLD));
                    });
                    row.col(|ui| {
                        if let Some((t, col)) = expires(m) {
                            ui.label(RichText::new(t).color(col));
                        }
                    });
                    row.col(|ui| match progress(m) {
                        Some((t, _)) if m.redirected => {
                            badge(ui, &t, pal::GOOD);
                        }
                        Some((t, Some(col))) => {
                            ui.label(RichText::new(t).color(col));
                        }
                        Some((t, None)) => {
                            ui.label(t);
                        }
                        None => {}
                    });
                });
            });
    });
    let goto = goods(ui, gs, st, src);
    ui.add_space(8.0);
    let changed = history(ui, gs, st, prefs);
    (changed, goto)
}

/// The most missions the recent history lists, newest first.
const HISTORY_ROWS: usize = 40;

const CLEAR_TIP: &str = "Hide these missions from this list, so it starts fresh. Only from this list: the game, its journals and every other page of Elite Nexus still have them, and you can show them again.";
const SHOW_AGAIN_TIP: &str = "Bring back every mission you cleared from this list";

/// The recent mission history, with Clear, Undo and Show them again. Returns true when `prefs` changed (to be saved).
fn history(ui: &mut Ui, gs: &GameState, st: &mut MissionsUi, prefs: &mut MissionsPrefs) -> bool {
    let cmdr = commander_key(gs);
    let all = &gs.missions.history;
    let cleared = prefs.cleared(cmdr);
    let list: Vec<&MissionRecord> = shown(all, cleared).collect();
    let hidden = all.len() - list.len();
    // Undo is for a Clear just made: until a mission ends after it, and for the commander who made it.
    if !list.is_empty() || st.undo.as_ref().is_some_and(|u| u.cmdr != cmdr) {
        st.undo = None;
    }
    let (mut clear, mut undo, mut show_all) = (false, false, false);
    let right = |ui: &mut Ui| {
        if !list.is_empty() && button(ui, "Clear").on_hover_text(CLEAR_TIP).clicked() {
            clear = true;
        }
    };
    card_with(ui, "Recent mission history", right, |ui| {
        // Reported by other developers and on Frontier's forums: the wingmate who handed the mission in gets the
        // MissionCompleted line, the others get none (docs/research/journal.md §5.6).
        let shared = "A mission a wingmate shared and handed in isn't counted: the game writes nothing about it in your journal.";
        if all.is_empty() {
            ui.label(RichText::new("None recorded.").color(pal::DIM));
            footnote(ui, shared);
            return;
        }
        let note = hidden_note(hidden);
        if list.is_empty() {
            empty(ui, icon::CHECK_CIRCLE, "History cleared", "Missions you complete, fail or abandon from now on show here.");
            ui.vertical_centered(|ui| match &st.undo {
                Some(u) => {
                    undo = button(ui, "Undo").on_hover_text("Bring back the missions you just cleared").clicked();
                    // An earlier Clear hid more than Undo brings back.
                    if u.before.is_some() {
                        ui.add_space(6.0);
                        show_all = button(ui, "Show them again").on_hover_text(SHOW_AGAIN_TIP).clicked();
                    }
                }
                None => show_all = button(ui, "Show them again").on_hover_text(SHOW_AGAIN_TIP).clicked(),
            });
            ui.add_space(12.0);
            footnote(ui, &note);
            footnote(ui, shared);
            return;
        }
        // The title last, in the width the other columns leave: a long one ends in "…" (whole on hover) instead of
        // widening the card past the window.
        Grid::new("mhist").num_columns(4).striped(true).spacing([12.0, 3.0]).max_col_width(ui.available_width()).show(ui, |ui| {
            for r in list.iter().rev().take(HISTORY_ROWS) {
                ui.label(RichText::new(r.ts.to_local_string("%d %b %H:%M")).color(pal::FAINT));
                let (t, c) = match r.outcome {
                    MissionOutcome::Completed => ("completed", pal::GOOD),
                    MissionOutcome::Failed => ("failed", pal::BAD),
                    MissionOutcome::Abandoned => ("abandoned", pal::WARN),
                };
                badge(ui, t, c);
                let (paid, col) = paid(r);
                ui.label(RichText::new(paid).font(display(14.0)).color(col));
                ui.add(egui::Label::new(&r.title).truncate());
                ui.end_row();
            }
        });
        if list.len() > HISTORY_ROWS {
            footnote(ui, &format!("The newest {HISTORY_ROWS} of {}.", nexus_data::names::group_digits(list.len() as i64)));
        }
        if hidden > 0 {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&note).size(size::FOOTNOTE).color(pal::DIM));
                show_all = button(ui, "Show them again").on_hover_text(SHOW_AGAIN_TIP).clicked();
            });
        }
        footnote(ui, shared);
    });
    let mut changed = false;
    if clear && let Some(u) = prefs.clear(cmdr, all) {
        st.undo = Some(u);
        changed = true;
    }
    if undo && let Some(u) = st.undo.take() {
        prefs.undo(&u);
        changed = true;
    }
    if show_all {
        prefs.show_all(cmdr);
        st.undo = None;
        changed = true;
    }
    changed
}

/// What the list says about the missions it leaves out, `hidden` of them, since it was cleared.
fn hidden_note(hidden: usize) -> String {
    let (missions, are, them) = if hidden == 1 { ("mission", "is", "it") } else { ("missions", "are", "them") };
    format!(
        "{} {missions} you cleared {are} hidden from this list only: the game, its journals and every other page of Elite Nexus still have {them}.",
        nexus_data::names::group_digits(hidden as i64),
    )
}

/// What a mission in the history paid, in its colour: a completed one's reward; a failed or abandoned one paid
/// nothing, so it says that, or the fine it cost (`MissionRecord::reward` holds a fine as a negative amount), rather
/// than a reward of "0 Cr".
fn paid(r: &MissionRecord) -> (String, Color32) {
    match r.outcome {
        MissionOutcome::Completed => (short_credits(r.reward), pal::GOLD),
        _ if r.reward < 0 => (format!("fined {}", short_credits(-r.reward)), pal::BAD),
        _ => ("no reward".into(), pal::DIM),
    }
}

/// A commodity your missions ask you to bring: how many tonnes are still to go, and for which missions.
struct Goods<'a> {
    /// The journal's symbol (`$HydrogenFuel_Name;`): what the pin and the lookups go by, whatever the game's language.
    symbol: &'a str,
    /// Its name as the missions give it, in the game's language.
    name: String,
    tonnes: i64,
    missions: Vec<&'a str>,
}

/// What the active missions ask you to bring, one entry per commodity: not a delivery's or a smuggling run's cargo,
/// which the mission hands you, nor a mission that's ready to hand in.
fn wanted_goods(gs: &GameState) -> Vec<Goods<'_>> {
    fn text(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|s| !s.is_empty())
    }
    let mut out: Vec<Goods> = Vec::new();
    for m in gs.missions.active.values() {
        let Some(symbol) = text(m.commodity_symbol.as_deref()).or(text(m.commodity.as_deref())) else { continue };
        let kind = m.name.to_ascii_lowercase();
        if kind.contains("delivery") || kind.contains("smuggle") || m.redirected {
            continue;
        }
        let tonnes = (m.count.unwrap_or(0) - m.cargo_delivered).max(0);
        if tonnes == 0 {
            continue;
        }
        match out.iter_mut().find(|g| g.symbol.eq_ignore_ascii_case(symbol)) {
            Some(g) => {
                g.tonnes += tonnes;
                g.missions.push(&m.title);
            }
            None => {
                // The game's name for it; without one (it gave only the symbol), the commodity's English name.
                let name = text(m.commodity.as_deref()).filter(|n| *n != symbol).map_or_else(|| nexus_data::commodities::display_name(symbol), str::to_string);
                out.push(Goods { symbol, name, tonnes, missions: vec![&m.title] });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The goods your missions ask for, each with where to get it. Returns what its panel asks the Market page to open.
fn goods(ui: &mut Ui, gs: &GameState, st: &mut MissionsUi, src: &Sources) -> Option<Goto> {
    let list = wanted_goods(gs);
    if list.is_empty() {
        return None;
    }
    let mut goto = None;
    ui.add_space(8.0);
    card(ui, "Goods your missions ask for", |ui| {
        crate::widgets::group(ui, |g| {
            for w in &list {
                g.row(&w.name, &w.missions.join(" · "), |ui| {
                    st.get.button(ui, w.symbol);
                    ui.label(RichText::new(format!("{} t", nexus_data::names::group_digits(w.tonnes))).color(pal::WARN));
                });
                if st.get.is_open(w.symbol) {
                    g.content(|ui| goto = st.get.panel(ui, gs, src, w.symbol));
                }
            }
        });
        crate::widgets::footnote(ui, "What your missions ask you to source, mine or donate. A delivery's cargo is handed to you with the mission, so it isn't here.");
    });
    goto
}

/// Splits `room` between columns that want the widths `want`. When they all fit, the spare room is shared in
/// proportion; when they don't, the widest are cut back to a common width first, so the shorter ones stay whole, and
/// none goes below its `floor` (the width of its heading).
fn share(want: [f32; 3], floor: [f32; 3], room: f32) -> [f32; 3] {
    let total: f32 = want.iter().sum();
    if total <= room {
        return want.map(|w| w * room / total);
    }
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| want[*a].total_cmp(&want[*b]));
    let (mut left, mut cap) = (room, room);
    for (n, i) in order.into_iter().enumerate() {
        cap = left / (want.len() - n) as f32;
        if want[i] > cap {
            break;
        }
        left -= want[i];
    }
    std::array::from_fn(|i| want[i].min(cap).max(floor[i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_columns_share_the_room() {
        let floor = [60.0; 3];
        // Room to spare: every column grows in proportion.
        assert_eq!(share([100.0, 200.0, 100.0], floor, 800.0), [200.0, 400.0, 200.0]);
        // Too little: the widest is cut first and the short ones stay whole...
        assert_eq!(share([400.0, 100.0, 150.0], floor, 450.0), [200.0, 100.0, 150.0]);
        // ...then the two widest share what the shortest leaves.
        assert_eq!(share([400.0, 100.0, 300.0], floor, 500.0), [200.0, 100.0, 200.0]);
        // Never narrower than a heading.
        assert_eq!(share([400.0, 100.0, 300.0], floor, 120.0), [60.0, 60.0, 60.0]);
    }

    #[test]
    fn failed_missions_show_no_reward() {
        let record = |outcome, reward| MissionRecord { id: 1, title: "Deliver 40 units of Tritium".into(), faction: String::new(), outcome, reward, ts: Timestamp(0) };
        assert_eq!(paid(&record(MissionOutcome::Completed, 18_500_000)).0, "18.5 M Cr");
        assert_eq!(paid(&record(MissionOutcome::Failed, 0)).0, "no reward");
        assert_eq!(paid(&record(MissionOutcome::Abandoned, 0)).0, "no reward");
        // The model keeps MissionFailed's Fine as a negative reward.
        assert_eq!(paid(&record(MissionOutcome::Failed, -50_000)), ("fined 50 k Cr".to_string(), pal::BAD));
    }

    /// A German client's missions: the goods go by the journal's symbol (two missions for one good are one entry),
    /// show the game's name, and the lookup finds the commodity; a delivery's cargo isn't asked for.
    #[test]
    fn goods_go_by_symbol() {
        let mut gs = GameState::default();
        let mission = |id: u64, name: &str, symbol: &str, local: &str, count: i64| Mission {
            id,
            name: name.into(),
            title: format!("Mission {id}"),
            commodity: Some(local.into()),
            commodity_symbol: Some(symbol.into()),
            count: Some(count),
            ..Default::default()
        };
        gs.missions.active.insert(1, mission(1, "Mission_Collect_Industry", "$HydrogenFuel_Name;", "Wasserstoff-Treibstoff", 12));
        gs.missions.active.insert(2, mission(2, "Mission_Mining_Boom", "$HydrogenFuel_Name;", "Wasserstoff-Treibstoff", 8));
        gs.missions.active.insert(3, mission(3, "Mission_Delivery", "$Gold_Name;", "Gold", 20));
        let goods = wanted_goods(&gs);
        assert_eq!(goods.len(), 1, "the delivery's gold comes with the mission");
        assert_eq!((goods[0].name.as_str(), goods[0].tonnes, goods[0].missions.len()), ("Wasserstoff-Treibstoff", 20, 2));
        assert_eq!(nexus_data::commodities::commodity(goods[0].symbol).map(|c| c.name.as_str()), Some("Hydrogen Fuel"));
    }

    fn ended(id: u64, ts: i64, outcome: MissionOutcome) -> MissionRecord {
        MissionRecord { id, title: format!("Mission {id}"), faction: String::new(), outcome, reward: 100_000, ts: Timestamp(ts) }
    }

    /// The missions the list shows, by id.
    fn listed(history: &[MissionRecord], cleared: Option<Timestamp>) -> Vec<u64> {
        shown(history, cleared).map(|r| r.id).collect()
    }

    /// Clear hides every mission the history holds, two handed in in the same second too, and a mission that ends
    /// afterwards shows.
    #[test]
    fn clearing_hides_the_history_so_far() {
        let mut history = vec![ended(1, 100, MissionOutcome::Completed), ended(2, 200, MissionOutcome::Failed), ended(3, 200, MissionOutcome::Abandoned)];
        let mut prefs = MissionsPrefs::default();
        assert_eq!(listed(&history, prefs.cleared("F1")), [1, 2, 3]);
        let undo = prefs.clear("F1", &history).expect("missions to clear");
        assert_eq!(undo, Undo { cmdr: "F1".into(), before: None });
        assert!(listed(&history, prefs.cleared("F1")).is_empty());
        assert_eq!(prefs.clear("F1", &history), None, "nothing new to clear");
        assert_eq!(MissionsPrefs::default().clear("F1", &[]), None, "nor in an empty history");
        history.push(ended(4, 201, MissionOutcome::Completed));
        assert_eq!(listed(&history, prefs.cleared("F1")), [4]);
    }

    /// Undo takes back only the last Clear; Show them again brings back the whole history.
    #[test]
    fn undo_and_show_them_again() {
        let mut history = vec![ended(1, 100, MissionOutcome::Completed)];
        let mut prefs = MissionsPrefs::default();
        prefs.clear("F1", &history).expect("missions to clear");
        history.push(ended(2, 200, MissionOutcome::Completed));
        let undo = prefs.clear("F1", &history).expect("a new mission to clear");
        assert_eq!(undo.before, Some(Timestamp(100)));
        assert!(listed(&history, prefs.cleared("F1")).is_empty());
        prefs.undo(&undo);
        assert_eq!(listed(&history, prefs.cleared("F1")), [2]);
        prefs.show_all("F1");
        assert_eq!(listed(&history, prefs.cleared("F1")), [1, 2]);
        assert!(prefs.cleared.is_empty(), "nothing left to keep in settings.json");
    }

    /// Each commander's history is cleared on its own (by FID; by name in journals that give none), and it stays cleared
    /// after a restart: settings.json keeps how far, and the history read again from the journals is hidden up to there.
    #[test]
    fn cleared_per_commander_and_kept() {
        let mut alpha = GameState::default();
        alpha.cmdr.fid = "F1".into();
        alpha.cmdr.name = "Alpha".into();
        let mut legacy = GameState::default();
        legacy.cmdr.name = "Legacy".into();
        assert_eq!((commander_key(&alpha), commander_key(&legacy)), ("F1", "Legacy"));
        let history = vec![ended(1, 100, MissionOutcome::Completed), ended(2, 200, MissionOutcome::Completed)];
        let mut prefs = MissionsPrefs::default();
        prefs.clear(commander_key(&alpha), &history);
        prefs.clear(commander_key(&legacy), &history[..1]);
        let saved = serde_json::to_string(&prefs).expect("saved");
        assert!(saved.contains(r#""F1":"1970-01-01T00:03:20Z""#), "{saved}");
        let back: MissionsPrefs = serde_json::from_str(&saved).expect("read back");
        assert_eq!(back, prefs);
        // The next start reads the history from the journals again.
        let replayed = history.clone();
        assert!(listed(&replayed, back.cleared("F1")).is_empty());
        assert_eq!(listed(&replayed, back.cleared("Legacy")), [2]);
        assert_eq!(listed(&replayed, back.cleared("F2")), [1, 2], "another commander's history is untouched");
    }

    #[test]
    fn says_what_is_hidden() {
        assert_eq!(hidden_note(1), "1 mission you cleared is hidden from this list only: the game, its journals and every other page of Elite Nexus still have it.");
        assert_eq!(hidden_note(1_234), "1,234 missions you cleared are hidden from this list only: the game, its journals and every other page of Elite Nexus still have them.");
    }

    /// The history card drawn headless, as the app draws it, with its buttons found by label and clicked.
    struct Card {
        ctx: egui::Context,
        gs: GameState,
        st: MissionsUi,
        prefs: MissionsPrefs,
        /// The buttons of the last drawing: label and centre.
        buttons: Vec<(String, egui::Pos2)>,
    }

    impl Card {
        fn new(history: Vec<MissionRecord>) -> Card {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            ctx.enable_accesskit();
            let mut gs = GameState::default();
            gs.cmdr.fid = "F1".into();
            gs.missions.history = history;
            let mut card = Card { ctx, gs, st: MissionsUi::default(), prefs: MissionsPrefs::default(), buttons: Vec::new() };
            card.draw(Vec::new());
            card
        }

        /// Draws the card once with `events`; returns whether the settings changed.
        fn draw(&mut self, events: Vec<egui::Event>) -> bool {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), events, ..Default::default() };
            let Card { ctx, gs, st, prefs, .. } = self;
            let mut changed = false;
            // A pass egui runs again (a new grid's sizing) has no input: what the first one did counts.
            let mut out = ctx.run_ui(input, |ui| changed |= history(ui, gs, st, prefs));
            // Nothing is painted here: the font atlas egui would upload is dropped.
            out.textures_delta.clear();
            let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
            self.buttons = nodes
                .iter()
                .filter(|(_, n)| n.role() == egui::accesskit::Role::Button)
                .filter_map(|(_, n)| Some((n.label()?.to_string(), n.bounds()?)))
                .map(|(label, b)| (label, egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)))
                .collect();
            changed
        }

        /// The buttons' labels, in alphabetical order.
        fn buttons(&self) -> Vec<&str> {
            let mut labels: Vec<&str> = self.buttons.iter().map(|(l, _)| l.as_str()).collect();
            labels.sort_unstable();
            labels
        }

        /// Clicks the button labelled `label` and draws the card again; returns whether the settings changed.
        fn click(&mut self, label: &str) -> bool {
            let pos = self.buttons.iter().find(|(l, _)| l == label).unwrap_or_else(|| panic!("no {label} button in {:?}", self.buttons)).1;
            let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
            let pressed = self.draw(vec![egui::Event::PointerMoved(pos), button(true)]);
            let released = self.draw(vec![button(false)]);
            self.draw(Vec::new());
            pressed || released
        }

        fn listed(&self) -> Vec<u64> {
            listed(&self.gs.missions.history, self.prefs.cleared(commander_key(&self.gs)))
        }
    }

    /// The card clicked through: Clear empties it and offers Undo, which takes back that Clear only; a mission that ends
    /// afterwards or another commander ends the Undo; Show them again brings back everything, also after a restart.
    #[test]
    fn the_card_clicked_through() {
        let mut card = Card::new(vec![ended(1, 100, MissionOutcome::Completed), ended(2, 200, MissionOutcome::Failed)]);
        assert_eq!(card.buttons(), ["Clear"]);
        assert!(card.click("Clear"), "to be saved");
        assert_eq!(card.prefs.cleared("F1"), Some(Timestamp(200)));
        assert_eq!(card.buttons(), ["Undo"], "nothing was hidden before: Undo brings it all back");
        assert!(card.click("Undo"));
        assert!(card.prefs.cleared.is_empty());
        assert_eq!(card.buttons(), ["Clear"]);
        // Cleared, then a mission ends: it shows, with Show them again for the cleared ones, and Undo is gone.
        card.click("Clear");
        card.gs.missions.history.push(ended(3, 300, MissionOutcome::Completed));
        card.draw(Vec::new());
        assert_eq!(card.buttons(), ["Clear", "Show them again"]);
        // Cleared again: Undo brings back that mission alone, Show them again all three.
        assert!(card.click("Clear"));
        assert_eq!(card.buttons(), ["Show them again", "Undo"]);
        card.click("Undo");
        assert_eq!(card.listed(), [3]);
        assert!(card.click("Show them again"));
        assert_eq!(card.listed(), [1, 2, 3]);
        // Another commander, whose history was cleared too: the Undo isn't theirs, and it doesn't come back.
        card.click("Clear");
        card.prefs.cleared.insert("F2".into(), Timestamp(300));
        card.gs.cmdr.fid = "F2".into();
        card.draw(Vec::new());
        assert_eq!(card.buttons(), ["Show them again"]);
        card.gs.cmdr.fid = "F1".into();
        card.draw(Vec::new());
        assert_eq!(card.buttons(), ["Show them again"]);
        // Elite Nexus starts again: settings.json kept how far.
        card.st = MissionsUi::default();
        card.draw(Vec::new());
        assert!(card.listed().is_empty());
        assert_eq!(card.buttons(), ["Show them again"]);
        assert!(card.click("Show them again"));
        assert_eq!((card.prefs.cleared("F1"), card.prefs.cleared("F2")), (None, Some(Timestamp(300))));
        assert_eq!(card.buttons(), ["Clear"]);
    }
}
