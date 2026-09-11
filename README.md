# p2pawn

**English** | [简体中文](README.zh-CN.md)

> Peer-to-peer chess in your terminal — no server, no account, just you and
> a colleague on the LAN.

Pawn to pawn, no middleman. A lightweight, pure-TUI chess client for quick
games with colleagues on the office LAN.

```
open terminal → run p2pawn → pick a colleague → play
```

## Features

- **Pure TUI** built with [ratatui](https://ratatui.rs) + crossterm: menu,
  lobby, board, movelist, clocks, dialogs, settings, help, game-over screen.
- **LAN auto-discovery** via UDP broadcast — no IP addresses to type, no
  server to deploy. Every client both advertises itself and listens.
- **Full chess rules** (legality, check, checkmate, stalemate, threefold
  repetition, fifty-move rule, castling, en passant, promotion, FEN, SAN/UCI)
  provided by the [`chess`](https://crates.io/crates/chess) crate — no
  hand-rolled engine.
- **Keyboard-first**: arrows move the cursor, `Enter` selects/moves,
  `Esc` cancels/menus, `Tab` flips the board. No typing `e2e4`.
- **Legal-move hints**: selected piece shows its destinations (`◎`).
- **Chess clocks**: 1+0 / 3+2 / 5+0 / 10+0 presets, increment support,
  timeout detection, clock sync from the opponent's reported time.
- **Draw offer / accept / decline, resignation**, opponent-disconnect
  handling (you win by abandonment).
- **PGN everywhere**: finished games are auto-saved as standard PGN files;
  the history screen lists them and replays them move by move
  (`←`/`→`/`Home`/`End`).
- **Simple TOML config**: player name, piece style (Unicode/ASCII),
  coordinates, hints, default time control, board flip.

## Build

```bash
cargo build --release
```

The result is a single self-contained executable
(`target/release/p2pawn[.exe]`, ~9 MB) that needs nothing but a terminal.

### Note on Windows without MSVC

The project builds out of the box with the MSVC toolchain. If you only have
the GNU toolchain, make sure `gcc`/`ld`/`dlltool` (e.g. from
[WinLibs](https://winlibs.com/)) are on `PATH`:

```bash
rustup toolchain install stable-x86_64-pc-windows-gnu
export PATH="/path/to/mingw64/bin:$PATH"
cargo +stable-x86_64-pc-windows-gnu build --release
```

## Usage

```
p2pawn
```

| Screen        | Keys                                                    |
|---------------|---------------------------------------------------------|
| Menu          | `↑↓` select, `⏎` confirm, `q`/`Esc` quit                |
| Lobby         | `↑↓` select player, `⏎` invite, `r` refresh, `Esc` back |
| Game          | `↑↓←→` cursor, `⏎` select & move, `Esc` menu, `Tab` flip, `D` draw offer |
| Promotion     | `←→` pick piece, `⏎` confirm                            |
| History       | `⏎` replay, `d` delete, `Esc` back                      |
| Replay        | `←`/`→` step, `Home`/`End` jump, `Esc` back             |
| Settings      | `⏎`/`←→` change values, `Esc` back (auto-saved)         |

## How LAN play works

```
Computer A                         Computer B
  UDP beacon :47610  ──broadcast──▶  listener
  listener   ◀──broadcast──────────  UDP beacon :47610
       └─────────── TCP :47611 ────────┘
             Invite → Accept → Start → Move…
```

- Discovery: JSON beacons over UDP broadcast (global + per-interface subnet
  broadcasts) every ~1.2 s; peers expire after 5 s.
- Game: one TCP connection (NDJSON, one message per line) between the two
  clients. The initiator sends `Invite`, the peer `Accept`/`Decline`, then
  `Start` assigns colors randomly and both sides play, syncing clocks with
  each move.

## Files

- Config: `<config_dir>/p2pawn/config.toml`
  (e.g. `~/AppData/Roaming/p2pawn/config.toml` on Windows,
  `~/.config/p2pawn/config.toml` on Linux)
- Games: `<config_dir>/p2pawn/games/*.pgn`

## Project layout

```
src/
  main.rs        terminal setup + event loop
  app.rs         application state machine, LAN orchestration
  input.rs       keyboard routing for every screen and modal
  ui.rs          all rendering (screens, board, modals)
  config.rs      TOML config
  util.rs        timestamp helpers
  game/
    session.rs   game state: moves, clock, draw/resign/timeout, end detection
    clock.rs     two-sided chess clock with increment
    san.rs       SAN formatting + UCI parsing (rules come from `chess`)
    pgn.rs       PGN generation/parsing + replay
    history.rs   local .pgn storage
  net/
    proto.rs     wire protocol (beacons + game messages)
    discovery.rs UDP beacon/listener threads
    conn.rs      TCP game connections + listener
tests/
  integration.rs discovery, full TCP game, render smoke tests
```

## Tests

```bash
cargo test
```

56 tests total (debug + release): SAN/PGN/UCI round-trips, checkmate/
stalemate/repetition/promotion/timeout detection, clock semantics, config
round-trip, history save/load, UDP discovery between two instances, a
complete TCP game (invite → accept → start → moves → draw offer → decline →
resign), busy declines, and TestBackend render smoke tests for every screen.

### Multi-process end-to-end test

`examples/lan_pair.rs` drives the **real** application state machine
(`App` + key handling + networking) headlessly, so two independent
processes can play an actual game over real UDP discovery and TCP:

```bash
cargo build --release --examples

# terminal 1: wait for an invite and accept it
P2PAWN_NAME=Alice ./target/release/examples/lan_pair host Alice

# terminal 2: discover peers, invite, play fool's mate
P2PAWN_NAME=Bob ./target/release/examples/lan_pair client Bob
```

Both sides discover each other over UDP broadcast, exchange
invite/accept/start over TCP, play 1. f3 e5 2. g4 Qh4# to checkmate, and
each saves its own PGN. `client-quit` simulates crashing mid-game (the
opponent then wins by abandonment), and `P2PAWN_SLOW_MS=<ms>` delays
each move so a game stays live for interacting with it from a third
instance (which will see the busy player's "Playing" status and be
refused).

`P2PAWN_NAME` also works with the main `p2pawn` binary to override
the player name without editing the config — handy for running two
instances on one machine.

## License

Copyright 2026 Xiangdong Li

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
