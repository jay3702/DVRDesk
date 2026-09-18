//! The rolling JSON history store — one flat file, atomic write-temp-then-
//! rename (same discipline `native`'s `AppSettings::save()` already uses),
//! keyed in memory by (channel, start) so repeated polls of a still-current
//! slot merge/overwrite instead of accumulating duplicates.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::guide_program::GuideProgram;

pub struct Store {
    path: PathBuf,
    programs: Mutex<HashMap<(String, i64), GuideProgram>>,
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let programs = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<GuideProgram>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|p| ((p.channel.clone(), p.start), p))
            .collect();
        Self { path, programs: Mutex::new(programs) }
    }

    pub fn merge(&self, new_programs: Vec<GuideProgram>) {
        let mut programs = self.programs.lock().unwrap();
        for p in new_programs {
            programs.insert((p.channel.clone(), p.start), p);
        }
    }

    pub fn prune(&self, cutoff: i64) {
        let mut programs = self.programs.lock().unwrap();
        programs.retain(|_, p| p.stop >= cutoff);
    }

    pub fn save(&self) -> std::io::Result<()> {
        let list: Vec<GuideProgram> = self.programs.lock().unwrap().values().cloned().collect();
        let json = serde_json::to_string(&list).map_err(std::io::Error::other)?;

        let tmp_path = self.path.with_extension("json.tmp");
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &self.path)
    }

    /// Any slot that overlaps `[from, to)` at all — not just ones fully
    /// contained in it, so a program spanning the boundary isn't dropped.
    pub fn query(&self, from: i64, to: i64) -> Vec<GuideProgram> {
        self.programs
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.stop > from && p.start < to)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.programs.lock().unwrap().len()
    }
}
