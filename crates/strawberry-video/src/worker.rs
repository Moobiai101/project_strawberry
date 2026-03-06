//! # Decode Worker
//!
//! Background worker for async video decoding.
//! This ensures the UI thread never blocks on FFmpeg operations.
//!
//! ## Scrubbing Optimization
//! During rapid scrubbing, old decode requests become stale. The `ClearPending`
//! command allows the VideoService to cancel outdated requests, ensuring only
//! the latest requested frame is decoded.

use crate::cache::FrameKey;
use crate::decode::{VideoDecoder, VideoFrame};
use std::path::PathBuf;
use std::sync::mpsc; // Use std::sync instead of tokio
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Commands sent to the decode worker
#[derive(Debug)]
pub enum DecodeCommand {
    /// Decode a specific frame
    DecodeFrame {
        path: PathBuf,
        timestamp: Duration,
        width: u32,
        height: u32,
    },
    /// Clear all pending decode requests (used during scrubbing)
    ///
    /// This drains the command queue, discarding stale requests.
    /// After clearing, only new requests will be processed.
    ClearPending,
    /// Provide metadata (not used heavily yet, but good for probing)
    Probe { path: PathBuf },
    /// Shutdown the worker
    Shutdown,
}

/// Results returned from the decode worker
#[derive(Debug)]
pub enum DecodeResult {
    /// Frame decoded successfully
    FrameReady {
        key: FrameKey,
        frame: Arc<VideoFrame>,
    },
    /// Error during decode
    Error { key: FrameKey, error: String },
    /// Worker stopped
    Stopped,
    /// Pending requests were cleared (for tracking)
    Cleared { count: usize },
}

/// Helper to request frames asynchronously
///
/// The DecodeWorker runs FFmpeg decode operations on a background thread,
/// preventing the UI from blocking. For scrubbing, use `request_decode_scrub()`
/// which clears stale requests first.
pub struct DecodeWorker {
    sender: mpsc::Sender<DecodeCommand>,
    // We don't hold the receiver here; the service holds it to poll results
}

impl DecodeWorker {
    /// Spawn a new background decode worker
    ///
    /// Returns (worker_handle, result_receiver)
    pub fn spawn() -> (Self, mpsc::Receiver<DecodeResult>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<DecodeCommand>();
        let (res_tx, res_rx) = mpsc::channel::<DecodeResult>();

        thread::spawn(move || {
            // Create a dedicated decoder for this thread
            let decoder: VideoDecoder = match VideoDecoder::new() {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!("Worker failed to create decoder: {}", e);
                    return; // Worker dies
                }
            };

            tracing::info!("Decode worker started");

            // Process commands via blocking receive (sync thread)
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    DecodeCommand::DecodeFrame {
                        path,
                        timestamp,
                        width,
                        height,
                    } => {
                        tracing::info!(
                            "Worker: Decoding frame at {:.2}s ({}x{})",
                            timestamp.as_secs_f64(),
                            width,
                            height
                        );
                        let key: FrameKey = FrameKey::new(path.clone(), timestamp, width, height);

                        match decoder.decode_frame_at_resolution(&path, timestamp, width, height) {
                            Ok(frame) => {
                                tracing::info!(
                                    "Worker: Frame decoded successfully at {:.2}s",
                                    timestamp.as_secs_f64()
                                );
                                // Send back result
                                if res_tx
                                    .send(DecodeResult::FrameReady {
                                        key,
                                        frame: Arc::new(frame),
                                    })
                                    .is_err()
                                {
                                    break; // Channel closed
                                }
                            }
                            Err(e) => {
                                let _ = res_tx.send(DecodeResult::Error {
                                    key,
                                    error: e.to_string(),
                                });
                            }
                        }
                    }
                    DecodeCommand::ClearPending => {
                        // Drain all pending commands (discard stale requests)
                        // This is the key optimization for scrubbing!
                        let mut cleared_count: usize = 0;
                        while let Ok(stale_cmd) = cmd_rx.try_recv() {
                            match stale_cmd {
                                DecodeCommand::Shutdown => {
                                    // Don't discard shutdown commands!
                                    let _ = res_tx.send(DecodeResult::Stopped);
                                    return;
                                }
                                _ => {
                                    cleared_count += 1;
                                    tracing::trace!("Cleared stale decode request");
                                }
                            }
                        }
                        if cleared_count > 0 {
                            tracing::debug!("Cleared {} stale decode requests", cleared_count);
                        }
                        let _ = res_tx.send(DecodeResult::Cleared {
                            count: cleared_count,
                        });
                    }
                    DecodeCommand::Probe { .. } => {
                        // TODO: Implement async probing if needed
                    }
                    DecodeCommand::Shutdown => {
                        let _ = res_tx.send(DecodeResult::Stopped);
                        break;
                    }
                }
            }

            tracing::info!("Decode worker stopped");
        });

        (Self { sender: cmd_tx }, res_rx)
    }

    /// Request a frame decode (standard method)
    ///
    /// Queues the request. If buffer is full, request is dropped.
    pub fn request_decode(&self, path: PathBuf, timestamp: Duration, width: u32, height: u32) {
        // Non-blocking send (fails if buffer full, which is fine to drop frames)
        let _ = self.sender.send(DecodeCommand::DecodeFrame {
            path,
            timestamp,
            width,
            height,
        });
    }

    /// Request a frame decode for scrubbing (clears stale requests first)
    ///
    /// This is optimized for rapid seeking/scrubbing:
    /// 1. Sends ClearPending to discard stale requests
    /// 2. Then sends the new decode request
    ///
    /// This ensures only the latest position is decoded, not outdated ones.
    pub fn request_decode_scrub(
        &self,
        path: PathBuf,
        timestamp: Duration,
        width: u32,
        height: u32,
    ) {
        // Clear any pending requests first
        let _ = self.sender.send(DecodeCommand::ClearPending);

        // Then send the actual decode request
        let _ = self.sender.send(DecodeCommand::DecodeFrame {
            path,
            timestamp,
            width,
            height,
        });
    }

    /// Clear all pending decode requests
    ///
    /// Call this when scrubbing starts or when user rapidly changes position.
    pub fn clear_pending(&self) {
        let _ = self.sender.send(DecodeCommand::ClearPending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_lifecycle() {
        let (worker, rx) = DecodeWorker::spawn();

        // Send shutdown
        let _ = worker.sender.send(DecodeCommand::Shutdown);

        // Should receive Stopped
        match rx.recv() {
            Ok(DecodeResult::Stopped) => {}
            Ok(r) => panic!("Expected Stopped, got {:?}", r),
            Err(_) => panic!("Channel closed unexpectedly"),
        }
    }

    #[test]
    fn test_clear_pending() {
        let (worker, rx) = DecodeWorker::spawn();

        // Clear pending (should work even with empty queue)
        worker.clear_pending();

        // Should receive Cleared result
        match rx.recv() {
            Ok(DecodeResult::Cleared { count }) => {
                assert_eq!(count, 0); // Nothing was pending
            }
            Ok(r) => panic!("Expected Cleared, got {:?}", r),
            Err(_) => panic!("Channel closed unexpectedly"),
        }

        // Shutdown
        let _ = worker.sender.send(DecodeCommand::Shutdown);
    }
}
