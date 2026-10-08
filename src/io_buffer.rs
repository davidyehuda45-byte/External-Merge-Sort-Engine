// Double-buffered read-ahead: a background thread fills blocks while the
// consumer processes the previous one. Buffers are recycled through a channel
// so steady-state allocation is zero.
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::time::Duration;

pub struct ReadAhead {
    block_rx: Option<Receiver<std::io::Result<Vec<u8>>>>,
    recycle_tx: SyncSender<Vec<u8>>,
    handle: Option<std::thread::JoinHandle<()>>,
    finished: bool,
}

impl ReadAhead {
    /// Opens `path` and starts a reader thread that delivers ~`block_size` chunks.
    /// `queue_depth` blocks are allowed in flight (2 = classic double buffering).
    pub fn open(path: &Path, block_size: usize, queue_depth: usize) -> std::io::Result<ReadAhead> {
        let file = File::open(path)?;
        Self::new(file, block_size, queue_depth)
    }

    pub fn new(file: File, block_size: usize, queue_depth: usize) -> std::io::Result<ReadAhead> {
        let (block_tx, block_rx) = sync_channel::<std::io::Result<Vec<u8>>>(queue_depth.max(1));
        let (recycle_tx, recycle_rx) = sync_channel::<Vec<u8>>(queue_depth.max(1) + 1);

        // Seed the recycle pool with the initial buffers.
        for _ in 0..(queue_depth.max(1) + 1) {
            let _ = recycle_tx.send(Vec::with_capacity(block_size));
        }

        let handle = std::thread::spawn(move || {
            let mut file = file;
            loop {
                // Get a buffer: prefer a recycled one, else allocate.
                let mut buf: Vec<u8> = recycle_rx.try_recv().unwrap_or_default();
                // NOTE: this std's `read(&mut Vec)` fills only up to the Vec's
                // current len, so we resize to the block size first and truncate
                // to the number of bytes actually read afterwards.
                buf.clear();
                buf.resize(block_size, 0);

                let mut filled = 0usize;
                loop {
                    if filled >= block_size {
                        break;
                    }
                    match file.read(&mut buf[filled..]) {
                        Ok(0) => break,
                        Ok(n) => filled += n,
                        Err(e) => {
                            let _ = block_tx.send(Err(e));
                            return;
                        }
                    }
                }

                if filled == 0 {
                    // EOF reached: drop the block sender so the consumer sees Disconnected.
                    drop(block_tx);
                    return;
                }

                buf.truncate(filled);
                if block_tx.send(Ok(buf)).is_err() {
                    // Consumer went away; stop reading.
                    return;
                }
            }
        });

        Ok(ReadAhead { block_rx: Some(block_rx), recycle_tx, handle: Some(handle), finished: false })
    }

    /// Returns the next filled block, or None at end of stream.
    /// The returned Vec must be returned via `recycle` after use (or dropped).
    ///
    /// `None` is returned ONLY when the reader thread closed the channel
    /// (clean EOF or a delivered `Err`). A `recv_timeout` expiry is a stall,
    /// not EOF: it is logged and retried up to 3x (600s each) before giving
    /// up with a warning + `None` so a hung disk doesn't hang forever
    /// silently.
    pub fn next_block(&mut self) -> Option<std::io::Result<Vec<u8>>> {
        if self.finished {
            return None;
        }
        let rx = self.block_rx.as_ref()?;
        for attempt in 1..=3 {
            match rx.recv_timeout(Duration::from_secs(600)) {
                Ok(res) => return Some(res),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.finished = true;
                    return None;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    eprintln!(
                        "warning: read-ahead stall 600s (attempt {}/3) — retrying",
                        attempt
                    );
                }
            }
        }
        eprintln!("warning: read-ahead timeout after 3x600s — treating as EOF");
        self.finished = true;
        None
    }

    /// Returns a consumed buffer to the pool (non-blocking: drops the buffer
    /// on a full pool instead of stalling the consumer).
    pub fn recycle(&self, buf: Vec<u8>) {
        let _ = self.recycle_tx.try_send(buf);
    }
}

impl Drop for ReadAhead {
    fn drop(&mut self) {
        // Drop the receiver FIRST so a reader thread blocked on a full queue
        // observes Disconnected and exits; only then join (no deadlock).
        self.finished = true;
        drop(self.block_rx.take());
        if let Some(h) = self.handle.take() {
            // Best-effort join; must never panic inside Drop.
            let _ = h.join();
        }
    }
}
