//! Active missions, mission history and the massacre-stack tracker.

use nexus_journal::events as ev;
use nexus_journal::Timestamp;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionOutcome {
    Completed,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, Default)]
pub struct Mission {
    pub id: u64,
    pub name: String,
    pub title: String,
    pub faction: String,
    pub accepted: Timestamp,
    pub expiry: Option<Timestamp>,
    pub reward: Option<i64>,
    pub wing: bool,
    pub influence: Option<String>,
    pub reputation: Option<String>,
    pub source_system: String,
    pub source_station: Option<String>,
    pub destination_system: Option<String>,
    pub destination_station: Option<String>,
    pub destination_settlement: Option<String>,
    pub target_faction: Option<String>,
    pub target_type: Option<String>,
    pub target: Option<String>,
    pub kill_count: Option<i64>,
    pub kills: i64,
    /// The commodity, in the game's language (`Commodity_Localised`; the symbol when there's none): to show.
    pub commodity: Option<String>,
    /// The journal's symbol for it (`$HydrogenFuel_Name;`): what lookups go by, whatever the game's language.
    pub commodity_symbol: Option<String>,
    pub count: Option<i64>,
    pub passenger_count: Option<i64>,
    pub passenger_type: Option<String>,
    pub passenger_vips: bool,
    pub passenger_wanted: bool,
    pub redirected: bool,
    /// True if we only know this mission from the `Missions` login snapshot.
    pub details_unknown: bool,
    pub cargo_delivered: i64,
    pub cargo_total: i64,
}

impl Mission {
    pub fn is_massacre(&self) -> bool {
        self.name.to_ascii_lowercase().contains("massacre") && self.kill_count.is_some()
    }

    pub fn kills_remaining(&self) -> i64 {
        self.kill_count.map(|k| (k - self.kills).max(0)).unwrap_or(0)
    }

    pub fn is_done(&self) -> bool {
        self.redirected || (self.is_massacre() && self.kills_remaining() == 0)
    }

    pub fn seconds_left(&self, now: Timestamp) -> Option<i64> {
        self.expiry.map(|e| e.0 - now.0)
    }
}

#[derive(Debug, Clone)]
pub struct MissionRecord {
    pub id: u64,
    pub title: String,
    pub faction: String,
    pub outcome: MissionOutcome,
    /// Credits a completed mission paid; for a failed or abandoned one, minus the fine it cost (0: no fine).
    pub reward: i64,
    pub ts: Timestamp,
}

/// Per issuing faction view of a massacre stack.
#[derive(Debug, Clone, Default)]
pub struct StackFaction {
    pub faction: String,
    pub missions: usize,
    pub kills_needed: i64,
    pub kills_done: i64,
    pub reward: i64,
}

/// All massacre missions against one target faction.
#[derive(Debug, Clone, Default)]
pub struct MassacreStack {
    pub target_faction: String,
    pub target_system: Option<String>,
    pub givers: Vec<StackFaction>,
    pub total_reward: i64,
    pub missions: usize,
    /// Each kill counts once for every issuing faction, so the stack is done after the largest
    /// per-faction requirement.
    pub kills_remaining: i64,
    pub kills_total: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Missions {
    pub active: BTreeMap<u64, Mission>,
    pub history: Vec<MissionRecord>,
}

impl Missions {
    pub fn on_snapshot(&mut self, m: &ev::Missions, ts: Timestamp) {
        let active_ids: Vec<u64> = m.active.iter().map(|x| x.mission_id).collect();
        self.active.retain(|id, _| active_ids.contains(id));
        for s in &m.active {
            let e = self.active.entry(s.mission_id).or_insert_with(|| Mission {
                id: s.mission_id,
                name: s.name.clone(),
                title: nexus_data::names::humanize_symbol(&s.name),
                details_unknown: true,
                ..Default::default()
            });
            if s.expires > 0 {
                e.expiry = Some(Timestamp(ts.0 + s.expires));
            }
        }
    }

    pub fn on_accepted(&mut self, a: &ev::MissionAccepted, ts: Timestamp, system: &str, station: Option<&str>) {
        self.active.insert(
            a.mission_id,
            Mission {
                id: a.mission_id,
                name: a.name.clone(),
                title: a.localised_name.clone().unwrap_or_else(|| nexus_data::names::humanize_symbol(&a.name)),
                faction: a.faction.clone(),
                accepted: ts,
                expiry: a.expiry.as_deref().and_then(Timestamp::parse),
                reward: a.reward,
                wing: a.wing.unwrap_or(false),
                influence: a.influence.clone(),
                reputation: a.reputation.clone(),
                source_system: system.to_string(),
                source_station: station.map(str::to_string),
                destination_system: a.destination_system.clone(),
                destination_station: a.destination_station.clone(),
                destination_settlement: a.destination_settlement.clone(),
                target_faction: a.target_faction.clone(),
                target_type: a.target_type_localised.clone().or_else(|| a.target_type.clone()),
                target: a.target_localised.clone().or_else(|| a.target.clone()),
                kill_count: a.kill_count,
                kills: 0,
                commodity: a.commodity_localised.clone().or_else(|| a.commodity.clone()),
                commodity_symbol: a.commodity.clone(),
                count: a.count,
                passenger_count: a.passenger_count,
                passenger_type: a.passenger_type.clone(),
                passenger_vips: a.passenger_vips.unwrap_or(false),
                passenger_wanted: a.passenger_wanted.unwrap_or(false),
                redirected: false,
                details_unknown: false,
                cargo_delivered: 0,
                cargo_total: a.count.unwrap_or(0),
            },
        );
    }

    pub fn on_completed(&mut self, c: &ev::MissionCompleted, ts: Timestamp) -> Option<i64> {
        let m = self.active.remove(&c.mission_id);
        let reward = c.reward.unwrap_or(0);
        self.history.push(MissionRecord {
            id: c.mission_id,
            title: c.localised_name.clone().or_else(|| m.as_ref().map(|m| m.title.clone())).unwrap_or_default(),
            faction: c.faction.clone(),
            outcome: MissionOutcome::Completed,
            reward,
            ts,
        });
        Some(reward)
    }

    pub fn on_ended(&mut self, e: &ev::MissionEnded, outcome: MissionOutcome, ts: Timestamp) {
        let m = self.active.remove(&e.mission_id);
        self.history.push(MissionRecord {
            id: e.mission_id,
            title: e.localised_name.clone().or_else(|| m.as_ref().map(|m| m.title.clone())).unwrap_or_default(),
            faction: m.map(|m| m.faction).unwrap_or_default(),
            outcome,
            reward: -e.fine.unwrap_or(0),
            ts,
        });
    }

    pub fn on_redirected(&mut self, r: &ev::MissionRedirected) -> Option<&Mission> {
        let m = self.active.get_mut(&r.mission_id)?;
        m.redirected = true;
        if r.new_destination_system.is_some() {
            m.destination_system = r.new_destination_system.clone();
        }
        if r.new_destination_station.is_some() {
            m.destination_station = r.new_destination_station.clone();
        }
        if let Some(k) = m.kill_count {
            m.kills = k;
        }
        Some(m)
    }

    /// A kill counts once per issuing faction: for every faction with an unfinished massacre
    /// mission against the victim's faction, the oldest such mission gets the kill.
    /// Returns the missions that just reached their kill count.
    pub fn on_bounty(&mut self, b: &ev::Bounty) -> Vec<u64> {
        let Some(victim) = b.victim_faction.as_deref() else { return Vec::new() };
        let mut credited_factions: Vec<String> = Vec::new();
        let mut finished = Vec::new();
        let mut ids: Vec<(Timestamp, u64)> = self
            .active
            .values()
            .filter(|m| m.is_massacre() && !m.is_done() && m.target_faction.as_deref() == Some(victim))
            .map(|m| (m.accepted, m.id))
            .collect();
        ids.sort();
        for (_, id) in ids {
            let Some(m) = self.active.get_mut(&id) else { continue };
            if credited_factions.contains(&m.faction) {
                continue;
            }
            credited_factions.push(m.faction.clone());
            m.kills += 1;
            if m.kills_remaining() == 0 {
                finished.push(id);
            }
        }
        finished
    }

    pub fn on_cargo_depot(&mut self, d: &ev::CargoDepot) {
        if let Some(m) = self.active.get_mut(&d.mission_id) {
            m.cargo_delivered = d.items_delivered;
            m.cargo_total = d.total_items_to_deliver;
        }
    }

    pub fn massacre_stacks(&self) -> Vec<MassacreStack> {
        let mut by_target: BTreeMap<String, MassacreStack> = BTreeMap::new();
        for m in self.active.values().filter(|m| m.is_massacre()) {
            let target = m.target_faction.clone().unwrap_or_else(|| "Unknown".into());
            let stack = by_target.entry(target.clone()).or_insert_with(|| MassacreStack {
                target_faction: target,
                target_system: m.destination_system.clone(),
                ..Default::default()
            });
            stack.missions += 1;
            stack.total_reward += m.reward.unwrap_or(0);
            let giver = match stack.givers.iter_mut().position(|g| g.faction == m.faction) {
                Some(i) => &mut stack.givers[i],
                None => {
                    stack.givers.push(StackFaction { faction: m.faction.clone(), ..Default::default() });
                    stack.givers.last_mut().expect("just pushed")
                }
            };
            giver.missions += 1;
            giver.kills_needed += m.kill_count.unwrap_or(0);
            giver.kills_done += m.kills.min(m.kill_count.unwrap_or(0));
            giver.reward += m.reward.unwrap_or(0);
        }
        let mut out: Vec<MassacreStack> = by_target
            .into_values()
            .map(|mut s| {
                s.givers.sort_by_key(|g| std::cmp::Reverse(g.kills_needed));
                s.kills_total = s.givers.iter().map(|g| g.kills_needed).max().unwrap_or(0);
                s.kills_remaining = s.givers.iter().map(|g| g.kills_needed - g.kills_done).max().unwrap_or(0);
                s
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.total_reward));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept(ms: &mut Missions, id: u64, faction: &str, kills: i64, ts: i64) {
        let a = ev::MissionAccepted {
            faction: faction.into(),
            name: "Mission_MassacreWing".into(),
            mission_id: id,
            target_faction: Some("Pirates".into()),
            kill_count: Some(kills),
            reward: Some(10_000_000),
            ..Default::default()
        };
        ms.on_accepted(&a, Timestamp(ts), "Sys", None);
    }

    #[test]
    fn kills_count_once_per_issuing_faction() {
        let mut ms = Missions::default();
        accept(&mut ms, 1, "A", 2, 1);
        accept(&mut ms, 2, "A", 3, 2);
        accept(&mut ms, 3, "B", 4, 3);
        let bounty = ev::Bounty { victim_faction: Some("Pirates".into()), ..Default::default() };
        ms.on_bounty(&bounty);
        assert_eq!(ms.active[&1].kills, 1);
        assert_eq!(ms.active[&2].kills, 0, "same faction's second mission waits");
        assert_eq!(ms.active[&3].kills, 1);
        let done = ms.on_bounty(&bounty);
        assert_eq!(done, vec![1]);
        ms.on_bounty(&bounty);
        assert_eq!(ms.active[&2].kills, 1, "faction A moves to its next mission");
        let stacks = ms.massacre_stacks();
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].kills_total, 5);
        assert_eq!(stacks[0].kills_remaining, 2);
        assert_eq!(stacks[0].total_reward, 30_000_000);
    }

    /// A German client's mission: the commodity shows in German, lookups go by the journal's symbol.
    #[test]
    fn commodity_name_and_symbol() {
        let mut ms = Missions::default();
        let a = ev::MissionAccepted {
            name: "Mission_Collect_Industry".into(),
            mission_id: 7,
            commodity: Some("$HydrogenFuel_Name;".into()),
            commodity_localised: Some("Wasserstoff-Treibstoff".into()),
            count: Some(12),
            ..Default::default()
        };
        ms.on_accepted(&a, Timestamp(1), "Sys", None);
        let m = &ms.active[&7];
        assert_eq!((m.commodity.as_deref(), m.commodity_symbol.as_deref()), (Some("Wasserstoff-Treibstoff"), Some("$HydrogenFuel_Name;")));
    }
}
