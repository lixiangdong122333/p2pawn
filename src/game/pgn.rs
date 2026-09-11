//! PGN generation and parsing, plus move-by-move replay.
//!
//! We generate standard PGN with the usual Seven Tag Roster plus a few
//! informational tags. Parsing is deliberately minimal: headers as `Key "Value"`
//! lines, then a movetext of SAN tokens with `;`/`{}` comments stripped. This
//! is enough for files produced by this tool and typical hand-written PGNs.

use chess::{Board, ChessMove, Game};

use super::session::Session;

/// Generate a complete PGN string for a finished (or ongoing) session.
pub fn session_to_pgn(session: &Session, started_iso: &str) -> String {
    let mut pgn = String::new();
    pgn.push_str(&format!("[Event \"p2pawn LAN game\"]\n"));
    pgn.push_str(&format!("[Site \"LAN\"]\n"));
    pgn.push_str(&format!("[Date \"{}\"]\n", iso_to_pgn_date(started_iso)));
    pgn.push_str("[Round \"-\"]\n");
    pgn.push_str(&format!(
        "[White \"{}\"]\n",
        escape_tag(&session.white_name)
    ));
    pgn.push_str(&format!("[Black \"{}\"]\n", escape_tag(&session.black_name)));
    pgn.push_str(&format!("[Result \"{}\"]\n", session.result_string()));
    if let Some(end) = &session.end {
        pgn.push_str(&format!("[Termination \"{}\"]\n", end.termination));
    }
    pgn.push_str(&format!(
        "[TimeControl \"{}\"]\n",
        tc_to_pgn(session.tc.base.as_secs(), session.tc.increment.as_secs())
    ));
    pgn.push('\n');

    // Movetext, wrapped at ~80 columns.
    let mut line = String::new();
    for (i, rec) in session.moves.iter().enumerate() {
        let token = if i % 2 == 0 {
            format!("{}. {}", i / 2 + 1, rec.san)
        } else {
            rec.san.clone()
        };
        if line.len() + token.len() + 1 > 78 {
            pgn.push_str(line.trim_end());
            pgn.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&token);
    }
    if !line.is_empty() {
        pgn.push_str(line.trim_end());
        pgn.push('\n');
    }
    pgn.push_str(session.result_string());
    pgn.push('\n');
    pgn
}

/// A parsed PGN game.
#[derive(Clone, Debug, Default)]
pub struct ParsedPgn {
    pub white: String,
    pub black: String,
    pub result: String,
    pub event: String,
    pub date: String,
    pub termination: String,
    /// SAN tokens in order.
    pub sans: Vec<String>,
}

pub fn parse_pgn(text: &str) -> ParsedPgn {
    let mut out = ParsedPgn {
        white: "White".into(),
        black: "Black".into(),
        ..ParsedPgn::default()
    };
    let mut movetext = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('[') {
            if let Some(end) = rest.strip_suffix(']') {
                if let Some((k, v)) = end.split_once(' ') {
                    let v = v.trim().trim_matches('"').to_string();
                    match k.trim() {
                        "White" => out.white = v,
                        "Black" => out.black = v,
                        "Result" => out.result = v,
                        "Event" => out.event = v,
                        "Date" => out.date = v,
                        "Termination" => out.termination = v,
                        _ => {}
                    }
                }
            }
        } else if !line.is_empty() {
            movetext.push_str(line);
            movetext.push(' ');
        }
    }
    out.sans = tokenize_movetext(&movetext);
    out
}

/// Strip comments, NAGs and the trailing result, return SAN tokens.
fn tokenize_movetext(text: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_brace = false;
    while let Some(c) = chars.next() {
        match c {
            '{' => in_brace = true,
            '}' => in_brace = false,
            ';' => {
                // Rest-of-line comment.
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        break;
                    }
                }
            }
            _ if in_brace => {}
            _ => cleaned.push(c),
        }
    }
    cleaned
        .split_whitespace()
        .filter(|tok| {
            // Drop move numbers ("1.", "1...") and the result marker.
            if tok.ends_with('.') || *tok == "1-0" || *tok == "0-1" || *tok == "1/2-1/2" || *tok == "*" {
                return false;
            }
            if tok.starts_with('$') {
                return false;
            }
            true
        })
        .map(|t| t.to_string())
        .collect()
}

/// Replay state over a list of SAN moves.
pub struct Replay {
    game: Game,
    board: Board,
    sans: Vec<String>,
    /// Number of moves applied (0 = start position).
    pub idx: usize,
}

impl Replay {
    pub fn new(sans: Vec<String>) -> Replay {
        Replay {
            game: Game::new(),
            board: Board::default(),
            sans,
            idx: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.sans.len()
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    /// SAN of the i-th move (already played or not).
    pub fn san_at(&self, i: usize) -> Option<&str> {
        self.sans.get(i).map(|s| s.as_str())
    }

    pub fn at_end(&self) -> bool {
        self.idx >= self.sans.len()
    }

    /// Apply the next move. Returns false at the end.
    pub fn next(&mut self) -> bool {
        if self.at_end() {
            return false;
        }
        match ChessMove::from_san(&self.board, &self.sans[self.idx]) {
            Ok(mv) => {
                self.game.make_move(mv);
                self.board = self.game.current_position();
                self.idx += 1;
                true
            }
            Err(_) => false, // stop on the first bad token
        }
    }

    /// Restart and re-apply `n` moves.
    pub fn seek(&mut self, n: usize) {
        self.game = Game::new();
        self.board = Board::default();
        self.idx = 0;
        for _ in 0..n {
            if !self.next() {
                break;
            }
        }
    }

    pub fn prev(&mut self) -> bool {
        if self.idx == 0 {
            return false;
        }
        self.seek(self.idx - 1);
        true
    }

    pub fn first(&mut self) {
        self.seek(0);
    }

    pub fn last(&mut self) {
        self.seek(self.sans.len());
    }
}

fn escape_tag(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn iso_to_pgn_date(iso: &str) -> String {
    // "2026-09-10T15:04:05" -> "2026.09.10"
    iso.split('T').next().unwrap_or("????.??.??").replace('-', ".")
}

fn tc_to_pgn(base_secs: u64, inc_secs: u64) -> String {
    if inc_secs == 0 {
        base_secs.to_string()
    } else {
        format!("{base_secs}+{inc_secs}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[Event "p2pawn LAN game"]
[Site "LAN"]
[Date "2026.09.10"]
[Round "-"]
[White "Alice"]
[Black "Bob"]
[Result "0-1"]
[Termination "Normal"]

1. f3 {oops} e5 2. g4 Qh4# 0-1
"#;

    #[test]
    fn parse_and_replay_sample() {
        let pgn = parse_pgn(SAMPLE);
        assert_eq!(pgn.white, "Alice");
        assert_eq!(pgn.black, "Bob");
        assert_eq!(pgn.result, "0-1");
        assert_eq!(pgn.sans, vec!["f3", "e5", "g4", "Qh4#"]);

        let mut r = Replay::new(pgn.sans.clone());
        assert!(r.next());
        assert!(r.next());
        assert!(r.next());
        assert!(r.next());
        assert!(!r.next());
        assert_eq!(r.board().status(), chess::BoardStatus::Checkmate);
        // Compare the piece placement and side to move; the chess crate's
        // halfmove/fullmove counters in Board's FEN are not game-aware.
        let fen = r.board().to_string();
        let fields: Vec<&str> = fen.split(' ').collect();
        assert_eq!(fields[..3].join(" "), "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq");
        assert!(r.prev());
        assert!(r.prev());
        r.first();
        assert_eq!(r.board(), &Board::default());
        r.last();
        assert!(r.at_end());
    }

    #[test]
    fn generated_pgn_roundtrip() {
        use chess::{ChessMove, Square};
        use crate::game::session::{Session, TimeControl};

        let mut s = Session::new("Alice".into(), "Bob".into(), None, TimeControl::BASE_3_2);
        let mv = ChessMove::new(Square::E2, Square::E4, None);
        s.try_move(mv.get_source(), mv.get_dest(), None).unwrap();
        let pgn = session_to_pgn(&s, "2026-09-10T09:00:00");
        assert!(pgn.contains("[White \"Alice\"]"));
        assert!(pgn.contains("[Date \"2026.09.10\"]"));
        assert!(pgn.contains("[TimeControl \"180+2\"]"));
        assert!(pgn.contains("1. e4"));

        let parsed = parse_pgn(&pgn);
        assert_eq!(parsed.sans, vec!["e4"]);
        let mut r = Replay::new(parsed.sans);
        assert!(r.next());
        let fen = r.board().to_string();
        let fields: Vec<&str> = fen.split(' ').collect();
        // Placement + side to move + castling rights (en-passant and move
        // counters in the chess crate's Board FEN are not game-aware).
        assert_eq!(fields[..3].join(" "), "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq");
    }
}
