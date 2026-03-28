const API_BASE = '/api';

let currentPage = 1;
let currentFilters = {
  instruction: '',
  signer: '',
  start_slot: '',
  end_slot: '',
};
let currentSort = { column: 'slot', order: 'desc' };
let totalRows = 0;
let pollingInterval = null;

const txBody = document.getElementById('tx-body');
const txCountSpan = document.getElementById('tx-count');
const statTotal = document.getElementById('stat-total');
const statSlot = document.getElementById('stat-slot');
const statPrograms = document.getElementById('stat-programs');
const statChTotal = document.getElementById('stat-ch-total');
const filterInstruction = document.getElementById('filter-instruction');
const filterSigner = document.getElementById('filter-signer');
const filterStartSlot = document.getElementById('filter-start-slot');
const filterEndSlot = document.getElementById('filter-end-slot');
const btnFilter = document.getElementById('btn-filter');
const btnReset = document.getElementById('btn-reset');
const btnPrev = document.getElementById('btn-prev');
const btnNext = document.getElementById('btn-next');
const pageLabel = document.getElementById('page-label');
const btnExportCsv = document.getElementById('btn-export-csv');
const modalOverlay = document.getElementById('modal-overlay');
const modalBody = document.getElementById('modal-body');
const modalClose = document.getElementById('modal-close');

// SQL tool elements
const dbSelect = document.getElementById('db-select');
const sqlInput = document.getElementById('sql-input');
const btnRunSql = document.getElementById('btn-run-sql');
const btnClearSql = document.getElementById('btn-clear-sql');
const btnExportJson = document.getElementById('btn-export-json');
const resultsInfo = document.getElementById('results-info');
const resultsTime = document.getElementById('results-time');
const resultsTableWrap = document.getElementById('results-table-wrap');
const historyList = document.getElementById('history-list');
const btnClearHistory = document.getElementById('btn-clear-history');

// Prebuilt query buttons
const prebuiltBtns = document.querySelectorAll('.prebuilt-btn');

// Toast
const toast = document.getElementById('toast');

// Helper functions
function showToast(message, type = 'info') {
  toast.textContent = message;
  toast.className = `visible toast-${type}`;
  setTimeout(() => {
    toast.classList.remove('visible');
  }, 3000);
}

async function fetchStats() {
  try {
    const res = await fetch(`${API_BASE}/stats`);
    if (!res.ok) throw new Error('Failed to fetch stats');
    const data = await res.json();
    statTotal.textContent = data.total_transactions.toLocaleString();
    statSlot.textContent = data.last_indexed_slot.toLocaleString();
    statPrograms.textContent = data.programs.length;
    statChTotal.textContent = data.clickhouse_total.toLocaleString();
  } catch (err) {
    console.error('Stats error:', err);
  }
}

async function fetchTransactions(page = 1) {
  const limit = 50;
  const offset = (page - 1) * limit;

  let url = `${API_BASE}/transactions?limit=${limit}&offset=${offset}`;
  if (currentFilters.instruction) url += `&instruction=${encodeURIComponent(currentFilters.instruction)}`;
  if (currentFilters.signer) url += `&signer=${encodeURIComponent(currentFilters.signer)}`;
  if (currentFilters.start_slot) url += `&start_slot=${currentFilters.start_slot}`;
  if (currentFilters.end_slot) url += `&end_slot=${currentFilters.end_slot}`;

  // sort not supported by backend yet; we'll keep as is

  try {
    const res = await fetch(url);
    if (!res.ok) throw new Error('Failed to fetch transactions');
    const txs = await res.json();
    totalRows = txs.length; // not ideal, but we'll handle pagination with next/prev only
    renderTable(txs);
    txCountSpan.textContent = totalRows;
    pageLabel.textContent = `Page ${page}`;
  } catch (err) {
    console.error('Transactions error:', err);
    showToast('Failed to load transactions', 'error');
  }
}

function renderTable(transactions) {
  if (!txBody) return;
  if (!transactions.length) {
    txBody.innerHTML = '<tr><td colspan="5">No transactions found</td></tr>';
    return;
  }

  txBody.innerHTML = transactions.map(tx => `
    <tr data-signature="${tx.signature}">
      <td class="mono">${tx.signature.slice(0, 8)}…</td>
      <td>${tx.slot}</td>
      <td><span class="badge badge-pink">${tx.instruction_name}</span></td>
      <td class="mono">
        <a href="https://explorer.solana.com/address/${tx.signer}" target="_blank" rel="noopener" onclick="event.stopPropagation()">${tx.signer.slice(0, 8)}…</a>
      </td>
      <td>${new Date(tx.timestamp * 1000).toLocaleString()}</td>
    </tr>
  `).join('');

  // Add click handlers
  document.querySelectorAll('#tx-body tr').forEach(tr => {
    tr.addEventListener('click', () => {
      const sig = tr.dataset.signature;
      if (sig) showTransactionDetail(sig);
    });
  });
}

async function showTransactionDetail(signature) {
  try {
    const res = await fetch(`${API_BASE}/transaction/${signature}`);
    if (!res.ok) throw new Error('Transaction not found');
    const tx = await res.json();

    modalBody.innerHTML = `
      <div class="detail-section">
        <div class="detail-section-title">Signature</div>
        <div class="json-block">${tx.signature}</div>
      </div>
      <div class="detail-section">
        <div class="detail-section-title">Details</div>
        <div class="detail-grid">
          <div class="detail-field"><div class="field-label">Slot</div><div class="field-value">${tx.slot}</div></div>
          <div class="detail-field"><div class="field-label">Time</div><div class="field-value">${new Date(tx.timestamp * 1000).toLocaleString()}</div></div>
          <div class="detail-field"><div class="field-label">Program</div><div class="field-value">${tx.program_id}</div></div>
          <div class="detail-field"><div class="field-label">Signer</div><div class="field-value"><a href="https://explorer.solana.com/address/${tx.signer}" target="_blank" rel="noopener">${tx.signer}</a> <span class="copy-link" onclick="navigator.clipboard.writeText('${tx.signer}').then(()=>showToast('Copied','info'))">copy</span></div></div>
        </div>
      </div>
      <div class="detail-section">
        <div class="detail-section-title">Instruction: ${tx.instruction_name}</div>
        <div class="json-block">${JSON.stringify(tx.instruction, null, 2)}</div>
      </div>
      <div class="detail-section">
        <div class="detail-section-title">Accounts</div>
        <div class="json-block">${JSON.stringify(tx.accounts, null, 2)}</div>
      </div>
    `;
    modalOverlay.classList.add('visible');
  } catch (err) {
    console.error('Detail error:', err);
    showToast('Failed to load transaction details', 'error');
  }
}

function closeModal() {
  modalOverlay.classList.remove('visible');
}

// SQL query execution
async function runSql() {
  const db = dbSelect.value;
  const sql = sqlInput.value.trim();
  if (!sql) {
    showToast('Please enter a SQL query', 'warning');
    return;
  }

  // Add to history
  addToHistory(db, sql);

  // Show loading
  resultsInfo.textContent = 'Executing...';
  resultsTime.textContent = '';
  resultsTableWrap.innerHTML = '<div class="empty-state"><div class="empty-icon">⏳</div><p>Running query...</p></div>';

  try {
    const res = await fetch(`${API_BASE}/sql`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ db, sql }),
    });
    if (!res.ok) {
      const errText = await res.text();
      throw new Error(errText);
    }
    const data = await res.json();
    resultsInfo.textContent = `${data.row_count} rows returned`;
    resultsTime.textContent = `${data.execution_time_ms} ms`;
    renderSqlResults(data.columns, data.rows);
  } catch (err) {
    console.error('SQL error:', err);
    resultsInfo.textContent = 'Error';
    resultsTime.textContent = '';
    resultsTableWrap.innerHTML = `<div class="empty-state"><div class="empty-icon">❌</div><p>${err.message}</p></div>`;
    showToast(err.message, 'error');
  }
}

function renderSqlResults(columns, rows) {
  if (!rows.length) {
    resultsTableWrap.innerHTML = '<div class="empty-state"><div class="empty-icon">📭</div><p>No results</p></div>';
    return;
  }

  const table = document.createElement('table');
  table.className = 'results-table';
  table.style.width = '100%';
  table.style.borderCollapse = 'collapse';

  const thead = document.createElement('thead');
  const headerRow = document.createElement('tr');
  columns.forEach(col => {
    const th = document.createElement('th');
    th.textContent = col;
    th.style.padding = '0.5rem';
    th.style.textAlign = 'left';
    th.style.borderBottom = '1px solid var(--border)';
    headerRow.appendChild(th);
  });
  thead.appendChild(headerRow);
  table.appendChild(thead);

  const tbody = document.createElement('tbody');
  rows.forEach(row => {
    const tr = document.createElement('tr');
    columns.forEach(col => {
      const td = document.createElement('td');
      td.style.padding = '0.5rem';
      td.style.borderBottom = '1px solid var(--border)';
      const val = row[col];
      if (val === null || val === undefined) td.textContent = 'null';
      else if (typeof val === 'object') td.textContent = JSON.stringify(val);
      else td.textContent = String(val);
      tr.appendChild(td);
    });
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);

  resultsTableWrap.innerHTML = '';
  resultsTableWrap.appendChild(table);
}

function addToHistory(db, sql) {
  let history = JSON.parse(localStorage.getItem('sql_history') || '[]');
  history.unshift({ db, sql, timestamp: Date.now() });
  if (history.length > 20) history.pop();
  localStorage.setItem('sql_history', JSON.stringify(history));
  renderHistory();
}

function renderHistory() {
  const history = JSON.parse(localStorage.getItem('sql_history') || '[]');
  if (!history.length) {
    historyList.innerHTML = '<div class="empty-state" style="padding:1.5rem;"><p>No history yet</p></div>';
    return;
  }

  historyList.innerHTML = history.map(item => `
    <div class="history-item" data-db="${item.db}" data-sql="${escapeHtml(item.sql)}">
      <div class="history-sql">${item.sql.slice(0, 60)}${item.sql.length > 60 ? '…' : ''}</div>
      <div class="history-meta">${item.db} · ${new Date(item.timestamp).toLocaleString()}</div>
    </div>
  `).join('');

  document.querySelectorAll('.history-item').forEach(el => {
    el.addEventListener('click', () => {
      const db = el.dataset.db;
      const sql = unescapeHtml(el.dataset.sql);
      dbSelect.value = db;
      sqlInput.value = sql;
    });
  });
}

function clearHistory() {
  localStorage.removeItem('sql_history');
  renderHistory();
  showToast('History cleared', 'info');
}

function escapeHtml(str) {
  return str.replace(/[&<>"]/g, function(m) {
    if (m === '&') return '&amp;';
    if (m === '<') return '&lt;';
    if (m === '>') return '&gt;';
    if (m === '"') return '&quot;';
    return m;
  });
}

function unescapeHtml(str) {
  return str.replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"');
}

function exportCsv() {
  // Build query to export current filtered view (up to 1000 rows)
  let url = `${API_BASE}/transactions?limit=1000&offset=0`;
  if (currentFilters.instruction) url += `&instruction=${encodeURIComponent(currentFilters.instruction)}`;
  if (currentFilters.signer) url += `&signer=${encodeURIComponent(currentFilters.signer)}`;
  if (currentFilters.start_slot) url += `&start_slot=${currentFilters.start_slot}`;
  if (currentFilters.end_slot) url += `&end_slot=${currentFilters.end_slot}`;

  fetch(url)
    .then(res => res.json())
    .then(txs => {
      if (!txs.length) {
        showToast('No data to export', 'warning');
        return;
      }
      const headers = ['signature', 'slot', 'timestamp', 'program_id', 'instruction_name', 'signer', 'accounts'];
      const rows = txs.map(tx => [
        tx.signature,
        tx.slot,
        new Date(tx.timestamp * 1000).toISOString(),
        tx.program_id,
        tx.instruction_name,
        tx.signer,
        JSON.stringify(tx.accounts)
      ]);
      const csvContent = [headers, ...rows].map(row => row.map(cell => `"${String(cell).replace(/"/g, '""')}"`).join(',')).join('\n');
      const blob = new Blob([csvContent], { type: 'text/csv' });
      const link = document.createElement('a');
      link.href = URL.createObjectURL(blob);
      link.download = `transactions_${new Date().toISOString()}.csv`;
      link.click();
      URL.revokeObjectURL(link.href);
    })
    .catch(err => {
      console.error('Export error:', err);
      showToast('Failed to export', 'error');
    });
}

function exportJson() {
  const db = dbSelect.value;
  const sql = sqlInput.value.trim();
  if (!sql) {
    showToast('No query to export', 'warning');
    return;
  }
  fetch(`${API_BASE}/sql`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ db, sql }),
  })
    .then(res => res.json())
    .then(data => {
      const json = JSON.stringify(data, null, 2);
      const blob = new Blob([json], { type: 'application/json' });
      const link = document.createElement('a');
      link.href = URL.createObjectURL(blob);
      link.download = `query_${new Date().toISOString()}.json`;
      link.click();
      URL.revokeObjectURL(link.href);
    })
    .catch(err => {
      console.error('Export error:', err);
      showToast('Failed to export', 'error');
    });
}

// Event listeners
btnFilter.addEventListener('click', () => {
  currentFilters = {
    instruction: filterInstruction.value,
    signer: filterSigner.value,
    start_slot: filterStartSlot.value,
    end_slot: filterEndSlot.value,
  };
  currentPage = 1;
  fetchTransactions(currentPage);
});

btnReset.addEventListener('click', () => {
  filterInstruction.value = '';
  filterSigner.value = '';
  filterStartSlot.value = '';
  filterEndSlot.value = '';
  currentFilters = { instruction: '', signer: '', start_slot: '', end_slot: '' };
  currentPage = 1;
  fetchTransactions(currentPage);
});

btnPrev.addEventListener('click', () => {
  if (currentPage > 1) {
    currentPage--;
    fetchTransactions(currentPage);
  }
});

btnNext.addEventListener('click', () => {
  if (totalRows >= 50) {
    currentPage++;
    fetchTransactions(currentPage);
  } else {
    showToast('No more pages', 'info');
  }
});

btnExportCsv.addEventListener('click', exportCsv);

modalClose.addEventListener('click', closeModal);
modalOverlay.addEventListener('click', (e) => {
  if (e.target === modalOverlay) closeModal();
});

btnRunSql.addEventListener('click', runSql);
btnClearSql.addEventListener('click', () => { sqlInput.value = ''; });
btnExportJson.addEventListener('click', exportJson);
btnClearHistory.addEventListener('click', clearHistory);

prebuiltBtns.forEach(btn => {
  btn.addEventListener('click', () => {
    const db = btn.dataset.db;
    const sql = btn.dataset.sql;
    if (db && sql) {
      dbSelect.value = db;
      sqlInput.value = sql;
      runSql();
    }
  });
});

sqlInput.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
    e.preventDefault();
    runSql();
  }
});

// Tab switching
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    const tabId = btn.dataset.tab;
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
    document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
    document.getElementById(`tab-${tabId}`).classList.add('active');
  });
});

// Initial load
fetchStats();
fetchTransactions(1);
pollingInterval = setInterval(() => {
  fetchStats();
  if (document.querySelector('.tab-btn.active').dataset.tab === 'transactions') {
    fetchTransactions(currentPage);
  }
}, 5000);

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
  if (pollingInterval) clearInterval(pollingInterval);
});