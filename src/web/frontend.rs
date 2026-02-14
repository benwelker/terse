//! Embedded HTML/CSS/JS frontend for the terse web dashboard.
//!
//! The entire SPA is compiled into the binary as a string constant.
//! No external assets, no build tools, no CDN dependencies.

/// The complete single-page dashboard HTML.
pub const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>terse Dashboard</title>
<style>
:root {
  --bg: #0d1117;
  --surface: #161b22;
  --border: #30363d;
  --text: #e6edf3;
  --text-muted: #8b949e;
  --accent: #58a6ff;
  --green: #3fb950;
  --yellow: #d29922;
  --red: #f85149;
  --purple: #bc8cff;
  --cyan: #39d2c0;
  --radius: 8px;
  --font: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
  --mono: 'SF Mono', 'Cascadia Code', 'Fira Code', monospace;
}

* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  background: var(--bg);
  color: var(--text);
  font-family: var(--font);
  font-size: 14px;
  line-height: 1.5;
}

/* Layout */
.app {
  max-width: 1200px;
  margin: 0 auto;
  padding: 24px;
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 24px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--border);
}

header h1 {
  font-size: 24px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 10px;
}

header h1 .logo {
  color: var(--accent);
  font-family: var(--mono);
  font-weight: 700;
}

header .subtitle {
  color: var(--text-muted);
  font-size: 13px;
}

.health-badges {
  display: flex;
  gap: 8px;
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 500;
  background: var(--surface);
  border: 1px solid var(--border);
}

.badge.ok { border-color: var(--green); color: var(--green); }
.badge.warn { border-color: var(--yellow); color: var(--yellow); }
.badge.err { border-color: var(--red); color: var(--red); }

/* Navigation */
nav {
  display: flex;
  gap: 4px;
  margin-bottom: 24px;
  background: var(--surface);
  border-radius: var(--radius);
  padding: 4px;
  border: 1px solid var(--border);
}

nav button {
  flex: 1;
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

nav button:hover { color: var(--text); background: rgba(255,255,255,0.04); }
nav button.active { background: var(--accent); color: #fff; }

/* Cards */
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px;
  margin-bottom: 16px;
}

.card h2 {
  font-size: 16px;
  font-weight: 600;
  margin-bottom: 16px;
  color: var(--text);
}

.card h3 {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 12px;
  color: var(--text-muted);
}

/* Stats grid */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 16px;
  margin-bottom: 24px;
}

.stat-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px;
  text-align: center;
}

.stat-card .value {
  font-size: 32px;
  font-weight: 700;
  font-family: var(--mono);
  color: var(--accent);
  line-height: 1.1;
}

.stat-card .value.green { color: var(--green); }
.stat-card .value.purple { color: var(--purple); }
.stat-card .value.cyan { color: var(--cyan); }

.stat-card .label {
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 6px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

/* Distribution bar */
.dist-bar {
  display: flex;
  height: 28px;
  border-radius: 6px;
  overflow: hidden;
  margin-bottom: 12px;
}

.dist-bar .seg {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  font-weight: 600;
  color: #fff;
  min-width: 30px;
  transition: width 0.4s;
}

.dist-bar .seg.fast { background: var(--green); }
.dist-bar .seg.smart { background: var(--purple); }
.dist-bar .seg.passthrough { background: var(--text-muted); }

.dist-legend {
  display: flex;
  gap: 16px;
  font-size: 12px;
  color: var(--text-muted);
}

.dist-legend span::before {
  content: '';
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 3px;
  margin-right: 4px;
  vertical-align: middle;
}

.dist-legend .fast::before { background: var(--green); }
.dist-legend .smart::before { background: var(--purple); }
.dist-legend .pt::before { background: var(--text-muted); }

/* Tables */
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

th, td {
  text-align: left;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}

th {
  color: var(--text-muted);
  font-weight: 500;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

td { color: var(--text); }
td.mono { font-family: var(--mono); font-size: 12px; }
td.num { text-align: right; font-family: var(--mono); }
th.num { text-align: right; }

tr:hover { background: rgba(255,255,255,0.02); }

/* Bar chart */
.chart {
  display: flex;
  align-items: flex-end;
  gap: 4px;
  height: 160px;
  padding-top: 20px;
  margin-bottom: 8px;
}

.chart .bar-group {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  height: 100%;
  justify-content: flex-end;
}

.chart .bar {
  width: 100%;
  max-width: 28px;
  background: var(--accent);
  border-radius: 3px 3px 0 0;
  min-height: 2px;
  transition: height 0.4s;
  position: relative;
}

.chart .bar:hover { opacity: 0.8; }

.chart .bar-label {
  font-size: 10px;
  color: var(--text-muted);
  margin-top: 6px;
  writing-mode: vertical-rl;
  text-orientation: mixed;
  transform: rotate(180deg);
  max-height: 60px;
  overflow: hidden;
}

.chart-tooltip {
  position: absolute;
  bottom: calc(100% + 6px);
  left: 50%;
  transform: translateX(-50%);
  background: #333;
  color: #fff;
  padding: 4px 8px;
  border-radius: 4px;
  font-size: 11px;
  white-space: nowrap;
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.15s;
}

.chart .bar:hover .chart-tooltip { opacity: 1; }

/* Config page */
.config-section {
  margin-bottom: 24px;
}

.config-section h3 {
  margin-bottom: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border);
}

.config-row {
  display: flex;
  align-items: center;
  padding: 8px 0;
  gap: 12px;
}

.config-row label {
  flex: 0 0 240px;
  font-size: 13px;
  color: var(--text);
}

.config-row .desc {
  font-size: 11px;
  color: var(--text-muted);
  display: block;
}

.config-row input[type="text"],
.config-row input[type="number"],
.config-row select {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 6px 10px;
  font-size: 13px;
  font-family: var(--mono);
  width: 220px;
}

.config-row input:focus,
.config-row select:focus {
  outline: none;
  border-color: var(--accent);
}

.toggle {
  position: relative;
  width: 40px;
  height: 22px;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle .slider {
  position: absolute;
  inset: 0;
  background: var(--border);
  border-radius: 22px;
  cursor: pointer;
  transition: 0.2s;
}

.toggle .slider::before {
  content: '';
  position: absolute;
  width: 16px;
  height: 16px;
  left: 3px;
  bottom: 3px;
  background: var(--text);
  border-radius: 50%;
  transition: 0.2s;
}

.toggle input:checked + .slider { background: var(--green); }
.toggle input:checked + .slider::before { transform: translateX(18px); }

/* Buttons */
.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}

.btn:hover { border-color: var(--accent); color: var(--accent); }
.btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }
.btn.primary:hover { opacity: 0.85; }
.btn.danger { border-color: var(--red); color: var(--red); }
.btn.danger:hover { background: var(--red); color: #fff; }

.btn-group {
  display: flex;
  gap: 8px;
  margin-top: 16px;
}

/* Toast notification */
.toast {
  position: fixed;
  bottom: 24px;
  right: 24px;
  padding: 12px 20px;
  border-radius: var(--radius);
  background: var(--green);
  color: #fff;
  font-weight: 500;
  font-size: 13px;
  transform: translateY(80px);
  opacity: 0;
  transition: all 0.3s;
  z-index: 1000;
}

.toast.show { transform: translateY(0); opacity: 1; }
.toast.error { background: var(--red); }

/* Loading */
.loading {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 40px;
  color: var(--text-muted);
}

.spinner {
  width: 20px;
  height: 20px;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
  margin-right: 10px;
}

@keyframes spin { to { transform: rotate(360deg); } }

/* Panels / Tabs */
.panel { display: none; }
.panel.active { display: block; }

/* Empty state */
.empty {
  text-align: center;
  padding: 40px 20px;
  color: var(--text-muted);
}

.empty .icon { font-size: 48px; margin-bottom: 12px; }
.empty p { max-width: 400px; margin: 0 auto; }

/* Responsive */
@media (max-width: 768px) {
  .stats-grid { grid-template-columns: repeat(2, 1fr); }
  .config-row { flex-direction: column; align-items: flex-start; }
  .config-row label { flex: none; }
  nav { flex-wrap: wrap; }
}
</style>
</head>
<body>
<div class="app">

  <!-- Header -->
  <header>
    <div>
      <h1><span class="logo">&gt;_ terse</span> Dashboard</h1>
      <div class="subtitle">Token Efficiency through Refined Stream Engineering</div>
    </div>
    <div class="health-badges" id="health-badges"></div>
  </header>

  <!-- Navigation -->
  <nav id="nav">
    <button class="active" data-panel="dashboard">Dashboard</button>
    <button data-panel="trends">Trends</button>
    <button data-panel="discover">Discovery</button>
    <button data-panel="config">Configuration</button>
  </nav>

  <!-- Dashboard Panel -->
  <div class="panel active" id="panel-dashboard">
    <div class="stats-grid" id="stats-grid">
      <div class="stat-card"><div class="value" id="stat-commands">—</div><div class="label">Commands Optimized</div></div>
      <div class="stat-card"><div class="value green" id="stat-savings">—</div><div class="label">Token Savings</div></div>
      <div class="stat-card"><div class="value purple" id="stat-original">—</div><div class="label">Original Tokens</div></div>
      <div class="stat-card"><div class="value cyan" id="stat-optimized">—</div><div class="label">Optimized Tokens</div></div>
    </div>

    <div class="card">
      <h2>Path Distribution</h2>
      <div class="dist-bar" id="dist-bar"></div>
      <div class="dist-legend">
        <span class="fast">Fast Path</span>
        <span class="smart">Smart Path</span>
        <span class="pt">Passthrough</span>
      </div>
    </div>

    <div class="card">
      <h2>Top Commands by Token Savings</h2>
      <table id="commands-table">
        <thead>
          <tr>
            <th>Command</th>
            <th class="num">Count</th>
            <th class="num">Original</th>
            <th class="num">Optimized</th>
            <th class="num">Savings %</th>
            <th>Optimizer</th>
          </tr>
        </thead>
        <tbody id="commands-tbody"></tbody>
      </table>
      <div class="empty" id="commands-empty" style="display:none">
        <div class="icon">📊</div>
        <p>No command data yet. Run some commands through terse to see analytics here.</p>
      </div>
    </div>
  </div>

  <!-- Trends Panel -->
  <div class="panel" id="panel-trends">
    <div class="card">
      <h2>Daily Token Savings (Last 30 Days)</h2>
      <div class="chart" id="trend-chart"></div>
      <div class="empty" id="trends-empty" style="display:none">
        <div class="icon">📈</div>
        <p>No trend data available yet. Analytics accumulate as you use terse with Claude Code.</p>
      </div>
    </div>

    <div class="card">
      <h2>Daily Summary</h2>
      <table>
        <thead>
          <tr>
            <th>Date</th>
            <th class="num">Commands</th>
            <th class="num">Tokens Saved</th>
            <th class="num">Avg Savings %</th>
          </tr>
        </thead>
        <tbody id="trends-tbody"></tbody>
      </table>
    </div>
  </div>

  <!-- Discovery Panel -->
  <div class="panel" id="panel-discover">
    <div class="card">
      <h2>Optimization Opportunities</h2>
      <p style="color:var(--text-muted);margin-bottom:16px;font-size:13px">
        Commands frequently handled by passthrough or smart path — candidates for new fast-path optimizers.
      </p>
      <table>
        <thead>
          <tr>
            <th>Command</th>
            <th class="num">Occurrences</th>
            <th class="num">Total Tokens</th>
            <th class="num">Avg Tokens</th>
            <th>Current Path</th>
          </tr>
        </thead>
        <tbody id="discover-tbody"></tbody>
      </table>
      <div class="empty" id="discover-empty" style="display:none">
        <div class="icon">🔍</div>
        <p>No discovery candidates found. All frequently-used commands are already on the fast path!</p>
      </div>
    </div>
  </div>

  <!-- Config Panel -->
  <div class="panel" id="panel-config">
    <div class="card">
      <h2>Configuration Editor</h2>
      <p style="color:var(--text-muted);margin-bottom:16px;font-size:13px">
        Changes are saved to <code style="color:var(--accent)">~/.terse/config.toml</code>.
        Fields update the global user config file directly.
      </p>

      <div class="config-section">
        <h3>General</h3>
        <div class="config-row">
          <label>Enabled<span class="desc">Master kill switch for all optimization</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-general-enabled" data-key="general.enabled"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Mode<span class="desc">Operation mode</span></label>
          <select id="cfg-general-mode" data-key="general.mode">
            <option value="hybrid">Hybrid</option>
            <option value="fast-only">Fast Only</option>
            <option value="smart-only">Smart Only</option>
            <option value="passthrough">Passthrough</option>
          </select>
        </div>
        <div class="config-row">
          <label>Profile<span class="desc">Performance preset</span></label>
          <select id="cfg-general-profile" data-key="general.profile">
            <option value="fast">Fast</option>
            <option value="balanced">Balanced</option>
            <option value="quality">Quality</option>
          </select>
        </div>
        <div class="config-row">
          <label>Safe Mode<span class="desc">Disable all optimization, log only</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-general-safe_mode" data-key="general.safe_mode"><span class="slider"></span></div>
        </div>
      </div>

      <div class="config-section">
        <h3>Fast Path</h3>
        <div class="config-row">
          <label>Enabled<span class="desc">Enable rule-based optimizers</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-fast_path-enabled" data-key="fast_path.enabled"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Timeout (ms)<span class="desc">Max time budget per optimizer</span></label>
          <input type="number" id="cfg-fast_path-timeout_ms" data-key="fast_path.timeout_ms" min="10" max="5000">
        </div>
        <div class="config-row">
          <label>Git Optimizer<span class="desc">Enable git command optimization</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-fast_path-optimizers-git" data-key="fast_path.optimizers.git"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>File Optimizer<span class="desc">Enable file command optimization</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-fast_path-optimizers-file" data-key="fast_path.optimizers.file"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Build Optimizer<span class="desc">Enable build/test optimization</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-fast_path-optimizers-build" data-key="fast_path.optimizers.build"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Docker Optimizer<span class="desc">Enable docker command optimization</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-fast_path-optimizers-docker" data-key="fast_path.optimizers.docker"><span class="slider"></span></div>
        </div>
      </div>

      <div class="config-section">
        <h3>Smart Path (LLM)</h3>
        <div class="config-row">
          <label>Enabled<span class="desc">Enable LLM-powered optimization via Ollama</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-smart_path-enabled" data-key="smart_path.enabled"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Model<span class="desc">Ollama model name</span></label>
          <input type="text" id="cfg-smart_path-model" data-key="smart_path.model">
        </div>
        <div class="config-row">
          <label>Temperature<span class="desc">0.0 = deterministic</span></label>
          <input type="number" id="cfg-smart_path-temperature" data-key="smart_path.temperature" min="0" max="2" step="0.1">
        </div>
        <div class="config-row">
          <label>Max Latency (ms)<span class="desc">LLM request timeout</span></label>
          <input type="number" id="cfg-smart_path-max_latency_ms" data-key="smart_path.max_latency_ms" min="500" max="60000" step="500">
        </div>
        <div class="config-row">
          <label>Ollama URL<span class="desc">Ollama API endpoint</span></label>
          <input type="text" id="cfg-smart_path-ollama_url" data-key="smart_path.ollama_url">
        </div>
      </div>

      <div class="config-section">
        <h3>Output Thresholds</h3>
        <div class="config-row">
          <label>Passthrough Below (bytes)<span class="desc">Outputs smaller than this skip optimization</span></label>
          <input type="number" id="cfg-output_thresholds-passthrough_below_bytes" data-key="output_thresholds.passthrough_below_bytes" min="0" step="256">
        </div>
        <div class="config-row">
          <label>Smart Path Above (bytes)<span class="desc">Outputs larger than this use smart path</span></label>
          <input type="number" id="cfg-output_thresholds-smart_path_above_bytes" data-key="output_thresholds.smart_path_above_bytes" min="0" step="1024">
        </div>
      </div>

      <div class="config-section">
        <h3>Preprocessing</h3>
        <div class="config-row">
          <label>Enabled<span class="desc">Run preprocessing before LLM calls</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-preprocessing-enabled" data-key="preprocessing.enabled"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Max Output (bytes)<span class="desc">Truncation limit after preprocessing</span></label>
          <input type="number" id="cfg-preprocessing-max_output_bytes" data-key="preprocessing.max_output_bytes" min="1024" step="1024">
        </div>
        <div class="config-row">
          <label>Noise Removal<span class="desc">Strip ANSI codes, progress bars, boilerplate</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-preprocessing-noise_removal" data-key="preprocessing.noise_removal"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Path Filtering<span class="desc">Filter noisy paths (node_modules, target, etc.)</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-preprocessing-path_filtering" data-key="preprocessing.path_filtering"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Deduplication<span class="desc">Remove repeated lines/blocks</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-preprocessing-deduplication" data-key="preprocessing.deduplication"><span class="slider"></span></div>
        </div>
      </div>

      <div class="config-section">
        <h3>Logging</h3>
        <div class="config-row">
          <label>Enabled<span class="desc">Enable command result logging</span></label>
          <div class="toggle"><input type="checkbox" id="cfg-logging-enabled" data-key="logging.enabled"><span class="slider"></span></div>
        </div>
        <div class="config-row">
          <label>Level<span class="desc">Log verbosity</span></label>
          <select id="cfg-logging-level" data-key="logging.level">
            <option value="error">Error</option>
            <option value="warn">Warn</option>
            <option value="info">Info</option>
            <option value="debug">Debug</option>
          </select>
        </div>
      </div>

      <div class="btn-group">
        <button class="btn primary" id="btn-save-config">Save Configuration</button>
        <button class="btn danger" id="btn-reset-config">Reset to Defaults</button>
      </div>
    </div>
  </div>

</div>

<!-- Toast -->
<div class="toast" id="toast"></div>

<script>
// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
let currentPanel = 'dashboard';
let statsData = null;
let trendsData = null;
let discoverData = null;
let configData = null;

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------
async function api(method, path, body) {
  const opts = { method, headers: {} };
  if (body) {
    opts.headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(body);
  }
  const res = await fetch(path, opts);
  return res.json();
}

function toast(msg, isError) {
  const el = document.getElementById('toast');
  el.textContent = msg;
  el.className = 'toast show' + (isError ? ' error' : '');
  setTimeout(() => el.className = 'toast', 3000);
}

function fmt(n) {
  if (n === undefined || n === null) return '—';
  return n.toLocaleString();
}

function pct(n) {
  if (n === undefined || n === null) return '—';
  return n.toFixed(1) + '%';
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------
document.getElementById('nav').addEventListener('click', e => {
  if (e.target.tagName !== 'BUTTON') return;
  const panel = e.target.dataset.panel;
  if (!panel) return;

  document.querySelectorAll('nav button').forEach(b => b.classList.remove('active'));
  e.target.classList.add('active');

  document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
  document.getElementById('panel-' + panel).classList.add('active');

  currentPanel = panel;
  loadPanel(panel);
});

// ---------------------------------------------------------------------------
// Load panel data
// ---------------------------------------------------------------------------
async function loadPanel(panel) {
  switch (panel) {
    case 'dashboard': return loadDashboard();
    case 'trends': return loadTrends();
    case 'discover': return loadDiscover();
    case 'config': return loadConfig();
  }
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------
async function loadDashboard() {
  try {
    statsData = await api('GET', '/api/stats');
    renderDashboard();
  } catch (e) {
    toast('Failed to load stats: ' + e.message, true);
  }
}

function renderDashboard() {
  const s = statsData;
  document.getElementById('stat-commands').textContent = fmt(s.total_commands);
  document.getElementById('stat-savings').textContent = pct(s.total_savings_pct);
  document.getElementById('stat-original').textContent = fmt(s.total_original_tokens);
  document.getElementById('stat-optimized').textContent = fmt(s.total_optimized_tokens);

  // Distribution bar
  const d = s.path_distribution;
  const bar = document.getElementById('dist-bar');
  if (d.fast + d.smart + d.passthrough > 0) {
    bar.innerHTML =
      (d.fast_pct > 0 ? `<div class="seg fast" style="width:${Math.max(d.fast_pct, 5)}%">${d.fast} fast</div>` : '') +
      (d.smart_pct > 0 ? `<div class="seg smart" style="width:${Math.max(d.smart_pct, 5)}%">${d.smart} smart</div>` : '') +
      (d.passthrough_pct > 0 ? `<div class="seg passthrough" style="width:${Math.max(d.passthrough_pct, 5)}%">${d.passthrough} pt</div>` : '');
  } else {
    bar.innerHTML = '<div class="seg passthrough" style="width:100%">No data</div>';
  }

  // Commands table
  const tbody = document.getElementById('commands-tbody');
  const empty = document.getElementById('commands-empty');
  if (s.command_stats.length === 0) {
    tbody.innerHTML = '';
    empty.style.display = 'block';
  } else {
    empty.style.display = 'none';
    tbody.innerHTML = s.command_stats.slice(0, 20).map(c => `
      <tr>
        <td class="mono">${esc(c.command)}</td>
        <td class="num">${fmt(c.count)}</td>
        <td class="num">${fmt(c.total_original_tokens)}</td>
        <td class="num">${fmt(c.total_optimized_tokens)}</td>
        <td class="num">${pct(c.avg_savings_pct)}</td>
        <td class="mono">${esc(c.primary_optimizer)}</td>
      </tr>
    `).join('');
  }
}

// ---------------------------------------------------------------------------
// Trends
// ---------------------------------------------------------------------------
async function loadTrends() {
  try {
    trendsData = await api('GET', '/api/trends?days=30');
    renderTrends();
  } catch (e) {
    toast('Failed to load trends: ' + e.message, true);
  }
}

function renderTrends() {
  const entries = trendsData.entries || [];
  const chart = document.getElementById('trend-chart');
  const tbody = document.getElementById('trends-tbody');
  const empty = document.getElementById('trends-empty');

  if (entries.length === 0) {
    chart.innerHTML = '';
    tbody.innerHTML = '';
    empty.style.display = 'block';
    return;
  }
  empty.style.display = 'none';

  const maxSaved = Math.max(...entries.map(e => e.tokens_saved), 1);

  chart.innerHTML = entries.map(e => {
    const h = Math.max((e.tokens_saved / maxSaved) * 100, 2);
    const label = e.date.slice(5); // MM-DD
    return `
      <div class="bar-group">
        <div class="bar" style="height:${h}%">
          <div class="chart-tooltip">${e.date}: ${fmt(e.tokens_saved)} tokens saved</div>
        </div>
        <div class="bar-label">${label}</div>
      </div>
    `;
  }).join('');

  tbody.innerHTML = entries.map(e => `
    <tr>
      <td class="mono">${esc(e.date)}</td>
      <td class="num">${fmt(e.commands)}</td>
      <td class="num">${fmt(e.tokens_saved)}</td>
      <td class="num">${pct(e.avg_savings_pct)}</td>
    </tr>
  `).join('');
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------
async function loadDiscover() {
  try {
    discoverData = await api('GET', '/api/discover');
    renderDiscover();
  } catch (e) {
    toast('Failed to load discovery data: ' + e.message, true);
  }
}

function renderDiscover() {
  const candidates = discoverData.candidates || [];
  const tbody = document.getElementById('discover-tbody');
  const empty = document.getElementById('discover-empty');

  if (candidates.length === 0) {
    tbody.innerHTML = '';
    empty.style.display = 'block';
    return;
  }
  empty.style.display = 'none';

  tbody.innerHTML = candidates.slice(0, 20).map(c => `
    <tr>
      <td class="mono">${esc(c.command)}</td>
      <td class="num">${fmt(c.count)}</td>
      <td class="num">${fmt(c.total_tokens)}</td>
      <td class="num">${fmt(c.avg_tokens)}</td>
      <td class="mono">${esc(c.current_path)}</td>
    </tr>
  `).join('');
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------
async function loadConfig() {
  try {
    configData = await api('GET', '/api/config');
    renderConfig();
  } catch (e) {
    toast('Failed to load config: ' + e.message, true);
  }
}

function renderConfig() {
  const c = configData.config;

  // General
  setToggle('cfg-general-enabled', c.general.enabled);
  setSelect('cfg-general-mode', c.general.mode);
  setSelect('cfg-general-profile', c.general.profile);
  setToggle('cfg-general-safe_mode', c.general.safe_mode);

  // Fast path
  setToggle('cfg-fast_path-enabled', c.fast_path.enabled);
  setNumber('cfg-fast_path-timeout_ms', c.fast_path.timeout_ms);
  setToggle('cfg-fast_path-optimizers-git', c.fast_path.optimizers.git);
  setToggle('cfg-fast_path-optimizers-file', c.fast_path.optimizers.file);
  setToggle('cfg-fast_path-optimizers-build', c.fast_path.optimizers.build);
  setToggle('cfg-fast_path-optimizers-docker', c.fast_path.optimizers.docker);

  // Smart path
  setToggle('cfg-smart_path-enabled', c.smart_path.enabled);
  setText('cfg-smart_path-model', c.smart_path.model);
  setNumber('cfg-smart_path-temperature', c.smart_path.temperature);
  setNumber('cfg-smart_path-max_latency_ms', c.smart_path.max_latency_ms);
  setText('cfg-smart_path-ollama_url', c.smart_path.ollama_url);

  // Output thresholds
  setNumber('cfg-output_thresholds-passthrough_below_bytes', c.output_thresholds.passthrough_below_bytes);
  setNumber('cfg-output_thresholds-smart_path_above_bytes', c.output_thresholds.smart_path_above_bytes);

  // Preprocessing
  setToggle('cfg-preprocessing-enabled', c.preprocessing.enabled);
  setNumber('cfg-preprocessing-max_output_bytes', c.preprocessing.max_output_bytes);
  setToggle('cfg-preprocessing-noise_removal', c.preprocessing.noise_removal);
  setToggle('cfg-preprocessing-path_filtering', c.preprocessing.path_filtering);
  setToggle('cfg-preprocessing-deduplication', c.preprocessing.deduplication);

  // Logging
  setToggle('cfg-logging-enabled', c.logging.enabled);
  setSelect('cfg-logging-level', c.logging.level);
}

function setToggle(id, val) {
  const el = document.getElementById(id);
  if (el) el.checked = !!val;
}

function setText(id, val) {
  const el = document.getElementById(id);
  if (el) el.value = val || '';
}

function setNumber(id, val) {
  const el = document.getElementById(id);
  if (el) el.value = val;
}

function setSelect(id, val) {
  const el = document.getElementById(id);
  if (el) el.value = val || '';
}

// Save config
document.getElementById('btn-save-config').addEventListener('click', async () => {
  const updates = [];

  // Gather all config inputs
  document.querySelectorAll('[data-key]').forEach(el => {
    const key = el.dataset.key;
    let value;
    if (el.type === 'checkbox') {
      value = el.checked ? 'true' : 'false';
    } else {
      value = el.value;
    }
    updates.push({ key, value });
  });

  try {
    const result = await api('PUT', '/api/config', { updates });
    if (result.success) {
      toast('Configuration saved successfully');
    } else {
      toast('Some settings failed: ' + result.errors.join(', '), true);
    }
  } catch (e) {
    toast('Failed to save config: ' + e.message, true);
  }
});

// Reset config
document.getElementById('btn-reset-config').addEventListener('click', async () => {
  if (!confirm('Reset all configuration to defaults? This will overwrite your config.toml file.')) return;

  try {
    const result = await api('POST', '/api/config/reset');
    if (result.success) {
      toast('Configuration reset to defaults');
      loadConfig();
    } else {
      toast('Failed to reset config', true);
    }
  } catch (e) {
    toast('Failed to reset config: ' + e.message, true);
  }
});

// ---------------------------------------------------------------------------
// Health badges
// ---------------------------------------------------------------------------
async function loadHealth() {
  try {
    const h = await api('GET', '/api/health');
    const badges = document.getElementById('health-badges');
    badges.innerHTML = [
      badge(h.platform, 'ok'),
      badge('Git', h.git_available ? 'ok' : 'warn'),
      badge('Ollama', h.ollama_available ? 'ok' : 'warn'),
      badge('Config', h.config_exists ? 'ok' : 'warn'),
    ].join('');
  } catch (e) {
    // Silently ignore health badge errors
  }
}

function badge(label, cls) {
  const dot = cls === 'ok' ? '●' : cls === 'warn' ? '○' : '✕';
  return `<span class="badge ${cls}">${dot} ${esc(label)}</span>`;
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------
function esc(s) {
  if (!s) return '';
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------
loadHealth();
loadDashboard();
</script>
</body>
</html>"##;
