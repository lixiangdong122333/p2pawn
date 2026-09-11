//! SAN (Standard Algebraic Notation) formatting and UCI parsing.
//!
//! This is presentation-layer formatting only: legality comes from the `chess`
//! crate's `MoveGen`; we only pick letters, disambiguation and check markers.

use chess::{Board, ChessMove, Piece, Square};

/// Format `mv` (legal in `board`) as SAN, e.g. "Nbd2", "exd5", "O-O", "e8=Q+".
pub fn san(board: &Board, mv: ChessMove) -> String {
    let src = mv.get_source();
    let dest = mv.get_dest();
    let piece = board.piece_on(src).unwrap_or(Piece::Pawn);

    let mut out = String::new();

    if piece == Piece::King
        && src
            .get_file()
            .to_index()
            .abs_diff(dest.get_file().to_index())
            == 2
    {
        // Castling.
        out.push_str(if dest.get_file().to_index() > src.get_file().to_index() {
            "O-O"
        } else {
            "O-O-O"
        });
    } else {
        let capture = is_capture(board, mv);
        match piece {
            Piece::Pawn => {
                if capture {
                    out.push(file_char(src.get_file()));
                    out.push('x');
                }
                out.push_str(&dest.to_string());
            }
            p => {
                out.push(piece_letter(p));
                if let Some(disamb) = disambiguation(board, mv) {
                    out.push_str(&disamb);
                }
                if capture {
                    out.push('x');
                }
                out.push_str(&dest.to_string());
            }
        }
        if let Some(promo) = mv.get_promotion() {
            out.push('=');
            out.push(piece_letter(promo));
        }
    }

    let after = board.make_move_new(mv);
    if after.checkers().popcnt() > 0 {
        out.push(if after.status() == chess::BoardStatus::Checkmate {
            '#'
        } else {
            '+'
        });
    }
    out
}

/// Parse a UCI move string ("e2e4", "e7e8q") against `board`.
pub fn uci_to_move(board: &Board, uci: &str) -> Option<ChessMove> {
    let bytes = uci.as_bytes();
    if bytes.len() != 4 && bytes.len() != 5 {
        return None;
    }
    let src = square_from_coords(bytes[0], bytes[1])?;
    let dest = square_from_coords(bytes[2], bytes[3])?;
    let promo = match bytes.get(4) {
        Some(b'q') => Some(Piece::Queen),
        Some(b'r') => Some(Piece::Rook),
        Some(b'b') => Some(Piece::Bishop),
        Some(b'n') => Some(Piece::Knight),
        _ => None,
    };
    let mv = ChessMove::new(src, dest, promo);
    move_gen_contains(board, mv).then_some(mv)
}

pub fn move_to_uci(mv: ChessMove) -> String {
    let mut s = format!("{}{}", mv.get_source(), mv.get_dest());
    if let Some(p) = mv.get_promotion() {
        s.push(piece_letter(p).to_ascii_lowercase());
    }
    s
}

fn move_gen_contains(board: &Board, mv: ChessMove) -> bool {
    chess::MoveGen::new_legal(board).any(|m| m == mv)
}

fn square_from_coords(file: u8, rank: u8) -> Option<Square> {
    if !(b'a'..=b'h').contains(&file) || !(b'1'..=b'8').contains(&rank) {
        return None;
    }
    Some(Square::make_square(
        chess::Rank::from_index((rank - b'1') as usize),
        chess::File::from_index((file - b'a') as usize),
    ))
}

fn piece_letter(p: Piece) -> char {
    match p {
        Piece::Pawn => 'P',
        Piece::Knight => 'N',
        Piece::Bishop => 'B',
        Piece::Rook => 'R',
        Piece::Queen => 'Q',
        Piece::King => 'K',
    }
}

fn file_char(f: chess::File) -> char {
    (b'a' + f.to_index() as u8) as char
}

fn rank_char(r: chess::Rank) -> char {
    (b'1' + r.to_index() as u8) as char
}

fn is_capture(board: &Board, mv: ChessMove) -> bool {
    let dest = mv.get_dest();
    if board.piece_on(dest).is_some() {
        return true;
    }
    // En passant: pawn moving diagonally onto an empty square.
    if board.piece_on(mv.get_source()) == Some(Piece::Pawn)
        && mv.get_source().get_file() != dest.get_file()
    {
        return true;
    }
    false
}

/// Determine the minimal SAN disambiguation for `mv` among same-type pieces
/// that can legally reach the same destination.
fn disambiguation(board: &Board, mv: ChessMove) -> Option<String> {
    let src = mv.get_source();
    let dest = mv.get_dest();
    let piece = board.piece_on(src)?;

    let rivals: Vec<Square> = chess::MoveGen::new_legal(board)
        .filter(|m| {
            m.get_dest() == dest
                && m.get_source() != src
                && board.piece_on(m.get_source()) == Some(piece)
        })
        .map(|m| m.get_source())
        .collect();
    if rivals.is_empty() {
        return None;
    }
    // Pawns never need piece disambiguation (file is written for captures).
    if piece == Piece::Pawn {
        return None;
    }

    let same_file = rivals.iter().any(|s| s.get_file() == src.get_file());
    let same_rank = rivals.iter().any(|s| s.get_rank() == src.get_rank());
    if !same_file {
        Some(file_char(src.get_file()).to_string())
    } else if !same_rank {
        Some(rank_char(src.get_rank()).to_string())
    } else {
        Some(src.to_string())
    }
}

/// Color of a square for the standard "light/dark" checkerboard shading.
pub fn square_is_light(sq: Square) -> bool {
    (sq.get_file().to_index() + sq.get_rank().to_index()) % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::MoveGen;

    fn san_of_first(board: &Board, pred: impl Fn(ChessMove) -> bool) -> String {
        let mv = MoveGen::new_legal(board).find(|&m| pred(m)).unwrap();
        san(board, mv)
    }

    #[test]
    fn basic_san() {
        let b = Board::default();
        let e2e4 = ChessMove::new(
            Square::make_square(chess::Rank::Second, chess::File::E),
            Square::make_square(chess::Rank::Fourth, chess::File::E),
            None,
        );
        assert_eq!(san(&b, e2e4), "e4");
    }

    #[test]
    fn uci_roundtrip() {
        let b = Board::default();
        let mv = uci_to_move(&b, "e2e4").unwrap();
        assert_eq!(move_to_uci(mv), "e2e4");
        assert!(uci_to_move(&b, "e2e5").is_none()); // illegal
        assert!(uci_to_move(&b, "xx").is_none());
    }

    #[test]
    fn disambiguation_two_knights() {
        // Knights on b1 and f3 can both reach d2 (d2 pawn removed).
        let b = "rnbqkbnr/pppppppp/8/8/8/5N2/PPP1PPPP/RNBQKB1R w KQkq - 0 1"
            .parse::<Board>()
            .unwrap();
        let s = san_of_first(&b, |m| {
            m.get_source() == Square::B1
                && m.get_dest() == Square::make_square(chess::Rank::Second, chess::File::D)
        });
        assert_eq!(s, "Nbd2");
        let s = san_of_first(&b, |m| {
            m.get_source() == Square::F3
                && m.get_dest() == Square::make_square(chess::Rank::Second, chess::File::D)
        });
        assert_eq!(s, "Nfd2");
    }

    #[test]
    fn mate_suffix() {
        // Fool's mate final position.
        let b = "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 0 3"
            .parse::<Board>()
            .unwrap();
        assert_eq!(b.status(), chess::BoardStatus::Checkmate);
        // The mating move Qh4# was played on the previous board:
        let prev = "rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq g3 0 2"
            .parse::<Board>()
            .unwrap();
        let s = san_of_first(&prev, |m| {
            m.get_dest() == Square::make_square(chess::Rank::Fourth, chess::File::H)
        });
        assert_eq!(s, "Qh4#");
    }

    #[test]
    fn castling_san() {
        let b = "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1"
            .parse::<Board>()
            .unwrap();
        // Filter by king's source so the rook move (Rg1) doesn't match first.
        let s = san_of_first(&b, |m| {
            m.get_source() == Square::E1
                && m.get_dest() == Square::make_square(chess::Rank::First, chess::File::G)
        });
        assert_eq!(s, "O-O");
        let s = san_of_first(&b, |m| {
            m.get_source() == Square::E1
                && m.get_dest() == Square::make_square(chess::Rank::First, chess::File::C)
        });
        assert_eq!(s, "O-O-O");
    }

    #[test]
    fn promotion_san() {
        let b = "8/P6k/8/8/8/8/8/K7 w - - 0 1".parse::<Board>().unwrap();
        let s = san_of_first(&b, |m| {
            m.get_dest() == Square::make_square(chess::Rank::Eighth, chess::File::A)
                && m.get_promotion() == Some(Piece::Queen)
        });
        assert_eq!(s, "a8=Q");
    }

    #[test]
    fn en_passant_san() {
        let b = "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3"
            .parse::<Board>()
            .unwrap();
        let s = san_of_first(&b, |m| {
            m.get_dest() == Square::make_square(chess::Rank::Sixth, chess::File::F)
        });
        assert_eq!(s, "exf6");
    }
}
