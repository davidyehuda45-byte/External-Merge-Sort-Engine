// Timing + memory metrics for the run summary.
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Samples this process's RSS on a background thread; reports the peak.
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
        self.result_rx.recv().unwrap_or(0) as usize
    }
}

pub fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

pub fn gb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}
