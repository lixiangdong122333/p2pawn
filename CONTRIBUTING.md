# Contributing to p2pawn

Thank you for your interest in contributing! This document describes the
development workflow, code standards, and how releases happen automatically.

**Language**: English | [简体中文](CONTRIBUTING.zh-CN.md)

## Workflow: branch → PR → squash

The `main` branch is protected: it only accepts **squash-merged pull
requests** that pass all required checks. Nobody (including maintainers)
pushes to it directly — the only exception is the release bot's version
bump commit.

1. **Branch** off the latest `main`:

   ```bash
   git switch main && git pull
   git switch -c feat/your-thing     # or fix/... docs/... etc.
   ```

2. **Commit freely** on your branch — commit messages there are yours.
   Only the **PR title** matters for automation (see next section).

3. **Push and open a PR** against `main`.

4. Wait for CI to pass (details below), then **Squash and merge**.
   The only allowed merge method is squash; the repo enforces it.

## Conventional Commits: the PR title drives the version number

The squash-merge turns your PR title into the single commit subject on
`main`, and the release pipeline derives the next version from it:

| PR title prefix | Version bump (0.x) | Example |
|---|---|---|
| `feat:` | **minor** | `feat: add LAN chat` → v0.2.0 |
| `fix:` | **patch** | `fix: clock drift` → v0.2.1 |
| `feat!:` or `BREAKING CHANGE:` in the description | **minor** (major after 1.0) | `feat!:` new protocol |
| `chore:`, `docs:`, `ci:`, `refactor:`, `test:` | **patch**, whenever the change shows up in the changelog (user-visible docs, dependency bumps); truly invisible maintenance (CI tweaks, internal refactors the changelog parsers skip) releases nothing | `docs: typo` → likely v0.2.1 |

Rules of thumb:

- The PR title **must** start with a conventional-commit type: `feat`,
  `fix`, `chore`, `docs`, `refactor`, `test`, `ci`. CI rejects anything
  else (checked by `amannn/action-semantic-pull-request`).
- Scope syntax is fine: `feat(replay): ...`.
- Maintenance changes are released as **patch** versions when they land in
  the changelog (docs, dependency updates); changes the changelog omits
  (CI tweaks the parsers skip) trigger no release. When in doubt, assume
  a merge produces a patch release.

## What CI checks (and what it means for you)

Every PR runs `.github/workflows/ci.yml`:

- **fmt + clippy** — `cargo fmt --check` and `cargo clippy --all-targets
  -- -D warnings`. Warnings are errors. Run locally before pushing:

  ```bash
  cargo fmt --all
  cargo clippy --all-targets -- -D warnings
  ```

- **test (debug) / test (release)** — the full suite in both profiles.
  UDP discovery, TCP game flows and TUI rendering are covered; if your
  change touches networking or UI, add or extend a test
  (`tests/integration.rs` has runnable examples).

- **license check** — `cargo-deny` verifies every dependency (including
  new ones you add) carries a permissive license from the allow-list in
  `deny.toml`. If you add a dependency with a new license expression,
  add the SPDX id to the allow-list. Copyleft licenses will be rejected.

- **PR title** — the conventional-commit lint from the previous section.

All four are **required checks** on `main`; a red one blocks merging.

## Code guidelines

- **Rust 2024 edition, stable toolchain** (pinned via `rust-toolchain.toml`).
- **No hand-rolled chess rules.** Board legality, mate/stalemate,
  repetition, en passant, promotion etc. all come from the `chess`
  crate. `src/game/san.rs` only *formats* SAN for display — if you find
  yourself writing move-generation logic, stop and check the `chess`
  crate first.
- **Keep the dependency tree lean.** This is a small LAN tool, not a
  platform. Before adding a crate, ask whether ~50 lines of std code
  would do.
- **Error handling**: I/O and network failures are user-facing — surface
  them (toast/dialog) instead of `unwrap()`ing in runtime paths.
  `unwrap()` is fine in tests and truly-infallible cases.
- **Match the surrounding style** — comment density and naming follow
  the existing code.
- If your change is user-facing, update **both** `README.md` and
  `README.zh-CN.md` (they are kept in sync), and it will land in the
  auto-generated CHANGELOG via the release pipeline.

## Testing notes

- Tests bind real UDP/TCP sockets on loopback; they run in parallel and
  are safe in CI and locally.
- `examples/lan_pair.rs` is a headless end-to-end driver that plays a
  real game between two processes over real sockets — useful for
  reproducing LAN behaviour without a second machine:

  ```bash
  cargo build --release --examples
  P2PAWN_NAME=Alice ./target/release/examples/lan_pair host Alice   # terminal 1
  P2PAWN_NAME=Bob   ./target/release/examples/lan_pair client Bob   # terminal 2
  ```

## How releases work (so you know what happens after merge)

Merging a PR triggers `.github/workflows/release.yml`:

1. The squash-commit's title determines the bump (see the table above);
   `chore(release):` commits from the bot itself are skipped to avoid a
   re-trigger loop.
2. The bot bumps `Cargo.toml`, regenerates `CHANGELOG.md` (git-cliff),
  pushes a `chore(release): vX.Y.Z` commit, and creates the tag +
   GitHub Release on that commit.
3. Matrix builds produce binaries for Windows (MSVC), Linux (musl,
   static) and macOS (aarch64) with `.sha256` checksums and attach them
   to the release.

You never cut a release manually. If a release run fails, re-running the
failed jobs is usually enough; the upload step is idempotent
(`--clobber`).

## Reporting issues

Include: OS + terminal, p2pawn version (`gh release list` or the commit
SHA), what you did, what happened vs. what you expected. For network
problems mention whether both machines are on the same subnet and any
firewall/VPN software in play.
