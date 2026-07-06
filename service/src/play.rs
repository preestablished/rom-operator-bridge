//! Continuous-play controller.
//!
//! "Play" advances the emulator frame-by-frame on a **dedicated OS thread** (not
//! the axum runtime, which must stay free, and not the serialized worker thread,
//! which must service Stop/Status). Each iteration flushes buffered input, runs
//! one captured frame via `BridgeBackend::play_step`, and publishes it into a
//! `tokio::watch` channel that `/ws/frames` subscribers read — `watch` inherently
//! holds only the **latest** frame and never blocks the producer, giving the
//! "always-newest / drop-oldest" delivery the UI wants.
//!
//! A shared `AtomicBool` stop flag (checked between frames) is how Pause / Stop /
//! fault halt the loop: the loop stops issuing Runs before the slot is torn down.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;

use tokio::sync::watch;

/// The latest streamed frame, framed as `[u64 frame_counter LE][PNG bytes]`.
/// `None` before the first frame of a run.
pub type FrameSlot = Option<Arc<Vec<u8>>>;

/// Build the binary `/ws/frames` payload for one frame.
pub fn frame_message(frame_counter: u64, png: &[u8]) -> Arc<Vec<u8>> {
    let mut buf = Vec::with_capacity(8 + png.len());
    buf.extend_from_slice(&frame_counter.to_le_bytes());
    buf.extend_from_slice(png);
    Arc::new(buf)
}

struct PlayHandle {
    session_id: String,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

/// Owns the single active Play loop (single-active-session) plus the frames
/// broadcast channel. Cloneable; all state is shared behind `Arc`.
#[derive(Clone)]
pub struct PlayController {
    inner: Arc<Mutex<Option<PlayHandle>>>,
    frames: watch::Sender<FrameSlot>,
}

impl PlayController {
    pub fn new() -> Self {
        let (frames, _rx) = watch::channel(None);
        Self {
            inner: Arc::new(Mutex::new(None)),
            frames,
        }
    }

    /// True iff a loop is currently registered for `session_id`.
    pub fn is_playing(&self, session_id: &str) -> bool {
        self.inner
            .lock()
            .expect("play mutex poisoned")
            .as_ref()
            .is_some_and(|handle| handle.session_id == session_id)
    }

    /// Register a freshly-spawned loop (stops any previous one first). The caller
    /// spawns the thread with the same `stop` flag it passes here.
    pub fn register(&self, session_id: String, stop: Arc<AtomicBool>, join: JoinHandle<()>) {
        self.stop_any();
        let mut guard = self.inner.lock().expect("play mutex poisoned");
        *guard = Some(PlayHandle {
            session_id,
            stop,
            join: Some(join),
        });
    }

    /// Stop the loop for `session_id` (no-op if not playing). Blocks until the
    /// loop thread exits (bounded to ~one frame of compute).
    pub fn stop(&self, session_id: &str) {
        let handle = {
            let mut guard = self.inner.lock().expect("play mutex poisoned");
            match guard.as_ref() {
                Some(handle) if handle.session_id == session_id => guard.take(),
                _ => None,
            }
        };
        Self::finish(handle);
        // A new run starts at frame_counter 0; clear the last streamed frame so a
        // stale image is not delivered to a late `/ws/frames` subscriber.
        let _ = self.frames.send(None);
    }

    /// Stop whatever loop is active (teardown paths: Stop / fault / TTL / replace).
    pub fn stop_any(&self) {
        let handle = self.inner.lock().expect("play mutex poisoned").take();
        Self::finish(handle);
        let _ = self.frames.send(None);
    }

    fn finish(handle: Option<PlayHandle>) {
        if let Some(mut handle) = handle {
            handle.stop.store(true, Ordering::SeqCst);
            if let Some(join) = handle.join.take() {
                let _ = join.join();
            }
        }
    }

    /// A fresh receiver for a `/ws/frames` connection.
    pub fn subscribe(&self) -> watch::Receiver<FrameSlot> {
        self.frames.subscribe()
    }

    /// The sender the loop uses to publish the latest frame.
    pub fn frames_sender(&self) -> watch::Sender<FrameSlot> {
        self.frames.clone()
    }
}

impl Default for PlayController {
    fn default() -> Self {
        Self::new()
    }
}
