const state = {
  config: null,
  editor: null,
  currentSession: null,
  currentPath: "",
  currentMode: "groovy",
  timeline: [],
  expanded: new Set([""]),
  fileCache: new Map()
};

const el = {
  treeRoot: document.getElementById("treeRoot"),
  workspacePath: document.getElementById("workspacePath"),
  activePath: document.getElementById("activePath"),
  statusText: document.getElementById("statusText"),
  modeBtn: document.getElementById("modeBtn"),
  saveBtn: document.getElementById("saveBtn"),
  snapshotBtn: document.getElementById("snapshotBtn"),
  reloadTimelineBtn: document.getElementById("reloadTimelineBtn"),
  refreshTreeBtn: document.getElementById("refreshTreeBtn"),
  timelineList: document.getElementById("timelineList")
};

function setStatus(text, isError = false) {
  el.statusText.textContent = text;
  el.statusText.style.color = isError ? "var(--danger)" : "var(--muted)";
}

async function api(url, options = {}) {
  const opts = {
    headers: { "Content-Type": "application/json" },
    ...options
  };
  const res = await fetch(url, opts);
  const data = await res.json();
  if (!res.ok) {
    throw new Error(data.error || "Unknown API error");
  }
  return data;
}

function initMonaco() {
  return new Promise((resolve) => {
    require.config({ paths: { vs: "https://cdn.jsdelivr.net/npm/monaco-editor@0.52.2/min/vs" } });
    require(["vs/editor/editor.main"], () => {
      monaco.editor.defineTheme("oleh-aqua", {
        base: "vs-dark",
        inherit: true,
        rules: [
          { token: "comment", foreground: "6EA272" },
          { token: "keyword", foreground: "6AB6FF" },
          { token: "string", foreground: "E9C06E" }
        ],
        colors: {
          "editor.background": "#0A152A",
          "editorLineNumber.foreground": "#4B6D9E",
          "editorLineNumber.activeForeground": "#8AD8FF",
          "editorCursor.foreground": "#7DFFD8",
          "editor.selectionBackground": "#21578566"
        }
      });

      state.editor = monaco.editor.create(document.getElementById("editor"), {
        value: "// Open a .docx or .groovy file from the left tree.",
        language: "groovy",
        theme: "oleh-aqua",
        fontFamily: "'JetBrains Mono', monospace",
        fontLigatures: true,
        minimap: { enabled: true },
        smoothScrolling: true,
        automaticLayout: true
      });
      resolve();
    });
  });
}

function setEditorMode(mode) {
  state.currentMode = mode === "text" ? "text" : "groovy";
  const language = state.currentMode === "groovy" ? "groovy" : "plaintext";
  monaco.editor.setModelLanguage(state.editor.getModel(), language);
  el.modeBtn.textContent = `Mode: ${state.currentMode === "groovy" ? "Groovy" : "Text"}`;
}

async function loadConfig() {
  state.config = await api("/api/config");
  el.workspacePath.textContent = state.config.workspace_root;
}

async function loadTree(path = "") {
  const query = path ? `?path=${encodeURIComponent(path)}` : "";
  const data = await api(`/api/tree${query}`);
  state.fileCache.set(path, data.entries);
  renderTree();
}

function icon(entry) {
  if (entry.is_dir) {
    return state.expanded.has(entry.path) ? "▾" : "▸";
  }
  if (entry.path.toLowerCase().endsWith(".docx")) return "🧩";
  if (entry.path.toLowerCase().endsWith(".groovy")) return "⚙";
  return "•";
}

function renderTreeNode(parentPath, depth = 0) {
  const entries = state.fileCache.get(parentPath) || [];
  const container = document.createElement("div");
  if (depth > 0) container.className = "tree-children";

  for (const entry of entries) {
    const row = document.createElement("div");
    row.className = `tree-item ${entry.is_dir ? "dir" : "file"}`;
    if (!entry.is_dir && entry.path === state.currentPath) row.classList.add("active");
    row.style.paddingLeft = `${10 + depth * 8}px`;
    row.innerHTML = `<span>${icon(entry)}</span><span>${entry.name}</span>`;

    row.addEventListener("click", async () => {
      if (entry.is_dir) {
        if (state.expanded.has(entry.path)) {
          state.expanded.delete(entry.path);
        } else {
          state.expanded.add(entry.path);
          if (!state.fileCache.has(entry.path)) {
            await loadTree(entry.path);
          }
        }
        renderTree();
      } else {
        await openFile(entry.path);
      }
    });

    container.appendChild(row);
    if (entry.is_dir && state.expanded.has(entry.path)) {
      container.appendChild(renderTreeNode(entry.path, depth + 1));
    }
  }
  return container;
}

function renderTree() {
  el.treeRoot.innerHTML = "";
  el.treeRoot.appendChild(renderTreeNode(""));
}

async function openFile(path) {
  try {
    setStatus(`Opening ${path}...`);
    const payload = await api("/api/open", {
      method: "POST",
      body: JSON.stringify({ path, mode: state.currentMode })
    });
    state.currentSession = payload.session_id;
    state.currentPath = payload.path;
    state.editor.setValue(payload.content || "");
    setEditorMode(payload.mode || "groovy");
    el.activePath.textContent = payload.path;
    localStorage.setItem("oge.lastSessionId", state.currentSession);
    setStatus(`Opened ${payload.path}`);
    renderTree();
    await loadTimeline();
  } catch (err) {
    setStatus(err.message, true);
  }
}

async function saveCurrent() {
  if (!state.currentSession) {
    setStatus("No active session to save.", true);
    return;
  }
  try {
    setStatus("Saving...");
    const content = state.editor.getValue();
    await api("/api/save", {
      method: "POST",
      body: JSON.stringify({
        session_id: state.currentSession,
        content,
        mode: state.currentMode
      })
    });
    setStatus(`Saved ${state.currentPath}`);
    await loadTimeline();
  } catch (err) {
    setStatus(err.message, true);
  }
}

async function snapshot() {
  if (!state.currentSession) {
    setStatus("No active session for snapshot.", true);
    return;
  }
  try {
    const summary = prompt("Snapshot summary", "Checkpoint");
    if (summary === null) return;
    await api("/api/timeline", {
      method: "POST",
      body: JSON.stringify({ session_id: state.currentSession, summary })
    });
    setStatus("Snapshot stored.");
    await loadTimeline();
  } catch (err) {
    setStatus(err.message, true);
  }
}

async function loadTimeline() {
  if (!state.currentSession) {
    el.timelineList.innerHTML = "";
    return;
  }

  const data = await api(`/api/timeline?session_id=${encodeURIComponent(state.currentSession)}`);
  state.timeline = data.entries || [];
  renderTimeline();
}

function renderTimeline() {
  el.timelineList.innerHTML = "";
  if (!state.timeline.length) {
    el.timelineList.innerHTML = `<div class="timeline-item"><div class="summary">No timeline entries yet.</div></div>`;
    return;
  }

  for (const item of state.timeline) {
    const card = document.createElement("div");
    card.className = "timeline-item";
    card.innerHTML = `
      <div class="meta">#${item.id} · ${new Date(item.created_at).toLocaleString()}</div>
      <div class="summary">${item.summary}</div>
      <button class="btn ghost">Load This Version</button>
    `;
    card.querySelector("button").addEventListener("click", () => revertTo(item.id));
    el.timelineList.appendChild(card);
  }
}

async function revertTo(entryId) {
  if (!state.currentSession) return;
  try {
    const data = await api("/api/timeline/revert", {
      method: "POST",
      body: JSON.stringify({
        session_id: state.currentSession,
        entry_id: entryId
      })
    });
    state.editor.setValue(data.content || "");
    setEditorMode(data.mode || state.currentMode);
    setStatus(`Loaded timeline entry #${entryId}. Save to write file.`);
    await loadTimeline();
  } catch (err) {
    setStatus(err.message, true);
  }
}

async function restoreSession() {
  const sessionId = localStorage.getItem("oge.lastSessionId");
  if (!sessionId) return;
  try {
    const data = await api(`/api/session?session_id=${encodeURIComponent(sessionId)}`);
    state.currentSession = data.session_id;
    state.currentPath = data.path;
    state.editor.setValue(data.content || "");
    setEditorMode(data.mode || "groovy");
    el.activePath.textContent = data.path;
    setStatus(`Restored session from ${new Date(data.updated_at).toLocaleString()}`);
    await loadTimeline();
  } catch (err) {
    setStatus("No previous session restored.");
  }
}

function wireUi() {
  el.modeBtn.addEventListener("click", () => {
    setEditorMode(state.currentMode === "groovy" ? "text" : "groovy");
  });
  el.saveBtn.addEventListener("click", saveCurrent);
  el.snapshotBtn.addEventListener("click", snapshot);
  el.reloadTimelineBtn.addEventListener("click", loadTimeline);
  el.refreshTreeBtn.addEventListener("click", async () => {
    state.fileCache.clear();
    state.expanded = new Set([""]);
    await loadTree("");
    setStatus("Tree refreshed.");
  });
  window.addEventListener("keydown", async (ev) => {
    if (ev.ctrlKey && ev.key.toLowerCase() === "s") {
      ev.preventDefault();
      await saveCurrent();
    }
  });
}

async function start() {
  try {
    await initMonaco();
    wireUi();
    await loadConfig();
    await loadTree("");
    await restoreSession();
    setStatus("Ready.");
  } catch (err) {
    setStatus(err.message || "Startup failed.", true);
  }
}

start();
