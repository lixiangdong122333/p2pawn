//! TCP game connections: NDJSON framing over a blocking stream.
//!
//! A reader thread owns the read half and pushes [`GameEvent`]s into a channel
//! consumed by the UI loop; writes happen on the UI thread (they are small and
//! rare). The app also runs an acceptor thread for incoming connections, which
//! are surfaced as [`LanEvent::Incoming`].

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::proto::GameMsg;

/// Events surfaced to the UI loop.
#[derive(Debug)]
pub enum LanEvent {
    /// A peer connected to our listener (before any message was read).
    Incoming(GameConnection),
    /// A message arrived on the active connection.
    Msg(GameMsg),
    /// The active connection died.
    Disconnected(String),
}

pub struct GameConnection {
    writer: TcpStream,
    events: Sender<LanEvent>,
    reader_handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for GameConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameConnection").finish_non_exhaustive()
    }
}

impl GameConnection {
    /// Prepare a connection without starting its reader thread.
    ///
    /// The reader is deliberately NOT started here: for accepted connections
    /// the `Incoming` event must be enqueued before any message from this
    /// connection, so the receiver starts the reader after taking ownership
    /// (see [`GameConnection::start_reader`]).
    fn new_unstarted(
        stream: TcpStream,
        events: Sender<LanEvent>,
    ) -> std::io::Result<GameConnection> {
        // A stream accepted from a non-blocking listener inherits that mode;
        // we do blocking I/O on dedicated threads.
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        let writer = stream.try_clone()?;
        Ok(GameConnection {
            writer,
            events,
            reader_handle: None,
        })
    }

    /// Spawn the reader thread. Must be called exactly once, by whoever
    /// received the `Incoming` event (or immediately after `connect`).
    pub fn start_reader(&mut self) -> std::io::Result<()> {
        let mut reader = BufReader::new(self.writer.try_clone()?);
        let tx = self.events.clone();
        self.reader_handle = Some(thread::spawn(move || {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(LanEvent::Disconnected("connection closed".into()));
                        return;
                    }
                    Ok(_) => {
                        if let Ok(msg) = serde_json::from_str::<GameMsg>(line.trim()) {
                            if tx.send(LanEvent::Msg(msg)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(LanEvent::Disconnected(e.to_string()));
                        return;
                    }
                }
            }
        }));
        Ok(())
    }

    /// Spawn a reader thread for `stream`, forwarding messages to `events`.
    pub fn spawn(stream: TcpStream, events: Sender<LanEvent>) -> std::io::Result<GameConnection> {
        let mut conn = GameConnection::new_unstarted(stream, events)?;
        conn.start_reader()?;
        Ok(conn)
    }

    /// Connect to `addr` (blocking) and spawn its reader thread.
    ///
    /// `connect_timeout` leaves the stream in non-blocking mode on some
    /// platforms; switch it back to blocking before use.
    pub fn connect(
        addr: SocketAddr,
        events: Sender<LanEvent>,
    ) -> std::io::Result<GameConnection> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        stream.set_nonblocking(false)?;
        GameConnection::spawn(stream, events)
    }

    /// Send one message (NDJSON). Errors are surfaced as disconnect events.
    pub fn send(&mut self, msg: &GameMsg) {
        if let Ok(s) = serde_json::to_string(msg) {
            if let Err(e) = self.writer.write_all(s.as_bytes()).and_then(|_| {
                self.writer.write_all(b"\n")?;
                self.writer.flush()
            }) {
                let _ = self.events.send(LanEvent::Disconnected(e.to_string()));
            }
        }
    }

    /// Close the connection. The reader thread is detached (not joined):
    /// once both peers drop their sockets the OS closes the stream and the
    /// thread exits on EOF/error; joining here can deadlock on Windows where
    /// shutdown() also affects cloned handles.
    pub fn close(&mut self) {
        self.reader_handle = None;
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }
}

/// Run a TCP listener in the background, pushing accepted connections to the UI.
pub struct Acceptor {
    pub local_port: u16,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Acceptor {
    /// Bind a listener, preferring [`super::proto::GAME_PORT`] but accepting an
    /// ephemeral port if it is taken (e.g. a second instance on one machine).
    pub fn start(events: Sender<LanEvent>) -> std::io::Result<Acceptor> {
        let listener = match TcpListener::bind(("0.0.0.0", super::proto::GAME_PORT)) {
            Ok(l) => l,
            Err(_) => TcpListener::bind(("0.0.0.0", 0))?,
        };
        let local_port = listener.local_addr()?.port();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = shutdown.clone();
        listener
            .set_nonblocking(true)
            .expect("set_nonblocking on fresh listener");
        let handle = thread::spawn(move || loop {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    // Hand the unstarted connection to the app: it starts the
                    // reader, guaranteeing `Incoming` is seen before any `Msg`
                    // from this connection.
                    if let Ok(conn) = GameConnection::new_unstarted(stream, events.clone()) {
                        if events.send(LanEvent::Incoming(conn)).is_err() {
                            return;
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => return,
            }
        });
        Ok(Acceptor {
            local_port,
            shutdown,
            handle: Some(handle),
        })
    }
}

impl Drop for Acceptor {
    fn drop(&mut self) {
        self.shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
