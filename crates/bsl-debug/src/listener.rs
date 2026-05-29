use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, error, trace};

use crate::client::DebugClient;
use crate::types::events::DebugEvent;

pub struct EventListener {
    stop: Arc<AtomicBool>,
}

impl EventListener {
    pub fn start(
        client: Arc<DebugClient>,
        poll_interval_ms: u64,
    ) -> (Self, mpsc::UnboundedReceiver<DebugEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        std::thread::spawn(move || {
            let interval = Duration::from_millis(poll_interval_ms);

            while !stop_clone.load(Ordering::Relaxed) {
                match client.ping() {
                    Ok(events) => {
                        if events.is_empty() {
                            trace!("ping: no events");
                        }
                        for event in events {
                            debug!(?event, "debug event received");
                            if tx.send(event).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        error!(%e, "ping failed");
                    }
                }

                std::thread::sleep(interval);
            }
        });

        (Self { stop }, rx)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for EventListener {
    fn drop(&mut self) {
        self.stop();
    }
}
