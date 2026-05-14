# CRDT-prosjekt

Kort prosjekt for nettverksprogrammering med fokus på CRDT i klient-server eller peer-to-peer arkitektur. Implementert i Rust med en webbasert demo (axum + WebSocket + vanilla JS) som kjører over et eget peer-to-peer sync-lag.

## Project layout

```
crates/
  crdt-core/   CRDT primitives implemented from scratch
               (GCounter, PNCounter, GSet, VectorClock, CvRDT trait)
  crdt-sync/   Generic peer-to-peer sync layer over any CvRDT
               (TCP + length-prefixed bincode frames, push + anti-entropy)
  todo-app/    axum web server + static frontend that demos the CRDTs
    src/
      main.rs     bootstrap: env vars, server, sync node
      protocol.rs WebSocket wire types
      state.rs    TodoState (composes GSet + VectorClock + maps)
      server.rs   axum routes + WebSocket handler
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

## Run a single replica

From the workspace root:

```sh
cargo run -p todo-app
```

Open <http://localhost:8080>, click **Connect**, add a todo. Open the URL in a second tab to see the shared state update live across both tabs.

## Run the multi-peer demo

Two replicas, each peering with the other. Open one terminal per replica.

Terminal 1:

```sh
PORT=8080 SYNC_PORT=9080 PEER_ID=peer-1 SYNC_PEERS=127.0.0.1:9081 \
  cargo run -p todo-app
```

Terminal 2:

```sh
PORT=8081 SYNC_PORT=9081 PEER_ID=peer-2 SYNC_PEERS=127.0.0.1:9080 \
  cargo run -p todo-app
```

Open <http://localhost:8080> in one browser window and <http://localhost:8081> in another. Add or toggle todos in either tab — they will converge in the other within milliseconds via the sync layer. The sync layer also runs an anti-entropy round every 5 seconds, so even if a push is lost both replicas still catch up.

Stop one replica, edit the other, restart the stopped one — the reconnect logic re-establishes the link and the two replicas converge again.

### Environment variables

| Variable      | Default                      | Description                                          |
|---------------|------------------------------|------------------------------------------------------|
| `PORT`        | `8080`                       | HTTP/WebSocket port for the browser.                 |
| `SYNC_PORT`   | `PORT + 1000`                | TCP port the sync layer listens on for peer traffic. |
| `PEER_ID`     | `peer-1`                     | Identifier used for this replica.                    |
| `SYNC_PEERS`  | *(empty)*                    | Comma-separated `host:port` list of peer sync ports. |
| `STATIC_DIR`  | `crates/todo-app/static`     | Folder served as the frontend.                       |
| `RUST_LOG`    | `todo_app=info,crdt_sync=info,axum=info` | Log filter.                              |

## Run the tests

All tests:

```sh
cargo test
```

Just the CRDT core (includes property tests for the CRDT laws via `proptest`):

```sh
cargo test -p crdt-core
```

Just the todo state (merge + apply semantics):

```sh
cargo test -p todo-app
```

## Linting and formatting

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## How the pieces fit together

```
 Browser tab A                            Browser tab B
      │                                         │
      │  WebSocket (JSON)                       │  WebSocket (JSON)
      ▼                                         ▼
 ┌───────────────┐   crdt-sync (TCP)   ┌───────────────┐
 │ Server peer-1 │ ◄─────────────────► │ Server peer-2 │
 │  TodoState A  │   bincode frames    │  TodoState B  │
 └───────────────┘                     └───────────────┘
```

Each server holds its own `TodoState` (a composition of `GSet`s, a `VectorClock`, and helper `HashMap`s). Browser actions go up via the WebSocket and mutate the state; `crdt-sync` then ships the new state to all configured peers, where it is merged using the CRDT join. Remote merges trigger a fresh WebSocket snapshot so every connected browser sees the change.

## Status

Work in progress. See [project-description.md](./project-description.md) for the assignment brief.
