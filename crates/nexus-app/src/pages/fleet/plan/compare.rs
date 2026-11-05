//! The Compare panel: what the plan comes to beside another build (your ship as it is, the hull as a shipyard sells
//! it, a saved build, another of your ships), figure by figure.

use crate::theme::{display, pal, size};
use crate::widgets::{self, short_credits};
use egui::{Label, RichText, Ui};
use egui_extras::Column;
use nexus_data::fitting::{Figures, Plan};

/// The build the plan is put beside (or its weapons tried against).
#[derive(Clone, PartialEq)]
pub struct CompareWith {
    /// "Your Kestrel Mk II as it is", "Explorer (saved)".
    pub label: String,
    /// What its column says: "As it is", "New hull", "Explorer".
    pub short: String,
    pub plan: Plan,
    /// Where it came from, to read it again when that changes (a saved build saved over).
    pub pick: Option<Pick>,
}

/// A build to pick: the commander's ship the plan is for, as it is; a hull as a shipyard sells it (its symbol); a
/// saved build (its name); another of the commander's ships (its ShipID).
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    AsItIs,
    NewHull(String),
    Saved(String),
    Ship(u64),
}

/// One build in a list to pick from: the name it's picked by (unique in the list), what it is, and which it is.
pub struct Choice {
    pub label: String,
    pub detail: String,
    pub pick: Pick,
}

impl Pick {
    /// The heading it's listed under.
    fn group(&self) -> &'static str {
        match self {
            Pick::AsItIs => "Your ship",
            Pick::NewHull(_) => "As a shipyard sells it",
            Pick::Saved(_) => "Saved builds",
            Pick::Ship(_) => "Your ships",
        }
    }
}

/// The choices whose name has what's typed, under a heading for each kind, for a grouped combobox.
pub fn rows(choices: &[Choice], typed: &str) -> Vec<widgets::ListRow> {
    let k = nexus_data::commodities::name_key(typed);
    let mut out = Vec::new();
    let mut last = "";
    for c in choices.iter().filter(|c| k.is_empty() || nexus_data::commodities::name_key(&c.label).contains(&k)) {
        let group = c.pick.group();
        if group != last {
            out.push(widgets::ListRow::Heading(group.to_string()));
            last = group;
        }
        out.push(widgets::ListRow::Item(widgets::Suggestion { text: c.label.clone(), detail: c.detail.clone(), starred: false }));
    }
    out
}

/// Makes every label in the list its own, adding a ship's ShipID where two would read the same.
pub fn unique(mut choices: Vec<Choice>) -> Vec<Choice> {
    let labels: Vec<String> = choices.iter().map(|c| c.label.clone()).collect();
    for c in &mut choices {
        if labels.iter().filter(|l| **l == c.label).count() > 1
            && let Pick::Ship(id) = c.pick
        {
            c.label = format!("{} #{id}", c.label);
        }
    }
    choices
}

/// One row: the figure, both values, how to write them, and whether more is better (None: neither).
struct Row {
    name: &'static str,
    ours: f64,
    theirs: f64,
    unit: Unit,
    more_is_better: Option<bool>,
}

#[derive(Clone, Copy)]
enum Unit {
    /// A unit after the number, with this many decimals.
    Of(&'static str, usize),
    Credits,
}

impl Unit {
    fn write(self, v: f64) -> String {
        match self {
            Unit::Of(unit, decimals) => format!("{v:.decimals$}{unit}"),
            Unit::Credits => short_credits(v.round() as i64),
        }
    }

    /// The difference, signed: "+1.25 ly", "−3.2 M Cr".
    fn change(self, d: f64) -> String {
        let sign = if d > 0.0 { "+" } else { "−" };
        match self {
            Unit::Of(unit, decimals) => format!("{sign}{:.decimals$}{unit}", d.abs()),
            Unit::Credits => format!("{sign}{}", short_credits(d.abs().round() as i64)),
        }
    }

    /// Smaller than the last decimal shown: no change.
    fn same(self, d: f64) -> bool {
        match self {
            Unit::Of(_, decimals) => d.abs() < 0.5 * 10f64.powi(-(decimals as i32)),
            Unit::Credits => d.abs() < 0.5,
        }
    }
}

/// The plan (`ours`) beside the other build's figures (`theirs`; `short` heads its column, `label` says it whole),
/// with the difference in a colour that says which is better.
pub fn table(ui: &mut Ui, ours: &Figures, label: &str, short: &str, theirs: &Figures) {
    let sdps = |f: &Figures| f.weapons.iter().map(|w| w.sustained_dps).sum::<f64>();
    let shields = |f: &Figures| f.shield.map_or(0.0, |s| s.strength);
    let spare = |f: &Figures| f.power_generated - f.power_deployed;
    let ly = |d| Unit::Of("\u{a0}ly", d);
    let rows = [
        Row { name: "Jump range (max)", ours: ours.max_range, theirs: theirs.max_range, unit: ly(2), more_is_better: Some(true) },
        Row { name: "Jump range, laden", ours: ours.laden_range, theirs: theirs.laden_range, unit: ly(2), more_is_better: Some(true) },
        Row { name: "On one tank", ours: ours.total_range, theirs: theirs.total_range, unit: ly(0), more_is_better: Some(true) },
        Row { name: "Top speed", ours: ours.speed, theirs: theirs.speed, unit: Unit::Of("\u{a0}m/s", 0), more_is_better: Some(true) },
        Row { name: "Boost", ours: ours.boost, theirs: theirs.boost, unit: Unit::Of("\u{a0}m/s", 0), more_is_better: Some(true) },
        Row { name: "Shields", ours: shields(ours), theirs: shields(theirs), unit: Unit::Of("\u{a0}MJ", 0), more_is_better: Some(true) },
        Row { name: "Armour", ours: ours.armour, theirs: theirs.armour, unit: Unit::Of("", 0), more_is_better: Some(true) },
        Row { name: "Firepower", ours: sdps(ours), theirs: sdps(theirs), unit: Unit::Of("\u{a0}DPS", 1), more_is_better: Some(true) },
        Row { name: "Power to spare", ours: spare(ours), theirs: spare(theirs), unit: Unit::Of("\u{a0}MW", 1), more_is_better: Some(true) },
        Row { name: "Unladen mass", ours: ours.unladen_mass, theirs: theirs.unladen_mass, unit: Unit::Of("\u{a0}t", 1), more_is_better: None },
        Row { name: "Cargo", ours: ours.cargo, theirs: theirs.cargo, unit: Unit::Of("\u{a0}t", 0), more_is_better: Some(true) },
        Row { name: "Fuel", ours: ours.fuel, theirs: theirs.fuel, unit: Unit::Of("\u{a0}t", 0), more_is_better: Some(true) },
        Row { name: "Cost", ours: ours.total_cost(), theirs: theirs.total_cost(), unit: Unit::Credits, more_is_better: Some(false) },
        Row { name: "Rebuy", ours: ours.rebuy(), theirs: theirs.rebuy(), unit: Unit::Credits, more_is_better: Some(false) },
    ];
    let tips: [&str; 14] = [
        "The game's own maximum: no cargo, fuel for one jump",
        "A full tank and a full hold",
        "Jumping on one full tank until it runs dry",
        "Four pips to engines, full tank, empty hold",
        "Four pips to engines, full tank, empty hold",
        "Generator, boosters and Guardian reinforcement",
        "Hull points",
        "Damage a second over time, reloads included",
        "What the power plant makes less what the ship draws with its hardpoints deployed",
        "Hull and modules, no fuel or cargo",
        "",
        "",
        "Hull and modules, at the discount set on Overview",
        "5 % of the cost",
    ];
    // Three figures' columns from what's left beside the names, so a narrow window keeps them all.
    let v = ((ui.available_width() - 110.0 - 4.0 * 12.0) / 3.0).clamp(72.0, 112.0);
    widgets::table(ui, "plan-compare", |t| {
        t.vscroll(false)
            .column(widgets::flex_column(110.0))
            .column(Column::exact(v))
            .column(Column::exact(v))
            .column(Column::exact(v))
            .header(30.0, |mut h| {
                for (t, tip) in [("", ""), ("This plan", "This plan"), (short, label), ("Difference", "This plan less the other")] {
                    h.col(|ui| {
                        let r = ui.add(Label::new(RichText::new(t).font(display(size::FOOTNOTE)).color(pal::DIM)).truncate());
                        if !tip.is_empty() {
                            r.on_hover_text(tip);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(28.0, rows.len(), |mut row| {
                    let i = row.index();
                    let Some(r) = rows.get(i) else { return };
                    row.col(|ui| {
                        let name = ui.add(Label::new(RichText::new(r.name).color(pal::DIM)).truncate());
                        if let Some(tip) = tips.get(i).filter(|t| !t.is_empty()) {
                            name.on_hover_text(*tip);
                        }
                    });
                    row.col(|ui| {
                        ui.label(r.unit.write(r.ours));
                    });
                    row.col(|ui| {
                        ui.label(r.unit.write(r.theirs));
                    });
                    row.col(|ui| {
                        let d = r.ours - r.theirs;
                        if !d.is_finite() || r.unit.same(d) {
                            ui.label(RichText::new("Same").color(pal::FAINT));
                            return;
                        }
                        let col = match r.more_is_better {
                            Some(more) if (d > 0.0) == more => pal::GOOD,
                            Some(_) => pal::BAD,
                            None => pal::TEXT,
                        };
                        ui.label(RichText::new(r.unit.change(d)).color(col));
                    });
                });
            });
    });
}
