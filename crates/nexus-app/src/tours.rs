//! Scan tours: Spansh's Road to Riches and exobiology planners, and tours through planets this
//! app found (untouched biology). Every tour becomes the active route on the Navigation page.

use crate::pages::navigation::{PlannedRoute, Waypoint, from_spansh};
use nexus_data::names::compact_credits;
use nexus_model::GameState;
use nexus_model::location::distance;
use nexus_spansh::{ExobiologyRouteRequest, Plotted, RichesRouteRequest, RouteWaypoints, ScanRoute, Spansh};

/// Spansh's own bounds for its planners (the limits of the website's forms).
pub mod limits {
    use std::ops::RangeInclusive;

    /// Jump range, ly.
    pub const RANGE_LY: RangeInclusive<f64> = 10.0..=100.0;
    /// Search radius of the Road to Riches and exobiology planners, ly.
    pub const RADIUS_LY: RangeInclusive<u32> = 1..=1000;
    /// Systems in one Road to Riches or exobiology route.
    pub const SYSTEMS: RangeInclusive<u32> = 1..=2000;
}

/// The current ship's jump range, within what Spansh plans with.
pub fn jump_range(gs: &GameState) -> f64 {
    let r = gs.fleet.current().map(|s| (s.max_jump_range * 10.0).round() / 10.0).filter(|r| *r > 0.0).unwrap_or(20.0);
    r.clamp(*limits::RANGE_LY.start(), *limits::RANGE_LY.end())
}

/// Keeps a planner's jump range on the ship you fly: while `range` holds the ship's range it was last given (`auto`),
/// it follows the ship; once changed by hand, it stays.
pub fn follow_range(range: &mut f64, auto: &mut f64, gs: &GameState) {
    let ship = jump_range(gs);
    if *range <= 0.0 || (*range - *auto).abs() < 1e-9 {
        *range = ship;
    }
    *auto = ship;
}

/// A scan route from Spansh and how it was found.
#[derive(Debug, Clone)]
pub struct Tour {
    pub route: ScanRoute,
    pub web_url: String,
    /// Search radius that produced the route, ly.
    pub radius: u32,
    /// The radius asked for, when nothing turned up within it and the search was widened.
    pub widened_from: Option<u32>,
}

impl Tour {
    pub fn is_empty(&self) -> bool {
        self.route.body_count() == 0
    }

    /// The route for Navigation.
    pub fn planned(&self, kind: &'static str) -> PlannedRoute {
        let waypoints: Vec<Waypoint> = self.route.waypoints().iter().map(from_spansh).collect();
        let stops = self.route.systems.iter().filter(|s| !s.bodies.is_empty()).count();
        let summary = format!("{stops} systems · {} bodies · ~{} Cr", self.route.body_count(), compact_credits(self.route.total_value() as i64));
        PlannedRoute::new(kind, summary, self.web_url.clone(), waypoints)
    }
}

/// Radii tried after an empty answer. Both planners skip populated systems, so a tour that starts
/// in the bubble needs a wide search.
pub fn wider_radii(asked: u32) -> impl Iterator<Item = u32> {
    [100, 250, 500, 1000].into_iter().filter(move |r| *r > asked)
}

/// Plots with the radius asked for and widens it while Spansh finds nothing.
async fn widening<F, Fut>(asked: u32, mut plot: F) -> Result<Tour, String>
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = nexus_spansh::Result<Plotted<ScanRoute>>>,
{
    let mut tour = None;
    for radius in std::iter::once(asked).chain(wider_radii(asked)) {
        let p = plot(radius).await.map_err(|e| e.to_string())?;
        let t = Tour { route: p.route, web_url: p.web_url, radius, widened_from: (radius != asked).then_some(asked) };
        if !t.is_empty() {
            return Ok(t);
        }
        tour = Some(t);
    }
    tour.ok_or_else(|| "Spansh returned nothing.".to_string())
}

/// Road to Riches (or one of its specialised variants), widened out of the bubble if needed.
pub async fn riches(spansh: &Spansh, req: RichesRouteRequest) -> Result<Tour, String> {
    let asked = req.radius;
    widening(asked, |radius| {
        let req = req.clone().radius(radius);
        let sp = spansh.clone();
        async move { sp.riches_route(&req).await }
    })
    .await
}

/// Spansh's exobiology planner: planets where commanders have already logged species.
pub async fn known_species(spansh: &Spansh, req: ExobiologyRouteRequest) -> Result<Tour, String> {
    let asked = req.radius;
    widening(asked, |radius| {
        let req = req.clone().radius(radius);
        let sp = spansh.clone();
        async move { sp.exobiology_route(&req).await }
    })
    .await
}

/// A planet to visit on a tour planned here.
#[derive(Debug, Clone)]
pub struct Stop {
    pub system: String,
    pub pos: Option<[f64; 3]>,
    /// Short body name and why it's on the tour.
    pub note: String,
    pub value: u64,
}

/// A tour through `stops`: planets in one system share a stop, and each leg goes to the nearest
/// system not yet visited (stops without coordinates come last).
pub fn tour_through(kind: &'static str, summary: String, origin: Option<[f64; 3]>, range_ly: f64, stops: Vec<Stop>) -> PlannedRoute {
    let mut systems: Vec<Stop> = Vec::new();
    for s in stops {
        match systems.iter_mut().find(|x| x.system.eq_ignore_ascii_case(&s.system)) {
            Some(x) => {
                x.note = format!("{}, {}", x.note, s.note);
                x.value += s.value;
            }
            None => systems.push(s),
        }
    }
    let mut here = origin;
    let mut ordered = Vec::with_capacity(systems.len());
    while !systems.is_empty() {
        let next = match here {
            Some(h) => systems
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.pos.map(|p| (i, distance(h, p))))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(0, |(i, _)| i),
            None => 0,
        };
        let s = systems.remove(next);
        let leg = match (here, s.pos) {
            (Some(a), Some(b)) => distance(a, b),
            _ => 0.0,
        };
        if s.pos.is_some() {
            here = s.pos;
        }
        ordered.push(Waypoint {
            system: s.system,
            pos: s.pos,
            star_class: None,
            jumps: (leg / range_ly.max(1.0)).ceil().max(1.0) as u32,
            neutron: false,
            refuel: false,
            scoopable: None,
            value: Some(s.value),
            note: s.note,
        });
    }
    PlannedRoute::new(kind, summary, String::new(), ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(system: &str, x: f64, note: &str) -> Stop {
        Stop { system: system.into(), pos: Some([x, 0.0, 0.0]), note: note.into(), value: 10 }
    }

    #[test]
    fn widening_skips_radii_already_tried() {
        assert_eq!(wider_radii(25).collect::<Vec<_>>(), vec![100, 250, 500, 1000]);
        assert_eq!(wider_radii(250).collect::<Vec<_>>(), vec![500, 1000]);
        assert_eq!(wider_radii(1000).count(), 0);
    }

    #[test]
    fn a_tour_goes_to_the_nearest_system_next_and_merges_planets() {
        let stops = vec![stop("Far", 300.0, "A 1"), stop("Near", 50.0, "B 2"), stop("Mid", 120.0, "C 3"), stop("Near", 50.0, "B 3")];
        let r = tour_through("Test", String::new(), Some([0.0, 0.0, 0.0]), 50.0, stops);
        let names: Vec<_> = r.waypoints.iter().map(|w| w.system.as_str()).collect();
        assert_eq!(names, ["Near", "Mid", "Far"]);
        assert_eq!(r.waypoints[0].note, "B 2, B 3");
        assert_eq!(r.waypoints[0].value, Some(20));
        assert_eq!(r.waypoints[1].jumps, 2, "70 ly at 50 ly per jump");
    }

    #[test]
    fn progress_counts_arrivals_so_a_loop_is_not_finished_at_the_start() {
        let w = |s: &str| Waypoint { system: s.into(), pos: None, star_class: None, jumps: 1, neutron: false, refuel: false, scoopable: None, value: None, note: String::new() };
        let mut r = PlannedRoute::new("Loop", String::new(), String::new(), vec![w("Home"), w("A"), w("B"), w("Home")]);
        r.advance("Home");
        assert_eq!(r.remaining().len(), 3, "the start is behind us, the way home is not");
        r.advance("Home");
        assert_eq!(r.remaining().len(), 3, "staying put changes nothing");
        r.advance("Elsewhere");
        assert_eq!(r.remaining().len(), 3, "a detour changes nothing");
        r.advance("B");
        assert_eq!(r.remaining().len(), 1, "skipping A is fine");
        assert!(!r.complete());
        r.advance("Home");
        assert!(r.remaining().is_empty());
        assert!(r.complete(), "the in-game route takes over");
    }
}
