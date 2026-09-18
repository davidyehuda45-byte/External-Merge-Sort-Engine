// Real-time progress reporting: shared atomic run-state fed by the engine,
// an indicatif ticker thread that renders bars + throughput, and an optional
// HTTP dashboard (live metrics page for headless/remote monitoring).
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Phase identifiers shared across threads.
pub const PHASE_IDLE: u8 = 0;
pub const PHASE_SPLIT: u8 = 1;
pub const PHASE_MERGE: u8 = 2;
pub const PHASE_DONE: u8 = 3;

/// Engine-side counters. The engine only does relaxed atomic adds (batched
/// where hot); rendering and dashboards read from these.
pub struct RunState {
    // Split phase
    pub input_total_bytes: AtomicU64,
    pub input_bytes_read: AtomicU64,
    pub chunks_done: AtomicU64,
    // Records observed in split == merge element estimate.
    pub records: AtomicU64,
    // Merge phase
    pub merge_done: AtomicU64,
    pub merge_total: AtomicU64,
    pub merge_pass: AtomicU64,
    pub io_bytes: AtomicU64,
    // Lifecycle: PHASE_* constants.
    pub phase: AtomicU8,
    // Latest RSS sample (bytes), written by the ticker (0 in quiet mode).
    pub mem_bytes: AtomicU64,
}

impl RunState {
    pub fn new(input_total_bytes: u64) -> Arc<RunState> {
        Arc::new(RunState {
            input_total_bytes: AtomicU64::new(input_total_bytes),
            input_bytes_read: AtomicU64::new(0),
            chunks_done: AtomicU64::new(0),
            records: AtomicU64::new(0),
            merge_done: AtomicU64::new(0),
            merge_total: AtomicU64::new(0),
            merge_pass: AtomicU64::new(0),
            io_bytes: AtomicU64::new(0),
            phase: AtomicU8::new(PHASE_IDLE),
            mem_bytes: AtomicU64::new(0),
        })
    }

    pub fn phase_name(&self) -> &'static str {
        match self.phase.load(Ordering::Relaxed) {
            PHASE_SPLIT => "split",
            PHASE_MERGE => "merge",
            PHASE_DONE => "done",
            _ => "idle",
        }
    }
}

/// Process start instant for elapsed/throughput calculations.
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn set_start() {
    let _ = START.set(Instant::now());
}

fn elapsed_secs() -> f64 {
    match START.get() {
        Some(t) => t.elapsed().as_secs_f64(),
        None => 0.0,
    }
}

/// Renders progress bars from RunState on a ticker thread.
pub struct Progress {
    stop: Arc<AtomicBool>,
    ticker: Option<std::thread::JoinHandle<()>>,
}

impl Progress {
    /// `enabled == false` (--quiet/--json) creates a no-op progress handle.
    pub fn new(state: Arc<RunState>, enabled: bool) -> Progress {
        if !enabled {
            return Progress { stop: Arc::new(AtomicBool::new(true)), ticker: None };
        }
        let mp = MultiProgress::new();
        let chunk_style = ProgressStyle::with_template(
            "{prefix} [{bar:32.cyan/blue}] {percent:>3}% {msg}",
        )
        .unwrap_or(ProgressStyle::default_bar());
        let merge_style = ProgressStyle::with_template(
            "{prefix} [{bar:32.green/yellow}] {percent:>3}% {msg}",
        )
        .unwrap_or(ProgressStyle::default_bar());

        let chunk_bar = mp.add(ProgressBar::new(100));
        chunk_bar.set_style(chunk_style);
        chunk_bar.set_prefix("chunk");

        let merge_bar = mp.add(ProgressBar::new(100));
        merge_bar.set_style(merge_style);
        merge_bar.set_prefix("merge ");

        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();

        let ticker = std::thread::spawn(move || {
            let mut sys = sysinfo::System::new();
            let pid = sysinfo::Pid::from_u32(std::process::id());

            loop {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                // Sample process memory for the dashboard + bar messages.
                sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
                if let Some(p) = sys.process(pid) {
                    state.mem_bytes.store(p.memory(), Ordering::Relaxed);
                }

                let phase = state.phase.load(Ordering::Relaxed);
                let bytes_read = state.input_bytes_read.load(Ordering::Relaxed);
                let bytes_total = state.input_total_bytes.load(Ordering::Relaxed);
                let merge_done = state.merge_done.load(Ordering::Relaxed);
                let merge_total = state.merge_total.load(Ordering::Relaxed);
                let pass = state.merge_pass.load(Ordering::Relaxed);
                let el = elapsed_secs();

                // Chunk bar.
                {
                    let pct = if bytes_total > 0 {
                        bytes_read.min(bytes_total) * 100 / bytes_total
                    } else {
                        0
                    };
                    chunk_bar.set_length(100);
                    chunk_bar.set_position(pct);
                    let mbps = if el > 0.05 {
                        bytes_read as f64 / (1024.0 * 1024.0) / el
                    } else {
                        0.0
                    };
                    let msg = format!(
                        "{:.1}/{:.1} MB @ {:.1} MB/s, {} chunks",
                        bytes_read as f64 / (1024.0 * 1024.0),
                        bytes_total as f64 / (1024.0 * 1024.0),
                        mbps,
                        state.chunks_done.load(Ordering::Relaxed)
                    );
                    if phase == PHASE_DONE {
                        chunk_bar.finish_with_message("selesai");
                    } else {
                        chunk_bar.set_message(msg);
                    }
                }

                // Merge bar.
                {
                    let pct = if merge_total > 0 {
                        merge_done.min(merge_total) * 100 / merge_total
                    } else {
                        0
                    };
                    merge_bar.set_length(100);
                    merge_bar.set_position(pct);
                    let mbps = if el > 0.05 {
                        state.io_bytes.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0) / el
                    } else {
                        0.0
                    };
                    let msg = if pass > 0 {
                        format!(
                            "{} / {} elem @ {:.1} MB/s (pass {})",
                            merge_done, merge_total, mbps, pass
                        )
                    } else {
                        "menunggu fase split".to_string()
                    };
                    if phase == PHASE_DONE {
                        merge_bar.finish_with_message("selesai");
                    } else {
                        merge_bar.set_message(msg);
                    }
                }

                std::thread::sleep(Duration::from_millis(200));
            }
        });

        Progress { stop, ticker: Some(ticker) }
    }

    /// Stops the ticker and finishes the bars with a final message.
    pub fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.ticker.take() {
            let _ = h.join();
        }
    }
}

/// Starts the dashboard HTTP server on `bind:port` (port 0 = random).
/// Returns the bound port. Serves "/" (auto-refresh HTML) and "/metrics"
/// (JSON) until the process exits.
pub fn start_dashboard(bind: &str, port: u16, state: Arc<RunState>) -> std::io::Result<u16> {
    let listener = TcpListener::bind((bind, port))?;
    let bound_port: u16 = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(_) => port,
    };

    std::thread::spawn(move || {
        for conn in listener.incoming() {
            match conn {
                Ok(stream) => handle_conn(stream, state.clone()),
                Err(_) => continue,
            }
        }
    });
    Ok(bound_port)
}

fn handle_conn(mut stream: TcpStream, state: Arc<RunState>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = [0u8; 1024];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    // Normalize: drop query string and leading slashes so "//metrics?x"
    // still resolves to the metrics endpoint.
    let raw = req.split_ascii_whitespace().nth(1).unwrap_or("/");
    let path = raw.split('?').next().unwrap_or("/");
    let norm = path.trim_start_matches('/');

    let (ctype, body) = if norm == "metrics" || norm.starts_with("metrics/") {
        ("application/json", metrics_json(&state))
    } else {
        ("text/html; charset=utf-8", dashboard_html().to_string())
    };

    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        ctype,
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// Samples RSS (fresh, so it works even in quiet mode with no ticker) and
/// builds the /metrics JSON document.
fn metrics_json(state: &RunState) -> String {
    // Fresh memory sample (requests arrive ~1/s).
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    let mem = match sys.process(pid) {
        Some(p) => p.memory(),
        None => 0,
    };

    let phase = state.phase.load(Ordering::Relaxed);
    let bytes_read = state.input_bytes_read.load(Ordering::Relaxed);
    let bytes_total = state.input_total_bytes.load(Ordering::Relaxed);
    let merge_done = state.merge_done.load(Ordering::Relaxed);
    let merge_total = state.merge_total.load(Ordering::Relaxed);
    let iob = state.io_bytes.load(Ordering::Relaxed);
    let el = elapsed_secs();

    let chunk_pct = if bytes_total > 0 { bytes_read.min(bytes_total) * 100 / bytes_total } else { 0 };
    let merge_pct = if merge_total > 0 { merge_done.min(merge_total) * 100 / merge_total } else { 0 };
    let chunk_mbps = if el > 0.05 { bytes_read as f64 / (1024.0 * 1024.0) / el } else { 0.0 };
    let merge_mbps = if el > 0.05 { iob as f64 / (1024.0 * 1024.0) / el } else { 0.0 };

    format!(
        "{{\"phase\":\"{}\",\"elapsed_s\":{:.2},\"finished\":{},\
         \"chunk\":{{\"bytes_read\":{},\"bytes_total\":{},\"percent\":{},\"chunks_done\":{},\"mbps\":{:.1}}},\
         \"merge\":{{\"done\":{},\"total\":{},\"percent\":{},\"pass\":{},\"io_bytes\":{},\"mbps\":{:.1}}},\
         \"records\":{},\"memory_bytes\":{}}}",
        state.phase_name(),
        el,
        phase == PHASE_DONE,
        bytes_read,
        bytes_total,
        chunk_pct,
        state.chunks_done.load(Ordering::Relaxed),
        chunk_mbps,
        merge_done,
        merge_total,
        merge_pct,
        state.merge_pass.load(Ordering::Relaxed),
        iob,
        merge_mbps,
        state.records.load(Ordering::Relaxed),
        mem
    )
}

fn dashboard_html() -> &'static str {
    // Pro dashboard: branded, card layout, no external assets (works offline/air-gapped).
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>MergeSort Pro — Live Dashboard</title>
<meta http-equiv="refresh" content="10">
<style>
  :root { --bg:#0b0f14; --card:#141b24; --line:#223041; --txt:#dbe4f0; --mut:#8fa1b5; --acc:#56c2d6; --acc2:#a3be8c; --warn:#ebcb8b; }
  * { box-sizing:border-box; }
  body { font-family: ui-sans-serif,system-ui,Segoe UI,Roboto,Arial; background:var(--bg); color:var(--txt); margin:0; }
  header { display:flex; align-items:center; justify-content:space-between; padding:18px 28px; border-bottom:1px solid var(--line); background:#0e141d; position:sticky; top:0; }
  .brand { font-weight:800; letter-spacing:.3px; } .brand span { color:var(--acc); }
  .pill { font-size:12px; padding:4px 12px; border-radius:999px; background:#1c2836; border:1px solid var(--line); }
  main { max-width:980px; margin:0 auto; padding:24px; }
  .grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(210px,1fr)); gap:14px; margin:16px 0; }
  .card { background:var(--card); border:1px solid var(--line); border-radius:14px; padding:16px 18px; }
  .card h3 { margin:0 0 10px; font-size:13px; text-transform:uppercase; letter-spacing:.8px; color:var(--mut); }
  .big { font-size:26px; font-weight:800; } .sub { color:var(--mut); font-size:13px; margin-top:4px; }
  .bar { height:14px; background:#0f1722; border:1px solid var(--line); border-radius:999px; overflow:hidden; margin:10px 0 6px; }
  .fill { height:100%; width:0%; background:linear-gradient(90deg,#3a7bd5,var(--acc)); transition:width .4s; }
  .fill.merge { background:linear-gradient(90deg,var(--acc2),var(--warn)); }
  .row { display:flex; justify-content:space-between; font-size:13px; padding:5px 0; border-top:1px dashed #1e2b3d; } .row:first-of-type{border-top:0;}
  .k { color:var(--mut); } .ok { color:var(--acc2); font-weight:700; }
  footer { color:var(--mut); font-size:12px; text-align:center; padding:18px; }
  code { background:#0f1722; padding:2px 6px; border-radius:6px; border:1px solid var(--line); }
  @media (max-width:600px){ main{padding:14px;} }
</style>
</head>
<body>
<header>
  <div class="brand">MergeSort <span>Pro</span> &mdash; External Sort Engine</div>
  <div class="pill">Phase: <b id="phase">-</b> &middot; <span id="elapsed">-</span>s</div>
</header>
<main>
  <div class="grid">
    <div class="card"><h3>Records</h3><div class="big" id="records">-</div><div class="sub">total detected in split phase</div></div>
    <div class="card"><h3>Memory (RSS)</h3><div class="big"><span id="mem">-</span> <span style="font-size:14px">MB</span></div><div class="sub">live process usage</div></div>
    <div class="card"><h3>Merge pass</h3><div class="big" id="merge-pass">-</div><div class="sub">multi-pass K-way merge depth</div></div>
    <div class="card"><h3>Status</h3><div class="big ok" id="status">RUNNING</div><div class="sub">JSON: <code>/metrics</code> &middot; auto-refresh 1s</div></div>
  </div>
  <div class="card">
    <h3>Split / Chunking — <span id="chunk-pct">0%</span></h3>
    <div class="bar"><div id="chunk-fill" class="fill"></div></div>
    <div class="row"><span class="k">bytes</span><span id="chunk-bytes">-</span></div>
    <div class="row"><span class="k">chunks done</span><span id="chunk-chunks">-</span></div>
    <div class="row"><span class="k">throughput</span><span id="chunk-mbps">-</span></div>
  </div>
  <div class="card" style="margin-top:14px">
    <h3>Merge — <span id="merge-pct">0%</span></h3>
    <div class="bar"><div id="merge-fill" class="fill merge"></div></div>
    <div class="row"><span class="k">elements</span><span id="merge-elems">-</span></div>
    <div class="row"><span class="k">I/O throughput</span><span id="merge-mbps">-</span></div>
    <div class="row"><span class="k">I/O total</span><span id="merge-io">-</span></div>
  </div>
</main>
<footer>MergeSort Pro &middot; Large-file external sort &middot; endpoint <code>/metrics</code> for Prometheus/CI &middot; bind to 127.0.0.1 in production</footer>
<script>
async function tick() {
  try {
    const j = await (await fetch('/metrics')).json();
    document.getElementById('phase').textContent = j.phase;
    document.getElementById('elapsed').textContent = j.elapsed_s.toFixed(1);
    document.getElementById('mem').textContent = (j.memory_bytes/1048576).toFixed(1);
    document.getElementById('records').textContent = Number(j.records).toLocaleString();
    document.getElementById('status').textContent = j.finished ? 'DONE' : 'RUNNING';
    document.getElementById('chunk-pct').textContent = j.chunk.percent + '%';
    document.getElementById('chunk-fill').style.width = j.chunk.percent + '%';
    document.getElementById('chunk-bytes').textContent =
      (j.chunk.bytes_read/1048576).toFixed(1) + ' / ' + (j.chunk.bytes_total/1048576).toFixed(1) + ' MB';
    document.getElementById('chunk-chunks').textContent = j.chunk.chunks_done;
    document.getElementById('chunk-mbps').textContent = j.chunk.mbps.toFixed(1) + ' MB/s';
    document.getElementById('merge-pct').textContent = j.merge.percent + '%';
    document.getElementById('merge-fill').style.width = j.merge.percent + '%';
    document.getElementById('merge-elems').textContent =
      Number(j.merge.done).toLocaleString() + ' / ' + Number(j.merge.total).toLocaleString();
    document.getElementById('merge-pass').textContent = j.merge.pass;
    document.getElementById('merge-mbps').textContent = j.merge.mbps.toFixed(1) + ' MB/s';
    document.getElementById('merge-io').textContent = (j.merge.io_bytes/1048576).toFixed(1) + ' MB';
  } catch (e) {}
}
setInterval(tick, 1000);
tick();
</script>
</body>
</html>
"#
}
