//! Wire protocol for LAN discovery (UDP broadcast) and game sessions (TCP + NDJSON).
//!
//! Everything is JSON, one object per datagram (UDP) or per line (TCP). Both sides
//! speak the same protocol; there is no dedicated server.

use serde::{Deserialize, Serialize};

/// UDP port used for discovery beacons (fixed, so peers can find each other).
pub const BEACON_PORT: u16 = 47610;
/// Preferred TCP port for the game listener. Falls back to an ephemeral port if taken.
pub const GAME_PORT: u16 = 47611;

/// How often a beacon is broadcast, and when an unseen peer is considered gone.
pub const BEACON_INTERVAL_MS: u64 = 1200;
pub const PEER_TIMEOUT_MS: u64 = 5000;

/// Status of a peer as advertised in beacons.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerStatus {
    Ready,
    Playing,
}

impl PeerStatus {
    pub fn label(self) -> &'static str {
        match self {
            PeerStatus::Ready => "Ready",
            PeerStatus::Playing => "Playing",
        }
    }
}

/// Discovery beacon, broadcast over UDP.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Beacon {
    /// Random per-run instance id, used to ignore our own beacons.
    pub id: String,
    pub name: String,
    pub status: PeerStatus,
    /// TCP port this peer listens on for game connections.
    pub tcp_port: u16,
    /// Protocol version; mismatched versions are ignored.
    pub proto: u16,
}

pub const PROTO_VERSION: u16 = 1;

/// Messages exchanged over a TCP game connection (one JSON object per line).
///
/// Flow: the initiator sends `Invite`, the peer answers `Accept`/`Decline`;
/// after `Accept` the initiator sends `Start` with the agreed setup and the
/// game begins. During play, moves carry the sender's remaining clock time so
/// both sides stay roughly in sync without a shared clock.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum GameMsg {
    /// Initiator -> peer: game request.
    Invite {
        name: String,
        tc_secs: u32,
        tc_inc: u32,
    },
    /// Peer -> initiator: request accepted.
    Accept,
    /// Peer -> initiator: request declined (optionally with a reason).
    Decline { reason: String },
    /// Initiator -> peer: game starts. `you_are_white` is from the receiver's perspective.
    Start {
        white_name: String,
        black_name: String,
        you_are_white: bool,
        tc_secs: u32,
        tc_inc: u32,
    },
    /// A chess move in UCI form ("e2e4", "e7e8q") plus the mover's remaining
    /// clock time in milliseconds after the move (increment already applied).
    Move { uci: String, clock_ms: u64 },
    /// Offer a draw.
    DrawOffer,
    DrawAccept,
    DrawDecline,
    /// Resign (sender loses).
    Resign,
    /// Sender's clock ran out (sender loses on time).
    Timeout,
    /// Clean goodbye before closing the connection.
    Bye,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_msgs() {
        let msgs = vec![
            GameMsg::Invite {
                name: "alice".into(),
                tc_secs: 300,
                tc_inc: 5,
            },
            GameMsg::Decline {
                reason: "busy".into(),
            },
            GameMsg::Start {
                white_name: "a".into(),
                black_name: "b".into(),
                you_are_white: true,
                tc_secs: 60,
                tc_inc: 0,
            },
            GameMsg::Move {
                uci: "e2e4".into(),
                clock_ms: 295_000,
            },
            GameMsg::DrawOffer,
            GameMsg::Resign,
            GameMsg::Bye,
        ];
        for m in msgs {
            let s = serde_json::to_string(&m).unwrap();
            let back: GameMsg = serde_json::from_str(&s).unwrap();
            assert_eq!(m, back);
        }
    }

    #[test]
    fn roundtrip_beacon() {
        let b = Beacon {
            id: "abc".into(),
            name: "alice".into(),
            status: PeerStatus::Ready,
            tcp_port: 47611,
            proto: PROTO_VERSION,
        };
        let s = serde_json::to_string(&b).unwrap();
        let back: Beacon = serde_json::from_str(&s).unwrap();
        assert_eq!(back.name, "alice");
        assert_eq!(back.status, PeerStatus::Ready);
    }
}
