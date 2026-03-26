const API_BASE = window.location.origin;
const POLL_INTERVAL_MS = 5000;
const PAGE_SIZE = 50;

const state = {
  page: 0,
  filters: {},
  transactions: [],
  sortKey: "slot",
  sortDir: "desc",
  sqlRunning: false,
  lastQueryResult: null,
};

// ── DOM REFS ──────────────────────────────────────────────────────

const el = (id) => document.getElementById(id);

const txBody = el("tx-body");
const txCount = el("tx-count");
const pageLabel = el("page-label");
const modalOverlay = el("modal-overlay");
const modalBody = el("modal-body");
const toastEl = el("toast");

// ── INIT ─────────────────────────────────────────────────────────

document.addEventListener("DOMContentLoaded", () => {
  initTabs();
  initFilters();
  initPagination();
  initSortHeaders();
  initModal();
  initSqlTool();
  initPrebuiltQueries();
  initQueryHistory();
  initExports();

  loadTransactions();
  loadStats();

  setInterval(() => {
    loadTransactions();
    loadStats();
  }, POLL_INTERVAL_MS);
});

// ── TABS ──────────────────────────────────────────────────────────

function initTabs() {
  document.querySelectorAll(".tab-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const target = btn.dataset.tab;
      document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
      document.querySelectorAll(".tab-panel").forEach((p) => p.classList.remove("active"));
      btn.classList.add("active");
      el(`tab-${target}`).classList.add("active");
    });
  });
}

// ── STATS ─────────────────────────────────────────────────────────

async function loadStats() {
  try {
    const data = await apiFetch("/stats");
    el("stat-total").textContent = formatNumber(data.postgres.total_transactions);
    el("stat-slot").textContent = formatNumber(data.postgres.last_indexed_slot);
    el("stat-programs").textContent = formatNumber(data.postgres.programs_indexed);
    el("stat-ch-total").textContent = formatNumber(data.clickhouse_total);
  } catch (e) {
    console.error("Failed to load stats:", e);
  }
}

// ── TRANSACTIONS ──────────────────────────────────────────────────

function initFilters() {
  el("btn-filter").addEventListener("click", () => {
    state.page = 0;
    state.filters = buildFilters();
    loadTransactions();
  });

  el("btn-reset").addEventListener("click", () => {
    el("filter-instruction").value = "";
    el("filter-signer").value = "";
    el("filter-start-slot").value = "";
    el("filter-end-slot").value = "";
    state.filters = {};
    state.page = 0;
    loadTransactions();
  });

  ["filter-instruction", "filter-signer", "filter-start-slot", "filter-end-slot"].forEach((id) => {
    el(id).addEventListener("keydown", (e) => {
      if (e.key === "Enter") el("btn-filter").click();
    });
  });
}

function buildFilters() {
  const f = {};
  const instruction = el("filter-instruction").value.trim();
  const signer = el("filter-signer").value.trim();
  const startSlot = el("filter-start-slot").value.trim();
  const endSlot = el("filter-end-slot").value.trim();
  if (instruction) f.instruction = instruction;
  if (signer) f.signer = signer;
  if (startSlot) f.start_slot = parseInt(startSlot, 10);
  if (endSlot) f.end_slot = parseInt(endSlot, 10);
  return f;
}

function initPagination() {
  el("btn-prev").addEventListener("click", () => {
    if (state.page > 0) {
      state.page--;
      loadTransactions();
    }
  });

  el("btn-next").addEventListener("click", () => {
    if (state.transactions.length === PAGE_SIZE) {
      state.page++;
      loadTransactions();
    }
  });
}

function initSortHeaders() {
  document.querySelectorAll("thead th[data-sort]").forEach((th) => {
    th.addEventListener("click", () => {
      const key = th.dataset.sort;
      if (state.sortKey === key) {
        state.sortDir = state.sortDir === "asc" ? "desc" : "asc";
      } else {
        state.sortKey = key;
        state.sortDir = "desc";
      }
      state.page = 0;
      loadTransactions();
    });
  });
}

async function loadTransactions() {
  const params = new URLSearchParams({
    limit: PAGE_SIZE,
    offset: state.page * PAGE_SIZE,
    ...state.filters,
  });

  txBody.innerHTML = `<tr class="loading-row"><td colspan="5"><span class="loading-spinner"></span></td></tr>`;

  try {
    const data = await apiFetch(`/transactions?${params}`);
    state.transactions = data;
    renderTransactions(data);
    txCount.textContent = `${data.length} rows`;
    pageLabel.textContent = `Page ${state.page + 1}`;
    el("btn-prev").disabled = state.page === 0;
    el("btn-next").disabled = data.length < PAGE_SIZE;
  } catch (e) {
    txBody.innerHTML = `<tr><td colspan="5" style="text-align:center;color:var(--error);padding:2rem;">${escapeHtml(e.message)}</td></tr>`;
    showToast(`Failed to load transactions: ${e.message}`, "error");
  }
}

function renderTransactions(transactions) {
  if (transactions.length === 0) {
    txBody.innerHTML = `
      <tr><td colspan="5">
        <div class="empty-state">
          <div class="empty-icon">🌸</div>
          <p>No transactions found</p>
        </div>
      </td></tr>
    `;
    return;
  }

  txBody.innerHTML = transactions
    .map(
      (tx) => `
      <tr data-sig="${escapeHtml(tx.signature)}">
        <td class="mono" title="${escapeHtml(tx.signature)}">${truncate(tx.signature, 16)}</td>
        <td class="mono">${formatNumber(tx.slot)}</td>
        <td><span class="badge badge-pink">${escapeHtml(tx.instruction.name)}</span></td>
        <td class="mono" title="${escapeHtml(tx.signer)}">${truncate(tx.signer, 14)}</td>
        <td>${tx.timestamp ? formatTime(tx.timestamp) : "—"}</td>
      </tr>
    `
    )
    .join("");

  txBody.querySelectorAll("tr").forEach((row) => {
    row.addEventListener("click", () => {
      const sig = row.dataset.sig;
      if (sig) openTransactionModal(sig);
    });
  });
}

// ── MODAL ─────────────────────────────────────────────────────────

function initModal() {
  el("modal-close").addEventListener("click", closeModal);
  modalOverlay.addEventListener("click", (e) => {
    if (e.target === modalOverlay) closeModal();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeModal();
  });
}

async function openTransactionModal(signature) {
  modalBody.innerHTML = `<div style="text-align:center;padding:2rem"><span class="loading-spinner"></span></div>`;
  modalOverlay.classList.add("visible");

  try {
    const tx = await apiFetch(`/transaction/${signature}`);
    renderModalContent(tx);
  } catch (e) {
    modalBody.innerHTML = `<div style="color:var(--error);padding:1rem">${escapeHtml(e.message)}</div>`;
  }
}

function renderModalContent(tx) {
  const accountsArr = Array.isArray(tx.accounts)
    ? tx.accounts
    : Object.values(tx.accounts || {});

  modalBody.innerHTML = `
    <div class="detail-section">
      <div class="detail-section-title">Overview</div>
      <div class="detail-grid">
        <div class="detail-field">
          <div class="field-label">Signature</div>
          <div class="field-value">${escapeHtml(tx.signature)}</div>
        </div>
        <div class="detail-field">
          <div class="field-label">Slot</div>
          <div class="field-value">${formatNumber(tx.slot)}</div>
        </div>
        <div class="detail-field">
          <div class="field-label">Instruction</div>
          <div class="field-value"><span class="badge badge-pink">${escapeHtml(tx.instruction.name)}</span></div>
        </div>
        <div class="detail-field">
          <div class="field-label">Signer</div>
          <div class="field-value">${escapeHtml(tx.signer)}</div>
        </div>
        <div class="detail-field">
          <div class="field-label">Program</div>
          <div class="field-value">${escapeHtml(tx.program_id)}</div>
        </div>
        <div class="detail-field">
          <div class="field-label">Timestamp</div>
          <div class="field-value">${tx.timestamp ? formatTime(tx.timestamp) : "—"}</div>
        </div>
      </div>
    </div>

    <div class="detail-section">
      <div class="detail-section-title">Instruction Args</div>
      <div class="json-block">${syntaxHighlight(tx.instruction.args)}</div>
    </div>

    ${
      accountsArr.length > 0
        ? `
      <div class="detail-section">
        <div class="detail-section-title">Accounts</div>
        <div class="json-block">${syntaxHighlight(accountsArr)}</div>
      </div>`
        : ""
    }
  `;
}

function closeModal() {
  modalOverlay.classList.remove("visible");
}

// ── SQL TOOL ──────────────────────────────────────────────────────

function initSqlTool() {
  el("btn-run-sql").addEventListener("click", runSqlQuery);
  el("sql-input").addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      runSqlQuery();
    }
  });
  el("btn-clear-sql").addEventListener("click", () => {
    el("sql-input").value = "";
  });
}

async function runSqlQuery() {
  if (state.sqlRunning) return;

  const sql = el("sql-input").value.trim();
  if (!sql) {
    showToast("Please enter a SQL query", "error");
    return;
  }

  const database = el("db-select").value;
  state.sqlRunning = true;
  el("sql-run-label").textContent = "Running…";
  el("btn-run-sql").disabled = true;

  const wrap = el("results-table-wrap");
  wrap.innerHTML = `<div class="empty-state"><span class="loading-spinner"></span></div>`;
  el("results-info").textContent = "Executing…";
  el("results-time").textContent = "";

  try {
    const result = await apiFetch("/query", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sql, database }),
    });

    state.lastQueryResult = result;
    renderQueryResults(result);
    addToQueryHistory(sql, database, result.row_count, result.execution_time_ms);
  } catch (e) {
    wrap.innerHTML = `<div class="empty-state"><p style="color:var(--error)">${escapeHtml(e.message)}</p></div>`;
    el("results-info").textContent = "Query failed";
    showToast(e.message, "error");
  } finally {
    state.sqlRunning = false;
    el("sql-run-label").textContent = "▶ Run";
    el("btn-run-sql").disabled = false;
  }
}

function renderQueryResults(result) {
  el("results-info").textContent = `${result.row_count} rows`;
  el("results-time").textContent = `${result.execution_time_ms}ms`;

  const wrap = el("results-table-wrap");

  if (result.rows.length === 0) {
    wrap.innerHTML = `<div class="empty-state"><div class="empty-icon">📭</div><p>Query returned 0 rows</p></div>`;
    return;
  }

  const columns = Object.keys(result.rows[0]);

  const headerHtml = columns.map((c) => `<th>${escapeHtml(c)}</th>`).join("");
  const bodyHtml = result.rows
    .map((row) => {
      const cells = columns
        .map((c) => {
          const val = row[c];
          const display =
            val === null || val === undefined
              ? '<span style="color:var(--text-dim)">null</span>'
              : typeof val === "object"
              ? `<span class="mono" style="font-size:0.72rem">${escapeHtml(JSON.stringify(val))}</span>`
              : escapeHtml(String(val));
          return `<td>${display}</td>`;
        })
        .join("");
      return `<tr>${cells}</tr>`;
    })
    .join("");

  wrap.innerHTML = `
    <table>
      <thead><tr>${headerHtml}</tr></thead>
      <tbody>${bodyHtml}</tbody>
    </table>
  `;
}

// ── PREBUILT QUERIES ──────────────────────────────────────────────

function initPrebuiltQueries() {
  document.querySelectorAll(".prebuilt-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      el("sql-input").value = btn.dataset.sql;
      el("db-select").value = btn.dataset.db;

      document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
      document.querySelectorAll(".tab-panel").forEach((p) => p.classList.remove("active"));
      document.querySelector('[data-tab="sql"]').classList.add("active");
      el("tab-sql").classList.add("active");

      runSqlQuery();
    });
  });
}

// ── QUERY HISTORY ─────────────────────────────────────────────────

const HISTORY_KEY = "bubblegum_query_history";
const MAX_HISTORY = 30;

function initQueryHistory() {
  renderQueryHistory();

  el("btn-clear-history").addEventListener("click", () => {
    localStorage.removeItem(HISTORY_KEY);
    renderQueryHistory();
    showToast("History cleared", "info");
  });
}

function addToQueryHistory(sql, db, rowCount, timeMs) {
  const history = getQueryHistory();
  const entry = {
    sql,
    db,
    rowCount,
    timeMs,
    ts: Date.now(),
  };
  history.unshift(entry);
  const trimmed = history.slice(0, MAX_HISTORY);
  localStorage.setItem(HISTORY_KEY, JSON.stringify(trimmed));
  renderQueryHistory();
}

function getQueryHistory() {
  try {
    return JSON.parse(localStorage.getItem(HISTORY_KEY) || "[]");
  } catch {
    return [];
  }
}

function renderQueryHistory() {
  const list = el("history-list");
  const history = getQueryHistory();

  if (history.length === 0) {
    list.innerHTML = `<div class="empty-state" style="padding:1.5rem;"><p>No history yet</p></div>`;
    return;
  }

  list.innerHTML = history
    .map(
      (entry, i) => `
      <div class="history-item" data-index="${i}">
        <div class="history-sql">${escapeHtml(entry.sql)}</div>
        <div class="history-meta">
          <span>${entry.db}</span>
          <span>${entry.rowCount} rows</span>
          <span>${entry.timeMs}ms</span>
          <span>${timeAgo(entry.ts)}</span>
        </div>
      </div>
    `
    )
    .join("");

  list.querySelectorAll(".history-item").forEach((item) => {
    item.addEventListener("click", () => {
      const entry = history[parseInt(item.dataset.index, 10)];
      el("sql-input").value = entry.sql;
      el("db-select").value = entry.db;
    });
  });
}

// ── EXPORTS ───────────────────────────────────────────────────────

function initExports() {
  el("btn-export-csv").addEventListener("click", () => {
    if (state.transactions.length === 0) {
      showToast("No transactions to export", "error");
      return;
    }
    exportToCsv(state.transactions);
  });

  el("btn-export-json").addEventListener("click", () => {
    if (!state.lastQueryResult || state.lastQueryResult.rows.length === 0) {
      showToast("No query results to export", "error");
      return;
    }
    exportToJson(state.lastQueryResult.rows);
  });
}

function exportToCsv(rows) {
  if (rows.length === 0) return;
  const cols = ["signature", "slot", "instruction", "signer", "timestamp"];
  const lines = [
    cols.join(","),
    ...rows.map((r) =>
      [
        csvEscape(r.signature),
        r.slot,
        csvEscape(r.instruction?.name || ""),
        csvEscape(r.signer),
        r.timestamp || "",
      ].join(",")
    ),
  ];
  downloadFile(lines.join("\n"), "transactions.csv", "text/csv");
  showToast("CSV downloaded", "success");
}

function exportToJson(rows) {
  downloadFile(JSON.stringify(rows, null, 2), "query_results.json", "application/json");
  showToast("JSON downloaded", "success");
}

function downloadFile(content, filename, mime) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

function csvEscape(val) {
  if (val == null) return "";
  const s = String(val);
  if (s.includes(",") || s.includes('"') || s.includes("\n")) {
    return `"${s.replace(/"/g, '""')}"`;
  }
  return s;
}

// ── API ───────────────────────────────────────────────────────────

async function apiFetch(path, options = {}) {
  const res = await fetch(`${API_BASE}${path}`, options);
  if (!res.ok) {
    let msg = `HTTP ${res.status}`;
    try {
      const body = await res.json();
      msg = body.error || msg;
    } catch (_) {}
    throw new Error(msg);
  }
  return res.json();
}

// ── TOAST ─────────────────────────────────────────────────────────

let toastTimer = null;

function showToast(message, type = "info") {
  toastEl.textContent = message;
  toastEl.className = `toast-${type}`;
  toastEl.classList.add("visible");

  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastEl.classList.remove("visible");
  }, 3500);
}

// ── UTILS ─────────────────────────────────────────────────────────

function escapeHtml(str) {
  if (str == null) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function truncate(str, len) {
  if (!str) return "";
  return str.length > len ? str.slice(0, len) + "…" : str;
}

function formatNumber(n) {
  if (n == null) return "—";
  return Number(n).toLocaleString();
}

function formatTime(unixTs) {
  const d = new Date(unixTs * 1000);
  return d.toLocaleString();
}

function timeAgo(ts) {
  const diff = Date.now() - ts;
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

function syntaxHighlight(json) {
  if (json == null) return "null";
  let str;
  try {
    str = typeof json === "string" ? json : JSON.stringify(json, null, 2);
  } catch {
    return String(json);
  }
  return escapeHtml(str)
    .replace(/"([^"]+)":/g, '<span style="color:var(--pink-soft)">"$1"</span>:')
    .replace(/: "([^"]*)"/g, ': <span style="color:var(--pink-pale)">"$1"</span>')
    .replace(/: (\d+)/g, ': <span style="color:var(--success)">$1</span>')
    .replace(/: (true|false)/g, ': <span style="color:var(--warning)">$1</span>')
    .replace(/: (null)/g, ': <span style="color:var(--text-dim)">$1</span>');
}