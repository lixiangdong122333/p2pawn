//! Local game history: finished games are stored as .pgn files under the
//! platform config directory, one file per game. No database.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::pgn::{self, ParsedPgn};
use super::session::Session;

pub struct History {
    dir: PathBuf,
}

/// One entry of the history list.
pub struct HistoryEntry {
    pub file: PathBuf,
    pub parsed: ParsedPgn,
    /// Short file name, e.g. "20260910-142233-a1b2".
    pub stem: String,
}

impl History {
    /// Use `<config_dir>/p2pawn/games`.
    pub fn default_dir() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("p2pawn").join("games"))
    }

    pub fn new(dir: PathBuf) -> History {
        History { dir }
    }

    pub fn open_default() -> Option<History> {
        History::default_dir().map(History::new)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Save a finished session as PGN. Returns the file path.
    pub fn save(&self, session: &Session, started_iso: &str) -> std::io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let stem = format!(
            "{}-{:x}",
            started_iso
                .split('T')
                .next()
                .unwrap_or("unknown")
                .replace('-', ""),
            stamp & 0xffff_ffff
        );
        let path = self.dir.join(format!("{stem}.pgn"));
        let text = pgn::session_to_pgn(session, started_iso);
        fs::write(&path, text)?;
        Ok(path)
    }

    /// All saved games, newest first.
    pub fn list(&self) -> Vec<HistoryEntry> {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<HistoryEntry> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pgn"))
            .filter_map(|path| {
                let stem = path.file_stem()?.to_string_lossy().to_string();
                let text = fs::read_to_string(&path).ok()?;
                Some(HistoryEntry {
                    file: path,
                    parsed: pgn::parse_pgn(&text),
                    stem,
                })
            })
            .collect();
        // File names start with yyyymmdd-hhmmss-ish data, so a plain sort
        // descending gives newest first.
        out.sort_by(|a, b| b.stem.cmp(&a.stem));
        out
    }

    /// Load one entry for replay.
    pub fn load(&self, entry: &HistoryEntry) -> Option<pgn::Replay> {
        let text = fs::read_to_string(&entry.file).ok()?;
        Some(pgn::Replay::new(pgn::parse_pgn(&text).sans))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::session::{Session, TimeControl};
    use chess::Color;

    #[test]
    fn save_and_list_roundtrip() {
        let dir = std::env::temp_dir().join(format!("p2pawn-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let history = History::new(dir.clone());

        let mut s = Session::new("Alice".into(), "Bob".into(), None, TimeControl::BASE_1_0);
        let parse = |x: &str| {
            let b = x.as_bytes();
            chess::Square::make_square(
                chess::Rank::from_index((b[1] - b'1') as usize),
                chess::File::from_index((b[0] - b'a') as usize),
            )
        };
        s.try_move(parse("e2"), parse("e4"), None).unwrap();
        s.try_move(parse("e7"), parse("e5"), None).unwrap();
        s.resign(Color::Black);

        let path = history.save(&s, "2026-09-10T09:00:00").unwrap();
        assert!(path.exists());

        let list = history.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].parsed.white, "Alice");
        assert_eq!(list[0].parsed.black, "Bob");
        assert_eq!(list[0].parsed.result, "1-0");
        assert_eq!(list[0].parsed.sans, vec!["e4", "e5"]);

        let replay = history.load(&list[0]).unwrap();
        assert_eq!(replay.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
