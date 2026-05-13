# CRDT-prosjekt

Kort prosjekt for nettverksprogrammering med fokus på CRDT i klient-server eller peer-to-peer arkitektur. Implementert i Rust med en webbasert demo (axum + WebSocket + vanilla JS).

## Project layout

```
crates/
  crdt-core/   CRDT primitives implemented from scratch
               (GCounter, PNCounter, GSet, VectorClock, CvRDT trait)
  crdt-sync/   Network sync layer between replicas (in progress)
  todo-app/    axum web server + static frontend that demos the CRDTs
    src/main.rs   server + WebSocket handler
    static/       index.html, style.css, app.js
```

## Prerequisites

- Rust stable (edition 2021). Install via [rustup](https://rustup.rs/) if you don't have it.
- A modern browser (Chrome / Firefox / Safari).
- No Node, npm, or any frontend toolchain required — the frontend is plain HTML/CSS/JS served by the Rust binary.

## Build

```sh
cargo build
```

For an optimized build:

```sh
cargo build --release
```

## Run the demo

From the workspace root:

```sh
cargo run -p todo-app
```

Then open <http://localhost:8080> in your browser and click **Connect**. Open the same URL in a second tab to see the shared state update live across both tabs.

### Optional environment variables

| Variable     | Default                      | Description                                  |
|--------------|------------------------------|----------------------------------------------|
| `PORT`       | `8080`                       | Port the HTTP/WebSocket server binds to.     |
| `PEER_ID`    | `peer-1`                     | Identifier used for this replica.            |
| `STATIC_DIR` | `crates/todo-app/static`     | Folder served as the frontend.               |
| `RUST_LOG`   | `todo_app=info,axum=info`    | Log filter (e.g. `todo_app=debug`).          |

Example — run a second instance on another port (useful once `crdt-sync` is wired up):

```sh
PORT=8081 PEER_ID=peer-2 cargo run -p todo-app
```

## Run the tests

All tests:

```sh
cargo test
```

Just the CRDT core (includes proptest-based property tests for the CRDT laws):

```sh
cargo test -p crdt-core
```

## Linting and formatting

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Status

Work in progress. See [project-description.md](./project-description.md) for the assignment brief.
