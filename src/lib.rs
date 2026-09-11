//! p2pawn — a LAN chess TUI client.
//!
//! The binary wires the modules together in `main.rs`; this library crate
//! exposes the same modules so integration tests can drive them directly.

pub mod app;
pub mod config;
pub mod game;
pub mod input;
pub mod net;
pub mod ui;
pub mod util;
