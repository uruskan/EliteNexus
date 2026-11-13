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
                colony_range_m: xb::colony_range_m(&genus).or_else(|| xb::colony_range_m(&species)),
                value: xb::species(&species).map(|s| s.value),
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
        self.unsold.push(rec.clone());
        ScanOutcome::Completed(rec)
    }

    /// Each sold sample takes one matching record off the unsold list. The match is on the species' codex symbol, which
    /// the game writes the same in every language; the display name can differ between the scan and the sale, and a
    /// match on it alone left sold samples listed as unsold. The name is only the fallback, for a sale without a symbol.
    pub fn on_sell(&mut self, s: &ev::SellOrganicData, ts: Timestamp) -> i64 {
        let total: i64 = s.bio_data.iter().map(|b| b.value + b.bonus).sum();
        for sold in &s.bio_data {
            let by_symbol = || {
                (!sold.species.is_empty()).then(|| self.unsold.iter().position(|u| u.species_symbol.eq_ignore_ascii_case(&sold.species))).flatten()
            };
            let by_name = || {
                let name = sold.species_localised.as_deref().unwrap_or(&sold.species);
                self.unsold.iter().position(|u| u.species.eq_ignore_ascii_case(name))
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

    /// Unsold exobiology data is lost on death.
    pub fn on_death(&mut self) {
        self.unsold.clear();
        self.current = None;
    }

    pub fn unsold_value(&self) -> i64 {
        self.unsold.iter().map(BioRecord::expected_payout).sum()
    }
}
