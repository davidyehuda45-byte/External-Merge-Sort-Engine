// Timing + memory metrics for the run summary.
//
// NOTE: the reported peak is this process's own RSS sampled via sysinfo.
// It does NOT include child processes (e.g. decompressors/converters spawned
// via std::process::Command) nor the OS page cache — treat it as a
// lower bound on total machine pressure, not whole-pipeline usage.
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Samples this process's RSS on a background thread; reports the peak.
/// Dropping without `finish()` still stops the thread (see Drop impl) and
/// discards the peak; call `finish()` to retrieve it.
pub struct MemorySampler {
    handle: Option<std::thread::JoinHandle<()>>,
    stop: std::sync::mpsc::Sender<()>,
    result_rx: std::sync::mpsc::Receiver<u64>,
}

impl MemorySampler {
    pub fn spawn(sample_interval: Duration) -> MemorySampler {
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<u64>();
        let handle = std::thread::spawn(move || {
            let pid = Pid::from_u32(std::process::id());
            let mut sys = System::new();
            let mut peak: u64 = 0;
            loop {
                sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                if let Some(p) = sys.process(pid) {
                    let m = p.memory();
                    if m > peak {
                        peak = m;
                    }
                }
                match stop_rx.recv_timeout(sample_interval) {
                    Ok(_) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            let _ = res_tx.send(peak);
        });
        MemorySampler { handle: Some(handle), stop: stop_tx, result_rx: res_rx }
    }

    /// Stops the sampler and returns the observed peak memory in bytes.
    pub fn finish(mut self) -> usize {
        if let Some(h) = self.handle.take() {
            let _ = self.stop.send(());
            let _ = h.join();
        }
        // `handle` is None now so the Drop impl below becomes a no-op.
        self.result_rx.recv().unwrap_or(0) as usize
    }
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        // Best-effort stop so a forgotten sampler never leaks its thread:
        // signal exit, then join. `finish()` takes `self` and nulls `handle`,
        // so this is a no-op after a normal finish. Never panics.
        if self.handle.is_some() {
            let _ = self.stop.send(());
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

pub fn gb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}
