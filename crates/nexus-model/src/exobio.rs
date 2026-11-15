//! Exobiology sampling assistant and unsold Vista Genomics data.

use crate::location::surface_distance;
use nexus_data::exobio as xb;
use nexus_journal::events as ev;
use nexus_journal::Timestamp;

#[derive(Debug, Clone, Copy)]
pub struct SamplePoint {
    pub lat: f64,
    pub lon: f64,
    pub ts: Timestamp,
}

#[derive(Debug, Clone)]
pub struct SampleInProgress {
    pub genus: String,
    pub species: String,
    pub species_symbol: String,
    pub variant: Option<String>,
    pub system_address: u64,
    pub body_id: u32,
    pub body_name: String,
    /// Where each sample was taken (live only — the journal does not record positions).
    pub points: Vec<SamplePoint>,
    /// 1..=3 samples taken.
    pub stage: u8,
    pub started: Timestamp,
    pub colony_range_m: Option<u32>,
    pub value: Option<i64>,
}

impl SampleInProgress {
    /// Distance to the nearest previous sample from `pos` on a body of `radius_m`.
    pub fn nearest_sample_m(&self, pos: (f64, f64), radius_m: f64) -> Option<f64> {
        self.points.iter().map(|p| surface_distance((p.lat, p.lon), pos, radius_m)).reduce(f64::min)
    }

    /// True when `pos` is far enough from every previous sample for the next one to count.
    pub fn far_enough(&self, pos: (f64, f64), radius_m: f64) -> Option<bool> {
        let range = f64::from(self.colony_range_m?);
        Some(self.nearest_sample_m(pos, radius_m).is_none_or(|d| d >= range))
    }
}

#[derive(Debug, Clone)]
pub struct BioRecord {
    pub ts: Timestamp,
    pub genus: String,
    pub species: String,
    /// The species' codex symbol (`$Codex_Ent_Stratum_07_Name;`): the same in every game language, unlike `species`.
    pub species_symbol: String,
    pub variant: Option<String>,
    pub system_address: u64,
    pub system: String,
    pub body_id: u32,
    pub body_name: String,
    pub value: i64,
    /// We expect the first-logged bonus (nobody had set foot on the body when we scanned it).
    pub bonus_expected: bool,
}

impl BioRecord {
    pub fn expected_payout(&self) -> i64 {
        self.value + if self.bonus_expected { self.value * xb::FIRST_LOGGED_BONUS_FACTOR } else { 0 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExoBio {
    pub current: Option<SampleInProgress>,
    pub unsold: Vec<BioRecord>,
    pub sold_total: i64,
    pub analysed_total: u64,
    pub sales: Vec<(Timestamp, i64)>,
    /// Sales the game logged without their samples (`SellOrganicData` with an empty `BioData`), since the list was last
    /// cleared and the last death. No journal says which samples they sold, or for how much.
    pub unlisted_sales: Vec<Timestamp>,
    /// How far the commander cleared the unsold list (Exobiology › Clear list, kept in Elite Nexus's settings): no
    /// sample analysed then or before is listed, and no sale the game logged without its samples then or before is
    /// remembered. It's set before the journals are read, so those samples never enter the list and a later sale takes
    /// the right one off it.
    pub cleared: Option<Timestamp>,
}

pub enum ScanOutcome {
    Started,
    Progress(u8),
    Completed(BioRecord),
}

impl ExoBio {
    #[allow(clippy::too_many_arguments)]
    pub fn on_scan_organic(
        &mut self,
        o: &ev::ScanOrganic,
        ts: Timestamp,
        position: Option<(f64, f64)>,
        system: &str,
        body_name: &str,
        body_unfootfalled: bool,
    ) -> ScanOutcome {
        let genus = o.genus_localised.clone().unwrap_or_else(|| nexus_data::names::humanize_symbol(&o.genus));
        let species = o.species_localised.clone().unwrap_or_else(|| nexus_data::names::humanize_symbol(&o.species));
        let point = position.map(|(lat, lon)| SamplePoint { lat, lon, ts });
        let same = self
            .current
            .as_ref()
            .is_some_and(|c| c.species_symbol == o.species && c.body_id == o.body && c.system_address == o.system_address);
        let stage = match o.scan_type.as_str() {
            "Log" => 1,
            "Sample" => 2,
            _ => 3,
        };
        if stage == 1 || !same {
            self.current = Some(SampleInProgress {
                genus: genus.clone(),
                species: species.clone(),
                species_symbol: o.species.clone(),
                variant: o.variant_localised.clone().or_else(|| o.variant.clone()),
                system_address: o.system_address,
                body_id: o.body,
                body_name: body_name.to_string(),
                points: Vec::new(),
                stage: 0,
                started: ts,
                // By codex symbol first: the display names are in the game's language and the data's are English.
                colony_range_m: xb::colony_range_m(&o.genus)
                    .or_else(|| xb::colony_range_m(&genus))
                    .or_else(|| xb::colony_range_m(&o.species))
                    .or_else(|| xb::colony_range_m(&species)),
                value: xb::species(&o.species).or_else(|| xb::species(&species)).map(|s| s.value),
            });
        }
        let cur = self.current.as_mut().expect("set above");
        cur.stage = stage;
        if let Some(p) = point {
            cur.points.push(p);
        }
        if stage < 3 {
            return if stage == 1 { ScanOutcome::Started } else { ScanOutcome::Progress(stage) };
        }
        let cur = self.current.take().expect("set above");
        self.analysed_total += 1;
        let rec = BioRecord {
            ts,
            genus: cur.genus,
            species: cur.species,
            species_symbol: cur.species_symbol,
            variant: cur.variant,
            system_address: o.system_address,
            system: system.to_string(),
            body_id: o.body,
            body_name: cur.body_name,
            value: cur.value.unwrap_or(0),
            bonus_expected: body_unfootfalled,
        };
        if !self.before_clear(ts) {
            self.unsold.push(rec.clone());
        }
        ScanOutcome::Completed(rec)
    }

    /// At or before the time the commander cleared the unsold list up to.
    fn before_clear(&self, ts: Timestamp) -> bool {
        self.cleared.is_some_and(|c| ts <= c)
    }

    /// Each sold sample takes one matching record off the unsold list. The match is on the species' codex symbol, which
    /// the game writes the same in every language; the display name can differ between the scan and the sale, and a
    /// match on it alone left sold samples listed as unsold. The name is only the fallback, for a sale without a symbol.
    /// Of the records that match, the first whose expected first-logged bonus agrees with the sale's `Bonus` comes off,
    /// or else the first: a sale of some of a species' samples would otherwise leave a bonus listed that was paid.
    ///
    /// The game sometimes logs a sale with no samples in it (an empty `BioData`; seen when a commander sold many pages
    /// of samples at once). That sale is remembered in [`Self::unlisted_sales`] and takes nothing off the list: nothing
    /// says which samples it sold.
    pub fn on_sell(&mut self, s: &ev::SellOrganicData, ts: Timestamp) -> i64 {
        if s.bio_data.is_empty() && !self.before_clear(ts) {
            self.unlisted_sales.push(ts);
        }
        let total: i64 = s.bio_data.iter().map(|b| b.value + b.bonus).sum();
        for sold in &s.bio_data {
            let bonus = sold.bonus > 0;
            let by_symbol = || {
                (!sold.species.is_empty()).then(|| matching(&self.unsold, bonus, |u| u.species_symbol.eq_ignore_ascii_case(&sold.species))).flatten()
            };
            let by_name = || {
                let name = sold.species_localised.as_deref().unwrap_or(&sold.species);
                matching(&self.unsold, bonus, |u| u.species.eq_ignore_ascii_case(name))
            };
            if let Some(i) = by_symbol().or_else(by_name) {
                self.unsold.remove(i);
            }
        }
        self.sold_total += total;
        self.sales.push((ts, total));
        total
    }

    /// Arriving in another system abandons a partly sampled organism (the game discards it).
    pub fn on_system(&mut self, system_address: u64) {
        if self.current.as_ref().is_some_and(|c| c.system_address != system_address) {
            self.current = None;
        }
    }

    /// Unsold exobiology data is lost on death, and with it any doubt about what an unlisted sale took.
    pub fn on_death(&mut self) {
        self.unsold.clear();
        self.unlisted_sales.clear();
        self.current = None;
    }

    pub fn unsold_value(&self) -> i64 {
        self.unsold.iter().map(BioRecord::expected_payout).sum()
    }

    /// The sales the game logged without their samples that may have sold some of the samples listed: those after the
    /// oldest of them was analysed (a sale can't take a sample analysed after it). Oldest first.
    pub fn doubtful_sales(&self) -> impl Iterator<Item = Timestamp> + '_ {
        let oldest = self.unsold.iter().map(|r| r.ts).min();
        self.unlisted_sales.iter().copied().filter(move |t| oldest.is_some_and(|o| *t > o))
    }

    /// Where Clear list clears the list up to: the newest of the samples listed and of the sales logged without their
    /// samples. None when there's nothing to clear.
    pub fn clear_point(&self) -> Option<Timestamp> {
        self.unsold.iter().map(|r| r.ts).chain(self.unlisted_sales.iter().copied()).max()
    }

    /// Clears the unsold list up to `upto` (from [`Self::clear_point`]): the samples analysed then or before leave it,
    /// and so do the sales logged without their samples. A sample analysed later stays. Never clears less than an
    /// earlier Clear did.
    pub fn clear(&mut self, upto: Timestamp) {
        if self.before_clear(upto) {
            return;
        }
        self.cleared = Some(upto);
        self.unsold.retain(|r| r.ts > upto);
        self.unlisted_sales.retain(|t| *t > upto);
    }
}

/// The first record `is` takes whose expected first-logged bonus is `bonus`, or else the first it takes.
fn matching(unsold: &[BioRecord], bonus: bool, is: impl Fn(&BioRecord) -> bool) -> Option<usize> {
    unsold.iter().position(|u| is(u) && u.bonus_expected == bonus).or_else(|| unsold.iter().position(is))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TECTONICAS: i64 = 19_010_800;

    /// A Stratum Tectonicas analysed on `body_id` at `ts`, with the first-logged bonus expected or not.
    fn tectonicas(body_id: u32, ts: i64, bonus_expected: bool) -> BioRecord {
        BioRecord {
            ts: Timestamp(ts),
            genus: "Stratum".into(),
            species: "Stratum Tectonicas".into(),
            species_symbol: "$Codex_Ent_Stratum_07_Name;".into(),
            variant: None,
            system_address: 100,
            system: "Alpha".into(),
            body_id,
            body_name: format!("Alpha {body_id}"),
            value: TECTONICAS,
            bonus_expected,
        }
    }

    /// A sale of one Tectonicas, paid with the first-logged bonus or not; `symbol` false: a sale naming it only.
    fn sale(bonus: bool, symbol: bool) -> ev::SellOrganicData {
        let sold = ev::BioDataSale {
            species: if symbol { "$Codex_Ent_Stratum_07_Name;".into() } else { String::new() },
            species_localised: Some("Stratum Tectonicas".into()),
            value: TECTONICAS,
            bonus: if bonus { TECTONICAS * xb::FIRST_LOGGED_BONUS_FACTOR } else { 0 },
            ..Default::default()
        };
        ev::SellOrganicData { market_id: 1, bio_data: vec![sold] }
    }

    fn bodies(bio: &ExoBio) -> Vec<u32> {
        bio.unsold.iter().map(|r| r.body_id).collect()
    }

    /// A sale of some of a species' samples takes off the one its bonus agrees with. The first on the list expected no
    /// bonus; taking it for a sale that paid one left the other listed at five times what it will fetch.
    #[test]
    fn a_sale_takes_off_the_sample_its_bonus_agrees_with() {
        let list = |second_bonus: bool| ExoBio { unsold: vec![tectonicas(2, 10, false), tectonicas(3, 20, second_bonus)], ..Default::default() };
        for symbol in [true, false] {
            let mut bio = list(true);
            bio.on_sell(&sale(true, symbol), Timestamp(30));
            assert_eq!(bodies(&bio), [2], "paid the bonus: the sample that expected it (symbol {symbol})");
            assert_eq!(bio.unsold_value(), TECTONICAS);
            let mut bio = list(true);
            bio.on_sell(&sale(false, symbol), Timestamp(30));
            assert_eq!(bodies(&bio), [3], "no bonus: the sample that expected none (symbol {symbol})");
        }
        // No sample agrees with the sale: the first comes off, as before.
        let mut bio = list(false);
        bio.on_sell(&sale(true, true), Timestamp(30));
        assert_eq!(bodies(&bio), [3]);
    }
}
