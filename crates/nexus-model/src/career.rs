//! The game's own statistics in plain words. Elite Dangerous writes them at every login (the `Statistics` event): the
//! figures of the cockpit's Statistics panel, totals since the commander was created. This module says what each one
//! is and how it reads, and builds what the Statistics page shows from them: where the credits came from and where they
//! went, the records, and the round numbers coming up.
//!
//! Keys come and go with the commander's history and the game's updates, so an unknown key still gets a readable label
//! (`humanize`) and a unit guessed from its name.

use nexus_data::names::{compact_credits, group_digits};
use nexus_journal::events::Statistics;
use serde_json::Value;

/// How a figure reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Count,
    Credits,
    Tonnes,
    /// Light years.
    Ly,
    Metres,
    /// A length of time, in seconds.
    Seconds,
    /// A ratio (CQC kills per death, wins per loss).
    Ratio,
    /// Words the game writes (the last Thargoid encounter's system, ship and date).
    Text,
}

/// A group of the game's statistics: its key in the event, and what the page calls it.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    pub key: &'static str,
    pub title: &'static str,
}

/// Every group, in the order the page lists them.
pub const GROUPS: &[Group] = &[
    Group { key: "Bank_Account", title: "Money and assets" },
    Group { key: "Trading", title: "Trade" },
    Group { key: "Mining", title: "Mining" },
    Group { key: "Exploration", title: "Exploration and travel" },
    Group { key: "Exobiology", title: "Exobiology" },
    Group { key: "Combat", title: "Combat" },
    Group { key: "TG_ENCOUNTERS", title: "Thargoids" },
    Group { key: "Thargoid_Encounter_Stats", title: "Thargoids (older figures)" },
    Group { key: "Passengers", title: "Passengers" },
    Group { key: "Search_And_Rescue", title: "Search and rescue" },
    Group { key: "Crafting", title: "Engineering" },
    Group { key: "Material_Trader_Stats", title: "Material traders" },
    Group { key: "Crew", title: "Crew" },
    Group { key: "Multicrew", title: "Multicrew" },
    Group { key: "FLEETCARRIER", title: "Fleet carrier" },
    Group { key: "Squadron", title: "Squadron" },
    Group { key: "Smuggling", title: "Smuggling" },
    Group { key: "Crime", title: "Crime" },
    Group { key: "CQC", title: "CQC" },
];

use Unit::*;

/// What each known figure is: (group, key, label, unit), in the order the page lists them.
const FIGURES: &[(&str, &str, &str, Unit)] = &[
    ("Bank_Account", "Current_Wealth", "Wealth (credits and assets)", Credits),
    ("Bank_Account", "Owned_Ship_Count", "Ships owned", Count),
    ("Bank_Account", "Spent_On_Ships", "Spent on ships", Credits),
    ("Bank_Account", "Spent_On_Outfitting", "Spent on outfitting", Credits),
    ("Bank_Account", "Spent_On_Repairs", "Spent on repairs", Credits),
    ("Bank_Account", "Spent_On_Fuel", "Spent on fuel", Credits),
    ("Bank_Account", "Spent_On_Ammo_Consumables", "Spent on ammunition and restocking", Credits),
    ("Bank_Account", "Insurance_Claims", "Ships lost and bought back (rebuys)", Count),
    ("Bank_Account", "Spent_On_Insurance", "Spent on rebuys", Credits),
    ("Bank_Account", "Suits_Owned", "Suits owned", Count),
    ("Bank_Account", "Weapons_Owned", "Hand weapons owned", Count),
    ("Bank_Account", "Spent_On_Suits", "Spent on suits", Credits),
    ("Bank_Account", "Spent_On_Weapons", "Spent on hand weapons", Credits),
    ("Bank_Account", "Spent_On_Suit_Consumables", "Spent on on-foot consumables", Credits),
    ("Bank_Account", "Premium_Stock_Bought", "Premium stock bought", Count),
    ("Bank_Account", "Spent_On_Premium_Stock", "Spent on premium stock", Credits),
    ("Bank_Account", "MercCoins_Current", "Merc Coins now", Count),
    ("Bank_Account", "MercCoins_Total_Earned", "Merc Coins earned", Count),
    ("Bank_Account", "MercCoins_Total_Spent", "Merc Coins spent", Count),
    ("Bank_Account", "MercCoins_Spent_On_MercGear", "Merc Coins spent on MercGear", Count),
    ("Bank_Account", "MercCoins_Spent_On_Engineering", "Merc Coins spent on engineering", Count),
    ("Trading", "Market_Profits", "Profit from trade", Credits),
    ("Trading", "Resources_Traded", "Goods traded", Tonnes),
    ("Trading", "Markets_Traded_With", "Markets traded with", Count),
    ("Trading", "Highest_Single_Transaction", "Best single sale", Credits),
    ("Trading", "Average_Profit", "Average profit, as the game works it out", Credits),
    ("Trading", "Data_Sold", "Data sold on foot", Count),
    ("Trading", "Goods_Sold", "Goods sold on foot", Count),
    ("Trading", "Assets_Sold", "Assets sold on foot", Count),
    ("Mining", "Mining_Profits", "Profit from mining", Credits),
    ("Mining", "Quantity_Mined", "Mined and refined", Tonnes),
    ("Mining", "Materials_Collected", "Materials collected", Count),
    ("Exploration", "Time_Played", "Time played", Seconds),
    ("Exploration", "Total_Hyperspace_Distance", "Distance jumped", Ly),
    ("Exploration", "Total_Hyperspace_Jumps", "Hyperspace jumps", Count),
    ("Exploration", "Systems_Visited", "Systems visited", Count),
    ("Exploration", "Greatest_Distance_From_Start", "Farthest from where you started", Ly),
    ("Exploration", "Exploration_Profits", "Earned from exploration data", Credits),
    ("Exploration", "Highest_Payout", "Best single exploration payout", Credits),
    ("Exploration", "Planets_Scanned_To_Level_2", "Planets scanned (level 2)", Count),
    ("Exploration", "Planets_Scanned_To_Level_3", "Planets mapped (level 3)", Count),
    ("Exploration", "Efficient_Scans", "Planets mapped within the probe target", Count),
    ("Exploration", "Fuel_Scooped", "Fuel scooped", Tonnes),
    ("Exploration", "Fuel_Purchased", "Fuel bought", Tonnes),
    ("Exploration", "Planet_Footfalls", "Planets walked on", Count),
    ("Exploration", "First_Footfalls", "First footfalls", Count),
    ("Exploration", "Settlements_Visited", "Settlements visited", Count),
    ("Exploration", "OnFoot_Distance_Travelled", "Distance on foot", Metres),
    ("Exploration", "Shuttle_Journeys", "Shuttle journeys", Count),
    ("Exploration", "Shuttle_Distance_Travelled", "Distance by shuttle", Ly),
    ("Exploration", "Spent_On_Shuttles", "Spent on shuttles", Credits),
    ("Exobiology", "Organic_Data_Profits", "Earned from organic data", Credits),
    ("Exobiology", "Organic_Data", "Organic data sold", Count),
    ("Exobiology", "First_Logged", "Species logged first", Count),
    ("Exobiology", "First_Logged_Profits", "Earned from species logged first", Credits),
    ("Exobiology", "Organic_Genus_Encountered", "Genera found", Count),
    ("Exobiology", "Organic_Species_Encountered", "Species found", Count),
    ("Exobiology", "Organic_Variant_Encountered", "Variants found", Count),
    ("Exobiology", "Organic_Genus", "Genera analysed", Count),
    ("Exobiology", "Organic_Species", "Species analysed", Count),
    ("Exobiology", "Organic_Planets", "Planets with organics analysed", Count),
    ("Exobiology", "Organic_Systems", "Systems with organics analysed", Count),
    ("Combat", "Bounties_Claimed", "Bounties claimed", Count),
    ("Combat", "Bounty_Hunting_Profit", "Earned from bounties", Credits),
    ("Combat", "Combat_Bonds", "Combat bonds", Count),
    ("Combat", "Combat_Bond_Profits", "Earned from combat bonds", Credits),
    ("Combat", "Assassinations", "Assassinations", Count),
    ("Combat", "Assassination_Profits", "Earned from assassinations", Credits),
    ("Combat", "Highest_Single_Reward", "Best single reward", Credits),
    ("Combat", "Skimmers_Killed", "Skimmers destroyed", Count),
    ("Combat", "ConflictZone_Total", "Conflict zones fought", Count),
    ("Combat", "ConflictZone_Total_Wins", "Conflict zones won", Count),
    ("Combat", "ConflictZone_High", "High-intensity conflict zones fought", Count),
    ("Combat", "ConflictZone_High_Wins", "High-intensity conflict zones won", Count),
    ("Combat", "ConflictZone_Medium", "Medium-intensity conflict zones fought", Count),
    ("Combat", "ConflictZone_Medium_Wins", "Medium-intensity conflict zones won", Count),
    ("Combat", "ConflictZone_Low", "Low-intensity conflict zones fought", Count),
    ("Combat", "ConflictZone_Low_Wins", "Low-intensity conflict zones won", Count),
    ("Combat", "Settlement_Defended", "Settlements defended", Count),
    ("Combat", "Settlement_Conquered", "Settlements conquered", Count),
    ("Combat", "OnFoot_Combat_Bonds", "Combat bonds on foot", Count),
    ("Combat", "OnFoot_Combat_Bonds_Profits", "Earned from combat bonds on foot", Credits),
    ("Combat", "OnFoot_Scavs_Killed", "Scavengers killed", Count),
    ("Combat", "OnFoot_Skimmers_Killed", "Skimmers destroyed on foot", Count),
    ("Combat", "OnFoot_Vehicles_Destroyed", "Vehicles destroyed on foot", Count),
    ("Combat", "OnFoot_Ships_Destroyed", "Ships destroyed on foot", Count),
    ("Combat", "Dropships_Booked", "Dropships booked", Count),
    ("Combat", "Dropships_Taken", "Dropship flights taken", Count),
    ("Combat", "Dropships_Cancelled", "Dropships cancelled", Count),
    ("TG_ENCOUNTERS", "TG_ENCOUNTER_KILLED", "Thargoids destroyed", Count),
    ("TG_ENCOUNTERS", "TG_SCOUT_COUNT", "Thargoid scouts destroyed", Count),
    ("TG_ENCOUNTERS", "TG_ENCOUNTER_TOTAL", "Thargoid encounters", Count),
    ("TG_ENCOUNTERS", "TG_ENCOUNTER_WAKES", "Thargoid wakes scanned", Count),
    ("TG_ENCOUNTERS", "TG_ENCOUNTER_IMPRINT", "Thargoid imprints", Count),
    ("TG_ENCOUNTERS", "TG_ENCOUNTER_TOTAL_LAST_SYSTEM", "Last encounter: system", Text),
    ("TG_ENCOUNTERS", "TG_ENCOUNTER_TOTAL_LAST_TIMESTAMP", "Last encounter: when (game date)", Text),
    ("TG_ENCOUNTERS", "TG_ENCOUNTER_TOTAL_LAST_SHIP", "Last encounter: ship", Text),
    ("Passengers", "Passengers_Missions_Delivered", "Passengers delivered", Count),
    ("Passengers", "Passengers_Missions_Accepted", "Passenger missions accepted", Count),
    ("Passengers", "Passengers_Missions_VIP", "VIP passengers", Count),
    ("Passengers", "Passengers_Missions_Bulk", "Bulk passengers", Count),
    ("Passengers", "Passengers_Missions_Ejected", "Passengers ejected", Count),
    ("Passengers", "Passengers_Missions_Disgruntled", "Passengers who left unhappy", Count),
    ("Search_And_Rescue", "SearchRescue_Profit", "Earned from search and rescue", Credits),
    ("Search_And_Rescue", "SearchRescue_Count", "Search and rescue hand-ins", Count),
    ("Search_And_Rescue", "SearchRescue_Traded", "Search and rescue items handed in", Count),
    ("Search_And_Rescue", "Salvage_Legal_POI", "Legal salvage from signal sources", Credits),
    ("Search_And_Rescue", "Salvage_Legal_Settlements", "Legal salvage from settlements", Credits),
    ("Search_And_Rescue", "Salvage_Illegal_POI", "Illegal salvage from signal sources", Credits),
    ("Search_And_Rescue", "Salvage_Illegal_Settlements", "Illegal salvage from settlements", Credits),
    ("Search_And_Rescue", "Maglocks_Opened", "Maglocks opened", Count),
    ("Search_And_Rescue", "Panels_Opened", "Panels opened", Count),
    ("Search_And_Rescue", "Settlements_State_FireOut", "Settlement fires put out", Count),
    ("Search_And_Rescue", "Settlements_State_Reboot", "Settlements rebooted", Count),
    ("Crafting", "Count_Of_Used_Engineers", "Engineers you've worked with", Count),
    ("Crafting", "Recipes_Generated", "Engineering rolls", Count),
    ("Crafting", "Recipes_Generated_Rank_1", "Grade 1 rolls", Count),
    ("Crafting", "Recipes_Generated_Rank_2", "Grade 2 rolls", Count),
    ("Crafting", "Recipes_Generated_Rank_3", "Grade 3 rolls", Count),
    ("Crafting", "Recipes_Generated_Rank_4", "Grade 4 rolls", Count),
    ("Crafting", "Recipes_Generated_Rank_5", "Grade 5 rolls", Count),
    ("Crafting", "Recipes_Applied", "Modifications fitted", Count),
    ("Crafting", "Recipes_Applied_Rank_1", "Grade 1 modifications fitted", Count),
    ("Crafting", "Recipes_Applied_Rank_2", "Grade 2 modifications fitted", Count),
    ("Crafting", "Recipes_Applied_Rank_3", "Grade 3 modifications fitted", Count),
    ("Crafting", "Recipes_Applied_Rank_4", "Grade 4 modifications fitted", Count),
    ("Crafting", "Recipes_Applied_Rank_5", "Grade 5 modifications fitted", Count),
    ("Crafting", "Recipes_Applied_On_Previously_Modified_Modules", "Modifications fitted over earlier ones", Count),
    ("Crafting", "Spent_On_Crafting", "Spent on engineering", Credits),
    ("Crafting", "Suits_Upgraded", "Suit upgrades", Count),
    ("Crafting", "Weapons_Upgraded", "Hand weapon upgrades", Count),
    ("Crafting", "Suits_Upgraded_Full", "Suits upgraded to the top grade", Count),
    ("Crafting", "Weapons_Upgraded_Full", "Hand weapons upgraded to the top grade", Count),
    ("Crafting", "Suit_Mods_Applied", "Suit modifications", Count),
    ("Crafting", "Weapon_Mods_Applied", "Hand weapon modifications", Count),
    ("Crafting", "Suit_Mods_Applied_Full", "Suits with every modification slot filled", Count),
    ("Crafting", "Weapon_Mods_Applied_Full", "Hand weapons with every modification slot filled", Count),
    ("Material_Trader_Stats", "Trades_Completed", "Trades at material traders", Count),
    ("Material_Trader_Stats", "Materials_Traded", "Materials traded", Count),
    ("Material_Trader_Stats", "Raw_Materials_Traded", "Raw materials traded", Count),
    ("Material_Trader_Stats", "Encoded_Materials_Traded", "Encoded materials traded", Count),
    ("Material_Trader_Stats", "Grade_1_Materials_Traded", "Grade 1 materials traded", Count),
    ("Material_Trader_Stats", "Grade_2_Materials_Traded", "Grade 2 materials traded", Count),
    ("Material_Trader_Stats", "Grade_3_Materials_Traded", "Grade 3 materials traded", Count),
    ("Material_Trader_Stats", "Grade_4_Materials_Traded", "Grade 4 materials traded", Count),
    ("Material_Trader_Stats", "Grade_5_Materials_Traded", "Grade 5 materials traded", Count),
    ("Material_Trader_Stats", "Assets_Traded_In", "Assets traded in", Count),
    ("Material_Trader_Stats", "Assets_Traded_Out", "Assets traded out", Count),
    ("Crew", "NpcCrew_TotalWages", "Wages paid to crew", Credits),
    ("Crew", "NpcCrew_Hired", "Crew hired", Count),
    ("Crew", "NpcCrew_Fired", "Crew dismissed", Count),
    ("Crew", "NpcCrew_Died", "Crew lost", Count),
    ("Multicrew", "Multicrew_Time_Total", "Time in multicrew", Seconds),
    ("Multicrew", "Multicrew_Gunner_Time_Total", "Time as a gunner", Seconds),
    ("Multicrew", "Multicrew_Fighter_Time_Total", "Time flying fighters", Seconds),
    ("Multicrew", "Multicrew_Credits_Total", "Earned in multicrew", Credits),
    ("Multicrew", "Multicrew_Fines_Total", "Fines in multicrew", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_TOTAL_JUMPS", "Carrier jumps", Count),
    ("FLEETCARRIER", "FLEETCARRIER_DISTANCE_TRAVELLED", "Distance the carrier jumped", Ly),
    ("FLEETCARRIER", "FLEETCARRIER_TRADEPROFIT_TOTAL", "Earned by the carrier's market", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_TRADESPEND_TOTAL", "Spent by the carrier's market", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_EXPORT_TOTAL", "Goods the carrier sold", Tonnes),
    ("FLEETCARRIER", "FLEETCARRIER_IMPORT_TOTAL", "Goods the carrier bought", Tonnes),
    ("FLEETCARRIER", "FLEETCARRIER_STOLENPROFIT_TOTAL", "Earned from stolen goods at the carrier", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_STOLENSPEND_TOTAL", "Spent on stolen goods at the carrier", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_SHIPYARD_SOLD", "Ships sold at the carrier", Count),
    ("FLEETCARRIER", "FLEETCARRIER_SHIPYARD_PROFIT", "Earned by the carrier's shipyard", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_OUTFITTING_SOLD", "Modules sold at the carrier", Count),
    ("FLEETCARRIER", "FLEETCARRIER_OUTFITTING_PROFIT", "Earned by the carrier's outfitting", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_REFUEL_TOTAL", "Ships refuelled at the carrier", Count),
    ("FLEETCARRIER", "FLEETCARRIER_REFUEL_PROFIT", "Earned from refuelling", Credits),
    ("FLEETCARRIER", "FLEETCARRIER_REPAIRS_TOTAL", "Ships repaired at the carrier", Count),
    ("FLEETCARRIER", "FLEETCARRIER_REARM_TOTAL", "Ships rearmed at the carrier", Count),
    ("FLEETCARRIER", "FLEETCARRIER_VOUCHERS_REDEEMED", "Vouchers redeemed at the carrier", Count),
    ("FLEETCARRIER", "FLEETCARRIER_VOUCHERS_PROFIT", "Value of vouchers redeemed at the carrier", Credits),
    ("Squadron", "Squadron_Bank_Credits_Deposited", "Credits deposited in the squadron bank", Credits),
    ("Squadron", "Squadron_Bank_Credits_Withdrawn", "Credits withdrawn from the squadron bank", Credits),
    ("Squadron", "Squadron_Bank_Commodities_Deposited_Num", "Commodities deposited", Count),
    ("Squadron", "Squadron_Bank_Commodities_Deposited_Value", "Value of commodities deposited", Credits),
    ("Squadron", "Squadron_Bank_Commodities_Withdrawn_Num", "Commodities withdrawn", Count),
    ("Squadron", "Squadron_Bank_Commodities_Withdrawn_Value", "Value of commodities withdrawn", Credits),
    ("Squadron", "Squadron_Bank_PersonalAssets_Deposited_Num", "Personal assets deposited", Count),
    ("Squadron", "Squadron_Bank_PersonalAssets_Deposited_Value", "Value of personal assets deposited", Credits),
    ("Squadron", "Squadron_Bank_PersonalAssets_Withdrawn_Num", "Personal assets withdrawn", Count),
    ("Squadron", "Squadron_Bank_PersonalAssets_Withdrawn_Value", "Value of personal assets withdrawn", Credits),
    ("Squadron", "Squadron_Bank_Ships_Deposited_Num", "Ships deposited", Count),
    ("Squadron", "Squadron_Bank_Ships_Deposited_Value", "Value of ships deposited", Credits),
    ("Squadron", "Squadron_Leaderboard_podiums", "Leaderboard podiums", Count),
    ("Smuggling", "Black_Markets_Profits", "Profit at black markets", Credits),
    ("Smuggling", "Resources_Smuggled", "Goods smuggled", Tonnes),
    ("Smuggling", "Black_Markets_Traded_With", "Black markets traded with", Count),
    ("Smuggling", "Highest_Single_Transaction", "Best single black-market sale", Credits),
    ("Smuggling", "Average_Profit", "Average black-market profit, as the game works it out", Credits),
    ("Crime", "Notoriety", "Notoriety now (0 to 10)", Count),
    ("Crime", "Fines", "Fines received", Count),
    ("Crime", "Total_Fines", "Fines, in all", Credits),
    ("Crime", "Bounties_Received", "Bounties put on you", Count),
    ("Crime", "Total_Bounties", "Bounties put on you, in all", Credits),
    ("Crime", "Highest_Bounty", "Highest bounty on you", Credits),
    ("Crime", "Total_Murders", "Murders", Count),
    ("Crime", "Citizens_Murdered", "Citizens murdered", Count),
    ("Crime", "Guards_Murdered", "Guards murdered", Count),
    ("Crime", "Omnipol_Murdered", "Omnipol officers murdered", Count),
    ("Crime", "Total_Stolen", "Items stolen", Count),
    ("Crime", "Goods_Stolen", "Goods stolen", Count),
    ("Crime", "Data_Stolen", "Data stolen", Count),
    ("Crime", "Sample_Stolen", "Samples stolen", Count),
    ("Crime", "Value_Stolen_StateChange", "Value stolen", Credits),
    ("Crime", "Profiles_Cloned", "Profiles cloned", Count),
    ("Crime", "Malware_Uploaded", "Malware uploaded", Count),
    ("Crime", "Settlements_State_Shutdown", "Settlements shut down", Count),
    ("Crime", "Production_Sabotage", "Production sabotaged", Count),
    ("Crime", "Production_Theft", "Production stolen", Count),
    ("Crime", "Turrets_Total", "Turrets taken out", Count),
    ("Crime", "Turrets_Destroyed", "Turrets destroyed", Count),
    ("Crime", "Turrets_Overloaded", "Turrets overloaded", Count),
    ("CQC", "CQC_Credits_Earned", "Earned in CQC", Credits),
    ("CQC", "CQC_Time_Played", "Time in CQC", Seconds),
    ("CQC", "CQC_Kills", "Kills", Count),
    ("CQC", "CQC_KD", "Kills per death", Ratio),
    ("CQC", "CQC_WL", "Wins per loss", Ratio),
];

/// The passenger kinds the game counts (`Passengers_Missions_<Kind>_<vip|bulk>_<delivered|ejected>`), and their names.
const PASSENGER_KINDS: &[(&str, &str)] = &[
    ("Tourist", "Tourists"),
    ("Refugee", "Refugees"),
    ("AidWorker", "Aid workers"),
    ("POW", "Prisoners of war"),
    ("Soldiers", "Soldiers"),
    ("Protestors", "Protesters"),
    ("Prisoners", "Prisoners"),
    ("FreedomFighters", "Freedom fighters"),
    ("Politicians", "Politicians"),
    ("PoliticalPrisoner", "Political prisoners"),
    ("SecurityForces", "Security forces"),
    ("Businessmen", "Business people"),
    ("ScienceTeams", "Science teams"),
    ("Explorers", "Explorers"),
    ("Celebrities", "Celebrities"),
    ("HeadOfState", "Heads of state"),
    ("Criminal", "Criminals"),
    ("Medical", "Medical teams"),
    ("Disgruntled", "Disgruntled passengers"),
];

/// A passenger count by kind: (kind's name, VIP, delivered).
fn passenger_key(key: &str) -> Option<(&'static str, bool, bool)> {
    let rest = key.strip_prefix("Passengers_Missions_")?;
    let (rest, delivered) = match rest.strip_suffix("_delivered") {
        Some(r) => (r, true),
        None => (rest.strip_suffix("_ejected")?, false),
    };
    let (kind, vip) = match rest.strip_suffix("_vip") {
        Some(k) => (k, true),
        None => (rest.strip_suffix("_bulk")?, false),
    };
    PASSENGER_KINDS.iter().find(|(k, _)| *k == kind).map(|(_, name)| (*name, vip, delivered))
}

/// A readable label for a key the table doesn't know: words split at underscores and capitals, the group's own
/// prefix dropped ("FLEETCARRIER_TOTAL_JUMPS" reads "Total jumps").
pub fn humanize(key: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for part in key.split('_').filter(|p| !p.is_empty()) {
        let split = nexus_data::names::split_camel(part);
        words.extend(split.split(' ').map(str::to_lowercase));
    }
    while words.len() > 1 && matches!(words[0].as_str(), "fleetcarrier" | "tg" | "cqc" | "multicrew" | "npccrew" | "searchrescue" | "squadron") {
        words.remove(0);
    }
    let mut out = words.join(" ");
    if let Some(first) = out.get(..1) {
        out = first.to_uppercase() + &out[1..];
    }
    out
}

/// The unit a key the table doesn't know probably has, from its name.
fn guess_unit(key: &str) -> Unit {
    let k = key.to_ascii_lowercase();
    if ["profit", "spent", "wages", "value", "credits", "payout", "reward"].iter().any(|w| k.contains(w)) {
        Credits
    } else if k.contains("distance") {
        Ly
    } else if k.contains("time") {
        Seconds
    } else {
        Count
    }
}

/// What a figure is called and how it reads.
pub fn describe(group: &str, key: &str) -> (String, Unit) {
    if let Some((_, _, label, unit)) = FIGURES.iter().find(|(g, k, _, _)| *g == group && *k == key) {
        return ((*label).to_string(), *unit);
    }
    if let Some((kind, vip, delivered)) = passenger_key(key) {
        return (format!("{kind} {} ({})", if delivered { "delivered" } else { "ejected" }, if vip { "VIP" } else { "bulk" }), Count);
    }
    if let Some(board) = key.strip_prefix("Squadron_Leaderboard_").and_then(|k| k.strip_suffix("_highestcontribution")) {
        let board = match board {
            "trade_illicit" => "illicit trade".to_string(),
            "bgs" => "BGS".to_string(),
            "cqc" => "CQC".to_string(),
            "powerplay" => "Powerplay".to_string(),
            "operationscore" => "operations".to_string(),
            "colonisation_contribution" => "colonisation".to_string(),
            other => other.replace('_', " "),
        };
        return (format!("Best squadron leaderboard contribution: {board}"), Count);
    }
    (humanize(key), guess_unit(key))
}

/// A number as the game wrote it (some arrive as text).
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `value` in `unit`, as the page writes it: credits short (39.33 B Cr), counts with their thousands grouped, time in
/// hours and minutes.
pub fn show(value: f64, unit: Unit) -> String {
    let whole = value.round() as i64;
    match unit {
        Count | Text => group_digits(whole),
        Credits => format!("{} Cr", compact_credits(whole)),
        Tonnes => format!("{} t", group_digits(whole)),
        Ly if value.abs() < 100.0 => format!("{value:.1} ly"),
        Ly => format!("{} ly", group_digits(whole)),
        Metres if value.abs() >= 1_000.0 => format!("{:.1} km", value / 1_000.0),
        Metres => format!("{whole} m"),
        Seconds => duration(whole),
        Ratio => format!("{value:.2}"),
    }
}

/// A length of time: "326 h 15 m", "45 m", "50 s".
pub fn duration(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..60 => format!("{s} s"),
        60..3_600 => format!("{} m", s / 60),
        _ => format!("{} h {} m", group_digits(s / 3_600), (s % 3_600) / 60),
    }
}

/// One figure of the game's statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub key: String,
    pub label: String,
    pub unit: Unit,
    /// The number (None for text).
    pub value: Option<f64>,
    pub text: Option<String>,
}

impl Figure {
    /// Nothing to report: zero, or empty text.
    pub fn is_zero(&self) -> bool {
        match (&self.value, &self.text) {
            (Some(v), _) => *v == 0.0,
            (None, Some(t)) => t.trim().is_empty(),
            (None, None) => true,
        }
    }

    pub fn shown(&self) -> String {
        match (&self.value, &self.text) {
            (Some(v), _) if self.unit != Text => show(*v, self.unit),
            (_, Some(t)) => t.clone(),
            (Some(v), None) => show(*v, Count),
            (None, None) => String::new(),
        }
    }
}

static NULL: Value = Value::Null;

/// The game's statistics, read.
#[derive(Debug, Clone, Copy)]
pub struct Career<'a> {
    pub stats: &'a Statistics,
}

impl<'a> Career<'a> {
    pub fn new(stats: &'a Statistics) -> Self {
        Career { stats }
    }

    /// A group by its key in the event (Null when the game didn't write it).
    pub fn group(&self, key: &str) -> &'a Value {
        let s = self.stats;
        match key {
            "Bank_Account" => &s.bank_account,
            "Combat" => &s.combat,
            "Crime" => &s.crime,
            "Smuggling" => &s.smuggling,
            "Trading" => &s.trading,
            "Mining" => &s.mining,
            "Exploration" => &s.exploration,
            "Passengers" => &s.passengers,
            "Search_And_Rescue" => &s.search_and_rescue,
            "Crafting" => &s.crafting,
            "Crew" => &s.crew,
            "Multicrew" => &s.multicrew,
            "Material_Trader_Stats" => &s.material_trader_stats,
            "Exobiology" => &s.exobiology,
            "Thargoid_Encounter_Stats" => &s.thargoid_encounter_stats,
            "TG_ENCOUNTERS" => &s.tg_encounters,
            "CQC" => &s.cqc,
            "FLEETCARRIER" => &s.fleetcarrier,
            "Squadron" => &s.squadron,
            _ => &NULL,
        }
    }

    /// A figure's number; 0 when the game didn't write it.
    pub fn get(&self, group: &str, key: &str) -> f64 {
        self.group(group).get(key).and_then(number).unwrap_or(0.0)
    }

    /// A figure as a whole number (credits, counts).
    pub fn int(&self, group: &str, key: &str) -> i64 {
        self.get(group, key).round() as i64
    }

    /// Text the game wrote for a figure ("" when it didn't).
    pub fn text(&self, group: &str, key: &str) -> &'a str {
        self.group(group).get(key).and_then(Value::as_str).unwrap_or("")
    }

    /// Every figure of a group: the known ones in the table's order, then the rest in the order the game wrote them.
    pub fn figures(&self, group: &str) -> Vec<Figure> {
        let Some(obj) = self.group(group).as_object() else { return Vec::new() };
        let figure = |key: &str, v: &Value| {
            let (label, unit) = describe(group, key);
            let text = v.as_str().map(str::to_string);
            let unit = if text.is_some() && number(v).is_none() { Text } else { unit };
            Figure { key: key.to_string(), label, unit, value: if unit == Text { None } else { number(v) }, text }
        };
        let mut out: Vec<Figure> = FIGURES.iter().filter(|(g, ..)| *g == group).filter_map(|(_, k, ..)| obj.get(*k).map(|v| figure(k, v))).collect();
        for (k, v) in obj {
            if !FIGURES.iter().any(|(g, fk, ..)| *g == group && *fk == k.as_str()) && (v.is_number() || v.is_string()) {
                out.push(figure(k, v));
            }
        }
        out
    }

    /// Where the credits came from, by the game's own totals, biggest first and only what's above zero. Mission
    /// rewards, ship and module sales aren't among them: the game doesn't total those.
    pub fn earnings(&self) -> Vec<(Earning, i64)> {
        let g = |group, key| self.int(group, key).max(0);
        let mut out = vec![
            (Earning::Trade, g("Trading", "Market_Profits")),
            (Earning::Mining, g("Mining", "Mining_Profits")),
            (Earning::Exploration, g("Exploration", "Exploration_Profits")),
            (Earning::Exobiology, g("Exobiology", "Organic_Data_Profits")),
            (Earning::Bounties, g("Combat", "Bounty_Hunting_Profit")),
            (Earning::CombatBonds, g("Combat", "Combat_Bond_Profits") + g("Combat", "OnFoot_Combat_Bonds_Profits")),
            (Earning::Assassinations, g("Combat", "Assassination_Profits")),
            (Earning::BlackMarkets, g("Smuggling", "Black_Markets_Profits")),
            (Earning::SearchAndRescue, g("Search_And_Rescue", "SearchRescue_Profit")),
            (Earning::Multicrew, g("Multicrew", "Multicrew_Credits_Total")),
            (Earning::Cqc, g("CQC", "CQC_Credits_Earned")),
        ];
        out.retain(|(_, v)| *v > 0);
        out.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
        out
    }

    /// Where the credits went, by the game's own totals, biggest first and only what's above zero.
    pub fn spending(&self) -> Vec<(Expense, i64)> {
        let g = |group, key| self.int(group, key).max(0);
        let mut out = vec![
            (Expense::Ships, g("Bank_Account", "Spent_On_Ships")),
            (Expense::Outfitting, g("Bank_Account", "Spent_On_Outfitting")),
            (Expense::CrewWages, g("Crew", "NpcCrew_TotalWages")),
            (Expense::Rebuys, g("Bank_Account", "Spent_On_Insurance")),
            (Expense::Repairs, g("Bank_Account", "Spent_On_Repairs")),
            (Expense::Ammo, g("Bank_Account", "Spent_On_Ammo_Consumables")),
            (Expense::Fuel, g("Bank_Account", "Spent_On_Fuel")),
            (Expense::OnFoot, g("Bank_Account", "Spent_On_Suits") + g("Bank_Account", "Spent_On_Weapons") + g("Bank_Account", "Spent_On_Suit_Consumables")),
            (Expense::Engineering, g("Crafting", "Spent_On_Crafting")),
            (Expense::PremiumStock, g("Bank_Account", "Spent_On_Premium_Stock")),
            (Expense::Shuttles, g("Exploration", "Spent_On_Shuttles")),
        ];
        out.retain(|(_, v)| *v > 0);
        out.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
        out
    }

    /// Passengers delivered and ejected by kind, most delivered first: (kind, delivered, ejected), VIP and bulk together.
    pub fn passengers(&self) -> Vec<(&'static str, i64, i64)> {
        let mut by: Vec<(&'static str, i64, i64)> = Vec::new();
        if let Some(obj) = self.group("Passengers").as_object() {
            for (k, v) in obj {
                let (Some((kind, _, delivered)), Some(n)) = (passenger_key(k), number(v)) else { continue };
                let n = n.round() as i64;
                let i = match by.iter().position(|(k, ..)| *k == kind) {
                    Some(i) => i,
                    None => {
                        by.push((kind, 0, 0));
                        by.len() - 1
                    }
                };
                if delivered {
                    by[i].1 += n;
                } else {
                    by[i].2 += n;
                }
            }
        }
        by.retain(|(_, d, e)| *d > 0 || *e > 0);
        by.sort_by_key(|(k, d, e)| (std::cmp::Reverse(*d), std::cmp::Reverse(*e), *k));
        by
    }

    /// Engineering rolls by grade, 1 to 5.
    pub fn rolls_by_grade(&self) -> [i64; 5] {
        std::array::from_fn(|i| self.int("Crafting", &format!("Recipes_Generated_Rank_{}", i + 1)).max(0))
    }

    /// The game's own records (bests), those above zero.
    pub fn records(&self) -> Vec<(Record, f64)> {
        let mut out = vec![
            (Record::BestTrade, self.get("Trading", "Highest_Single_Transaction")),
            (Record::BestExplorationPayout, self.get("Exploration", "Highest_Payout")),
            (Record::BestCombatReward, self.get("Combat", "Highest_Single_Reward")),
            (Record::FarthestFromStart, self.get("Exploration", "Greatest_Distance_From_Start")),
            (Record::BestBlackMarketSale, self.get("Smuggling", "Highest_Single_Transaction")),
            (Record::HighestBounty, self.get("Crime", "Highest_Bounty")),
        ];
        out.retain(|(_, v)| *v > 0.0);
        out
    }

    /// The round numbers coming up on the career's main counters, nearest first (as a share of the way there).
    pub fn milestones(&self) -> Vec<Milestone> {
        let counters: [(&str, &str, &str, Unit); 11] = [
            ("Exploration", "Total_Hyperspace_Distance", "Light years jumped", Ly),
            ("Exploration", "Total_Hyperspace_Jumps", "Hyperspace jumps", Count),
            ("Exploration", "Systems_Visited", "Systems visited", Count),
            ("Exploration", "Time_Played", "Hours in the game", Seconds),
            ("Exploration", "Planets_Scanned_To_Level_3", "Planets mapped", Count),
            ("Trading", "Market_Profits", "Profit from trade", Credits),
            ("Mining", "Mining_Profits", "Profit from mining", Credits),
            ("Exobiology", "Organic_Data_Profits", "Earned from organic data", Credits),
            ("Combat", "Bounties_Claimed", "Bounties claimed", Count),
            ("Passengers", "Passengers_Missions_Delivered", "Passengers delivered", Count),
            ("Exploration", "Planet_Footfalls", "Planets walked on", Count),
        ];
        let mut out: Vec<Milestone> = counters
            .into_iter()
            .filter_map(|(group, key, label, unit)| {
                let value = self.get(group, key);
                // Time counts in hours: the next milestone is a round number of hours.
                let (value, scale) = if unit == Seconds { (value / 3_600.0, 3_600.0) } else { (value, 1.0) };
                (value >= 1.0).then(|| Milestone { label, unit, value: value * scale, target: next_round(value) * scale })
            })
            .collect();
        out.sort_by(|a, b| b.progress().total_cmp(&a.progress()));
        out
    }
}

/// Where credits came from, in the game's totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Earning {
    Trade,
    Mining,
    Exploration,
    Exobiology,
    Bounties,
    CombatBonds,
    Assassinations,
    BlackMarkets,
    SearchAndRescue,
    Multicrew,
    Cqc,
}

impl Earning {
    pub fn label(self) -> &'static str {
        match self {
            Earning::Trade => "Trade",
            Earning::Mining => "Mining",
            Earning::Exploration => "Exploration data",
            Earning::Exobiology => "Exobiology data",
            Earning::Bounties => "Bounties",
            Earning::CombatBonds => "Combat bonds",
            Earning::Assassinations => "Assassinations",
            Earning::BlackMarkets => "Black markets",
            Earning::SearchAndRescue => "Search and rescue",
            Earning::Multicrew => "Multicrew",
            Earning::Cqc => "CQC",
        }
    }
}

/// Where credits went, in the game's totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Expense {
    Ships,
    Outfitting,
    CrewWages,
    Rebuys,
    Repairs,
    Ammo,
    Fuel,
    OnFoot,
    Engineering,
    PremiumStock,
    Shuttles,
}

impl Expense {
    pub fn label(self) -> &'static str {
        match self {
            Expense::Ships => "Ships",
            Expense::Outfitting => "Outfitting",
            Expense::CrewWages => "Crew wages",
            Expense::Rebuys => "Rebuys",
            Expense::Repairs => "Repairs",
            Expense::Ammo => "Ammunition and restocking",
            Expense::Fuel => "Fuel",
            Expense::OnFoot => "Suits, weapons and consumables",
            Expense::Engineering => "Engineering",
            Expense::PremiumStock => "Premium stock",
            Expense::Shuttles => "Shuttles",
        }
    }
}

/// A record the game keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record {
    BestTrade,
    BestExplorationPayout,
    BestCombatReward,
    FarthestFromStart,
    BestBlackMarketSale,
    HighestBounty,
}

impl Record {
    pub fn label(self) -> &'static str {
        match self {
            Record::BestTrade => "Best single sale",
            Record::BestExplorationPayout => "Best exploration payout",
            Record::BestCombatReward => "Best combat reward",
            Record::FarthestFromStart => "Farthest from where you started",
            Record::BestBlackMarketSale => "Best black-market sale",
            Record::HighestBounty => "Highest bounty on your head",
        }
    }

    pub fn unit(self) -> Unit {
        if self == Record::FarthestFromStart { Ly } else { Credits }
    }
}

/// A round number coming up on a counter.
#[derive(Debug, Clone, PartialEq)]
pub struct Milestone {
    pub label: &'static str,
    pub unit: Unit,
    pub value: f64,
    pub target: f64,
}

impl Milestone {
    /// The share of the way there, 0 to 1.
    pub fn progress(&self) -> f64 {
        if self.target > 0.0 { (self.value / self.target).clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// The next round number above `v`: 1, 2.5 or 5 times a power of ten (10, 25, 50, 100, 250, …).
pub fn next_round(v: f64) -> f64 {
    if !(v.is_finite() && v >= 0.0) {
        return 1.0;
    }
    let mut power = 1.0;
    loop {
        for m in [1.0, 2.5, 5.0] {
            if m * power > v {
                return m * power;
            }
        }
        power *= 10.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commander's figures from a real `Statistics` line (trimmed).
    fn stats() -> Statistics {
        let line = r#"{ "Bank_Account":{ "Current_Wealth":33892018245, "Spent_On_Ships":666817817, "Spent_On_Outfitting":530827972, "Spent_On_Repairs":3450188, "Spent_On_Fuel":94123, "Spent_On_Ammo_Consumables":371112, "Insurance_Claims":17, "Spent_On_Insurance":52488926, "Owned_Ship_Count":17, "Spent_On_Suits":150000, "Spent_On_Weapons":125000, "Spent_On_Suit_Consumables":180000, "Spent_On_Premium_Stock":0 },
            "Combat":{ "Bounties_Claimed":17, "Bounty_Hunting_Profit":1602389, "Combat_Bonds":0, "Combat_Bond_Profits":0, "Assassinations":3, "Assassination_Profits":64161, "Highest_Single_Reward":696670, "Skimmers_Killed":2, "ConflictZone_High":0 },
            "Trading":{ "Markets_Traded_With":20, "Market_Profits":39329585226, "Resources_Traded":4158, "Average_Profit":497842850.96203, "Highest_Single_Transaction":1973790225 },
            "Mining":{ "Mining_Profits":0, "Quantity_Mined":0, "Materials_Collected":2383 },
            "Exploration":{ "Systems_Visited":893, "Exploration_Profits":35914875, "Highest_Payout":2651558, "Total_Hyperspace_Distance":46289, "Total_Hyperspace_Jumps":1419, "Greatest_Distance_From_Start":1169.4679759103, "Time_Played":1174500, "OnFoot_Distance_Travelled":14808 },
            "Passengers":{ "Passengers_Missions_Accepted":280, "Passengers_Missions_Tourist_vip_delivered":1108, "Passengers_Missions_Medical_vip_delivered":406, "Passengers_Missions_ScienceTeams_vip_delivered":135, "Passengers_Missions_ScienceTeams_vip_ejected":17, "Passengers_Missions_Soldiers_vip_ejected":6, "Passengers_Missions_Delivered":2239, "Passengers_Missions_Disgruntled":35 },
            "Crafting":{ "Recipes_Generated":71, "Recipes_Generated_Rank_1":25, "Recipes_Generated_Rank_2":18, "Recipes_Generated_Rank_3":12, "Recipes_Generated_Rank_4":11, "Recipes_Generated_Rank_5":5 },
            "Crew":{ "NpcCrew_TotalWages":797214774, "NpcCrew_Hired":2 },
            "Multicrew":{ "Multicrew_Time_Total":4968, "Multicrew_Credits_Total":570949 },
            "Exobiology":{ "Organic_Data_Profits":240523800, "First_Logged":11 },
            "TG_ENCOUNTERS":{ "TG_ENCOUNTER_TOTAL":2, "TG_ENCOUNTER_TOTAL_LAST_SYSTEM":"HIP 23716", "TG_ENCOUNTER_TOTAL_LAST_SHIP":"Anaconda" },
            "Squadron":{ "Squadron_Leaderboard_bgs_highestcontribution":0, "Squadron_Bank_Credits_Deposited":0 },
            "FLEETCARRIER":{ "FLEETCARRIER_TOTAL_JUMPS":5, "FLEETCARRIER_DISTANCE_TRAVELLED":1276.8346947207, "FLEETCARRIER_SOMETHING_NEW":3 } }"#;
        serde_json::from_str(line).expect("statistics")
    }

    #[test]
    fn every_figure_reads_in_plain_words() {
        let s = stats();
        let c = Career::new(&s);
        for group in GROUPS {
            for f in c.figures(group.key) {
                assert!(!f.label.contains('_') && !f.label.is_empty(), "{}: {}", group.key, f.label);
                assert!(f.label.chars().next().is_some_and(char::is_uppercase), "{}", f.label);
            }
        }
        // The table's own order: the headline figures come first.
        let trade: Vec<String> = c.figures("Trading").into_iter().map(|f| f.label).collect();
        assert_eq!(trade[0], "Profit from trade");
        // Passengers by kind, and keys the table doesn't know yet.
        assert_eq!(describe("Passengers", "Passengers_Missions_ScienceTeams_vip_ejected").0, "Science teams ejected (VIP)");
        assert_eq!(describe("Passengers", "Passengers_Missions_Refugee_bulk_delivered").0, "Refugees delivered (bulk)");
        assert_eq!(describe("FLEETCARRIER", "FLEETCARRIER_SOMETHING_NEW").0, "Something new");
        assert_eq!(describe("Squadron", "Squadron_Leaderboard_bgs_highestcontribution").0, "Best squadron leaderboard contribution: BGS");
        assert_eq!(describe("Combat", "OnFoot_Things_Done"), ("On foot things done".to_string(), Count));
        assert_eq!(describe("Combat", "OnFoot_Things_Profits").1, Credits);
        let carrier = c.figures("FLEETCARRIER");
        assert_eq!(carrier.last().map(|f| f.label.as_str()), Some("Something new"), "unknown keys follow the known ones");
        // Text stays text; numbers read in their units.
        let tg = c.figures("TG_ENCOUNTERS");
        assert!(tg.iter().any(|f| f.unit == Text && f.shown() == "HIP 23716"));
        let explo = c.figures("Exploration");
        let shown = |label: &str| explo.iter().find(|f| f.label == label).map(Figure::shown).unwrap_or_default();
        assert_eq!(shown("Time played"), "326 h 15 m");
        assert_eq!(shown("Distance jumped"), "46,289 ly");
        assert_eq!(shown("Farthest from where you started"), "1,169 ly");
        assert_eq!(shown("Distance on foot"), "14.8 km");
        assert_eq!(shown("Earned from exploration data"), "35.9 M Cr");
        assert!(c.figures("Mining").iter().any(|f| f.is_zero()), "zeros are known as such, for hiding");
        // A group the game didn't write has no figures.
        assert!(c.figures("CQC").is_empty());
    }

    #[test]
    fn where_the_credits_came_from_and_went() {
        let s = stats();
        let c = Career::new(&s);
        let earned = c.earnings();
        assert_eq!(earned.first(), Some(&(Earning::Trade, 39_329_585_226)));
        assert_eq!(earned.iter().map(|(e, _)| *e).collect::<Vec<_>>(), [Earning::Trade, Earning::Exobiology, Earning::Exploration, Earning::Bounties, Earning::Multicrew, Earning::Assassinations]);
        assert!(earned.iter().all(|(_, v)| *v > 0), "nothing at zero");
        let spent = c.spending();
        assert_eq!(spent.iter().map(|(e, _)| *e).take(4).collect::<Vec<_>>(), [Expense::CrewWages, Expense::Ships, Expense::Outfitting, Expense::Rebuys]);
        assert_eq!(spent.iter().find(|(e, _)| *e == Expense::OnFoot).map(|(_, v)| *v), Some(455_000), "suits, weapons and consumables together");
        assert!(!spent.iter().any(|(e, _)| *e == Expense::PremiumStock));
    }

    #[test]
    fn passengers_by_kind_records_and_grades() {
        let s = stats();
        let c = Career::new(&s);
        assert_eq!(c.passengers(), [("Tourists", 1108, 0), ("Medical teams", 406, 0), ("Science teams", 135, 17), ("Soldiers", 0, 6)]);
        assert_eq!(c.rolls_by_grade(), [25, 18, 12, 11, 5]);
        let records = c.records();
        assert_eq!(records.first(), Some(&(Record::BestTrade, 1_973_790_225.0)));
        assert!(records.iter().any(|(r, _)| *r == Record::FarthestFromStart));
        assert!(!records.iter().any(|(r, _)| *r == Record::HighestBounty), "no Crime figures: no record");
    }

    #[test]
    fn milestones_are_the_next_round_numbers() {
        assert_eq!(next_round(0.0), 1.0);
        assert_eq!(next_round(1.0), 2.5);
        assert_eq!(next_round(893.0), 1_000.0);
        assert_eq!(next_round(1_000.0), 2_500.0);
        assert_eq!(next_round(46_289.0), 50_000.0);
        assert_eq!(next_round(39_329_585_226.0), 50_000_000_000.0);
        let s = stats();
        let m = Career::new(&s).milestones();
        // 240.5 M of 250 M Cr, then 46,289 of 50,000 ly and 2,239 of 2,500 passengers; hours count as hours (326 of 500).
        assert_eq!(m.iter().take(3).map(|x| x.label).collect::<Vec<_>>(), ["Earned from organic data", "Light years jumped", "Passengers delivered"]);
        let hours = m.iter().find(|x| x.label == "Hours in the game").expect("time played");
        assert_eq!((hours.value / 3_600.0).floor(), 326.0);
        assert_eq!(hours.target, 500.0 * 3_600.0);
        assert!(m.windows(2).all(|w| w[0].progress() >= w[1].progress()));
        assert!(!m.iter().any(|x| x.label == "Profit from mining"), "a counter at zero has no milestone");
    }

    #[test]
    fn durations_read_in_hours_and_minutes() {
        assert_eq!(duration(50), "50 s");
        assert_eq!(duration(45 * 60), "45 m");
        assert_eq!(duration(1_174_500), "326 h 15 m");
        assert_eq!(duration(-5), "0 s");
        assert_eq!(show(0.88175, Ratio), "0.88");
        assert_eq!(show(820.0, Metres), "820 m");
        assert_eq!(show(12.34, Ly), "12.3 ly");
    }
}
