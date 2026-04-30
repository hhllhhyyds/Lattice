const state = {
  selectedSessionId: null,
  stream: null,
  streamReconnectHandle: null,
  streamAfterEventId: null,
  currentMessages: [],
  currentEvents: [],
  currentStatus: null,
};

const el = {
  createSessionBtn: document.getElementById("create-session-btn"),
  refreshSessionsBtn: document.getElementById("refresh-sessions-btn"),
  refreshDetailBtn: document.getElementById("refresh-detail-btn"),
  sessionName: document.getElementById("session-name"),
  sessionList: document.getElementById("session-list"),
  activeSessionLabel: document.getElementById("active-session-label"),
  statusChip: document.getElementById("status-chip"),
  statusSummary: document.getElementById("status-summary"),
  messageList: document.getElementById("message-list"),
  eventList: document.getElementById("event-list"),
  eventCountBadge: document.getElementById("event-count-badge"),
  messageForm: document.getElementById("message-form"),
  messageInput: document.getElementById("message-input"),
  sendBtn: document.getElementById("send-btn"),
  provider: document.getElementById("provider"),
  model: document.getElementById("model"),
  maxIterations: document.getElementById("max-iterations"),
  toast: document.getElementById("toast"),
};

function showToast(message, isError = false) {
  el.toast.textContent = message;
  el.toast.classList.remove("hidden", "error");
  if (isError) {
    el.toast.classList.add("error");
  }
  window.clearTimeout(showToast.timer);
  showToast.timer = window.setTimeout(() => {
    el.toast.classList.add("hidden");
  }, 3200);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
    ...options,
  });

  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const data = await response.json();
      message = data.error?.message || message;
    } catch {}
    throw new Error(message);
  }

  if (response.status === 204) {
    return null;
  }

  return response.json();
}

function formatTime(value) {
  if (!value) return "-";
  return new Date(value).toLocaleString();
}

function setStatusChip(status) {
  el.statusChip.textContent = status || "idle";
  el.statusChip.className = `status-chip ${status || "idle"}`;
}

function sessionTitle(session) {
  const created = formatTime(session.createdAt);
  return {
    title: session.metadata?.name || session.sessionId.slice(0, 8),
    meta: `${session.status} · ${created}`,
  };
}

function renderSessions(sessions) {
  if (!sessions.length) {
    el.sessionList.className = "session-list empty-state";
    el.sessionList.textContent = "还没有会话，先创建一个。";
    return;
  }

  el.sessionList.className = "session-list";
  el.sessionList.innerHTML = "";

  sessions.forEach((session) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `session-card${session.sessionId === state.selectedSessionId ? " active" : ""}`;
    const label = sessionTitle(session);
    button.innerHTML = `
      <p class="session-title">${label.title}</p>
      <p class="session-meta">${label.meta}</p>
    `;
    button.addEventListener("click", () => selectSession(session.sessionId));
    el.sessionList.appendChild(button);
  });
}

function summarizePayload(payload) {
  switch (payload.type) {
    case "userMessage":
      return payload.content;
    case "thinking":
      return payload.reasoning;
    case "toolCallRequested":
      return `${payload.tool} ${JSON.stringify(payload.params)}`;
    case "toolCallResult":
      return `exit=${payload.exitCode}\nstdout:\n${payload.stdout}\nstderr:\n${payload.stderr}`;
    case "toolCallError":
      return payload.error;
    case "finalAnswer":
      return payload.answer;
    case "stateChange":
      return `${payload.from} -> ${payload.to}`;
    default:
      return JSON.stringify(payload, null, 2);
  }
}

function renderMessages(messages) {
  if (!messages.length) {
    el.messageList.className = "message-list empty-state";
    el.messageList.textContent = "当前会话还没有消息。";
    return;
  }

  el.messageList.className = "message-list";
  el.messageList.innerHTML = "";

  messages.forEach((message) => {
    const item = document.createElement("article");
    item.className = `message-item ${message.role}`;
    item.innerHTML = `
      <p class="message-meta">${message.role} · ${formatTime(message.timestamp)}</p>
      <pre class="message-content">${escapeHtml(message.content)}</pre>
    `;
    el.messageList.appendChild(item);
  });
}

function renderEvents(events) {
  el.eventCountBadge.textContent = String(events.length);
  if (!events.length) {
    el.eventList.className = "event-list empty-state";
    el.eventList.textContent = "当前会话还没有事件。";
    return;
  }

  el.eventList.className = "event-list";
  el.eventList.innerHTML = "";

  events
    .slice()
    .reverse()
    .forEach((event) => {
      const item = document.createElement("article");
      item.className = "event-item";
      item.innerHTML = `
        <header>
          <p class="event-title">${event.payload.type} · ${event.actor}</p>
          <span class="event-time">${formatTime(event.timestamp)}</span>
        </header>
        <pre class="event-body">${escapeHtml(summarizePayload(event.payload))}</pre>
      `;
      el.eventList.appendChild(item);
    });
}

function renderStatus(status) {
  setStatusChip(status?.runStatus);
  const latest = status?.latestEvent
    ? `${status.latestEvent.payloadType} · ${status.latestEvent.actor}`
    : "-";
  el.statusSummary.innerHTML = `
    <div><dt>会话</dt><dd>${state.selectedSessionId || "-"}</dd></div>
    <div><dt>开始时间</dt><dd>${formatTime(status?.runStartedAt)}</dd></div>
    <div><dt>结束时间</dt><dd>${formatTime(status?.runCompletedAt)}</dd></div>
    <div><dt>事件数</dt><dd>${status?.eventCount ?? 0}</dd></div>
    <div><dt>最新事件</dt><dd>${latest}</dd></div>
  `;
}

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function closeSessionStream() {
  window.clearTimeout(state.streamReconnectHandle);
  state.streamReconnectHandle = null;
  if (state.stream) {
    state.stream.close();
    state.stream = null;
  }
}

function resetCurrentSessionState() {
  state.currentMessages = [];
  state.currentEvents = [];
  state.currentStatus = null;
  state.streamAfterEventId = null;
  el.activeSessionLabel.textContent = state.selectedSessionId
    ? `会话 ${state.selectedSessionId.slice(0, 8)}`
    : "未选择会话";
  renderMessages(state.currentMessages);
  renderEvents(state.currentEvents);
  renderStatus(state.currentStatus);
}

function syncStatusFromEvent(event) {
  if (!state.currentStatus) {
    state.currentStatus = {
      runStatus: "idle",
      runStartedAt: null,
      runCompletedAt: null,
      eventCount: 0,
      latestEvent: null,
    };
  }

  state.currentStatus.eventCount = state.currentEvents.length;
  state.currentStatus.latestEvent = {
    eventId: event.eventId,
    actor: event.actor,
    payloadType: event.payload.type,
    timestamp: event.timestamp,
  };

  if (event.payload.type === "userMessage") {
    state.currentStatus.runStatus = "running";
    state.currentStatus.runStartedAt = state.currentStatus.runStartedAt || event.timestamp;
    state.currentStatus.runCompletedAt = null;
  } else if (event.payload.type === "finalAnswer") {
    state.currentStatus.runStatus = "completed";
    state.currentStatus.runCompletedAt = event.timestamp;
  }
}

function appendMessageFromEvent(event) {
  if (event.payload.type === "userMessage") {
    state.currentMessages.push({
      role: "user",
      content: event.payload.content,
      timestamp: event.timestamp,
    });
  } else if (event.payload.type === "finalAnswer") {
    state.currentMessages.push({
      role: "assistant",
      content: event.payload.answer,
      timestamp: event.timestamp,
    });
  }
}

function handleIncomingEvent(event) {
  if (event.sessionId !== state.selectedSessionId) {
    return;
  }

  if (state.currentEvents.some((item) => item.eventId === event.eventId)) {
    return;
  }

  state.currentEvents.push(event);
  state.streamAfterEventId = event.eventId;
  appendMessageFromEvent(event);
  syncStatusFromEvent(event);
  renderMessages(state.currentMessages);
  renderEvents(state.currentEvents);
  renderStatus(state.currentStatus);
}

function scheduleStreamReconnect() {
  window.clearTimeout(state.streamReconnectHandle);
  if (!state.selectedSessionId) return;
  state.streamReconnectHandle = window.setTimeout(() => {
    openSessionStream().catch((error) => showToast(error.message, true));
  }, 1000);
}

async function openSessionStream() {
  closeSessionStream();
  if (!state.selectedSessionId) return;

  const query = state.streamAfterEventId
    ? `after=${encodeURIComponent(state.streamAfterEventId)}`
    : "includeHistory=true";
  const stream = new EventSource(`/v1/sessions/${state.selectedSessionId}/stream?${query}`);
  state.stream = stream;

  stream.addEventListener("session_event", (message) => {
    try {
      handleIncomingEvent(JSON.parse(message.data));
    } catch (error) {
      console.error("failed to parse SSE event", error);
    }
  });

  stream.addEventListener("done", async () => {
    if (!state.currentStatus) {
      state.currentStatus = {
        runStatus: "completed",
        runStartedAt: null,
        runCompletedAt: new Date().toISOString(),
        eventCount: state.currentEvents.length,
        latestEvent: null,
      };
    } else {
      state.currentStatus.runStatus = "completed";
      state.currentStatus.runCompletedAt =
        state.currentStatus.runCompletedAt || new Date().toISOString();
    }
    renderStatus(state.currentStatus);
    await loadSessions();
    closeSessionStream();
    scheduleStreamReconnect();
  });

  stream.onerror = () => {
    if (state.stream !== stream) {
      return;
    }
    closeSessionStream();
    scheduleStreamReconnect();
  };
}

async function loadSessions(selectLatest = false) {
  const data = await api("/v1/sessions");
  if (!state.selectedSessionId && data.sessions.length) {
    state.selectedSessionId = data.sessions[0].sessionId;
  }
  if (selectLatest && data.sessions.length) {
    state.selectedSessionId = data.sessions[data.sessions.length - 1].sessionId;
  }
  renderSessions(data.sessions);
  return data.sessions;
}

async function loadCurrentSession() {
  if (!state.selectedSessionId) {
    closeSessionStream();
    resetCurrentSessionState();
    return;
  }

  el.activeSessionLabel.textContent = `会话 ${state.selectedSessionId.slice(0, 8)}`;

  const [messages, events, status] = await Promise.all([
    api(`/v1/sessions/${state.selectedSessionId}/messages`),
    api(`/v1/sessions/${state.selectedSessionId}/events`),
    api(`/v1/sessions/${state.selectedSessionId}/status`),
  ]);

  state.currentMessages = messages.messages;
  state.currentEvents = events.events;
  state.currentStatus = status;
  state.streamAfterEventId = status.latestEvent?.eventId || null;
  renderMessages(state.currentMessages);
  renderEvents(state.currentEvents);
  renderStatus(state.currentStatus);
  await openSessionStream();
}

async function selectSession(sessionId) {
  if (sessionId === state.selectedSessionId && state.stream) {
    return;
  }
  closeSessionStream();
  state.selectedSessionId = sessionId;
  resetCurrentSessionState();
  try {
    await loadSessions();
    await loadCurrentSession();
  } catch (error) {
    showToast(error.message, true);
  }
}

async function createSession() {
  el.createSessionBtn.disabled = true;
  try {
    const name = el.sessionName.value.trim();
    const body = name ? { metadata: { name, tags: [] } } : {};
    await api("/v1/sessions", {
      method: "POST",
      body: JSON.stringify(body),
    });
    el.sessionName.value = "";
    await loadSessions(true);
    await loadCurrentSession();
    showToast("会话已创建");
  } catch (error) {
    showToast(error.message, true);
  } finally {
    el.createSessionBtn.disabled = false;
  }
}

async function sendMessage(event) {
  event.preventDefault();
  if (!state.selectedSessionId) {
    showToast("请先创建或选择一个会话", true);
    return;
  }

  const content = el.messageInput.value.trim();
  if (!content) {
    showToast("消息不能为空", true);
    return;
  }

  el.sendBtn.disabled = true;
  try {
    const body = { content };
    if (el.provider.value) body.provider = el.provider.value;
    if (el.model.value.trim()) body.model = el.model.value.trim();
    if (el.maxIterations.value.trim()) {
      body.maxIterations = Number(el.maxIterations.value);
    }

    const data = await api(`/v1/sessions/${state.selectedSessionId}/messages`, {
      method: "POST",
      body: JSON.stringify(body),
    });
    el.messageInput.value = "";

    state.currentStatus = {
      ...(state.currentStatus || {}),
      sessionId: state.selectedSessionId,
      runStatus: "running",
      runStartedAt: new Date().toISOString(),
      runCompletedAt: null,
      eventCount: state.currentEvents.length,
      latestEvent: state.currentStatus?.latestEvent || null,
    };
    renderStatus(state.currentStatus);
    await loadSessions();
    showToast(`任务已提交：${data.runId}`);
  } catch (error) {
    showToast(error.message, true);
  } finally {
    el.sendBtn.disabled = false;
  }
}

el.createSessionBtn.addEventListener("click", createSession);
el.refreshSessionsBtn.addEventListener("click", () => loadSessions().catch((error) => showToast(error.message, true)));
el.refreshDetailBtn.addEventListener("click", () => loadCurrentSession().catch((error) => showToast(error.message, true)));
el.messageForm.addEventListener("submit", sendMessage);

loadSessions(true)
  .then(() => loadCurrentSession())
  .catch((error) => showToast(error.message, true));

window.addEventListener("beforeunload", closeSessionStream);
