//! A persisted, per-server channel→genre table — built to fix a real
//! instability the "match whatever's airing right now against XMLTV
//! category keywords" heuristic had on its own (the same frustration the
//! user reported experiencing with the genre filter in the official
//! Channels apps too): a channel's genre-filter membership could silently
//! flip from one guide refresh to the next depending purely on what
//! happened to be airing at that exact moment, rather than staying put.
//!
//! The fix: assign a channel a genre *once* — the first time its
//! currently-airing program clearly matches one of the five genre
//! keywords — then leave it alone. An empty slot still gets filled in on a
//! later update if nothing matched yet, but a filled slot is never
//! silently overwritten. `ui/live.rs`'s category filter reads from this
//! table, not from the moment-to-moment "what's on now" heuristic
//! directly — that heuristic now only ever runs once per channel, to seed
//! the table, not every frame.
//!
//! Keyed by channel *number*, not `id` — `ui/live.rs`'s own dedup and
//! source-filter logic already treats `number` as a grid row's stable
//! identity regardless of which underlying per-source `Channel` row wins
//! dedup on a given day, so this table follows the same convention. Scoped
//! per server (`AppSettings.active_server_id`) since two different DVR
//! servers can easily reuse the same channel numbers for entirely
//! different channels.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::api::guide::GuideProgram;
use crate::api::types::Channel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Genre {
    Movies,
    Sports,
    Drama,
    News,
    Kids,
}

impl Genre {
    /// The same keyword set the old per-frame heuristic used — this is now
    /// the only place it's applied, and only once per channel rather than
    /// continuously.
    const ALL_WITH_KEYWORDS: [(Genre, &'static str); 5] = [
        (Genre::Movies, "movie"),
        (Genre::Sports, "sport"),
        (Genre::Drama, "drama"),
        (Genre::News, "news"),
        (Genre::Kids, "child"),
    ];
}

fn classify(categories: &[String]) -> Option<Genre> {
    Genre::ALL_WITH_KEYWORDS
        .into_iter()
        .find(|(_, keyword)| {
            categories
                .iter()
                .any(|c| c.to_lowercase().contains(keyword))
        })
        .map(|(genre, _)| genre)
}

pub struct ChannelGenres {
    path: Option<PathBuf>,
    // server_id -> channel_number -> genre
    by_server: HashMap<String, HashMap<String, Genre>>,
}

impl ChannelGenres {
    pub fn load(path: Option<PathBuf>) -> Self {
        let by_server = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, by_server }
    }

    pub fn get(&self, server_id: &str, channel_number: &str) -> Option<Genre> {
        self.by_server.get(server_id)?.get(channel_number).copied()
    }

    /// Fills in any channel that doesn't have a genre yet, from whatever's
    /// airing right now — never touches one that already has one. Returns
    /// whether anything changed, so the caller only writes to disk when it
    /// actually needs to.
    pub fn seed_missing(
        &mut self,
        server_id: &str,
        channels: &[Channel],
        guide: &[GuideProgram],
        now: i64,
    ) -> bool {
        let current_by_channel: HashMap<&str, &GuideProgram> = guide
            .iter()
            .filter(|p| p.start <= now && now < p.stop)
            .map(|p| (p.channel.as_str(), p))
            .collect();

        let map = self.by_server.entry(server_id.to_string()).or_default();
        let mut changed = false;
        for c in channels {
            if map.contains_key(&c.number) {
                continue;
            }
            let Some(prog) = current_by_channel.get(c.number.as_str()) else {
                continue;
            };
            if let Some(genre) = classify(&prog.categories) {
                map.insert(c.number.clone(), genre);
                changed = true;
            }
        }
        changed
    }

    /// Clears every server's assignments — the deliberate escape hatch for
    /// a channel that got locked into the wrong genre by an unusual
    /// currently-airing program the first time it was ever seen (e.g. a
    /// news channel airing a one-off movie special the moment its guide
    /// data first loaded). No per-channel edit UI exists yet — this is the
    /// blunt instrument until that's asked for.
    pub fn reset(&mut self) {
        self.by_server.clear();
    }

    /// Atomic write-temp-then-rename, matching `AppSettings::save()`'s own
    /// discipline — a crash mid-write should never leave an unparseable
    /// file behind.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Err(std::io::Error::other("no data directory available"));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.by_server)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, path)
    }
}
