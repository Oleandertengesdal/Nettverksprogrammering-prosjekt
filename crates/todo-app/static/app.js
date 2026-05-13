const PEER_COLORS = [
  { color: 'var(--replica-a)', soft: 'var(--replica-a-soft)' },
  { color: 'var(--replica-b)', soft: 'var(--replica-b-soft)' },
  { color: 'var(--replica-c)', soft: 'var(--replica-c-soft)' },
  { color: '#ff9f0a', soft: 'rgba(255, 159, 10, 0.12)' },
  { color: '#64d2ff', soft: 'rgba(100, 210, 255, 0.12)' },
  { color: '#ff375f', soft: 'rgba(255, 55, 95, 0.12)' },
];

const state = {
  peers: new Map(),
  localPeerId: null,
  connectionStatus: 'disconnected',
  pendingLocalOps: 0,
  log: [],
  transport: null,
};

const tpl = document.getElementById('replica-template');

function init() {
  bindControls();
  render();
  log('system', 'Frontend ready. Click Connect to open the WebSocket.');
}

function bindControls() {
  document.getElementById('connect-btn').addEventListener('click', async () => {
    if (state.connectionStatus === 'connected' || state.connectionStatus === 'connecting') return;
    await connect();
  });

  document.getElementById('resync-btn').addEventListener('click', async () => {
    if (!state.transport) {
      log('system', 'Cannot request resync before connecting.');
      return;
    }
    try {
      await state.transport.requestResync();
      log('sync', 'Requested resync from backend.');
    } catch (err) {
      log('system', `Resync failed: ${err.message}`);
    }
  });

  document.getElementById('clear-log').addEventListener('click', () => {
    state.log = [];
    renderLog();
  });
}

async function connect() {
  setConnectionStatus('connecting');
  try {
    state.transport = await connectTransport({
      onStatus: handleTransportStatus,
      onSnapshot: handleSnapshot,
      onPeerUpsert: handlePeerUpsert,
      onPeerRemove: handlePeerRemove,
      onRemoteOperation: handleRemoteOperation,
    });
    setConnectionStatus('connected');
    log('system', 'Connected to transport. Waiting for first snapshot...');
  } catch (err) {
    setConnectionStatus('error');
    log('system', `Connect failed: ${err.message}`);
  }
}

async function connectTransport(handlers) {
  const wsProtocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsHost = location.host || 'localhost:8080';
  const url = `${wsProtocol}//${wsHost}/ws`;

  return new Promise((resolve, reject) => {
    let ws;
    try {
      ws = new WebSocket(url);
    } catch (err) {
      reject(new Error(`Could not open WebSocket: ${err.message}`));
      return;
    }

    let opened = false;

    ws.addEventListener('open', () => {
      opened = true;
      handlers.onStatus('connected');
      resolve({
        sendOperation(op) {
          if (ws.readyState !== WebSocket.OPEN) {
            return Promise.reject(new Error('WebSocket is not open'));
          }
          ws.send(JSON.stringify(op));
          return Promise.resolve();
        },
        requestResync() {
          if (ws.readyState !== WebSocket.OPEN) {
            return Promise.reject(new Error('WebSocket is not open'));
          }
          ws.send(JSON.stringify({ type: 'resync' }));
          return Promise.resolve();
        },
        disconnect() {
          ws.close();
        },
      });
    });

    ws.addEventListener('message', (event) => {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch (err) {
        log('system', `Received malformed message: ${err.message}`);
        return;
      }
      routeServerMessage(msg, handlers);
    });

    ws.addEventListener('close', () => {
      handlers.onStatus('disconnected');
    });

    ws.addEventListener('error', () => {
      handlers.onStatus('error');
      if (!opened) {
        reject(new Error(`Could not connect to ${url}`));
      }
    });
  });
}

function routeServerMessage(msg, handlers) {
  if (!msg || typeof msg.type !== 'string') return;
  switch (msg.type) {
    case 'snapshot':
      handlers.onSnapshot({
        localPeerId: msg.localPeerId ?? msg.local_peer_id ?? null,
        peers: msg.peers ?? [],
      });
      break;
    case 'peer.upsert':
      if (msg.peer) handlers.onPeerUpsert(msg.peer);
      break;
    case 'peer.remove':
      handlers.onPeerRemove(msg.peerId ?? msg.peer_id);
      break;
    case 'operation':
      if (msg.op) handlers.onRemoteOperation(msg.op);
      break;
    default:
      console.warn('Unknown server message type:', msg.type);
  }
}

function handleTransportStatus(status) {
  // status should be one of: disconnected, connecting, connected, error
  setConnectionStatus(status);
}

function handleSnapshot(snapshot) {
  // TODO: Align this shape with your Rust backend protocol.
  // Suggested snapshot shape:
  // {
  //   localPeerId: "peer-1",
  //   peers: [
  //     {
  //       id: "peer-1",
  //       name: "Peer 1",
  //       online: true,
  //       clock: { "peer-1": 3, "peer-2": 1 },
  //       todos: [{ id: "t1", text: "Write report", completed: false, createdBy: "peer-1" }]
  //     }
  //   ]
  // }
  state.localPeerId = snapshot.localPeerId || null;
  state.peers.clear();

  for (const peer of snapshot.peers || []) {
    state.peers.set(peer.id, normalizePeer(peer));
  }

  state.pendingLocalOps = 0;
  render();
  log('sync', `Snapshot received (${state.peers.size} peer${state.peers.size === 1 ? '' : 's'}).`);
}

function handlePeerUpsert(peer) {
  state.peers.set(peer.id, normalizePeer(peer));
  render();
  log('system', `${displayName(peer)} is ${peer.online ? 'online' : 'offline'}.`);
}

function handlePeerRemove(peerId) {
  const peer = state.peers.get(peerId);
  state.peers.delete(peerId);
  render();
  log('system', `${peer ? displayName(peer) : peerId} left.`);
}

function handleRemoteOperation(_op) {
  // TODO: If backend sends delta operations instead of full snapshots,
  // apply op here and update state.peers accordingly.
  // Keeping this explicit avoids local simulation logic in the frontend.
}

async function sendLocalOperation(op) {
  if (!state.transport) {
    log('system', 'Not connected. Local operation was not sent.');
    return;
  }
  try {
    state.pendingLocalOps++;
    await state.transport.sendOperation(op);
    render();
  } catch (err) {
    state.pendingLocalOps = Math.max(0, state.pendingLocalOps - 1);
    log('system', `Failed to send operation: ${err.message}`);
    render();
  }
}

function normalizePeer(peer) {
  return {
    id: peer.id,
    name: peer.name || `Peer ${peer.id}`,
    online: !!peer.online,
    todos: Array.isArray(peer.todos) ? peer.todos : [],
    clock: peer.clock && typeof peer.clock === 'object' ? peer.clock : {},
  };
}

function displayName(peer) {
  return peer.name || `Peer ${peer.id}`;
}

function setConnectionStatus(status) {
  state.connectionStatus = status;
  renderSyncStatus();
}

function render() {
  renderPeerCards();
  renderSyncStatus();
  renderLocalPeerId();
  renderLog();
}

function renderPeerCards() {
  const container = document.getElementById('replicas');
  container.innerHTML = '';

  const peers = [...state.peers.values()].sort((a, b) => a.id.localeCompare(b.id));
  if (peers.length === 0) {
    const empty = document.createElement('article');
    empty.className = 'replica';
    empty.innerHTML = '<div class="empty-state" style="padding:28px 16px">No peers connected yet.</div>';
    container.appendChild(empty);
    return;
  }

  for (const peer of peers) {
    container.appendChild(buildPeerCard(peer));
  }
}

function buildPeerCard(peer) {
  const frag = tpl.content.cloneNode(true);
  const card = frag.querySelector('.replica');

  const colors = colorForPeer(peer.id);
  card.style.setProperty('--replica-color', colors.color);
  card.style.setProperty('--replica-color-soft', colors.soft);
  card.classList.toggle('offline', !peer.online);
  card.dataset.peerId = peer.id;

  card.querySelector('.replica-avatar').textContent = displayName(peer).charAt(0).toUpperCase();
  card.querySelector('.replica-name').textContent = displayName(peer);
  card.querySelector('.replica-id').textContent = peer.id;

  const peerState = card.querySelector('.peer-state');
  peerState.textContent = peer.online ? 'online' : 'offline';
  peerState.classList.toggle('offline', !peer.online);

  const isLocal = peer.id === state.localPeerId;
  const input = card.querySelector('.add-input');
  input.placeholder = isLocal ? 'Add todo on this peer...' : 'Read-only remote peer';
  input.disabled = !isLocal;

  const form = card.querySelector('.add-form');
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (!isLocal) return;
    const text = input.value.trim();
    if (!text) return;

    await sendLocalOperation({
      type: 'todo.add',
      text,
      peerId: peer.id,
    });

    input.value = '';
  });

  const list = card.querySelector('.todo-list');
  list.innerHTML = '';

  if (peer.todos.length === 0) {
    const li = document.createElement('li');
    li.className = 'empty-state';
    li.textContent = 'No todos';
    list.appendChild(li);
  } else {
    for (const todo of peer.todos) {
      list.appendChild(buildTodoRow(todo, isLocal, peer.id));
    }
  }

  renderClock(card.querySelector('.clock-entries'), peer.clock);

  const pending = card.querySelector('.pending-badge');
  pending.hidden = !(isLocal && state.pendingLocalOps > 0);

  return card;
}

function buildTodoRow(todo, canEdit, peerId) {
  const li = document.createElement('li');
  li.className = 'todo-item' + (todo.completed ? ' completed' : '');

  const checkbox = document.createElement('input');
  checkbox.type = 'checkbox';
  checkbox.className = 'todo-checkbox';
  checkbox.checked = !!todo.completed;
  checkbox.disabled = !canEdit;
  checkbox.addEventListener('change', async () => {
    if (!canEdit) return;
    await sendLocalOperation({
      type: 'todo.toggle',
      id: todo.id,
      peerId,
    });
  });

  const origin = document.createElement('span');
  origin.className = 'origin-badge';
  const originColors = colorForPeer(todo.createdBy || peerId);
  origin.style.background = originColors.color;

  const text = document.createElement('span');
  text.className = 'todo-text';
  text.textContent = todo.text;

  const del = document.createElement('button');
  del.className = 'delete-btn';
  del.type = 'button';
  del.textContent = '×';
  del.title = 'Delete';
  del.disabled = !canEdit;
  del.addEventListener('click', async () => {
    if (!canEdit) return;
    await sendLocalOperation({
      type: 'todo.delete',
      id: todo.id,
      peerId,
    });
  });

  li.append(checkbox, origin, text, del);
  return li;
}

function renderClock(container, clock) {
  container.innerHTML = '';
  const keys = Object.keys(clock || {}).sort();
  if (keys.length === 0) {
    container.textContent = 'n/a';
    return;
  }

  for (const key of keys) {
    const span = document.createElement('span');
    span.className = 'clock-entry';
    span.textContent = `${key}:${clock[key]}`;
    container.appendChild(span);
  }
}

function colorForPeer(peerId) {
  if (!peerId) return PEER_COLORS[0];
  let hash = 0;
  for (let i = 0; i < peerId.length; i++) hash = ((hash << 5) - hash) + peerId.charCodeAt(i);
  const index = Math.abs(hash) % PEER_COLORS.length;
  return PEER_COLORS[index];
}

function renderSyncStatus() {
  const dot = document.querySelector('#sync-status .status-dot');
  const text = document.getElementById('sync-status-text');
  const connectBtn = document.getElementById('connect-btn');

  if (state.connectionStatus === 'connected') {
    dot.className = 'status-dot online';
    text.textContent = 'Connected (live updates)';
    connectBtn.disabled = true;
  } else if (state.connectionStatus === 'connecting') {
    dot.className = 'status-dot diverged';
    text.textContent = 'Connecting...';
    connectBtn.disabled = true;
  } else if (state.connectionStatus === 'error') {
    dot.className = 'status-dot diverged';
    text.textContent = 'Connection error';
    connectBtn.disabled = false;
  } else {
    dot.className = 'status-dot diverged';
    text.textContent = 'Disconnected';
    connectBtn.disabled = false;
  }
}

function renderLocalPeerId() {
  const el = document.getElementById('local-peer-id');
  el.textContent = state.localPeerId || 'not assigned';
}

function log(kind, msg) {
  state.log.unshift({ ts: new Date(), kind, msg });
  if (state.log.length > 80) state.log.length = 80;
  renderLog();
}

function renderLog() {
  const list = document.getElementById('log-list');
  list.innerHTML = '';

  if (state.log.length === 0) {
    const li = document.createElement('li');
    li.className = 'empty';
    li.textContent = 'No activity yet.';
    list.appendChild(li);
    return;
  }

  for (const entry of state.log) {
    const li = document.createElement('li');

    const time = document.createElement('span');
    time.className = 'log-time';
    time.textContent = formatTime(entry.ts);

    const msg = document.createElement('span');
    msg.className = 'log-msg';

    if (entry.kind === 'sync') {
      msg.innerHTML = `<span class="log-pill sync">SYNC</span>${escapeHtml(entry.msg)}`;
    } else {
      msg.textContent = entry.msg;
    }

    li.append(time, msg);
    list.appendChild(li);
  }
}

function formatTime(d) {
  const pad = (n) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function escapeHtml(s) {
  const div = document.createElement('div');
  div.textContent = s;
  return div.innerHTML;
}

init();
