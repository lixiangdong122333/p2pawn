//! Application state machine: screens, input handling, LAN orchestration.

use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use chess::{Color, Piece, Square};

use crate::config::{Config, HintStyle};
use crate::game::history::History;
use crate::game::pgn::Replay;
use crate::game::session::{Session, TimeControl};
use crate::net::conn::{Acceptor, GameConnection, LanEvent};
use crate::net::discovery::Discovery;
use crate::net::proto::{GameMsg, PeerStatus};
use crate::util::now_iso;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Menu,
    Lobby,
    LocalSetup,
    Playing,
    History,
    Replay,
    Settings,
    Help,
}

/// Color choice for the local setup screen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    White,
    Black,
    Random,
}

impl ColorChoice {
    pub const ALL: [ColorChoice; 3] = [
        ColorChoice::White,
        ColorChoice::Black,
        ColorChoice::Random,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ColorChoice::White => "White",
            ColorChoice::Black => "Black",
            ColorChoice::Random => "Random",
        }
    }
}

/// Modal dialog rendered on top of the current screen.
pub enum Modal {
    /// Promotion piece picker.
    Promotion { from: Square, to: Square, sel: usize },
    /// In-game menu (Esc).
    GameMenu { sel: usize },
    /// Game over summary.
    GameOver,
    /// Opponent offers a draw.
    DrawIncoming,
    /// Someone invited us; we hold the connection until they answer or we decide.
    IncomingInvite { sel: usize },
    /// We invited someone; waiting for their answer.
    InviteWait { peer: String },
    /// Invite was declined / connection failed.
    Info { title: String, body: String },
    /// Leave the game (counts as resignation if still running).
    LeaveConfirm { sel: usize },
    /// Quit the program.
    QuitConfirm { sel: usize },
    /// Delete a history entry.
    DeleteConfirm { sel: usize },
}

pub const MENU_ITEMS: [&str; 6] = [
    "Local Game",
    "LAN Game",
    "Game History",
    "Settings",
    "Help",
    "Quit",
];

pub struct App {
    pub config: Config,
    pub discovery: Discovery,
    /// Kept alive so the game listener runs as long as the app does.
    #[allow(dead_code)]
    pub acceptor: Acceptor,
    pub lan_tx: Sender<LanEvent>,
    pub lan_rx: Receiver<LanEvent>,
    pub history: Option<History>,

    pub screen: Screen,
    pub quit: bool,

    // Menu / lists
    pub menu_sel: usize,
    pub lobby_sel: usize,
    pub lobby_status: String,
    pub history_sel: usize,
    pub settings_sel: usize,
    pub settings_editing_name: bool,

    // Local setup
    pub setup_sel: usize,
    pub setup_color: ColorChoice,
    pub setup_tc: TimeControl,

    // Game
    pub session: Option<Session>,
    pub conn: Option<GameConnection>,
    pub cursor: Option<Square>,
    pub selected: Option<Square>,
    pub view_flip: bool,
    pub started_iso: String,
    pub game_saved: bool,
    /// Pending incoming connection (before an Invite has been read).
    pub pending_conn: Option<GameConnection>,
    /// Brief status message shown on the game screen.
    pub toast: Option<(String, Instant)>,

    // Replay
    pub replay: Option<Replay>,

    // LAN invite handshake state
    pub invite_peer: Option<String>,
    pub invite_tc: TimeControl,

    pub modal: Option<Modal>,
}

impl App {
    pub fn new() -> std::io::Result<App> {
        let config = Config::load();
        let (lan_tx, lan_rx) = std::sync::mpsc::channel();
        let acceptor = Acceptor::start(lan_tx.clone())?;
        let discovery = Discovery::start(config.player_name.clone(), acceptor.local_port)?;
        let history = History::open_default();
        Ok(App {
            config,
            discovery,
            acceptor,
            lan_tx,
            lan_rx,
            history,
            screen: Screen::Menu,
            quit: false,
            menu_sel: 0,
            lobby_sel: 0,
            lobby_status: String::new(),
            history_sel: 0,
            settings_sel: 0,
            settings_editing_name: false,
            setup_sel: 0,
            setup_color: ColorChoice::White,
            setup_tc: TimeControl::BASE_5_0,
            session: None,
            conn: None,
            cursor: Some(Square::E2),
            selected: None,
            view_flip: false,
            started_iso: String::new(),
            game_saved: false,
            pending_conn: None,
            toast: None,
            replay: None,
            invite_peer: None,
            invite_tc: TimeControl::BASE_5_0,
            modal: None,
        })
    }

    // ------------------------------------------------------------------
    // Helpers

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    pub fn view_flipped(&self) -> bool {
        // In a LAN game, black sees the board from their side by default;
        // the config flip always applies on top of that.
        let lan_black = self
            .session
            .as_ref()
            .and_then(|s| s.my_color)
            .is_some_and(|c| c == Color::Black);
        (lan_black || self.config.flip_board) ^ self.view_flip
    }

    fn history_entries(&self) -> usize {
        self.history.as_ref().map(|h| h.list().len()).unwrap_or(0)
    }

    /// Number of stored games (public for input handlers).
    pub fn history_len(&self) -> usize {
        self.history_entries()
    }

    /// Move the cursor visually, taking the current orientation into account.
    pub fn move_cursor(&mut self, df: i32, dr: i32) {
        let flip = self.view_flipped();
        // Visual deltas to board deltas.
        let (bf, br) = (df * if flip { -1 } else { 1 }, dr * if flip { -1 } else { 1 });
        if let Some(sq) = self.cursor {
            let f = sq.get_file().to_index() as i32 + bf;
            let r = sq.get_rank().to_index() as i32 + br;
            if (0..8).contains(&f) && (0..8).contains(&r) {
                self.cursor = Some(Square::make_square(
                    chess::Rank::from_index(r as usize),
                    chess::File::from_index(f as usize),
                ));
            }
        }
    }

    /// Legal destinations of the selected piece (for hints).
    pub fn selected_targets(&self) -> Vec<Square> {
        match (&self.session, self.selected, self.config.show_hints) {
            (Some(s), Some(sq), HintStyle::All) if !s.is_over() => s.legal_targets(sq),
            _ => Vec::new(),
        }
    }

    pub fn last_move_squares(&self) -> Option<(Square, Square)> {
        self.session.as_ref().and_then(|s| {
            s.moves
                .last()
                .and_then(|m| parse_uci_squares(&m.uci))
        })
    }

    /// Square of the king that is currently in check, if any.
    pub fn check_square(&self) -> Option<Square> {
        let s = self.session.as_ref()?;
        let board = s.board();
        (board.checkers().popcnt() > 0).then(|| board.king_square(board.side_to_move()))
    }

    // ------------------------------------------------------------------
    // Game lifecycle

    pub fn start_local_game(&mut self) {
        let color = match self.setup_color {
            ColorChoice::White => Some(Color::White),
            ColorChoice::Black => Some(Color::Black),
            ColorChoice::Random => None,
        };
        let name = self.config.player_name.clone();
        let (white, black) = match color {
            Some(Color::White) => (name, "Black".to_string()),
            Some(Color::Black) => ("White".to_string(), name),
            None => (name, "Friend".to_string()),
        };
        let mut s = Session::new(white, black, None, self.setup_tc);
        s.clock.start(Color::White);
        self.session = Some(s);
        self.started_iso = now_iso();
        self.game_saved = false;
        self.cursor = Some(Square::E2);
        self.selected = None;
        self.view_flip = false;
        self.screen = Screen::Playing;
    }

    /// Start a LAN game as the initiator (we just got Accept).
    fn start_lan_game_initiator(&mut self, peer_name: &str, tc: TimeControl) {
        let you_white = rand_bool();
        let my = self.config.player_name.clone();
        let (white_name, black_name) = if you_white {
            (my.clone(), peer_name.to_string())
        } else {
            (peer_name.to_string(), my.clone())
        };
        if let Some(conn) = self.conn.as_mut() {
            conn.send(&GameMsg::Start {
                white_name: white_name.clone(),
                black_name: black_name.clone(),
                you_are_white: !you_white,
                tc_secs: tc.base.as_secs() as u32,
                tc_inc: tc.increment.as_secs() as u32,
            });
        }
        let mut s = Session::new(white_name, black_name, Some(if you_white { Color::White } else { Color::Black }), tc);
        s.clock.start(Color::White);
        self.session = Some(s);
        self.started_iso = now_iso();
        self.game_saved = false;
        self.cursor = Some(Square::E2);
        self.selected = None;
        self.view_flip = false;
        self.discovery.set_status(PeerStatus::Playing);
        self.screen = Screen::Playing;
    }

    /// Start a LAN game as the invited side (we just got Start).
    fn start_lan_game_guest(&mut self, white_name: String, black_name: String, you_white: bool, tc: TimeControl) {
        let mut s = Session::new(
            white_name,
            black_name,
            Some(if you_white { Color::White } else { Color::Black }),
            tc,
        );
        s.clock.start(Color::White);
        self.session = Some(s);
        self.started_iso = now_iso();
        self.game_saved = false;
        self.cursor = Some(Square::E7);
        self.selected = None;
        self.view_flip = false;
        self.discovery.set_status(PeerStatus::Playing);
        self.screen = Screen::Playing;
    }

    /// Send our move to the opponent, with our remaining clock time.
    fn send_move(&mut self, uci: &str) {
        let mover = self.session.as_ref().map(|s| !s.side_to_move());
        if let (Some(conn), Some(mover), Some(s)) = (self.conn.as_mut(), mover, self.session.as_ref()) {
            conn.send(&GameMsg::Move {
                uci: uci.to_string(),
                clock_ms: s.clock.remaining(mover).as_millis() as u64,
            });
        }
    }

    /// Apply the cursor move (Enter on a target square).
    pub fn attempt_move(&mut self, to: Square) {
        let Some(sess) = self.session.as_mut() else { return };
        let Some(from) = self.selected else { return };
        if sess.is_over() || !sess.is_my_turn() {
            return;
        }
        if !sess.legal_targets(from).contains(&to) {
            return;
        }
        if sess.needs_promotion(from, to) {
            self.modal = Some(Modal::Promotion { from, to, sel: 0 });
            return;
        }
        if let Some(rec) = sess.try_move(from, to, None) {
            self.selected = None;
            self.send_move(&rec.uci);
            self.after_move();
        }
    }

    pub fn attempt_promotion(&mut self, piece: Piece) {
        let Some(Modal::Promotion { from, to, .. }) = self.modal.take() else { return };
        let Some(sess) = self.session.as_mut() else { return };
        if let Some(rec) = sess.try_move(from, to, Some(piece)) {
            self.selected = None;
            self.send_move(&rec.uci);
            self.after_move();
        }
    }

    /// End-of-game bookkeeping (save, status, modal) after any move.
    fn after_move(&mut self) {
        self.finalize_if_over();
        if self.session.as_ref().is_some_and(|s| !s.is_over()) {
            self.maybe_open_draw_modal();
        }
    }

    fn maybe_open_draw_modal(&mut self) {
        let offered = self.session.as_ref().and_then(|s| s.draw_offered_by);
        let my = self.session.as_ref().and_then(|s| s.my_color);
        if let (Some(by), Some(me)) = (offered, my) {
            if by != me {
                self.modal = Some(Modal::DrawIncoming);
            }
        }
    }

    /// Save the game and update LAN status once it has ended.
    pub fn finalize_if_over(&mut self) {
        if !self.session.as_ref().is_some_and(|s| s.is_over()) || self.game_saved {
            return;
        }
        self.game_saved = true;
        self.discovery.set_status(PeerStatus::Ready);
        if let (Some(sess), Some(hist)) = (&self.session, &self.history) {
            match hist.save(sess, &self.started_iso) {
                Ok(path) => {
                    let shown = path.display().to_string();
                    self.toast(format!("saved: {shown}"));
                }
                Err(e) => self.toast(format!("save failed: {e}")),
            }
        }
        self.modal = Some(Modal::GameOver);
    }

    /// Leave the game screen; resigning if it is still running.
    pub fn leave_game(&mut self, resign: bool) {
        if let Some(sess) = self.session.as_mut() {
            if resign && !sess.is_over() {
                let me = sess.my_color;
                if let Some(me) = me {
                    sess.resign(me);
                    if let Some(conn) = self.conn.as_mut() {
                        conn.send(&GameMsg::Resign);
                    }
                }
            }
        }
        if let Some(conn) = self.conn.as_mut() {
            conn.send(&GameMsg::Bye);
            conn.close();
        }
        self.conn = None;
        if let Some(pc) = self.pending_conn.take() {
            let mut pc = pc;
            pc.close();
        }
        self.session = None;
        self.modal = None;
        self.selected = None;
        self.discovery.set_status(PeerStatus::Ready);
        self.screen = Screen::Menu;
        self.menu_sel = 0;
    }

    // ------------------------------------------------------------------
    // LAN events

    pub fn poll_lan(&mut self) {
        while let Ok(ev) = self.lan_rx.try_recv() {
            match ev {
                LanEvent::Incoming(conn) => self.handle_incoming(conn),
                LanEvent::Msg(msg) => self.handle_msg(msg),
                LanEvent::Disconnected(why) => self.handle_disconnect(why),
            }
        }
        // Clock bookkeeping for the running game.
        if let Some(sess) = self.session.as_mut() {
            sess.clock.tick();
            if let Some(flagged) = sess.clock.flagged() {
                let mine = sess.my_color == Some(flagged);
                sess.timeout(flagged);
                if mine {
                    if let Some(conn) = self.conn.as_mut() {
                        conn.send(&GameMsg::Timeout);
                    }
                }
                self.finalize_if_over();
            }
        }
        // Expire stale toast.
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed().as_secs() >= 6)
        {
            self.toast = None;
        }
    }

    fn handle_incoming(&mut self, conn: GameConnection) {
        if self.session.is_some() {
            // Busy: decline politely and drop the connection.
            let mut conn = conn;
            conn.send(&GameMsg::Decline {
                reason: "busy".into(),
            });
            conn.close();
            return;
        }
        if self.pending_conn.is_some() {
            let mut old = self.pending_conn.take().unwrap();
            old.send(&GameMsg::Decline {
                reason: "busy".into(),
            });
            old.close();
        }
        self.pending_conn = Some(conn);
        // Start reading now that we own the connection; the Invite message
        // will arrive via LanEvent::Msg (guaranteed after this Incoming).
        if let Some(pc) = self.pending_conn.as_mut() {
            let _ = pc.start_reader();
        }
    }

    fn handle_disconnect(&mut self, why: String) {
        if self.session.is_some() {
            // Opponent vanished mid-game: we win by abandonment.
            if let Some(conn) = self.conn.take() {
                let mut conn = conn;
                conn.close();
            }
            if let Some(sess) = self.session.as_mut() {
                if !sess.is_over() {
                    let me = sess.my_color.unwrap_or(Color::White);
                    sess.abandoned(me);
                }
            }
            self.finalize_if_over();
        } else {
            // Connection died during the invite handshake.
            if self.modal.is_some() {
                self.modal = None;
            }
            self.pending_conn = None;
            self.conn = None;
            match self.screen {
                Screen::Lobby => {
                    self.lobby_status = format!("connection failed: {why}");
                }
                _ => {}
            }
        }
    }

    fn handle_msg(&mut self, msg: GameMsg) {
        // An Invite arrives on the pending connection.
        if let GameMsg::Invite { name, tc_secs, tc_inc } = &msg {
            if self.pending_conn.is_some() && self.session.is_none() {
                self.modal = Some(Modal::IncomingInvite { sel: 0 });
                self.invite_peer = Some(name.clone());
                self.invite_tc = TimeControl::new(*tc_secs as u64, *tc_inc as u64);
                return;
            }
        }

        match msg {
            GameMsg::Accept => {
                if matches!(self.modal, Some(Modal::InviteWait { .. })) {
                    let peer = match self.invite_peer.clone() {
                        Some(p) => p,
                        None => return,
                    };
                    let tc = self.invite_tc;
                    self.modal = None;
                    self.start_lan_game_initiator(&peer, tc);
                }
            }
            GameMsg::Decline { reason } => {
                if matches!(self.modal, Some(Modal::InviteWait { .. })) {
                    self.conn = None;
                    self.modal = Some(Modal::Info {
                        title: "Invite declined".into(),
                        body: format!("Reason: {reason}"),
                    });
                    self.screen = Screen::Lobby;
                }
            }
            GameMsg::Start { white_name, black_name, you_are_white, tc_secs, tc_inc } => {
                if self.session.is_none() && self.pending_conn.is_some() {
                    self.conn = self.pending_conn.take();
                    self.modal = None;
                    let tc = TimeControl::new(tc_secs as u64, tc_inc as u64);
                    self.start_lan_game_guest(white_name, black_name, you_are_white, tc);
                }
            }
            GameMsg::Move { uci, clock_ms } => {
                if let Some(sess) = self.session.as_mut() {
                    if sess.apply_remote_uci(&uci, clock_ms).is_some() {
                        self.after_move();
                    }
                }
            }
            GameMsg::DrawOffer => {
                if let Some(sess) = self.session.as_mut() {
                    if !sess.is_over() {
                        let them = sess.my_color.map(|c| !c).unwrap_or(Color::Black);
                        sess.offer_draw(them);
                        self.maybe_open_draw_modal();
                    }
                }
            }
            GameMsg::DrawAccept => {
                if let Some(sess) = self.session.as_mut() {
                    sess.accept_draw();
                }
                self.finalize_if_over();
            }
            GameMsg::DrawDecline => {
                if let Some(sess) = self.session.as_mut() {
                    sess.decline_draw();
                }
                self.toast("draw declined");
            }
            GameMsg::Resign => {
                if let Some(sess) = self.session.as_mut() {
                    let me = sess.my_color.unwrap_or(Color::White);
                    sess.resign(!me);
                }
                self.finalize_if_over();
            }
            GameMsg::Timeout => {
                // Opponent flagged themselves.
                if let Some(sess) = self.session.as_mut() {
                    let me = sess.my_color.unwrap_or(Color::White);
                    sess.timeout(!me);
                }
                self.finalize_if_over();
            }
            GameMsg::Bye => {
                if self.session.is_some() {
                    if let Some(conn) = self.conn.take() {
                        let mut conn = conn;
                        conn.close();
                    }
                    if let Some(sess) = self.session.as_mut() {
                        if !sess.is_over() {
                            let me = sess.my_color.unwrap_or(Color::White);
                            sess.abandoned(me);
                        }
                    }
                    self.finalize_if_over();
                }
            }
            GameMsg::Invite { .. } => {
                // Invite while busy/none pending: declined in handle_incoming;
                // a stray Invite on the active conn is ignored.
            }
        }
    }
}

fn parse_uci_squares(uci: &str) -> Option<(Square, Square)> {
    let b = uci.as_bytes();
    if b.len() < 4 {
        return None;
    }
    let sq = |c: u8, r: u8| -> Option<Square> {
        if (b'a'..=b'h').contains(&c) && (b'1'..=b'8').contains(&r) {
            Some(Square::make_square(
                chess::Rank::from_index((r - b'1') as usize),
                chess::File::from_index((c - b'a') as usize),
            ))
        } else {
            None
        }
    };
    Some((sq(b[0], b[1])?, sq(b[2], b[3])?))
}

fn rand_bool() -> bool {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() & 1 == 1)
        .unwrap_or(true)
}
