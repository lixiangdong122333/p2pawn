//! Shared game session state, used by both local and LAN games.
//!
//! All chess rules (legality, check, mate, stalemate, repetition, 50-move)
//! come from the `chess` crate's `Game`/`Board`. This layer adds the clock,
//! player names, draw offers and end-of-game bookkeeping.

use std::time::Duration;

use chess::{Board, ChessMove, Color, Game, MoveGen, Piece, Square};

use super::clock::Clock;
use super::san;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeControl {
    pub base: Duration,
    pub increment: Duration,
}

impl TimeControl {
    pub const BASE_1_0: TimeControl = TimeControl::new(60, 0);
    pub const BASE_3_2: TimeControl = TimeControl::new(180, 2);
    pub const BASE_5_0: TimeControl = TimeControl::new(300, 0);
    pub const BASE_10_0: TimeControl = TimeControl::new(600, 0);

    pub const fn new(base_secs: u64, inc_secs: u64) -> TimeControl {
        TimeControl {
            base: Duration::from_secs(base_secs),
            increment: Duration::from_secs(inc_secs),
        }
    }

    /// Presets offered in the UI.
    pub const PRESETS: [TimeControl; 4] = [
        TimeControl::BASE_1_0,
        TimeControl::BASE_3_2,
        TimeControl::BASE_5_0,
        TimeControl::BASE_10_0,
    ];

    pub fn label(&self) -> String {
        Clock::label(self.base, self.increment)
    }
}

/// How the game ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameEnd {
    pub outcome: Outcome,
    /// Human-readable reason, shown in the UI.
    pub reason: String,
    /// PGN `Termination` header value.
    pub termination: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Win(Color),
    Draw,
}

impl Outcome {
    pub fn result_string(&self) -> &'static str {
        match self {
            Outcome::Win(Color::White) => "1-0",
            Outcome::Win(Color::Black) => "0-1",
            Outcome::Draw => "1/2-1/2",
        }
    }
}

/// One applied move, in both notations.
#[derive(Clone, Debug)]
pub struct MoveRecord {
    pub uci: String,
    pub san: String,
}

pub struct Session {
    game: Game,
    /// Cached current position (`Game::current_position` returns an owned
    /// Board on every call, so we keep one here to hand out references).
    board: Board,
    pub clock: Clock,
    pub white_name: String,
    pub black_name: String,
    /// Our color in a LAN game; `None` for a local two-player game.
    pub my_color: Option<Color>,
    pub moves: Vec<MoveRecord>,
    pub end: Option<GameEnd>,
    /// The color that currently has a draw offer on the table.
    pub draw_offered_by: Option<Color>,
    pub tc: TimeControl,
}

impl Session {
    pub fn new(
        white_name: String,
        black_name: String,
        my_color: Option<Color>,
        tc: TimeControl,
    ) -> Session {
        Session {
            game: Game::new(),
            board: Board::default(),
            clock: Clock::new(tc.base, tc.increment),
            white_name,
            black_name,
            my_color,
            moves: Vec::new(),
            end: None,
            draw_offered_by: None,
            tc,
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn side_to_move(&self) -> Color {
        self.game.side_to_move()
    }

    pub fn is_my_turn(&self) -> bool {
        match self.my_color {
            Some(c) => self.side_to_move() == c,
            None => true, // local game: whoever is to move
        }
    }

    pub fn is_over(&self) -> bool {
        self.end.is_some()
    }

    pub fn name_of(&self, color: Color) -> &str {
        match color {
            Color::White => &self.white_name,
            Color::Black => &self.black_name,
        }
    }

    /// Legal destinations for a piece on `sq`.
    pub fn legal_targets(&self, sq: Square) -> Vec<Square> {
        let mut dests: Vec<Square> = MoveGen::new_legal(self.board())
            .filter(|m| m.get_source() == sq)
            .map(|m| m.get_dest())
            .collect();
        dests.sort_by_key(|s| s.to_int());
        dests.dedup();
        dests
    }

    /// True if moving from `from` to `to` requires a promotion choice.
    pub fn needs_promotion(&self, from: Square, to: Square) -> bool {
        MoveGen::new_legal(self.board())
            .any(|m| m.get_source() == from && m.get_dest() == to && m.get_promotion().is_some())
    }

    /// Validate and apply a local move. Returns the record if legal.
    /// Handles end-of-game detection and clock switching.
    pub fn try_move(
        &mut self,
        from: Square,
        to: Square,
        promo: Option<Piece>,
    ) -> Option<MoveRecord> {
        if self.is_over() {
            return None;
        }
        let mv = ChessMove::new(from, to, promo);
        if !self.board().legal(mv) {
            return None;
        }
        Some(self.apply_move(mv))
    }

    /// Apply a move from a UCI string (remote opponent). `clock_ms` is the
    /// mover's remaining time as measured on their machine, which we adopt.
    pub fn apply_remote_uci(&mut self, uci: &str, clock_ms: u64) -> Option<MoveRecord> {
        if self.is_over() {
            return None;
        }
        let mv = san::uci_to_move(self.board(), uci)?;
        let mover = self.game.side_to_move();
        let record = self.apply_move(mv);
        self.clock
            .set_remaining(mover, Duration::from_millis(clock_ms));
        Some(record)
    }

    fn apply_move(&mut self, mv: ChessMove) -> MoveRecord {
        let san_text = san::san(&self.board, mv);
        let uci_text = san::move_to_uci(mv);
        let mover = self.game.side_to_move();
        if !self.game.make_move(mv) {
            // Callers validate legality first; this is unreachable in practice.
            debug_assert!(false, "apply_move called with an illegal move");
        }
        self.board = self.game.current_position();
        self.draw_offered_by = None; // any move cancels a standing draw offer
        self.clock.switch(mover);
        let record = MoveRecord {
            uci: uci_text,
            san: san_text,
        };
        self.moves.push(record.clone());
        self.detect_move_end();
        record
    }

    /// Check for natural endings after a move (mate, stalemate, repetition,
    /// 50-move rule). All detection delegated to the `chess` crate.
    fn detect_move_end(&mut self) {
        if self.end.is_some() {
            return;
        }
        match self.game.result() {
            Some(chess::GameResult::WhiteCheckmates) => {
                self.finish(Outcome::Win(Color::White), "checkmate", "Normal");
            }
            Some(chess::GameResult::BlackCheckmates) => {
                self.finish(Outcome::Win(Color::Black), "checkmate", "Normal");
            }
            Some(chess::GameResult::Stalemate) => {
                self.finish(Outcome::Draw, "stalemate", "Normal");
            }
            _ => {
                if self.game.can_declare_draw() {
                    // Distinguish 50-move from threefold using the halfmove
                    // clock in the current FEN.
                    let fen = self.board().to_string();
                    let halfmove: u32 = fen
                        .split(' ')
                        .nth(4)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    if halfmove >= 100 {
                        self.finish(Outcome::Draw, "fifty-move rule", "Fifty-move rule");
                    } else {
                        self.finish(Outcome::Draw, "threefold repetition", "Repetition");
                    }
                }
            }
        }
    }

    pub fn offer_draw(&mut self, color: Color) {
        if self.end.is_none() {
            self.draw_offered_by = Some(color);
        }
    }

    pub fn accept_draw(&mut self) {
        if self.end.is_none() {
            self.finish(Outcome::Draw, "draw by agreement", "Agreement");
        }
    }

    pub fn decline_draw(&mut self) {
        self.draw_offered_by = None;
    }

    pub fn resign(&mut self, color: Color) {
        if self.end.is_none() {
            let winner = !color;
            self.finish(
                Outcome::Win(winner),
                &format!("{} resigned", self.name_of(color)),
                "Normal",
            );
        }
    }

    /// `loser`'s clock ran out.
    pub fn timeout(&mut self, loser: Color) {
        if self.end.is_none() {
            self.finish(
                Outcome::Win(!loser),
                &format!("{} ran out of time", self.name_of(loser)),
                "Time forfeit",
            );
        }
    }

    /// The other side vanished from the network.
    pub fn abandoned(&mut self, winner: Color) {
        if self.end.is_none() {
            self.finish(Outcome::Win(winner), "opponent disconnected", "Abandoned");
        }
    }

    fn finish(&mut self, outcome: Outcome, reason: &str, termination: &'static str) {
        self.clock.freeze();
        self.draw_offered_by = None;
        self.end = Some(GameEnd {
            outcome,
            reason: reason.to_string(),
            termination,
        });
    }

    /// Outcome string for the PGN `Result` header ("*", "1-0", ...).
    pub fn result_string(&self) -> &'static str {
        match &self.end {
            Some(e) => e.outcome.result_string(),
            None => "*",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_sq(s: &str) -> Square {
        let b = s.as_bytes();
        Square::make_square(
            chess::Rank::from_index((b[1] - b'1') as usize),
            chess::File::from_index((b[0] - b'a') as usize),
        )
    }

    #[test]
    fn full_short_game_mate() {
        let mut s = Session::new(
            "W".into(),
            "B".into(),
            Some(Color::White),
            TimeControl::BASE_5_0,
        );
        // Fool's mate: f3 e5 g4 Qh4#
        let seq = [("f2", "f3"), ("e7", "e5"), ("g2", "g4"), ("d8", "h4")];
        for (from, to) in seq {
            let rec = s
                .try_move(parse_sq(from), parse_sq(to), None)
                .expect("legal move");
            assert!(!rec.san.is_empty());
        }
        assert!(s.is_over());
        let end = s.end.clone().unwrap();
        assert_eq!(end.outcome, Outcome::Win(Color::Black));
        assert_eq!(s.result_string(), "0-1");
        assert_eq!(s.moves[3].san, "Qh4#");
    }

    #[test]
    fn remote_uci_and_clock_sync() {
        let mut s = Session::new(
            "W".into(),
            "B".into(),
            Some(Color::White),
            TimeControl::BASE_3_2,
        );
        // White moves locally first (it is white's turn), then black's move
        // arrives from the network.
        s.try_move(parse_sq("e2"), parse_sq("e4"), None).unwrap();
        let rec = s.apply_remote_uci("e7e5", 299_000).unwrap();
        assert_eq!(rec.san, "e5");
        assert_eq!(s.side_to_move(), Color::White);
        // Black's clock was overwritten with the reported time.
        assert_eq!(
            s.clock.remaining(Color::Black),
            Duration::from_millis(299_000)
        );
        assert!(s.apply_remote_uci("e2e5", 0).is_none()); // illegal
    }

    #[test]
    fn draw_by_agreement() {
        let mut s = Session::new("W".into(), "B".into(), None, TimeControl::BASE_5_0);
        s.try_move(parse_sq("e2"), parse_sq("e4"), None).unwrap();
        s.offer_draw(Color::White);
        assert_eq!(s.draw_offered_by, Some(Color::White));
        // Any move cancels the standing offer.
        s.try_move(parse_sq("e7"), parse_sq("e5"), None).unwrap();
        assert_eq!(s.draw_offered_by, None);
        s.offer_draw(Color::Black);
        s.accept_draw();
        assert!(s.is_over());
        assert_eq!(s.result_string(), "1/2-1/2");
    }

    #[test]
    fn threefold_repetition_auto_draw() {
        let mut s = Session::new("W".into(), "B".into(), None, TimeControl::BASE_5_0);
        // Repeat the position from the start three times.
        let seq = [
            ("g1", "f3"),
            ("g8", "f6"),
            ("f3", "g1"),
            ("f6", "g8"),
            ("g1", "f3"),
            ("g8", "f6"),
            ("f3", "g1"),
            ("f6", "g8"),
        ];
        for (from, to) in seq {
            s.try_move(parse_sq(from), parse_sq(to), None).unwrap();
        }
        assert!(s.is_over());
        assert_eq!(s.result_string(), "1/2-1/2");
        assert!(s.end.unwrap().reason.contains("repetition"));
    }

    #[test]
    fn resignation_and_timeout() {
        let mut s = Session::new(
            "W".into(),
            "B".into(),
            Some(Color::White),
            TimeControl::BASE_1_0,
        );
        s.resign(Color::Black);
        assert_eq!(s.result_string(), "1-0");
        let mut s2 = Session::new("W".into(), "B".into(), None, TimeControl::BASE_1_0);
        s2.timeout(Color::White);
        assert_eq!(s2.result_string(), "0-1");
        assert_eq!(s2.end.unwrap().termination, "Time forfeit");
    }

    #[test]
    fn legal_targets_and_promotion_flag() {
        let mut s = Session::new("W".into(), "B".into(), None, TimeControl::BASE_5_0);
        // 1. a4 b5 2. axb5 a6 3. bxa6 h5 4. a7 h4 5. axb8=Q
        for (from, to) in [
            ("a2", "a4"),
            ("b7", "b5"),
            ("a4", "b5"),
            ("a7", "a6"),
            ("b5", "a6"),
            ("h7", "h5"),
            ("a6", "a7"),
            ("h5", "h4"),
        ] {
            s.try_move(parse_sq(from), parse_sq(to), None).unwrap();
        }
        let targets = s.legal_targets(parse_sq("a7"));
        assert!(targets.contains(&parse_sq("b8")));
        assert!(s.needs_promotion(parse_sq("a7"), parse_sq("b8")));
        assert!(!s.needs_promotion(parse_sq("e2"), parse_sq("e4")));
        let rec = s
            .try_move(parse_sq("a7"), parse_sq("b8"), Some(Piece::Queen))
            .unwrap();
        assert_eq!(rec.san, "axb8=Q");
        assert_eq!(rec.uci, "a7b8q");
    }
}
