//! All rendering: screens, board, modals. Pure functions of `App` state.

use chess::{Board, Color, Piece, Square};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color as C, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Modal, Screen, MENU_ITEMS};
use crate::config::{HintStyle, PieceStyle};
use crate::game::session::{Outcome, Session, TimeControl};
use crate::net::proto::PeerStatus;

// Palette
const DIM: C = C::Rgb(110, 110, 130);
const ACCENT: C = C::Rgb(255, 205, 100);
const LIGHT_SQ: C = C::Rgb(72, 62, 52);
const DARK_SQ: C = C::Rgb(46, 40, 34);
const SEL_BG: C = C::Rgb(120, 95, 40);
const HINT: C = C::Rgb(140, 190, 120);
const CURSOR_BG: C = C::Rgb(70, 90, 60);
const CHECK_BG: C = C::Rgb(120, 40, 40);

pub fn draw(f: &mut Frame, app: &App) {
    match app.screen {
        Screen::Menu => draw_menu(f, app),
        Screen::Lobby => draw_lobby(f, app),
        Screen::LocalSetup => draw_setup(f, app),
        Screen::Playing => draw_game(f, app),
        Screen::History => draw_history(f, app),
        Screen::Replay => draw_replay(f, app),
        Screen::Settings => draw_settings(f, app),
        Screen::Help => draw_help(f, app),
    }
    if let Some(modal) = &app.modal {
        draw_modal(f, app, modal);
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect {
        x,
        y,
        width: w.min(area.width),
        height: h.min(area.height),
    }
}

/// A rounded popup block, cleared underneath. Returns the inner area.
fn popup(f: &mut Frame, title: &str, w: u16, h: u16) -> Rect {
    let area = f.area();
    let rect = centered(area, w, h);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title_top(format!(" {title} ").bold().fg(ACCENT).into_centered_line());
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    inner
}

// ---------------------------------------------------------------------
// Menu

fn draw_menu(f: &mut Frame, app: &App) {
    let inner = popup(f, "♟ p2pawn", 44, 15);

    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    let items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let sel = i == app.menu_sel;
            let line = if sel {
                Line::from(format!("❯ {label}"))
                    .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))
            } else {
                Line::from(format!("  {label}")).style(Style::new().fg(C::Gray))
            };
            ListItem::new(line)
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.menu_sel));
    f.render_stateful_widget(List::new(items), rows[0], &mut state);

    f.render_widget(
        Paragraph::new(Line::from("↑↓ select  ⏎ confirm  q quit").fg(DIM)).centered(),
        rows[1],
    );
}

// ---------------------------------------------------------------------
// Lobby

fn draw_lobby(f: &mut Frame, app: &App) {
    let inner = popup(f, "LAN Players", 48, 17);

    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).split(inner);

    let peers = app.discovery.peers();
    let items: Vec<ListItem> = if peers.is_empty() {
        vec![ListItem::new(
            Line::from("searching for players...").fg(DIM).centered(),
        )]
    } else {
        peers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let sel = i == app.lobby_sel;
                let status_style = match p.status {
                    PeerStatus::Ready => Style::new().fg(C::Green),
                    PeerStatus::Playing => Style::new().fg(DIM),
                };
                let name_style = if sel {
                    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(C::Gray)
                };
                let line = Line::from(vec![
                    Span::styled(if sel { "❯ " } else { "  " }, Style::new().fg(ACCENT)),
                    Span::styled(format!("  {:<18}", p.name), name_style),
                    Span::styled(p.status.label(), status_style),
                ]);
                ListItem::new(line)
            })
            .collect()
    };
    let mut state = ListState::default();
    if !peers.is_empty() {
        state.select(Some(app.lobby_sel));
    }
    f.render_stateful_widget(List::new(items), rows[0], &mut state);

    let status = if app.lobby_status.is_empty() {
        format!(
            "{} player(s) on the network   (you: {})",
            peers.len(),
            app.config.player_name
        )
    } else {
        app.lobby_status.clone()
    };
    let footer = Paragraph::new(vec![
        Line::from(status).fg(DIM),
        Line::from(""),
        Line::from("⏎ invite  r refresh  esc back").fg(DIM),
    ])
    .wrap(Wrap { trim: false });
    f.render_widget(footer, rows[1]);
}

// ---------------------------------------------------------------------
// Local setup

fn draw_setup(f: &mut Frame, app: &App) {
    let inner = popup(f, "New Local Game", 48, 11);

    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(inner);

    let row_style = |i: usize| {
        if i == app.setup_sel {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(C::Gray)
        }
    };
    let val_style = |i: usize| {
        if i == app.setup_sel {
            Style::new().fg(ACCENT)
        } else {
            Style::new().fg(C::Gray)
        }
    };

    let color_row = Line::from(vec![
        Span::styled("  Your color    ", row_style(0)),
        Span::styled(format!("◂ {} ▸", app.setup_color.label()), val_style(0)),
    ]);
    let tc_row = Line::from(vec![
        Span::styled("  Time control  ", row_style(1)),
        Span::styled(format!("◂ {} ▸", app.setup_tc.label()), val_style(1)),
    ]);
    let start_row = Line::from(if app.setup_sel == 2 {
        "  ❯ Start game"
    } else {
        "    Start game"
    })
    .style(row_style(2));

    f.render_widget(Paragraph::new(color_row), rows[0]);
    f.render_widget(Paragraph::new(tc_row), rows[1]);
    f.render_widget(Paragraph::new(start_row), rows[2]);
    f.render_widget(
        Paragraph::new(Line::from("←→ change  ⏎ select  esc back").fg(DIM)),
        rows[3],
    );
}

// ---------------------------------------------------------------------
// Game screen

fn draw_game(f: &mut Frame, app: &App) {
    let Some(sess) = app.session.as_ref() else { return };
    let area = f.area();

    let cols = Layout::horizontal([Constraint::Min(21), Constraint::Min(28)]).split(area);
    let board_rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(11),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(cols[0]);

    let me = sess.my_color.unwrap_or(Color::White);
    let opp = !me;

    render_player_bar(f, board_rows[0], sess, opp);
    render_board(f, board_rows[1], app, Some(sess), sess.board());
    render_player_bar(f, board_rows[2], sess, me);
    render_game_status(f, board_rows[3], sess);

    let panel_rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(cols[1]);

    render_game_header(f, panel_rows[0], sess);
    let toast = app
        .toast
        .as_ref()
        .map(|(t, _)| Line::from(t.clone()).fg(HINT))
        .unwrap_or_default();
    f.render_widget(
        Paragraph::new(toast).wrap(Wrap { trim: false }),
        panel_rows[1],
    );
    let sans: Vec<&str> = sess.moves.iter().map(|m| m.san.as_str()).collect();
    render_movelist(f, panel_rows[2], &sans, usize::MAX);
    f.render_widget(
        Paragraph::new(Line::from("↑↓←→ move  ⏎ select  esc menu").fg(DIM)),
        panel_rows[3],
    );
}

fn render_player_bar(f: &mut Frame, area: Rect, sess: &Session, color: Color) {
    if area.width < 10 {
        return;
    }
    let time = fmt_clock(sess.clock.remaining(color));
    let to_move = sess.side_to_move() == color && !sess.is_over();
    let name = sess.name_of(color);
    let captured = captured_string(sess.board(), color);
    let name_span = Span::styled(
        format!(
            " {} {}",
            if color == Color::White { "◇" } else { "◆" },
            name
        ),
        if to_move {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(C::Gray)
        },
    );
    let right = if to_move {
        Span::styled(format!("{time} ◂"), Style::new().fg(ACCENT).bold())
    } else {
        Span::styled(time, Style::new().fg(C::Gray))
    };
    let mut line = Line::from(name_span);
    if !captured.is_empty() {
        line.push_span(Span::styled(format!("  {captured}"), Style::new().fg(DIM)));
    }
    let pad = (area.width as usize)
        .saturating_sub(line.width() + right.content.len())
        .max(1);
    line.push_span(Span::raw(" ".repeat(pad)));
    line.push_span(right);
    f.render_widget(Paragraph::new(line), area);
}

fn fmt_clock(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!("{:2}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{:2}:{:02}.{}", secs / 60, secs % 60, d.subsec_millis() / 100)
    }
}

/// Opponent pieces missing from the board (captured by `by_color`).
fn captured_string(board: &Board, by_color: Color) -> String {
    let opponent = !by_color;
    let mut out = String::new();
    for piece in [Piece::Queen, Piece::Rook, Piece::Bishop, Piece::Knight, Piece::Pawn] {
        let have = (*board.pieces(piece) & *board.color_combined(opponent)).popcnt();
        let full = if piece == Piece::Pawn { 8 } else { 2 };
        for _ in have..full {
            out.push_str(&uni_piece(piece, opponent, PieceStyle::Unicode));
        }
    }
    out
}

fn uni_piece(piece: Piece, color: Color, style: PieceStyle) -> String {
    match style {
        PieceStyle::Unicode => match (color, piece) {
            (Color::White, Piece::King) => "♔",
            (Color::White, Piece::Queen) => "♕",
            (Color::White, Piece::Rook) => "♖",
            (Color::White, Piece::Bishop) => "♗",
            (Color::White, Piece::Knight) => "♘",
            (Color::White, Piece::Pawn) => "♙",
            (Color::Black, Piece::King) => "♚",
            (Color::Black, Piece::Queen) => "♛",
            (Color::Black, Piece::Rook) => "♜",
            (Color::Black, Piece::Bishop) => "♝",
            (Color::Black, Piece::Knight) => "♞",
            (Color::Black, Piece::Pawn) => "♟",
        }
        .to_string(),
        PieceStyle::Ascii => {
            let c = match piece {
                Piece::King => 'K',
                Piece::Queen => 'Q',
                Piece::Rook => 'R',
                Piece::Bishop => 'B',
                Piece::Knight => 'N',
                Piece::Pawn => 'P',
            };
            match color {
                Color::White => c.to_string(),
                Color::Black => c.to_ascii_lowercase().to_string(),
            }
        }
    }
}

/// Render the 8x8 board. `session` provides selection/last-move/check context;
/// pass `None` for the replay screen.
fn render_board(f: &mut Frame, area: Rect, app: &App, session: Option<&Session>, board: &Board) {
    if area.width < 17 || area.height < 8 {
        return;
    }
    let flip = match session {
        Some(_) => app.view_flipped(),
        None => app.config.flip_board,
    };
    let show_coords = app.config.show_coordinates;
    let (selected_targets, last_move, check_sq) = match session {
        Some(_) => (
            app.selected_targets(),
            app.last_move_squares(),
            app.check_square(),
        ),
        None => (Vec::new(), None, None),
    };
    let cursor = session.and_then(|_| app.cursor);

    let grid_w: u16 = 16;
    let grid_h: u16 = 8;
    let left_pad: u16 = if show_coords { 2 } else { 0 };

    let free_w = area.width.saturating_sub(left_pad + if show_coords { 1 } else { 0 });
    let free_h = area.height.saturating_sub(if show_coords { 1 } else { 0 });
    let grid = Rect {
        x: area.x + left_pad + free_w.saturating_sub(grid_w) / 2,
        y: area.y + free_h.saturating_sub(grid_h) / 2,
        width: grid_w,
        height: grid_h,
    };

    for vr in 0..8usize {
        let rank_idx = if flip { vr } else { 7 - vr };
        for vf in 0..8usize {
            let file_idx = if flip { 7 - vf } else { vf };
            let sq = Square::make_square(
                chess::Rank::from_index(rank_idx),
                chess::File::from_index(file_idx),
            );
            let cell = Rect {
                x: grid.x + (vf as u16) * 2,
                y: grid.y + vr as u16,
                width: 2,
                height: 1,
            };

            let mut bg = if crate::game::san::square_is_light(sq) {
                LIGHT_SQ
            } else {
                DARK_SQ
            };
            if Some(sq) == cursor {
                bg = CURSOR_BG;
            }
            if let Some((from, to)) = last_move {
                if sq == from || sq == to {
                    bg = SEL_BG;
                }
            }
            if Some(sq) == check_sq {
                bg = CHECK_BG;
            }
            let is_target = selected_targets.contains(&sq);

            let (sym, fg) = if let Some(piece) = board.piece_on(sq) {
                let color = board.color_on(sq).unwrap_or(Color::White);
                let fg = if color == Color::White {
                    C::White
                } else {
                    C::Black
                };
                (uni_piece(piece, color, app.config.piece_style), fg)
            } else if is_target && app.config.show_hints == HintStyle::All {
                ("◎".to_string(), HINT)
            } else {
                (" ".to_string(), C::Reset)
            };
            let style = Style::new().bg(bg).fg(fg);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(format!("{sym} "), style))),
                cell,
            );
        }
        if show_coords {
            let num = (if flip { vr + 1 } else { 8 - vr }).to_string();
            f.render_widget(
                Paragraph::new(Span::styled(num, Style::new().fg(DIM))),
                Rect {
                    x: grid.x.saturating_sub(2),
                    y: grid.y + vr as u16,
                    width: 2,
                    height: 1,
                },
            );
        }
    }
    if show_coords {
        for vf in 0..8usize {
            let file_idx = if flip { 7 - vf } else { vf };
            let letter = (b'a' + file_idx as u8) as char;
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("{letter} "),
                    Style::new().fg(DIM),
                )),
                Rect {
                    x: grid.x + (vf as u16) * 2,
                    y: grid.y + grid_h,
                    width: 2,
                    height: 1,
                },
            );
        }
    }
}

fn render_game_status(f: &mut Frame, area: Rect, sess: &Session) {
    let mut msg = String::from("tab flip");
    if sess.board().checkers().popcnt() > 0 {
        msg = format!("⚠ CHECK  {msg}");
    }
    if sess.my_color.is_some() && !sess.is_over() {
        let turn = if sess.is_my_turn() {
            "your move"
        } else {
            "waiting..."
        };
        msg = format!("{turn}  {msg}");
    }
    f.render_widget(Paragraph::new(Line::from(msg).fg(DIM)), area);
}

fn render_game_header(f: &mut Frame, area: Rect, sess: &Session) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    f.render_widget(
        Paragraph::new(Span::styled(
            format!(" {} vs {} ", sess.white_name, sess.black_name),
            Style::new().fg(ACCENT).bold(),
        )),
        rows[0],
    );
    let status = if sess.is_over() {
        let e = sess.end.as_ref().unwrap();
        format!("{}  ({})", e.outcome.result_string(), e.reason)
    } else {
        format!(
            "move {}  {} to move  {}",
            sess.moves.len() / 2 + 1,
            if sess.side_to_move() == Color::White {
                "white"
            } else {
                "black"
            },
            sess.tc.label()
        )
    };
    f.render_widget(
        Paragraph::new(Span::styled(status, Style::new().fg(C::Gray))),
        rows[1],
    );
}

/// Two-column movelist, scrolled so the tail stays visible; `highlight`
/// marks a move index (replay) or nothing (`usize::MAX`).
fn render_movelist(f: &mut Frame, area: Rect, sans: &[&str], highlight: usize) {
    let block = Block::bordered()
        .title_top(" Moves ")
        .border_style(Style::new().fg(DIM));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width < 10 {
        return;
    }
    let mut lines: Vec<Line> = Vec::new();
    for (i, chunk) in sans.chunks(2).enumerate() {
        let w = chunk.first().copied().unwrap_or("");
        let b = chunk.get(1).copied().unwrap_or("");
        let w_sel = highlight == i * 2;
        let b_sel = highlight == i * 2 + 1;
        lines.push(Line::from(vec![
            Span::styled(format!("{:2}. ", i + 1), Style::new().fg(DIM)),
            Span::styled(
                format!("{w:<7}"),
                if w_sel {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new().fg(C::Gray)
                },
            ),
            Span::styled(
                format!("{b:<7}"),
                if b_sel {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new().fg(C::Gray)
                },
            ),
        ]));
    }
    let visible = inner.height as usize;
    let skip = lines.len().saturating_sub(visible);
    let shown: Vec<Line> = lines.into_iter().skip(skip).collect();
    f.render_widget(Paragraph::new(shown), inner);
}

// ---------------------------------------------------------------------
// History

/// "20260910-142233-abcd" -> "2026-09-10"
fn stem_to_date(stem: &str) -> String {
    stem.split('-')
        .next()
        .filter(|d| d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()))
        .map(|d| format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8]))
        .unwrap_or_else(|| stem.to_string())
}

fn draw_history(f: &mut Frame, app: &App) {
    let inner = popup(f, "Game History", 58, 20);

    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let entries = app.history.as_ref().map(|h| h.list()).unwrap_or_default();
    let items: Vec<ListItem> = if entries.is_empty() {
        vec![ListItem::new(
            Line::from("no saved games yet").fg(DIM).centered(),
        )]
    } else {
        entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let sel = i == app.history_sel;
                let result = match e.parsed.result.as_str() {
                    "1-0" => "1-0",
                    "0-1" => "0-1",
                    "1/2-1/2" => "½-½",
                    _ => "*",
                };
                let date = stem_to_date(&e.stem);
                let line = Line::from(vec![
                    Span::styled(if sel { "❯ " } else { "  " }, Style::new().fg(ACCENT)),
                    Span::styled(format!("{date}  "), Style::new().fg(DIM)),
                    Span::styled(
                        format!("{} vs {}", e.parsed.white, e.parsed.black),
                        if sel {
                            Style::new().fg(ACCENT)
                        } else {
                            Style::new().fg(C::Gray)
                        },
                    ),
                    Span::styled(format!("  {result}"), Style::new().fg(C::Green)),
                ]);
                ListItem::new(line)
            })
            .collect()
    };
    let mut state = ListState::default();
    if !entries.is_empty() {
        state.select(Some(app.history_sel));
    }
    f.render_stateful_widget(List::new(items), rows[0], &mut state);
    f.render_widget(
        Paragraph::new(Line::from("⏎ replay  d delete  esc back").fg(DIM)),
        rows[1],
    );
}

// ---------------------------------------------------------------------
// Replay

fn draw_replay(f: &mut Frame, app: &App) {
    let Some(replay) = app.replay.as_ref() else {
        return;
    };
    let area = f.area();
    let cols = Layout::horizontal([Constraint::Min(21), Constraint::Min(28)]).split(area);
    let board_rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(11),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(cols[0]);

    f.render_widget(
        Paragraph::new(Span::styled(" Replay ", Style::new().fg(ACCENT).bold())),
        board_rows[0],
    );
    render_board(f, board_rows[1], app, None, replay.board());
    f.render_widget(
        Paragraph::new(Line::from(format!("move {}/{}", replay.idx, replay.len())).fg(DIM)),
        board_rows[2],
    );
    f.render_widget(
        Paragraph::new(Line::from("← prev  → next  home/end  esc back").fg(DIM)),
        board_rows[3],
    );

    let sans: Vec<&str> = (0..replay.len())
        .filter_map(|i| replay.san_at(i))
        .collect();
    render_movelist(f, cols[1], &sans, replay.idx);
}

// ---------------------------------------------------------------------
// Settings

fn draw_settings(f: &mut Frame, app: &App) {
    let inner = popup(f, "Settings", 56, 13);

    let rows = Layout::vertical([Constraint::Length(1); 8]).split(inner);

    let s = |i: usize| {
        if i == app.settings_sel {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(C::Gray)
        }
    };
    let tc = TimeControl::new(app.config.tc_base_secs, app.config.tc_inc_secs);
    let editing = app.settings_editing_name;

    let name_row = Line::from(vec![
        Span::styled(
            if editing { "▏ Player name  " } else { "  Player name  " },
            s(0),
        ),
        Span::styled(
            format!(
                "{}{}",
                app.config.player_name,
                if editing { "▏" } else { "" }
            ),
            if editing { Style::new().fg(ACCENT) } else { s(0) },
        ),
    ]);
    let piece_row = Line::from(vec![
        Span::styled("  Piece style  ", s(1)),
        Span::styled(
            format!("◂ {} ▸", app.config.piece_style.label()),
            s(1),
        ),
    ]);
    let coord_row = Line::from(vec![
        Span::styled("  Coordinates  ", s(2)),
        Span::styled(
            format!(
                "◂ {} ▸",
                if app.config.show_coordinates { "on" } else { "off" }
            ),
            s(2),
        ),
    ]);
    let hint_row = Line::from(vec![
        Span::styled("  Move hints   ", s(3)),
        Span::styled(
            format!(
                "◂ {} ▸",
                match app.config.show_hints {
                    HintStyle::All => "all legal moves",
                    HintStyle::Cursor => "cursor only",
                    HintStyle::None => "off",
                }
            ),
            s(3),
        ),
    ]);
    let tc_row = Line::from(vec![
        Span::styled("  Default time ", s(4)),
        Span::styled(format!("◂ {} ▸", tc.label()), s(4)),
    ]);
    let flip_row = Line::from(vec![
        Span::styled("  Flip board   ", s(5)),
        Span::styled(
            format!(
                "◂ {} ▸",
                if app.config.flip_board { "on" } else { "off" }
            ),
            s(5),
        ),
    ]);
    let back_row = Line::from(if app.settings_sel == 6 {
        "  ❯ Back"
    } else {
        "    Back"
    })
    .style(s(6));

    f.render_widget(Paragraph::new(name_row), rows[0]);
    f.render_widget(Paragraph::new(piece_row), rows[1]);
    f.render_widget(Paragraph::new(coord_row), rows[2]);
    f.render_widget(Paragraph::new(hint_row), rows[3]);
    f.render_widget(Paragraph::new(tc_row), rows[4]);
    f.render_widget(Paragraph::new(flip_row), rows[5]);
    f.render_widget(Paragraph::new(back_row), rows[6]);
    f.render_widget(
        Paragraph::new(Line::from("←→/⏎ change  esc back  auto-saved").fg(DIM)),
        rows[7],
    );
}

// ---------------------------------------------------------------------
// Help

fn draw_help(f: &mut Frame, app: &App) {
    let inner = popup(f, "Help", 62, 22);

    let text = vec![
        Line::from(Span::styled("Keys", Style::new().fg(ACCENT).bold())),
        Line::from("  ↑ ↓ ← →   move cursor / navigate"),
        Line::from("  Enter      select piece, confirm move, confirm dialog"),
        Line::from("  Esc        back / cancel / open in-game menu"),
        Line::from("  Tab        flip board view"),
        Line::from("  R          refresh player list (lobby)"),
        Line::from("  D          offer draw (game) / delete (history)"),
        Line::from("  Q          quit (menu)"),
        Line::from(""),
        Line::from(Span::styled(
            "Playing over LAN",
            Style::new().fg(ACCENT).bold(),
        )),
        Line::from("  Both players run this program on the same network."),
        Line::from("  LAN Game lists everyone automatically; pick a player to invite."),
        Line::from("  Color is random; finished games are saved to your history."),
        Line::from(""),
        Line::from(Span::styled("Files", Style::new().fg(ACCENT).bold())),
        Line::from(format!(
            "  Config: {}",
            crate::config::Config::path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "n/a".into())
        )),
        Line::from(format!(
            "  Games:  {}",
            app.history
                .as_ref()
                .map(|h| h.dir().display().to_string())
                .unwrap_or_else(|| "n/a".into())
        )),
    ];
    f.render_widget(Paragraph::new(text), inner);
}

// ---------------------------------------------------------------------
// Modals

fn choice_span(sel: usize, idx: usize, label: &str) -> Span<'static> {
    let style = if sel == idx {
        Style::new().fg(ACCENT).bold()
    } else {
        Style::new().fg(DIM)
    };
    Span::styled(format!("[ {label} ]"), style)
}

fn draw_modal(f: &mut Frame, app: &App, modal: &Modal) {
    match modal {
        Modal::Promotion { sel, .. } => {
            let inner = popup(f, "Promote pawn", 46, 7);
            let labels = ["Queen ♕", "Rook ♖", "Bishop ♗", "Knight ♘"];
            let mut line = Line::default();
            for (i, label) in labels.iter().enumerate() {
                if i > 0 {
                    line.push_span(Span::raw("  "));
                }
                line.push_span(choice_span(*sel, i, label));
            }
            f.render_widget(Paragraph::new(line).centered(), inner);
        }
        Modal::GameMenu { sel } => {
            let inner = popup(f, "Menu", 32, 9);
            let items = ["Resume", "Offer draw", "Resign", "Leave game"];
            let lines: Vec<Line> = items
                .iter()
                .enumerate()
                .map(|(i, it)| Line::from(choice_span(*sel, i, it)))
                .collect();
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::GameOver => {
            let Some(sess) = app.session.as_ref() else { return };
            let Some(end) = sess.end.as_ref() else { return };
            let inner = popup(f, "Game over", 46, 7);
            let headline = match end.outcome {
                Outcome::Win(c) => format!("{} wins — {}", sess.name_of(c), end.reason),
                Outcome::Draw => format!("Draw — {}", end.reason),
            };
            let text = vec![
                Line::from(Span::styled(
                    end.outcome.result_string().to_string(),
                    Style::new().fg(ACCENT).bold(),
                ))
                .centered(),
                Line::from(Span::styled(headline, Style::new().fg(C::Gray))).centered(),
                Line::from(""),
                Line::from(Span::styled(
                    "⏎ back to menu  esc view board",
                    Style::new().fg(DIM),
                ))
                .centered(),
            ];
            f.render_widget(Paragraph::new(text), inner);
        }
        Modal::DrawIncoming => {
            let inner = popup(f, "Draw offered", 46, 6);
            let text = vec![
                Line::from("Your opponent offers a draw.").fg(C::Gray).centered(),
                Line::from(""),
                Line::from(vec![
                    choice_span(0, 0, "Accept"),
                    Span::raw("  "),
                    choice_span(1, 1, "Decline"),
                ])
                .centered(),
            ];
            f.render_widget(Paragraph::new(text), inner);
        }
        Modal::IncomingInvite { sel } => {
            let inner = popup(f, "Game invitation", 52, 6);
            let peer = app.invite_peer.clone().unwrap_or_default();
            let text = vec![
                Line::from(Span::styled(
                    format!(
                        "{peer} invites you to a game ({})",
                        app.invite_tc.label()
                    ),
                    Style::new().fg(C::Gray),
                ))
                .centered(),
                Line::from(""),
                Line::from(vec![
                    choice_span(*sel, 0, "Accept"),
                    Span::raw("  "),
                    choice_span(*sel, 1, "Decline"),
                ])
                .centered(),
            ];
            f.render_widget(Paragraph::new(text), inner);
        }
        Modal::InviteWait { peer } => {
            let inner = popup(f, "Waiting", 46, 6);
            let text = vec![
                Line::from(Span::styled(
                    format!("Waiting for {peer} to accept..."),
                    Style::new().fg(C::Gray),
                ))
                .centered(),
                Line::from(Span::styled("esc cancel", Style::new().fg(DIM))).centered(),
            ];
            f.render_widget(Paragraph::new(text), inner);
        }
        Modal::Info { title, body } => {
            let inner = popup(f, title, 46, 6);
            f.render_widget(
                Paragraph::new(Line::from(body.clone()).fg(C::Gray)).centered(),
                inner,
            );
        }
        Modal::LeaveConfirm { sel } => {
            let inner = popup(f, "Leave game?", area_w(f, 46), 6);
            let text = vec![
                Line::from("Leaving counts as resignation.")
                    .fg(C::Gray)
                    .centered(),
                Line::from(""),
                Line::from(vec![
                    choice_span(*sel, 0, "Resign & leave"),
                    Span::raw("  "),
                    choice_span(*sel, 1, "Stay"),
                ])
                .centered(),
            ];
            f.render_widget(Paragraph::new(text), inner);
        }
        Modal::QuitConfirm { sel } => {
            let inner = popup(f, "Quit?", 36, 5);
            let text = vec![Line::from(vec![
                choice_span(*sel, 0, "Quit"),
                Span::raw("  "),
                choice_span(*sel, 1, "Cancel"),
            ])
            .centered()];
            f.render_widget(Paragraph::new(text), inner);
        }
        Modal::DeleteConfirm { sel } => {
            let inner = popup(f, "Delete game?", 42, 6);
            let text = vec![Line::from(vec![
                choice_span(*sel, 0, "Delete"),
                Span::raw("  "),
                choice_span(*sel, 1, "Cancel"),
            ])
            .centered()];
            f.render_widget(Paragraph::new(text), inner);
        }
    }
}

/// Width helper for the LeaveConfirm popup (kept symmetric with others).
fn area_w(_f: &Frame, w: u16) -> u16 {
    w
}
