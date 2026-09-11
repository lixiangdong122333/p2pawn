//! LAN peer discovery via UDP broadcast.
//!
//! Each running client periodically broadcasts a JSON `Beacon` on the discovery
//! port (plus every interface's subnet broadcast address) and listens for
//! beacons from other clients. There is no server; the beacon carries the TCP
//! port the peer listens on, which is what the lobby connects to.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use if_addrs::get_if_addrs;

use super::proto::{
    BEACON_INTERVAL_MS, BEACON_PORT, Beacon, PEER_TIMEOUT_MS, PROTO_VERSION, PeerStatus,
};

/// A peer visible on the LAN.
#[derive(Clone, Debug)]
pub struct Peer {
    /// Discovery instance id (used to ignore our own beacons).
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub status: PeerStatus,
    /// Address to dial for a game connection.
    pub tcp_addr: SocketAddr,
    pub last_seen: Instant,
}

/// Shared state for the discovery threads.
struct Shared {
    own_id: String,
    peers: Mutex<HashMap<String, Peer>>,
    status: AtomicU8, // PeerStatus as u8
    refresh: AtomicBool,
    shutdown: Arc<AtomicBool>,
}

pub const STATUS_READY: u8 = 0;
pub const STATUS_PLAYING: u8 = 1;

/// Handle to the running discovery machinery. Drop it to stop.
pub struct Discovery {
    shared: Arc<Shared>,
    socket: Arc<UdpSocket>,
}

fn make_discovery_socket() -> io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // Allow several instances on one machine (e.g. testing) to share the port;
    // broadcast datagrams are delivered to all bound sockets.
    sock.set_reuse_address(true)?;
    // SO_REUSEPORT (unix only) lets multiple instances on one host bind the
    // beacon port; requires socket2's "all" feature. Not available on Windows.
    #[cfg(all(
        unix,
        not(target_os = "solaris"),
        not(target_os = "illumos"),
        not(target_os = "cygwin"),
        not(target_os = "nuttx"),
        not(target_os = "wasi")
    ))]
    sock.set_reuse_port(true)?;
    sock.set_broadcast(true)?;
    // Must bind the well-known beacon port to receive broadcasts addressed to
    // it; with SO_REUSEADDR several instances can share it.
    match sock.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, BEACON_PORT).into()) {
        Ok(()) => Ok(sock.into()),
        Err(_) => {
            // Port taken without reuse (shouldn't normally happen): fall back to
            // an ephemeral port so we can at least keep broadcasting.
            sock.bind(&"0.0.0.0:0".parse::<SocketAddr>().unwrap().into())?;
            Ok(sock.into())
        }
    }
}

/// All addresses we broadcast to: the global broadcast plus every interface's
/// subnet broadcast. Deduplicated.
fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut out = vec![Ipv4Addr::BROADCAST];
    for target in iface_broadcasts() {
        if !out.contains(&target) {
            out.push(target);
        }
    }
    out
}

fn iface_broadcasts() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if let Ok(ifaces) = get_if_addrs() {
        for iface in ifaces {
            let v4 = match iface.addr {
                if_addrs::IfAddr::V4(v4) => v4,
                if_addrs::IfAddr::V6(_) => continue,
            };
            // Only broadcast on sensible global/unicast networks.
            if v4.is_loopback() || v4.ip.is_unspecified() {
                continue;
            }
            let addr = u32::from(v4.ip);
            let mask = u32::from(v4.netmask);
            let bcast = Ipv4Addr::from(addr | !mask);
            if !out.contains(&bcast) {
                out.push(bcast);
            }
        }
    }
    out
}

impl Discovery {
    /// Start beacon + listener threads. `name` is the advertised player name,
    /// `tcp_port` the game listener port.
    pub fn start(name: String, tcp_port: u16) -> io::Result<Discovery> {
        let socket = Arc::new(make_discovery_socket()?);
        let shared = Arc::new(Shared {
            own_id: format!("{:x}", new_instance_id()),
            peers: Mutex::new(HashMap::new()),
            status: AtomicU8::new(STATUS_READY),
            refresh: AtomicBool::new(false),
            shutdown: Arc::new(AtomicBool::new(false)),
        });

        // Beacon sender.
        {
            let socket = socket.clone();
            let shared = shared.clone();
            let name = name.clone();
            thread::spawn(move || beacon_loop(socket, shared, name, tcp_port));
        }
        // Listener.
        {
            let socket = socket.clone();
            let shared = shared.clone();
            thread::spawn(move || listen_loop(socket, shared));
        }

        Ok(Discovery { shared, socket })
    }

    /// Update the status advertised in beacons.
    pub fn set_status(&self, status: PeerStatus) {
        let v = match status {
            PeerStatus::Ready => STATUS_READY,
            PeerStatus::Playing => STATUS_PLAYING,
        };
        self.shared.status.store(v, Ordering::Relaxed);
    }

    /// Ask the beacon thread to send immediately (used by the "refresh" key).
    pub fn refresh(&self) {
        self.shared.refresh.store(true, Ordering::Relaxed);
        // Send a beacon from the calling thread too so refresh feels instant.
        send_beacon(&self.socket, &self.shared, "refresh", 0);
    }

    /// Current visible peers, pruned of stale ones, sorted by name.
    pub fn peers(&self) -> Vec<Peer> {
        let mut peers: Vec<Peer> = {
            let mut guard = self.shared.peers.lock().unwrap();
            guard.retain(|_, p| p.last_seen.elapsed().as_millis() as u64 <= PEER_TIMEOUT_MS);
            guard.values().cloned().collect()
        };
        peers.sort_by_key(|a| a.name.to_lowercase());
        peers
    }
}

fn new_instance_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut seed = nanos ^ (pid << 32) ^ 0x9E3779B97F4A7C15;
    // xorshift to decorrelate pid and timestamp bits.
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    seed
}

fn beacon_loop(socket: Arc<UdpSocket>, shared: Arc<Shared>, name: String, tcp_port: u16) {
    loop {
        if shared.shutdown.load(Ordering::Relaxed) {
            return;
        }
        send_beacon(&socket, &shared, &name, tcp_port);
        // Sleep in small slices so `refresh` can interrupt.
        for _ in 0..(BEACON_INTERVAL_MS / 100) {
            if shared.shutdown.load(Ordering::Relaxed) {
                return;
            }
            if shared.refresh.swap(false, Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn send_beacon(socket: &UdpSocket, shared: &Shared, name: &str, tcp_port: u16) {
    let status = match shared.status.load(Ordering::Relaxed) {
        STATUS_PLAYING => PeerStatus::Playing,
        _ => PeerStatus::Ready,
    };
    let beacon = Beacon {
        id: shared.own_id.clone(),
        name: name.to_string(),
        status,
        tcp_port,
        proto: PROTO_VERSION,
    };
    let Ok(payload) = serde_json::to_vec(&beacon) else {
        return;
    };
    for target in broadcast_targets() {
        let _ = socket.send_to(&payload, SocketAddrV4::new(target, BEACON_PORT));
    }
}

fn listen_loop(socket: Arc<UdpSocket>, shared: Arc<Shared>) {
    let mut buf = [0u8; 1024];
    // A sticky read timeout: Windows surfaces read-timeout expiry as
    // WSAETIMEDOUT (kind TimedOut) rather than WouldBlock, and the old
    // save/restore dance also reset it to None (blocking) each iteration.
    if socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .is_err()
    {
        return;
    }
    while !shared.shutdown.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Ok(beacon) = serde_json::from_slice::<Beacon>(&buf[..n]) {
                    if beacon.proto != PROTO_VERSION || beacon.id == shared.own_id {
                        continue;
                    }
                    let ip = match from.ip() {
                        std::net::IpAddr::V4(v4) => v4,
                        std::net::IpAddr::V6(_) => continue,
                    };
                    let peer = Peer {
                        id: beacon.id.clone(),
                        name: beacon.name.clone(),
                        status: beacon.status,
                        tcp_addr: SocketAddr::V4(SocketAddrV4::new(ip, beacon.tcp_port)),
                        last_seen: Instant::now(),
                    };
                    shared.peers.lock().unwrap().insert(beacon.id.clone(), peer);
                }
            }
            // Read-timeout expiry: keep looping (WouldBlock on Unix,
            // TimedOut on Windows).
            Err(ref e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            // Transient errors: back off briefly rather than dying.
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}
