//! Keyboard handling: routes crossterm key events to screen-specific logic.

use chess::Piece;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, ColorChoice, Modal, Screen};
use crate::game::session::TimeControl;
use crate::net::proto::{GameMsg, PeerStatus};

pub fn handle_key(app: &mut App, key: KeyEvent) {
    if key.kind != crossterm::event::KeyEventKind::Press {
        return;
    }
    // Global: Ctrl+C always quits.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    if let Some(modal) = app.modal.take() {
        if handle_modal_key(app, &modal, key) {
            return;
        }
        app.modal = Some(modal);
    }
    match app.screen {
        Screen::Menu => handle_menu_key(app, key),
        Screen::Lobby => handle_lobby_key(app, key),
        Screen::LocalSetup => handle_setup_key(app, key),
        Screen::Playing => handle_game_key(app, key),
        Screen::History => handle_history_key(app, key),
        Screen::Replay => handle_replay_key(app, key),
        Screen::Settings => handle_settings_key(app, key),
        Screen::Help => handle_help_key(app, key),
    }
}

// Extra App methods used only by key handlers.
impl App {
    fn offer_draw(&mut self) {
        let me = self.session.as_ref().and_then(|s| s.my_color);
        let Some(me) = me else {
            self.toast("draw offers are for LAN games");
            return;
        };
        if let Some(sess) = self.session.as_mut()
            && !sess.is_over()
        {
            sess.offer_draw(me);
            if let Some(conn) = self.conn.as_mut() {
                conn.send(&GameMsg::DrawOffer);
            }
            self.toast("draw offered");
        }
    }

    fn decline_invite(&mut self) {
        if let Some(mut conn) = self.pending_conn.take() {
            conn.send(&GameMsg::Decline {
                reason: "declined".into(),
            });
            conn.close();
        }
        self.modal = None;
    }

    fn delete_history_entry(&mut self) {
        if let Some(hist) = &self.history {
            let entries = hist.list();
            if let Some(e) = entries.get(self.history_sel) {
                let _ = std::fs::remove_file(&e.file);
            }
        }
        let len = self.history_len();
        if self.history_sel >= len.saturating_sub(1) {
            self.history_sel = self.history_sel.saturating_sub(1);
        }
    }
}

// ---------------------------------------------------------------------
// Modal dialogs

/// Returns true if the key was consumed by the modal.
fn handle_modal_key(app: &mut App, modal: &Modal, key: KeyEvent) -> bool {
    match modal {
        Modal::Promotion { from, to, sel } => {
            const PIECES: [Piece; 4] = [Piece::Queen, Piece::Rook, Piece::Bishop, Piece::Knight];
            let (from, to, sel) = (*from, *to, *sel);
            match key.code {
                KeyCode::Left | KeyCode::Up => {
                    app.modal = Some(Modal::Promotion {
                        from,
                        to,
                        sel: (sel + 3) % 4,
                    });
                }
                KeyCode::Right | KeyCode::Down => {
                    app.modal = Some(Modal::Promotion {
                        from,
                        to,
                        sel: (sel + 1) % 4,
                    });
                }
                KeyCode::Enter => {
                    app.modal = Some(Modal::Promotion { from, to, sel });
                    app.attempt_promotion(PIECES[sel]);
                }
                KeyCode::Esc => {
                    app.modal = None;
                    app.selected = None;
                }
                _ => app.modal = Some(Modal::Promotion { from, to, sel }),
            }
            true
        }
        Modal::GameMenu { sel } => {
            const ITEMS: usize = 4; // Resume, Offer draw, Resign, Leave game
            match key.code {
                KeyCode::Up => {
                    app.modal = Some(Modal::GameMenu {
                        sel: (sel + ITEMS - 1) % ITEMS,
                    });
                }
                KeyCode::Down => {
                    app.modal = Some(Modal::GameMenu {
                        sel: (sel + 1) % ITEMS,
                    });
                }
                KeyCode::Esc => app.modal = None,
                KeyCode::Enter => {
                    app.modal = None;
                    match sel {
                        0 => {}
                        1 => app.offer_draw(),
                        2 => app.leave_game(true),
                        _ => app.modal = Some(Modal::LeaveConfirm { sel: 0 }),
                    }
                }
                _ => app.modal = Some(Modal::GameMenu { sel: *sel }),
            }
            true
        }
        Modal::GameOver => match key.code {
            KeyCode::Enter => {
                app.leave_game(false);
                true
            }
            KeyCode::Esc => {
                app.modal = None; // keep viewing the final position
                true
            }
            _ => true,
        },
        Modal::DrawIncoming => {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(sess) = app.session.as_mut() {
                        sess.accept_draw();
                    }
                    if let Some(conn) = app.conn.as_mut() {
                        conn.send(&GameMsg::DrawAccept);
                    }
                    app.modal = None;
                    app.finalize_if_over();
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    if let Some(sess) = app.session.as_mut() {
                        sess.decline_draw();
                    }
                    if let Some(conn) = app.conn.as_mut() {
                        conn.send(&GameMsg::DrawDecline);
                    }
                    app.modal = None;
                }
                _ => {}
            }
            true
        }
        Modal::IncomingInvite { sel } => match key.code {
            KeyCode::Left | KeyCode::Right => {
                app.modal = Some(Modal::IncomingInvite { sel: 1 - *sel });
                true
            }
            KeyCode::Enter => {
                if *sel == 0 {
                    if let Some(conn) = app.pending_conn.as_mut() {
                        conn.send(&GameMsg::Accept);
                    }
                    app.modal = None;
                    app.toast("waiting for the game to start...");
                } else {
                    app.decline_invite();
                }
                true
            }
            KeyCode::Esc => {
                app.decline_invite();
                true
            }
            _ => true,
        },
        Modal::InviteWait { peer } => {
            if key.code == KeyCode::Esc {
                if let Some(conn) = app.conn.as_mut() {
                    conn.send(&GameMsg::Bye);
                    conn.close();
                }
                app.conn = None;
                app.modal = None;
                let _ = peer;
            }
            true
        }
        Modal::Info { .. } => {
            app.modal = None;
            true
        }
        Modal::LeaveConfirm { sel } => match key.code {
            KeyCode::Left | KeyCode::Right => {
                app.modal = Some(Modal::LeaveConfirm { sel: 1 - *sel });
                true
            }
            KeyCode::Esc => {
                app.modal = None;
                true
            }
            KeyCode::Enter => {
                app.modal = None;
                if *sel == 0 {
                    app.leave_game(true);
                }
                true
            }
            _ => true,
        },
        Modal::QuitConfirm { sel } => match key.code {
            KeyCode::Left | KeyCode::Right => {
                app.modal = Some(Modal::QuitConfirm { sel: 1 - *sel });
                true
            }
            KeyCode::Esc => {
                app.modal = None;
                true
            }
            KeyCode::Enter => {
                app.modal = None;
                if *sel == 0 {
                    app.quit = true;
                }
                true
            }
            _ => true,
        },
        Modal::DeleteConfirm { sel } => match key.code {
            KeyCode::Left | KeyCode::Right => {
                app.modal = Some(Modal::DeleteConfirm { sel: 1 - *sel });
                true
            }
            KeyCode::Esc => {
                app.modal = None;
                true
            }
            KeyCode::Enter => {
                app.modal = None;
                if *sel == 0 {
                    app.delete_history_entry();
                }
                true
            }
            _ => true,
        },
    }
}

// ---------------------------------------------------------------------
// Screens

const MENU_LEN: usize = 6;

fn handle_menu_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up => app.menu_sel = (app.menu_sel + MENU_LEN - 1) % MENU_LEN,
        KeyCode::Down => app.menu_sel = (app.menu_sel + 1) % MENU_LEN,
        KeyCode::Enter | KeyCode::Char(' ') => match app.menu_sel {
            0 => {
                app.setup_tc = app.config.default_tc();
                app.screen = Screen::LocalSetup;
                app.setup_sel = 0;
            }
            1 => {
                app.screen = Screen::Lobby;
                app.lobby_sel = 0;
                app.lobby_status.clear();
                app.discovery.refresh();
            }
            2 => {
                app.screen = Screen::History;
                app.history_sel = 0;
            }
            3 => {
                app.screen = Screen::Settings;
                app.settings_sel = 0;
                app.settings_editing_name = false;
            }
            4 => app.screen = Screen::Help,
            _ => app.modal = Some(Modal::QuitConfirm { sel: 0 }),
        },
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
            app.modal = Some(Modal::QuitConfirm { sel: 0 });
        }
        _ => {}
    }
}

fn handle_lobby_key(app: &mut App, key: KeyEvent) {
    let peers = app.discovery.peers().len();
    match key.code {
        KeyCode::Up => {
            if peers > 0 {
                app.lobby_sel = (app.lobby_sel + peers - 1) % peers;
            }
        }
        KeyCode::Down => {
            if peers > 0 {
                app.lobby_sel = (app.lobby_sel + 1) % peers;
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => app.discovery.refresh(),
        KeyCode::Enter => {
            let list = app.discovery.peers();
            if let Some(peer) = list.get(app.lobby_sel) {
                if peer.status == PeerStatus::Playing {
                    app.lobby_status = format!("{} is already playing", peer.name);
                    return;
                }
                let addr = peer.tcp_addr;
                let name = app.config.player_name.clone();
                let tc = app.config.default_tc();
                app.invite_peer = Some(peer.name.clone());
                app.invite_tc = tc;
                match crate::net::conn::GameConnection::connect(addr, app.lan_tx.clone()) {
                    Ok(mut conn) => {
                        conn.send(&GameMsg::Invite {
                            name,
                            tc_secs: tc.base.as_secs() as u32,
                            tc_inc: tc.increment.as_secs() as u32,
                        });
                        app.conn = Some(conn);
                        app.modal = Some(Modal::InviteWait {
                            peer: peer.name.clone(),
                        });
                    }
                    Err(e) => app.lobby_status = format!("connect failed: {e}"),
                }
            }
        }
        KeyCode::Esc => app.screen = Screen::Menu,
        _ => {}
    }
}

fn handle_setup_key(app: &mut App, key: KeyEvent) {
    const ROWS: usize = 3; // color, time control, start
    match key.code {
        KeyCode::Up => app.setup_sel = (app.setup_sel + ROWS - 1) % ROWS,
        KeyCode::Down => app.setup_sel = (app.setup_sel + 1) % ROWS,
        KeyCode::Left | KeyCode::Right => {
            let dir: i32 = if matches!(key.code, KeyCode::Right) {
                1
            } else {
                -1
            };
            match app.setup_sel {
                0 => {
                    let idx = ColorChoice::ALL
                        .iter()
                        .position(|c| *c == app.setup_color)
                        .unwrap_or(0) as i32;
                    let n = ColorChoice::ALL.len() as i32;
                    app.setup_color = ColorChoice::ALL[((idx + dir + n) % n) as usize];
                }
                1 => {
                    let presets = TimeControl::PRESETS;
                    let idx = presets.iter().position(|t| *t == app.setup_tc).unwrap_or(0) as i32;
                    let n = presets.len() as i32;
                    app.setup_tc = presets[((idx + dir + n) % n) as usize];
                }
                _ => {}
            }
        }
        KeyCode::Enter => match app.setup_sel {
            0 | 1 => {
                // Rows 0/1 also cycle with Enter, like Left/Right.
                let right = KeyEvent::from(KeyCode::Right);
                handle_setup_key(app, right);
            }
            _ => app.start_local_game(),
        },
        KeyCode::Esc => app.screen = Screen::Menu,
        _ => {}
    }
}

fn handle_game_key(app: &mut App, key: KeyEvent) {
    let over = app.session.as_ref().is_some_and(|s| s.is_over());
    match key.code {
        KeyCode::Up => app.move_cursor(0, 1),
        KeyCode::Down => app.move_cursor(0, -1),
        KeyCode::Left => app.move_cursor(-1, 0),
        KeyCode::Right => app.move_cursor(1, 0),
        KeyCode::Enter => {
            if over {
                return;
            }
            let Some(cursor) = app.cursor else { return };
            if let Some(sel) = app.selected {
                if sel == cursor {
                    app.selected = None; // toggle off
                } else {
                    app.attempt_move(cursor);
                }
            } else if app
                .session
                .as_ref()
                .is_some_and(|s| s.is_my_turn() && !s.legal_targets(cursor).is_empty())
            {
                app.selected = Some(cursor);
            }
        }
        KeyCode::Esc => {
            if over {
                app.leave_game(false);
            } else {
                app.modal = Some(Modal::GameMenu { sel: 0 });
            }
        }
        KeyCode::Tab => app.view_flip = !app.view_flip,
        KeyCode::Char('d') | KeyCode::Char('D') => app.offer_draw(),
        _ => {}
    }
}

fn handle_history_key(app: &mut App, key: KeyEvent) {
    let len = app.history_len();
    match key.code {
        KeyCode::Up => {
            if len > 0 {
                app.history_sel = (app.history_sel + len - 1) % len;
            }
        }
        KeyCode::Down => {
            if len > 0 {
                app.history_sel = (app.history_sel + 1) % len;
            }
        }
        KeyCode::Enter => {
            if let Some(hist) = &app.history {
                let entries = hist.list();
                if let Some(e) = entries.get(app.history_sel)
                    && let Some(replay) = hist.load(e)
                {
                    app.replay = Some(replay);
                    app.screen = Screen::Replay;
                }
            }
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            if len > 0 {
                app.modal = Some(Modal::DeleteConfirm { sel: 0 });
            }
        }
        KeyCode::Esc => app.screen = Screen::Menu,
        _ => {}
    }
}

fn handle_replay_key(app: &mut App, key: KeyEvent) {
    let Some(replay) = app.replay.as_mut() else {
        app.screen = Screen::History;
        return;
    };
    match key.code {
        KeyCode::Left | KeyCode::Backspace => {
            replay.prev();
        }
        KeyCode::Right | KeyCode::Char(' ') | KeyCode::Enter => {
            replay.advance();
        }
        KeyCode::Home => replay.first(),
        KeyCode::End => replay.last(),
        KeyCode::Esc => {
            app.replay = None;
            app.screen = Screen::History;
        }
        _ => {}
    }
}

fn handle_settings_key(app: &mut App, key: KeyEvent) {
    const ROWS: usize = 7;
    if app.settings_editing_name {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                app.settings_editing_name = false;
                let _ = app.config.save();
            }
            KeyCode::Backspace => {
                app.config.player_name.pop();
            }
            KeyCode::Char(c) if !c.is_control() && app.config.player_name.chars().count() < 20 => {
                app.config.player_name.push(c);
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Up => app.settings_sel = (app.settings_sel + ROWS - 1) % ROWS,
        KeyCode::Down => app.settings_sel = (app.settings_sel + 1) % ROWS,
        KeyCode::Enter | KeyCode::Left | KeyCode::Right => match app.settings_sel {
            0 => app.settings_editing_name = true,
            1 => app.config.toggle_piece_style(),
            2 => app.config.show_coordinates = !app.config.show_coordinates,
            3 => app.config.toggle_hints(),
            4 => {
                let presets = TimeControl::PRESETS;
                let idx = presets
                    .iter()
                    .position(|t| {
                        t.base.as_secs() == app.config.tc_base_secs
                            && t.increment.as_secs() == app.config.tc_inc_secs
                    })
                    .unwrap_or(0);
                let next = presets[(idx + 1) % presets.len()];
                app.config.tc_base_secs = next.base.as_secs();
                app.config.tc_inc_secs = next.increment.as_secs();
            }
            5 => app.config.flip_board = !app.config.flip_board,
            _ => app.screen = Screen::Menu,
        },
        KeyCode::Esc => app.screen = Screen::Menu,
        _ => {}
    }
    if app.screen == Screen::Settings {
        let _ = app.config.save();
    }
}

fn handle_help_key(app: &mut App, key: KeyEvent) {
    if matches!(
        key.code,
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Enter
    ) {
        app.screen = Screen::Menu;
    }
}
