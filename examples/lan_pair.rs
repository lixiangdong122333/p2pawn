//! Headless multi-process LAN end-to-end driver.
//!
//! Runs the *real* application state machine (`App` + `input::handle_key` +
//! `net`) without a terminal, driven by synthetic key presses, so two
//! instances on one machine can play a full game over actual UDP discovery
//! and TCP.
//!
//! Usage:
//!
//! ```text
//! lan_pair host  <name>          # wait for an invite, accept, play black/white
//! lan_pair client <name>         # discover peers, invite, play a full game
//! lan_pair client-quit <name>    # invite, play two moves, then vanish
//! ```
//!
//! The game is scripted as fool's mate (1. f3 e5 2. g4 Qh4#), so whichever
//! color a side gets, it plays its own moves and the game ends in checkmate.
//! On completion the driver prints the result, the SAN move list, and the
//! saved PGN, then exits 0. On timeout it exits 1.

use std::time::{Duration, Instant};

use chess::{Color, Square};
use crossterm::event::{KeyCode, KeyEvent};

use p2pawn::app::{App, Modal, Screen};
use p2pawn::input::handle_key;

/// Fool's mate in UCI, in ply order.
const SCRIPT: [&str; 4] = ["f2f3", "e7e5", "g2g4", "d8h4"];

fn press(app: &mut App, code: KeyCode) {
    handle_key(app, KeyEvent::from(code));
}

fn sq(x: &str) -> Square {
    let b = x.as_bytes();
    Square::make_square(
        chess::Rank::from_index((b[1] - b'1') as usize),
        chess::File::from_index((b[0] - b'a') as usize),
    )
}

fn modal_label(m: &Option<Modal>) -> &'static str {
    match m {
        None => "none",
        Some(Modal::Promotion { .. }) => "promotion",
        Some(Modal::GameMenu { .. }) => "game-menu",
        Some(Modal::GameOver) => "game-over",
        Some(Modal::DrawIncoming) => "draw-incoming",
        Some(Modal::IncomingInvite { .. }) => "incoming-invite",
        Some(Modal::InviteWait { .. }) => "invite-wait",
        Some(Modal::Info { .. }) => "info",
        Some(Modal::LeaveConfirm { .. }) => "leave-confirm",
        Some(Modal::QuitConfirm { .. }) => "quit-confirm",
        Some(Modal::DeleteConfirm { .. }) => "delete-confirm",
    }
}

/// The move this side should play now, if any.
fn scripted_move(my: Color, plies: usize) -> Option<&'static str> {
    match (my, plies) {
        (Color::White, 0) => Some(SCRIPT[0]),
        (Color::Black, 1) => Some(SCRIPT[1]),
        (Color::White, 2) => Some(SCRIPT[2]),
        (Color::Black, 3) => Some(SCRIPT[3]),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (mode, name) = match (args.get(1).map(String::as_str), args.get(2)) {
        (Some(m @ ("host" | "client" | "client-quit")), Some(n)) => {
            (m.to_string(), n.clone())
        }
        _ => {
            eprintln!("usage: lan_pair <host|client|client-quit> <name>");
            std::process::exit(2);
        }
    };
    // Safe: no other threads exist yet at this point in main.
    unsafe { std::env::set_var("P2PAWN_NAME", &name) };

    let mut app = App::new().expect("start app (discovery + listener)");

    // Menu -> LAN Game, exactly as a user would.
    app.menu_sel = 1;
    press(&mut app, KeyCode::Enter);
    assert!(
        matches!(app.screen, Screen::Lobby),
        "should be in the lobby now"
    );
    println!("[{name}] in lobby, waiting for peers...");

    let deadline = Instant::now() + Duration::from_secs(45);
    let mut invited = false;
    let mut accepted = false;
    let mut next_move_at = Instant::now();
    loop {
        app.poll_lan();
        if Instant::now() > deadline {
            eprintln!(
                "[{name}] TIMEOUT (screen={:?}, modal={}, lobby_status={:?})",
                app.screen,
                modal_label(&app.modal),
                app.lobby_status
            );
            std::process::exit(1);
        }

        match mode.as_str() {
            "host" => {
                if matches!(app.modal, Some(Modal::IncomingInvite { .. })) && !accepted {
                    accepted = true;
                    let peer = app.invite_peer.clone().unwrap_or_default();
                    println!("[{name}] invite from {peer} -> accepting");
                    press(&mut app, KeyCode::Enter);
                }
            }
            "client" | "client-quit" => {
                if !invited
                    && matches!(app.screen, Screen::Lobby)
                    && app.modal.is_none()
                    && app.conn.is_none()
                    && !app.discovery.peers().is_empty()
                {
                    let peer = app.discovery.peers().remove(0);
                    println!(
                        "[{name}] discovered {} ({}) at {} -> inviting",
                        peer.name,
                        peer.status.label(),
                        peer.tcp_addr
                    );
                    press(&mut app, KeyCode::Enter);
                    invited = true;
                }
            }
            _ => unreachable!(),
        }

        // Play our scripted move when it is our turn. P2PAWN_SLOW_MS
        // inserts a delay before each move (to keep a game running long
        // enough for other scenarios to interact with it).
        if app
            .session
            .as_ref()
            .is_some_and(|s| !s.is_over() && s.is_my_turn())
        {
            let s = app.session.as_ref().unwrap();
            let (my, plies) = (s.my_color.unwrap(), s.moves.len());
            let slow_ms: u64 = std::env::var("P2PAWN_SLOW_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let move_due = Instant::now() >= next_move_at;
            if move_due {
                if let Some(uci) = scripted_move(my, plies) {
                    app.selected = Some(sq(&uci[..2]));
                    app.attempt_move(sq(&uci[2..]));
                    println!("[{name}] played {uci} (ply {})", plies + 1);
                    next_move_at = Instant::now() + Duration::from_millis(slow_ms);
                }
            }
        }

        // Scenario 2: vanish mid-game after two plies.
        if mode == "client-quit" && app.session.as_ref().is_some_and(|s| s.moves.len() >= 2) {
            println!("[{name}] quitting without goodbye (simulating crash)...");
            std::process::exit(0);
        }

        // Done?
        if app.session.as_ref().is_some_and(|s| s.is_over()) {
            let s = app.session.as_ref().unwrap();
            let end = s.end.as_ref().unwrap();
            let sans: Vec<&str> = s.moves.iter().map(|m| m.san.as_str()).collect();
            println!(
                "[{name}] GAME OVER: {} ({})  my color: {:?}",
                end.outcome.result_string(),
                end.reason,
                s.my_color
            );
            println!("[{name}] moves: {}", sans.join(" "));
            if let Some(hist) = &app.history {
                let mut newest: Option<(String, std::path::PathBuf, String)> = None;
                for e in hist.list() {
                    if let Ok(text) = std::fs::read_to_string(&e.file) {
                        let better = newest
                            .as_ref()
                            .is_none_or(|(k, _, _)| e.stem > *k);
                        if better {
                            newest = Some((e.stem.clone(), e.file.clone(), text));
                        }
                    }
                }
                if let Some((_, path, text)) = newest {
                    println!("[{name}] PGN saved to {}:", path.display());
                    for line in text.lines() {
                        println!("    {line}");
                    }
                }
            }
            std::process::exit(0);
        }

        std::thread::sleep(Duration::from_millis(30));
    }
}
