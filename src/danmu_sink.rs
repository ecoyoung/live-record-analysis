use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::db::Store;
use crate::models::DanmuEvent;

/// Batches danmaku inserts so 10 rooms do not lock SQLite per chat line.
pub struct DanmuSink {
    tx: Sender<DanmuEvent>,
}

impl DanmuSink {
    pub fn spawn(store: Arc<Store>) -> Self {
        let (tx, rx) = mpsc::channel::<DanmuEvent>();
        thread::spawn(move || {
            let mut buf: Vec<DanmuEvent> = Vec::with_capacity(64);
            loop {
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(ev) => {
                        buf.push(ev);
                        while let Ok(more) = rx.try_recv() {
                            buf.push(more);
                            if buf.len() >= 64 {
                                break;
                            }
                        }
                        if buf.len() >= 64 {
                            let _ = store.insert_danmu_batch(&buf);
                            buf.clear();
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if !buf.is_empty() {
                            let _ = store.insert_danmu_batch(&buf);
                            buf.clear();
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        if !buf.is_empty() {
                            let _ = store.insert_danmu_batch(&buf);
                        }
                        break;
                    }
                }
            }
        });
        Self { tx }
    }

    pub fn push(&self, ev: DanmuEvent) {
        let _ = self.tx.send(ev);
    }
}
