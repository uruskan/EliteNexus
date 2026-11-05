//! Saved builds: plans kept on this computer (`builds.json` in Elite Nexus's data folder) to open again, rename,
//! delete or compare with. Each is the build in the game's own Loadout format, with the engineering planned for it.

use nexus_data::fitting::{Goal, Plan};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// One saved build.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedBuild {
    pub name: String,
    /// When it was saved, unix seconds.
    pub saved: i64,
    /// The build as the game writes it (with the engineering the game reported).
    pub loadout: Value,
    /// The engineering planned for it, by slot ("Armour" for the bulkheads).
    pub goals: Vec<SavedGoal>,
    /// The commander's ship it was planned from (its ShipID), to compare with again when it's opened.
    pub ship: Option<u64>,
    /// Fields this version doesn't know (a later version's), written back as they were.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedGoal {
    pub slot: String,
    pub blueprint: String,
    pub grade: u8,
    pub roll: f64,
    pub experimental: Option<String>,
    /// Figures typed in exactly, by coriolis-data's name.
    pub exact: Vec<SavedFigure>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedFigure {
    pub key: String,
    pub value: f64,
}

impl SavedBuild {
    /// `ship`: the commander's ship the plan started from, if it did.
    pub fn from_plan(name: &str, plan: &Plan, ship: Option<u64>) -> SavedBuild {
        let goal = |slot: &str, g: &Goal| SavedGoal {
            slot: slot.to_string(),
            blueprint: g.blueprint.clone(),
            grade: g.grade,
            roll: g.roll,
            experimental: g.experimental.clone(),
            exact: g.exact.iter().map(|(key, value)| SavedFigure { key: key.clone(), value: *value }).collect(),
            extra: serde_json::Map::new(),
        };
        let goals = plan.bulkheads_goal.iter().map(|g| goal("Armour", g)).chain(plan.fitted.iter().filter_map(|f| f.goal.as_ref().map(|g| goal(&f.slot, g)))).collect();
        SavedBuild { name: name.trim().to_string(), saved: nexus_journal::Timestamp::now().0, loadout: plan.to_loadout(), goals, ship, extra: serde_json::Map::new() }
    }

    /// The plan again; None when the build can't be read.
    pub fn to_plan(&self) -> Option<Plan> {
        let mut plan = Plan::from_loadout(&self.loadout)?;
        for g in &self.goals {
            let goal = Goal {
                blueprint: g.blueprint.clone(),
                grade: g.grade.clamp(1, 5),
                roll: if g.roll.is_finite() { g.roll.clamp(0.0, 1.0) } else { 1.0 },
                experimental: g.experimental.clone(),
                exact: g.exact.iter().filter(|x| x.value.is_finite() && !x.key.is_empty()).map(|x| (x.key.clone(), x.value)).collect(),
            };
            if g.slot.eq_ignore_ascii_case("Armour") {
                plan.bulkheads_goal = Some(goal);
            } else if let Some(f) = plan.fitted.iter_mut().find(|f| f.slot.eq_ignore_ascii_case(&g.slot)) {
                f.goal = Some(goal);
            }
        }
        Some(plan)
    }

    /// The hull's name.
    pub fn hull(&self) -> String {
        nexus_data::ships::ship_name(self.loadout.get("Ship").and_then(Value::as_str).unwrap_or_default())
    }
}

/// The saved builds and where they live.
#[derive(Default)]
pub struct Builds {
    /// The file, once it has been read (or found not to exist yet); None: not read, and nothing is written.
    path: Option<PathBuf>,
    pub list: Vec<SavedBuild>,
    /// Entries this version can't read (a hand edit, a later version's), written back as they are.
    kept: Vec<Value>,
    /// Why the file couldn't be read or written: an error, to say on the page.
    pub problem: Option<String>,
    /// What to know about the file that isn't an error: entries kept as they are, a copy kept aside.
    pub notice: Option<String>,
    /// After a failed read, when to try again (not every frame).
    retry_at: Option<std::time::Instant>,
}

/// The file's entries: the builds, and the entries that aren't one (kept as they are). A byte order mark (an editor's)
/// is passed over.
fn parse(bytes: &[u8]) -> Result<(Vec<SavedBuild>, Vec<Value>), String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let entries: Vec<Value> = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let (mut list, mut kept) = (Vec::new(), Vec::new());
    for v in entries {
        match serde_json::from_value::<SavedBuild>(v.clone()) {
            Ok(b) if !b.name.trim().is_empty() => list.push(b),
            _ => kept.push(v),
        }
    }
    Ok((list, kept))
}

impl Builds {
    /// Reads the saved builds the first time they're needed, and again after the file couldn't be read.
    pub fn ensure_loaded(&mut self) {
        if self.path.is_none() && self.retry_at.is_none_or(|t| std::time::Instant::now() >= t) {
            self.load(crate::settings::data_dir().join("builds.json"));
        }
    }

    fn load(&mut self, path: PathBuf) {
        self.notice = None;
        match std::fs::read(&path) {
            Ok(bytes) => match parse(&bytes) {
                Ok((list, kept)) => {
                    self.problem = None;
                    self.notice = (!kept.is_empty()).then(|| format!("{} entries in the saved builds file can't be read here; they're kept in it as they are.", kept.len()));
                    (self.list, self.kept) = (list, kept);
                }
                Err(e) => {
                    // Not a list at all: kept aside under a name of its own (once: the same file isn't copied again),
                    // before saving starts a new one.
                    tracing::warn!("builds.json not read: {e}");
                    let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let same = std::fs::read_dir(path.parent().unwrap_or(std::path::Path::new(".")))
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .find(|p| name(p).starts_with("builds.unreadable-") && std::fs::read(p).is_ok_and(|b| b == bytes));
                    let aside = path.with_file_name(format!("builds.unreadable-{}.json", nexus_journal::Timestamp::now().0));
                    let kept = match same {
                        Some(p) => Some(p),
                        None => std::fs::copy(&path, &aside).ok().map(|_| aside),
                    };
                    self.problem = None;
                    self.notice = Some(match kept {
                        Some(p) => format!("The saved builds file can't be read: it's kept as {}, and saving a build starts a new one.", name(&p)),
                        None => "The saved builds file can't be read, nor kept aside: saving a build replaces it.".into(),
                    });
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.problem = None,
            Err(e) => {
                // Locked or unreachable for now: nothing is written over it, and it's read again next time.
                tracing::warn!("builds.json not read: {e}");
                self.problem = Some(format!("The saved builds file can't be read ({e}), so builds can't be saved until it can."));
                self.retry_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
                return;
            }
        }
        self.retry_at = None;
        self.path = Some(path);
    }

    /// Saves a build under its name, replacing one of the same name.
    pub fn save(&mut self, build: SavedBuild) -> Result<(), String> {
        self.ensure_loaded();
        let mut list = self.list.clone();
        match list.iter_mut().find(|b| b.name.eq_ignore_ascii_case(&build.name)) {
            Some(b) => *b = build,
            None => list.push(build),
        }
        self.commit(list)
    }

    /// Renames the build at `index`; not to nothing, nor to another build's name.
    pub fn rename(&mut self, index: usize, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A build needs a name.".into());
        }
        if self.list.iter().enumerate().any(|(i, b)| i != index && b.name.eq_ignore_ascii_case(name)) {
            return Err(format!("You have a saved build called \u{201c}{name}\u{201d} already."));
        }
        let mut list = self.list.clone();
        match list.get_mut(index) {
            Some(b) => b.name = name.to_string(),
            None => return Ok(()),
        }
        self.commit(list)
    }

    pub fn delete(&mut self, index: usize) -> Result<(), String> {
        if index >= self.list.len() {
            return Ok(());
        }
        let mut list = self.list.clone();
        list.remove(index);
        self.commit(list)
    }

    pub fn find(&self, name: &str) -> Option<&SavedBuild> {
        self.list.iter().find(|b| b.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Writes `list` and, once it's on disk, keeps it; a failed write changes nothing and says why.
    fn commit(&mut self, list: Vec<SavedBuild>) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            return Err(self.problem.clone().unwrap_or_else(|| "The saved builds file hasn't been read.".into()));
        };
        match write_file(&path, &list, &self.kept) {
            Ok(()) => {
                self.list = list;
                self.problem = None;
                Ok(())
            }
            Err(e) => {
                tracing::warn!("builds.json not saved: {e}");
                let why = format!("The saved builds couldn't be written: {e}");
                self.problem = Some(why.clone());
                Err(why)
            }
        }
    }
}

/// The builds and the kept entries, written whole through a temporary file flushed to the disk before it takes the
/// file's place, so neither a crash nor a power cut leaves half of it.
fn write_file(path: &std::path::Path, list: &[SavedBuild], kept: &[Value]) -> std::io::Result<()> {
    use std::io::Write;
    let mut entries: Vec<Value> = list.iter().map(|b| serde_json::to_value(b).map_err(std::io::Error::other)).collect::<Result<_, _>>()?;
    entries.extend(kept.iter().cloned());
    let bytes = serde_json::to_vec_pretty(&entries).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_data::fitting::{self, Goal};

    #[test]
    fn a_saved_build_comes_back_whole() {
        let hull = fitting::hull("python").expect("Python");
        let mut plan = Plan::stock(hull);
        plan.set("FrameShiftDrive", Some("int_hyperdrive_size5_class5"));
        if let Some(f) = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive") {
            f.goal = Some(Goal { roll: 0.5, experimental: Some("special_fsd_heavy".into()), exact: vec![("optmass".into(), 1700.0)], ..Goal::new("FSD_LongRange", 5) });
        }
        plan.bulkheads_goal = Some(Goal::new("Armour_HeavyDuty", 3));
        let saved = SavedBuild::from_plan(" Explorer ", &plan, Some(7));
        assert_eq!((saved.name.as_str(), saved.ship), ("Explorer", Some(7)));
        let text = serde_json::to_string(&vec![saved.clone()]).expect("json");
        let back: Vec<SavedBuild> = serde_json::from_str(&text).expect("read");
        assert_eq!(back[0].to_plan().expect("plan"), plan);
        assert_eq!(saved.hull(), "Python");
        // A file from a later version with more fields, or fewer, still reads.
        let odd: Vec<SavedBuild> = serde_json::from_str(r#"[{"name":"x","loadout":{"Ship":"python","Modules":[]},"later":1}]"#).expect("read");
        assert!(odd[0].goals.is_empty());
    }

    /// The file is the commander's: an entry this version can't read is kept as it is, a file that isn't a list is
    /// kept aside, and a write that fails changes nothing.
    #[test]
    fn the_builds_file_loses_nothing() {
        let dir = std::env::temp_dir().join(format!("nexus-builds-test-{}-{}", std::process::id(), nexus_journal::Timestamp::now().0));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("builds.json");
        // An editor's byte order mark, a field from a later version, and two entries this version can't read.
        std::fs::write(&path, "\u{feff}[{\"name\":\"Good\",\"loadout\":{\"Ship\":\"python\",\"Modules\":[]},\"later\":{\"x\":1}},{\"name\":\"Bad\",\"grade\":\"six\",\"goals\":7},{\"name\":\"\"}]").expect("write");
        let mut builds = Builds::default();
        builds.load(path.clone());
        assert_eq!((builds.list.len(), builds.kept.len()), (1, 2));
        assert!(builds.problem.is_none() && builds.notice.as_deref().is_some_and(|p| p.contains("2 entries")), "{:?}", builds.notice);
        let plan = Plan::stock(fitting::hull("anaconda").expect("Anaconda"));
        builds.save(SavedBuild::from_plan("New", &plan, None)).expect("saved");
        let on_disk: Vec<Value> = serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        assert_eq!(on_disk.len(), 4, "two builds and the two kept entries");
        assert!(on_disk.iter().any(|v| v["grade"] == "six"));
        assert!(on_disk.iter().any(|v| v["name"] == "Good" && v["later"]["x"] == 1), "a later version's field stays");
        // Renaming onto a name taken, or to nothing, is refused.
        assert!(builds.rename(1, "good").is_err() && builds.rename(1, "  ").is_err());
        // Not a list at all: kept aside under a name of its own, once, and saving starts afresh.
        std::fs::write(&path, "{ not json").expect("write");
        let asides = || std::fs::read_dir(&dir).expect("dir").filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().starts_with("builds.unreadable-")).count();
        let mut broken = Builds::default();
        broken.load(path.clone());
        assert!(broken.list.is_empty() && broken.notice.is_some() && broken.problem.is_none());
        assert_eq!(asides(), 1, "the unreadable file is kept");
        Builds::default().load(path.clone());
        assert_eq!(asides(), 1, "and not copied again");
        // Without a file read, nothing is written.
        let mut unread = Builds { problem: Some("locked".into()), ..Builds::default() };
        assert!(unread.commit(Vec::new()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
