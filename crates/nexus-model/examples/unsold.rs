//! Debug helper: lists systems the model considers unsold.

use nexus_journal::{list_journal_files, load_history, EventCtx};
use nexus_model::{Alerts, Model};

fn main() {
    let dir = nexus_journal::default_journal_dir().expect("journal dir");
    let files = list_journal_files(&dir).expect("list");
    let mut model = Model::default();
    let mut alerts = Alerts::disabled();
    load_history(&files, 32, |_f, fe| {
        for e in &fe.events {
            let ctx = EventCtx { ts: e.ts, name: fe.name(e), line: fe.line(e), live: false };
            model.apply(&ctx, &e.event, &mut alerts);
        }
    });
    let gs = model.active().expect("cmdr");
    let mut v: Vec<_> = gs.exploration.unsold().collect();
    v.sort_by_key(|s| s.last_visit);
    for s in v {
        let types: std::collections::BTreeSet<&str> = s.bodies.values().filter(|b| b.has_scan).map(|b| b.scan_type.as_str()).collect();
        println!(
            "{} {:<40} scanned {:>2} value {:>9} types {:?}",
            s.last_visit.to_iso(),
            s.name,
            s.scanned_bodies(),
            s.unsold_value(),
            types
        );
    }
}
