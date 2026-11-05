//! Commander identity, credits, ranks, reputation, engineers, Powerplay and squadron.

use nexus_data::ranks::RankKind;
use nexus_journal::events as ev;
use nexus_journal::Timestamp;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct RankLadder {
    pub combat: u32,
    pub trade: u32,
    pub explore: u32,
    pub soldier: u32,
    pub exobiologist: u32,
    pub empire: u32,
    pub federation: u32,
    pub cqc: u32,
}

/// One ladder's value in a `Rank`, `Progress` or `Promotion` event.
pub fn rank_value(r: &ev::RankSet, k: RankKind) -> Option<u32> {
    match k {
        RankKind::Combat => r.combat,
        RankKind::Trade => r.trade,
        RankKind::Explore => r.explore,
        RankKind::Soldier => r.soldier,
        RankKind::Exobiologist => r.exobiologist,
        RankKind::Empire => r.empire,
        RankKind::Federation => r.federation,
        RankKind::Cqc => r.cqc,
    }
}

impl RankLadder {
    pub fn apply(&mut self, r: &ev::RankSet) {
        let set = |slot: &mut u32, v: Option<u32>| {
            if let Some(v) = v {
                *slot = v;
            }
        };
        set(&mut self.combat, r.combat);
        set(&mut self.trade, r.trade);
        set(&mut self.explore, r.explore);
        set(&mut self.soldier, r.soldier);
        set(&mut self.exobiologist, r.exobiologist);
        set(&mut self.empire, r.empire);
        set(&mut self.federation, r.federation);
        set(&mut self.cqc, r.cqc);
    }

    pub fn get(&self, k: RankKind) -> u32 {
        match k {
            RankKind::Combat => self.combat,
            RankKind::Trade => self.trade,
            RankKind::Explore => self.explore,
            RankKind::Soldier => self.soldier,
            RankKind::Exobiologist => self.exobiologist,
            RankKind::Empire => self.empire,
            RankKind::Federation => self.federation,
            RankKind::Cqc => self.cqc,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EngineerStatus {
    pub id: u32,
    pub name: String,
    /// Known / Invited / Acquainted / Unlocked / Barred.
    pub progress: String,
    pub rank: Option<u32>,
    pub rank_progress: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct PowerplayStatus {
    pub power: String,
    pub rank: u32,
    pub merits: i64,
    pub time_pledged_secs: i64,
    pub session_merits: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Squadron {
    pub name: String,
    pub id: Option<u64>,
    pub rank: Option<u32>,
    pub rank_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SuperpowerReputation {
    pub empire: f64,
    pub federation: f64,
    pub alliance: f64,
    pub independent: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Commander {
    pub name: String,
    pub fid: String,
    pub credits: i64,
    pub loan: i64,
    pub game_mode: String,
    pub group: Option<String>,
    pub horizons: bool,
    pub odyssey: bool,
    /// The flags exactly as the last LoadGame stated them (None = key absent), for EDDN headers.
    pub load_game_flags: (Option<bool>, Option<bool>),
    pub game_version: String,
    pub game_build: String,
    pub language: String,
    pub ranks: RankLadder,
    /// Percent towards the next rank, as the game last reported it (`Progress`, written at login): read it through
    /// [`Commander::rank_progress`].
    pub progress: RankLadder,
    /// Ranks promoted since the game last reported progress: theirs isn't known until it does again.
    pub promoted: Vec<RankKind>,
    pub reputation: SuperpowerReputation,
    pub powerplay: Option<PowerplayStatus>,
    pub squadron: Option<Squadron>,
    pub squadron_applications: Vec<String>,
    pub engineers: BTreeMap<u32, EngineerStatus>,
    pub statistics: Option<Arc<ev::Statistics>>,
    /// When the game wrote `statistics` (at a login).
    pub statistics_at: Timestamp,
    pub last_login: Timestamp,
}

impl Commander {
    pub fn on_load_game(&mut self, l: &ev::LoadGame, ts: Timestamp) {
        if !l.commander.is_empty() {
            self.name = l.commander.clone();
        }
        if !l.fid.is_empty() {
            self.fid = l.fid.clone();
        }
        self.credits = l.credits;
        self.loan = l.loan;
        self.game_mode = l.game_mode.clone().unwrap_or_default();
        self.group = l.group.clone();
        self.horizons = l.horizons.unwrap_or(false);
        self.odyssey = l.odyssey.unwrap_or(false);
        self.load_game_flags = (l.horizons, l.odyssey);
        if !l.gameversion.is_empty() {
            self.game_version = l.gameversion.clone();
        }
        if !l.build.is_empty() {
            self.game_build = l.build.clone();
        }
        if !l.language.is_empty() {
            self.language = l.language.clone();
        }
        self.last_login = ts;
        if let Some(pp) = self.powerplay.as_mut() {
            pp.session_merits = 0;
        }
    }

    pub fn on_engineer_progress(&mut self, e: &ev::EngineerProgress) {
        if let Some(list) = &e.engineers {
            for en in list {
                self.engineers.insert(
                    en.engineer_id,
                    EngineerStatus {
                        id: en.engineer_id,
                        name: en.engineer.clone(),
                        progress: en.progress.clone(),
                        rank: en.rank,
                        rank_progress: en.rank_progress,
                    },
                );
            }
        } else if let Some(id) = e.engineer_id {
            let entry = self.engineers.entry(id).or_insert_with(|| EngineerStatus { id, ..Default::default() });
            if let Some(n) = &e.engineer {
                entry.name = n.clone();
            }
            if let Some(p) = &e.progress {
                entry.progress = p.clone();
            }
            if e.rank.is_some() {
                entry.rank = e.rank;
            }
            if e.rank_progress.is_some() {
                entry.rank_progress = e.rank_progress;
            }
        }
    }

    pub fn on_reputation(&mut self, r: &ev::Reputation) {
        if let Some(v) = r.empire {
            self.reputation.empire = v;
        }
        if let Some(v) = r.federation {
            self.reputation.federation = v;
        }
        if let Some(v) = r.alliance {
            self.reputation.alliance = v;
        }
        if let Some(v) = r.independent {
            self.reputation.independent = v;
        }
    }

    /// Legacy (Horizons 3.8) clients write 3.x versions; Live is 4.x.
    pub fn is_legacy(&self) -> bool {
        self.game_version.starts_with("3.")
    }

    pub fn rank_name(&self, k: RankKind) -> &'static str {
        k.name_of(self.ranks.get(k))
    }

    /// Percent towards the next rank. None after a promotion, until the game reports progress again (at the next
    /// login): the one it reported before belongs to the old rank.
    pub fn rank_progress(&self, k: RankKind) -> Option<u32> {
        (!self.promoted.contains(&k)).then(|| self.progress.get(k))
    }

    /// `Progress`: every ladder it reports is known again.
    pub fn on_progress(&mut self, p: &ev::RankSet) {
        self.progress.apply(p);
        self.promoted.retain(|k| rank_value(p, *k).is_none());
    }

    /// `Promotion`: the ladders it names have a new rank, and progress on them is no longer known.
    pub fn on_promotion(&mut self, p: &ev::RankSet) {
        self.ranks.apply(p);
        for k in RankKind::ALL {
            if rank_value(p, k).is_some() && !self.promoted.contains(&k) {
                self.promoted.push(k);
            }
        }
    }

    pub fn unlocked_engineers(&self) -> impl Iterator<Item = &EngineerStatus> {
        self.engineers.values().filter(|e| e.progress == "Unlocked")
    }
}
