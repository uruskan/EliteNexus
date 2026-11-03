//! Follow my flight (Settings › Exploration): the page that fits what the commander is doing, the way Elite Dangerous
//! Exploration Buddy switches its tabs. Pure: `Follower::step` reads the model and names the page to open; the app
//! decides whether it may (the setting, the page shown, a journal followed live).

use crate::pages::Page;
use nexus_journal::companion::{GuiFocus, Status};
use nexus_model::context::{Activity, Context};
use nexus_model::GameState;
use std::sync::Arc;

/// What Follow my flight follows: the commander's context (`nexus_model::context`), reduced to the pages here the
/// way Elite Dangerous Exploration Buddy reduces it to its tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flight {
    /// Nothing to follow: not in the game, docked, driving, on foot inside.
    Idle,
    /// The galaxy map, a charging hyperspace jump, hyperspace: Navigation.
    Route,
    /// Flying the system, orbital cruise included: Exploration.
    System,
    /// Under the "Planet view below" altitude over a body, landed, or on foot outside: Exobiology when the body has
    /// life still to analyse, Exploration otherwise.
    Planet,
}

impl Flight {
    /// `altitude_m` is Status.json's altitude, when it gives one; `planet_m` is Settings' "Planet view below".
    fn of(cx: &Context, altitude_m: Option<f64>, planet_m: f64) -> Flight {
        // Plotting the next trip, wherever the map is opened: docked or in an SRV too.
        if cx.focus == GuiFocus::GalaxyMap {
            return Flight::Route;
        }
        match cx.activity {
            Activity::NotInGame | Activity::Docked | Activity::Srv(_) => Flight::Idle,
            Activity::ChargingJump | Activity::Hyperspace => Flight::Route,
            Activity::Landed | Activity::OnFootPlanet => Flight::Planet,
            Activity::NearBody | Activity::Glide => if altitude_m.is_some_and(|a| a < planet_m) { Flight::Planet } else { Flight::System },
            Activity::Supercruise | Activity::Space => Flight::System,
        }
    }
}

/// What Follow my flight compares from one Status.json update or journal event to the next.
struct Snapshot {
    /// The Status.json the model held when this was taken, compared by pointer (`Arc::ptr_eq`): the model makes a
    /// new `Arc` for every update, while the timestamp inside has one-second resolution and could miss one.
    status: Option<Arc<Status>>,
    /// `GameState::events_applied` when this was taken.
    events: u64,
    flight: Flight,
    /// The current system is fully explored (Settings "High-value body" threshold).
    done: bool,
}

/// What the commander was doing at the last Status.json update or journal event.
#[derive(Default)]
pub struct Follower {
    /// `None` until the first look at a commander.
    last: Option<Snapshot>,
}

impl Follower {
    /// Reads what the commander is doing (`Context::of`, from Status.json while the game runs) and, when it changed,
    /// names the page that fits: Navigation in the galaxy map, while a hyperspace jump charges and in hyperspace;
    /// Exploration while flying the system; Exobiology under the "Planet view below" altitude (`planet_m`), landed or
    /// on foot outside, when the body has life still to analyse (Exploration otherwise); Navigation too once the
    /// system is done (`high_value`: Settings' "High-value body"). The first look only takes the snapshot. Works
    /// only when a Status.json update or a journal event arrived: on every other frame it costs one counter
    /// comparison and one pointer comparison, no allocation.
    pub fn step(&mut self, gs: &GameState, planet_m: f64, high_value: i64) -> Option<Page> {
        let same_status = |p: &Snapshot| match (&p.status, &gs.status) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if self.last.as_ref().is_some_and(|p| p.events == gs.events_applied && same_status(p)) {
            return None;
        }
        let cx = Context::of(gs);
        let flight = Flight::of(&cx, gs.status.as_ref().and_then(|s| s.altitude), planet_m);
        let system = gs.exploration.current_system();
        let done = system.is_some_and(|s| s.fully_explored(high_value));
        let prev = self.last.replace(Snapshot { status: gs.status.clone(), events: gs.events_applied, flight, done });
        // The first look only takes the snapshot.
        let prev = prev?;
        if flight != prev.flight {
            match flight {
                Flight::Route => Some(Page::Navigation),
                Flight::System => Some(Page::Exploration),
                Flight::Planet => {
                    // The body by its id when the journal named it, else by Status.json's name.
                    let body = cx.body.as_ref().zip(system).and_then(|(b, s)| b.id.and_then(|id| s.bodies.get(&id)).or_else(|| s.bodies.values().find(|x| x.name == b.name)));
                    Some(if body.is_some_and(|b| b.bio_signals > 0 && !b.bio_done()) { Page::Exobiology } else { Page::Exploration })
                }
                Flight::Idle => None,
            }
        } else if done && !prev.done {
            Some(Page::Navigation)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::context::Vehicle;

    fn cx(activity: Activity, focus: GuiFocus) -> Context {
        Context { activity, focus, danger: false, body: None }
    }

    /// The pages Follow my flight moves between, from the commander's context, EDEB's way.
    #[test]
    fn flight_from_context() {
        const KM150: f64 = 150_000.0;
        let of = |a: Activity, alt: Option<f64>| Flight::of(&cx(a, GuiFocus::None), alt, KM150);
        assert_eq!(of(Activity::Supercruise, None), Flight::System);
        assert_eq!(of(Activity::Space, None), Flight::System);
        assert_eq!(of(Activity::ChargingJump, None), Flight::Route);
        assert_eq!(of(Activity::Hyperspace, None), Flight::Route);
        assert_eq!(of(Activity::NearBody, Some(400_000.0)), Flight::System, "orbital cruise");
        assert_eq!(of(Activity::NearBody, Some(149_999.0)), Flight::Planet);
        assert_eq!(of(Activity::NearBody, None), Flight::System, "no altitude isn't under it");
        assert_eq!(of(Activity::Glide, Some(20_000.0)), Flight::Planet);
        assert_eq!(of(Activity::Landed, Some(0.0)), Flight::Planet);
        assert_eq!(of(Activity::OnFootPlanet, Some(0.0)), Flight::Planet);
        assert_eq!(of(Activity::Srv(Vehicle::Scarab), Some(0.0)), Flight::Idle, "driving changes nothing");
        assert_eq!(of(Activity::Docked, None), Flight::Idle);
        assert_eq!(of(Activity::NotInGame, None), Flight::Idle);
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::GalaxyMap), Some(1_000.0), KM150), Flight::Route, "the galaxy map wins");
        assert_eq!(Flight::of(&cx(Activity::Docked, GuiFocus::GalaxyMap), None, KM150), Flight::Route, "docked too");
        assert_eq!(Flight::of(&cx(Activity::Srv(Vehicle::Scarab), GuiFocus::GalaxyMap), Some(0.0), KM150), Flight::Route, "and in an SRV");
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::SystemMap), Some(1_000.0), KM150), Flight::Planet, "the system map doesn't");
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::None), Some(100_000.0), 50_000.0), Flight::System, "the altitude is the setting's");
    }
}
