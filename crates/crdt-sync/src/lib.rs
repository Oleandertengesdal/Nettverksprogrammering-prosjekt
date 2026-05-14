//! # CRDT Sync
//!
//! Peer-to-peer synchronization layer for state-based CRDTs.
//!
//! Each [`SyncNode`] owns a shared `Arc<RwLock<S>>` over a CRDT state
//! `S: CvRDT`, listens for incoming TCP connections on a configured
//! address, and maintains outgoing TCP connections to a list of peer
//! addresses. State is exchanged as bincode-serialized
//! [`SyncMessage`]s over length-prefixed frames.
//!
//! Two sync triggers:
//! - **Push-on-change**: when the local state mutates, call
//!   [`SyncNode::notify_change`] to broadcast immediately.
//! - **Anti-entropy**: every [`ANTI_ENTROPY_INTERVAL`] the outgoing
//!   loop sends the latest state regardless of local changes, so
//!   replicas that missed a push will still converge.

use bytes::Bytes;
use crdt_core::CvRDT;
use futures::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

pub const ANTI_ENTROPY_INTERVAL: Duration = Duration::from_secs(5);
pub const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Messages exchanged between peers over a sync connection.
#[derive(Serialize, Deserialize)]
pub enum SyncMessage<S> {
    /// Identifies the sending peer when a connection opens.
    Hello { peer_id: String },
    /// A full state snapshot to be merged on receipt.
    State(S),
}

/// A peer-to-peer sync node parameterized over any state-based CRDT.
pub struct SyncNode<S> {
    peer_id: String,
    state: Arc<RwLock<S>>,
    listen_addr: SocketAddr,
    peers: Vec<SocketAddr>,
    on_local_change: broadcast::Sender<()>,
    on_remote_update: broadcast::Sender<()>,
}

impl<S> SyncNode<S>
where
    S: CvRDT + Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Build a new sync node. Call [`run`](Self::run) to start the
    /// listener and outgoing connectors.
    pub fn new(
        peer_id: String,
        state: Arc<RwLock<S>>,
        listen_addr: SocketAddr,
        peers: Vec<SocketAddr>,
    ) -> Self {
        let (on_local_change, _) = broadcast::channel(16);
        let (on_remote_update, _) = broadcast::channel(16);
        Self {
            peer_id,
            state,
            listen_addr,
            peers,
            on_local_change,
            on_remote_update,
        }
    }

    /// Returns a sender that the application should fire whenever it
    /// mutates the local state. Outgoing peers will be woken up and
    /// will push the new state immediately.
    pub fn change_notifier(&self) -> broadcast::Sender<()> {
        self.on_local_change.clone()
    }

    /// Subscribe to notifications fired whenever a remote state has
    /// been merged into the local state. Useful for re-rendering UIs.
    pub fn subscribe_remote_updates(&self) -> broadcast::Receiver<()> {
        self.on_remote_update.subscribe()
    }

    /// Starts the listener and one outgoing task per peer. Returns
    /// once both are spawned; the tasks run for the lifetime of the
    /// process.
    pub async fn run(self) -> anyhow::Result<()> {
        spawn_listener(
            self.listen_addr,
            self.peer_id.clone(),
            self.state.clone(),
            self.on_remote_update.clone(),
        )
        .await?;

        for peer_addr in self.peers {
            spawn_outgoing(
                peer_addr,
                self.peer_id.clone(),
                self.state.clone(),
                self.on_local_change.clone(),
            );
        }

        Ok(())
    }
}

async fn spawn_listener<S>(
    addr: SocketAddr,
    peer_id: String,
    state: Arc<RwLock<S>>,
    on_remote_update: broadcast::Sender<()>,
) -> anyhow::Result<()>
where
    S: CvRDT + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(target: "crdt_sync", "sync listener bound to {}", addr);

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, remote)) => {
                    tracing::info!(target: "crdt_sync", "accepted sync connection from {}", remote);
                    let peer_id = peer_id.clone();
                    let state = state.clone();
                    let on_remote_update = on_remote_update.clone();
                    tokio::spawn(async move {
                        if let Err(e) = serve_incoming::<S>(stream, peer_id, state, on_remote_update).await {
                            tracing::warn!(target: "crdt_sync", "incoming connection ended: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!(target: "crdt_sync", "accept error: {}", e);
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
        }
    });

    Ok(())
}

async fn serve_incoming<S>(
    stream: TcpStream,
    peer_id: String,
    state: Arc<RwLock<S>>,
    on_remote_update: broadcast::Sender<()>,
) -> anyhow::Result<()>
where
    S: CvRDT + Serialize + DeserializeOwned + Send + Sync,
{
    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

    send_message::<S>(&mut framed, &SyncMessage::Hello { peer_id }).await?;

    while let Some(frame) = framed.next().await {
        let bytes = frame?;
        let msg: SyncMessage<S> = bincode::deserialize(&bytes)?;
        match msg {
            SyncMessage::Hello { peer_id } => {
                tracing::debug!(target: "crdt_sync", "hello from {}", peer_id);
            }
            SyncMessage::State(remote_state) => {
                {
                    let mut local = state.write().await;
                    local.merge(&remote_state);
                }
                let _ = on_remote_update.send(());
            }
        }
    }
    Ok(())
}

fn spawn_outgoing<S>(
    addr: SocketAddr,
    peer_id: String,
    state: Arc<RwLock<S>>,
    on_local_change: broadcast::Sender<()>,
) where
    S: CvRDT + Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        loop {
            match TcpStream::connect(addr).await {
                Ok(stream) => {
                    tracing::info!(target: "crdt_sync", "connected outgoing sync to {}", addr);
                    let mut change_rx = on_local_change.subscribe();
                    if let Err(e) = drive_outgoing::<S>(stream, &peer_id, &state, &mut change_rx).await {
                        tracing::warn!(target: "crdt_sync", "outgoing sync to {} ended: {}", addr, e);
                    }
                }
                Err(e) => {
                    tracing::debug!(target: "crdt_sync", "connect to {} failed: {}", addr, e);
                }
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    });
}

async fn drive_outgoing<S>(
    stream: TcpStream,
    peer_id: &str,
    state: &Arc<RwLock<S>>,
    change_rx: &mut broadcast::Receiver<()>,
) -> anyhow::Result<()>
where
    S: CvRDT + Serialize + DeserializeOwned + Clone + Send + Sync,
{
    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

    send_message::<S>(
        &mut framed,
        &SyncMessage::Hello {
            peer_id: peer_id.to_string(),
        },
    )
    .await?;

    push_state::<S>(&mut framed, state).await?;

    let mut ticker = tokio::time::interval(ANTI_ENTROPY_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;

    loop {
        tokio::select! {
            res = change_rx.recv() => {
                if res.is_err() {
                    return Ok(());
                }
                push_state::<S>(&mut framed, state).await?;
            }
            _ = ticker.tick() => {
                push_state::<S>(&mut framed, state).await?;
            }
        }
    }
}

async fn push_state<S>(
    framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
    state: &Arc<RwLock<S>>,
) -> anyhow::Result<()>
where
    S: CvRDT + Serialize + DeserializeOwned + Clone,
{
    let snapshot = {
        let guard = state.read().await;
        guard.clone()
    };
    send_message::<S>(framed, &SyncMessage::State(snapshot)).await
}

async fn send_message<S>(
    framed: &mut Framed<TcpStream, LengthDelimitedCodec>,
    msg: &SyncMessage<S>,
) -> anyhow::Result<()>
where
    S: Serialize,
{
    let encoded = bincode::serialize(msg)?;
    framed.send(Bytes::from(encoded)).await?;
    Ok(())
}
