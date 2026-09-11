//! Local TOML configuration: player name, board style, time control, etc.
//! Stored at `<config_dir>/p2pawn/config.toml`.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::game::session::TimeControl;

/// Piece rendering style.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PieceStyle {
    /// ♔♕♖♗♘♙ / ♚♛♜♝♞♟
    Unicode,
    /// K Q R B N P / k q r b n p
    Ascii,
}

impl PieceStyle {
    pub fn label(self) -> &'static str {
        match self {
            PieceStyle::Unicode => "Unicode (♞)",
            PieceStyle::Ascii => "ASCII (N)",
        }
    }
}

/// Show hints for legal destinations of the selected piece.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HintStyle {
    /// Show all legal destinations (◎ / highlight).
    All,
    /// Only highlight the cursor target when it is a legal move.
    Cursor,
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub player_name: String,
    pub piece_style: PieceStyle,
    pub show_coordinates: bool,
    pub show_hints: HintStyle,
    /// Default time control (base seconds, increment seconds).
    pub tc_base_secs: u64,
    pub tc_inc_secs: u64,
    /// Flip the board so black is at the bottom.
    pub flip_board: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            player_name: default_name(),
            piece_style: PieceStyle::Unicode,
            show_coordinates: true,
            show_hints: HintStyle::All,
            tc_base_secs: TimeControl::BASE_5_0.base.as_secs(),
            tc_inc_secs: TimeControl::BASE_5_0.increment.as_secs(),
            flip_board: false,
        }
    }
}

fn default_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .map(|s| s.chars().take(20).collect())
        .unwrap_or_else(|_| "Player".into())
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("p2pawn").join("config.toml"))
    }

    /// One-time migration from the pre-rename `lanchess` directory, so early
    /// adopters keep their config and saved games.
    fn migrate_old_dir() {
        let (Some(old), Some(new)) = (
            dirs::config_dir().map(|d| d.join("lanchess")),
            dirs::config_dir().map(|d| d.join("p2pawn")),
        ) else {
            return;
        };
        if old.is_dir() && !new.exists() {
            let _ = fs::rename(&old, &new);
        }
    }

    /// Load the config, writing a default file on first run.
    ///
    /// The `P2PAWN_NAME` environment variable overrides the player name
    /// without touching the file on disk (handy for running several
    /// instances on one machine).
    pub fn load() -> Config {
        Self::migrate_old_dir();
        let Some(path) = Config::path() else {
            return Config::default();
        };
        let mut cfg = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|_| Config::default()),
            Err(_) => {
                let cfg = Config::default();
                let _ = cfg.save();
                cfg
            }
        };
        if let Ok(name) = std::env::var("P2PAWN_NAME") {
            let name: String = name.chars().filter(|c| !c.is_control()).take(20).collect();
            if !name.is_empty() {
                cfg.player_name = name;
            }
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Config::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        fs::write(&path, text)
    }

    pub fn default_tc(&self) -> TimeControl {
        TimeControl::new(self.tc_base_secs, self.tc_inc_secs)
    }

    pub fn toggle_piece_style(&mut self) {
        self.piece_style = match self.piece_style {
            PieceStyle::Unicode => PieceStyle::Ascii,
            PieceStyle::Ascii => PieceStyle::Unicode,
        };
    }

    pub fn toggle_hints(&mut self) {
        self.show_hints = match self.show_hints {
            HintStyle::All => HintStyle::Cursor,
            HintStyle::Cursor => HintStyle::None,
            HintStyle::None => HintStyle::All,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cfg = Config {
            player_name: "tester".into(),
            piece_style: PieceStyle::Ascii,
            show_coordinates: false,
            show_hints: HintStyle::None,
            tc_base_secs: 180,
            tc_inc_secs: 2,
            flip_board: true,
        };
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.player_name, "tester");
        assert_eq!(back.piece_style, PieceStyle::Ascii);
        assert_eq!(back.show_hints, HintStyle::None);
        assert_eq!(back.default_tc(), TimeControl::new(180, 2));
    }

    #[test]
    fn unknown_fields_fall_back_to_defaults() {
        let s = "player_name = \"x\"\nunknown_field = 3\n";
        let cfg: Config = toml::from_str(s).unwrap();
        assert_eq!(cfg.player_name, "x");
        assert_eq!(cfg.piece_style, PieceStyle::Unicode);
    }
}
