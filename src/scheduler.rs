use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::barrage::BarrageHub;
use crate::barrage_ws::BarrageStart;
use crate::collectors::collect_all;
use crate::danmu_sink::DanmuSink;
use crate::db::Store;
use crate::detector::detect;
use crate::models::{now_str, Anchor, ClientConfig, LiveRoom, LiveStatus, RecordStatus};
use crate::platforms::match_url;
use crate::pool::run_bounded;
use crate::recorder::Recorder;

pub struct Scheduler {
    store: Arc<Store>,
    cfg: ClientConfig,
    recorder: Arc<Recorder>,
    hub: Arc<BarrageHub>,
    stop: Arc<AtomicBool>,
    pub detecting: Arc<AtomicBool>,
}

impl Scheduler {
    pub fn new(store: Arc<Store>, cfg: ClientConfig) -> Self {
        let recorder = Arc::new(Recorder::new(cfg.clone()));
        let store_hook = store.clone();
        recorder.set_on_finish(Arc::new(move |_aid, video_id, path| {
            let title = store_hook
                .video_path(&video_id)
                .ok()
                .flatten()
                .map(|(_, t)| t)
                .unwrap_or_default();
            let _ = crate::analysis::finalize_recording(
                &store_hook,
                &video_id,
                &path.to_string_lossy(),
                &title,
            );
        }));
        Self {
            recorder,
            hub: Arc::new(BarrageHub::default()),
            store,
            cfg,
            stop: Arc::new(AtomicBool::new(false)),
            detecting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn recorder(&self) -> Arc<Recorder> {
        self.recorder.clone()
    }

    pub fn add_anchor(&self, url: &str, auto_record: i64, barrage: i64) -> Result<Anchor, String> {
        let parsed = match_url(url)?;
        let name = if !parsed.web_rid.is_empty() {
            parsed.web_rid.clone()
        } else if !parsed.username.is_empty() {
            parsed.username.clone()
        } else if !parsed.sec_uid.is_empty() {
            parsed.sec_uid.clone()
        } else {
            url.to_string()
        };
        let mut a = Anchor {
            anchor_name: name,
            anchor_platform: parsed.spec.platform_name.clone(),
            platform_value: parsed.spec.platform_value,
            live_url: if parsed.spec.bean == "DouYinLive" {
                url.to_string()
            } else {
                String::new()
            },
            home_url: if parsed.spec.bean == "DouYinHomeLive" {
                url.to_string()
            } else {
                String::new()
            },
            app_share_url: if parsed.spec.bean == "DouYinAppShareLive" {
                url.to_string()
            } else {
                String::new()
            },
            is_auto_record: auto_record,
            sec_uid: parsed.sec_uid,
            web_rid: parsed.web_rid,
            source_url: url.to_string(),
            is_barrage_monitoring: barrage,
            add_time: now_str(),
            ..Anchor::default()
        };
        self.store
            .upsert_anchor(&mut a)
            .map_err(|e| e.to_string())?;
        Ok(a)
    }

    pub fn start_record(&self, anchor_id: i64) -> Result<serde_json::Value, String> {
        if self.recorder.is_recording(anchor_id) {
            return Ok(serde_json::json!({"ok": true, "already": true}));
        }
        let mut a = self
            .store
            .get_anchor(anchor_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "anchor not found".to_string())?;
        let url = first_url(&a);
        let room = detect(&url);
        if !room.live || (room.stream_url.is_empty() && room.hls_url.is_empty()) {
            return Ok(serde_json::json!({
                "ok": false,
                "error": if room.error.is_empty() { "offline" } else { &room.error },
                "room": room,
            }));
        }
        apply_room(&mut a, &room);
        self.store.upsert_anchor(&mut a).map_err(|e| e.to_string())?;
        self.ingest(&a, &room)
    }

    pub fn stop_record(&self, anchor_id: i64) -> Result<serde_json::Value, String> {
        self.recorder.stop(anchor_id);
        self.hub.stop(anchor_id);
        if let Ok(Some(mut a)) = self.store.get_anchor(anchor_id) {
            a.record_status = RecordStatus::Idle as i64;
            let _ = self.store.upsert_anchor(&mut a);
        }
        Ok(serde_json::json!({"ok": true}))
    }

    fn ingest(&self, a: &Anchor, room: &LiveRoom) -> Result<serde_json::Value, String> {
        let stream = if room.stream_url.is_empty() {
            &room.hls_url
        } else {
            &room.stream_url
        };
        let video = self.recorder.start(a, stream, &room.title)?;
        let _ = self.store.insert_video(&video);
        if let Some(id) = a.id {
            if let Ok(Some(mut row)) = self.store.get_anchor(id) {
                row.record_status = RecordStatus::Recording as i64;
                row.last_record_time = now_str();
                let _ = self.store.upsert_anchor(&mut row);
            }
        }
        attach_barrage(&self.hub, self.store.clone(), a, room, &video.video_id, &video.batch_number);
        persist_collects(&self.store, a, room);
        Ok(serde_json::json!({
            "ok": true,
            "video_id": video.video_id,
            "path": video.storage_path,
        }))
    }

    pub fn start_detection(&self) {
        if self.detecting.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stop.store(false, Ordering::SeqCst);
        let store = self.store.clone();
        let recorder = self.recorder.clone();
        let hub = self.hub.clone();
        let stop = self.stop.clone();
        let detecting = self.detecting.clone();
        let interval = self.cfg.detection_fre.max(2) as u64;
        let max_conc = self.cfg.max_concurrency;
        let stagger = self.cfg.stagger_delay_ms;
        let cycle = Arc::new(AtomicU64::new(0));
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let n = cycle.fetch_add(1, Ordering::Relaxed);
                let anchors = store.list_anchors().unwrap_or_default();
                let stop2 = stop.clone();
                let store2 = store.clone();
                let recorder2 = recorder.clone();
                let hub2 = hub.clone();
                run_bounded(
                    anchors,
                    max_conc,
                    stagger,
                    || stop2.load(Ordering::SeqCst),
                    move |mut a| {
                        let url = first_url(&a);
                        if url.is_empty() {
                            return;
                        }
                        let Some(id) = a.id else { return };
                        if recorder2.is_recording(id) {
                            // Every 4th tick, re-probe so a finished live does not leak ffmpeg.
                            if n % 4 != 0 {
                                return;
                            }
                            let room = detect(&url);
                            apply_room(&mut a, &room);
                            let _ = store2.upsert_anchor(&mut a);
                            if !room.live {
                                recorder2.stop(id);
                                hub2.stop(id);
                                a.record_status = RecordStatus::Idle as i64;
                                let _ = store2.upsert_anchor(&mut a);
                            }
                            return;
                        }
                        let room = detect(&url);
                        apply_room(&mut a, &room);
                        let _ = store2.upsert_anchor(&mut a);
                        if room.live
                            && a.is_auto_record != 0
                            && !recorder2.is_recording(id)
                            && !(room.stream_url.is_empty() && room.hls_url.is_empty())
                        {
                            let stream = if room.stream_url.is_empty() {
                                &room.hls_url
                            } else {
                                &room.stream_url
                            };
                            if let Ok(video) = recorder2.start(&a, stream, &room.title) {
                                let _ = store2.insert_video(&video);
                                a.record_status = RecordStatus::Recording as i64;
                                a.last_record_time = now_str();
                                let _ = store2.upsert_anchor(&mut a);
                                attach_barrage(
                                    &hub2,
                                    store2.clone(),
                                    &a,
                                    &room,
                                    &video.video_id,
                                    &video.batch_number,
                                );
                            }
                        }
                        if !room.live && recorder2.is_recording(id) {
                            recorder2.stop(id);
                            hub2.stop(id);
                            a.record_status = RecordStatus::Idle as i64;
                            let _ = store2.upsert_anchor(&mut a);
                        }
                    },
                );
                let end = std::time::Instant::now() + Duration::from_secs(interval);
                while !stop.load(Ordering::SeqCst) && std::time::Instant::now() < end {
                    thread::sleep(Duration::from_millis(200));
                }
            }
            detecting.store(false, Ordering::SeqCst);
        });
    }

    pub fn stop_detection(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.hub.stop_all();
        if let Ok(list) = self.store.list_anchors() {
            for a in list {
                if let Some(id) = a.id {
                    self.recorder.stop(id);
                }
            }
        }
    }

    pub fn status(&self) -> serde_json::Value {
        let (used, max) = self.recorder.slots();
        serde_json::json!({
            "detecting": self.detecting.load(Ordering::SeqCst),
            "recording": self.recorder.recording_ids(),
            "barrage": self.hub.active(),
            "slots_used": used,
            "slots_max": max,
            "ffmpeg": self.recorder.has_ffmpeg(),
            "signer": crate::sign::configured(),
            "max_concurrency": self.cfg.max_concurrency,
            "stagger_delay_ms": self.cfg.stagger_delay_ms,
            "reconnect": self.cfg.reconnect,
            "detection_fre": self.cfg.detection_fre,
        })
    }
}

fn attach_barrage(
    hub: &BarrageHub,
    store: Arc<Store>,
    a: &Anchor,
    room: &LiveRoom,
    video_id: &str,
    batch: &str,
) {
    if a.is_barrage_monitoring == 0 {
        return;
    }
    let Some(id) = a.id else { return };
    if room.room_id.is_empty() && !room.wss_url.starts_with("wss://") {
        return;
    }
    let sink = DanmuSink::spawn(store);
    hub.start(
        BarrageStart {
            anchor_id: id,
            room_id: room.room_id.clone(),
            wss_url: room.wss_url.clone(),
            video_id: video_id.to_string(),
            batch: batch.to_string(),
        },
        move |ev| sink.push(ev),
    );
}

fn persist_collects(store: &Store, a: &Anchor, room: &LiveRoom) {
    let Some(id) = a.id else { return };
    for c in collect_all(a, Some(room)) {
        let raw = serde_json::to_string(&c.raw).unwrap_or_default();
        let _ = store.insert_collect(id, &c.platform_id, c.success, &raw, &c.error);
    }
}

fn first_url(a: &Anchor) -> String {
    for s in [&a.source_url, &a.live_url, &a.home_url, &a.app_share_url] {
        if !s.is_empty() {
            return s.clone();
        }
    }
    String::new()
}

fn apply_room(a: &mut Anchor, room: &LiveRoom) {
    a.live_status = if room.live {
        LiveStatus::Live as i64
    } else {
        LiveStatus::Offline as i64
    };
    if !room.stream_url.is_empty() {
        a.stream_url = room.stream_url.clone();
    } else if !room.hls_url.is_empty() {
        a.stream_url = room.hls_url.clone();
    }
    a.online_number = room.online.to_string();
    if !room.nickname.is_empty() {
        a.anchor_name = room.nickname.clone();
    }
    if !room.avatar.is_empty() {
        a.anchor_avatar = room.avatar.clone();
    }
    if !room.sec_uid.is_empty() {
        a.sec_uid = room.sec_uid.clone();
    }
    if !room.web_rid.is_empty() {
        a.web_rid = room.web_rid.clone();
    }
    if room.live {
        a.start_time = now_str();
    }
}
