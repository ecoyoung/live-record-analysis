use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::models::{now_str, Anchor, AnchorVideo, ClientConfig, RecordStatus};

pub struct RecordSession {
    pub video_id: String,
    pub stop: Arc<Mutex<bool>>,
    pub child: Mutex<Option<Child>>,
}

pub type FinishHook = Arc<dyn Fn(i64, String, PathBuf) + Send + Sync>;

pub struct Recorder {
    pub cfg: ClientConfig,
    sessions: Arc<Mutex<HashMap<i64, Arc<RecordSession>>>>,
    ffmpeg: Option<PathBuf>,
    on_finish: Arc<Mutex<Option<FinishHook>>>,
}

impl Recorder {
    pub fn new(cfg: ClientConfig) -> Self {
        Self {
            ffmpeg: which("ffmpeg"),
            cfg,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            on_finish: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_on_finish(&self, hook: FinishHook) {
        *self.on_finish.lock().unwrap() = Some(hook);
    }

    pub fn is_recording(&self, anchor_id: i64) -> bool {
        self.sessions.lock().unwrap().contains_key(&anchor_id)
    }

    pub fn recording_ids(&self) -> Vec<i64> {
        self.sessions.lock().unwrap().keys().copied().collect()
    }

    pub fn has_ffmpeg(&self) -> bool {
        self.ffmpeg.is_some()
    }

    pub fn slots(&self) -> (usize, usize) {
        (self.recording_ids().len(), self.cfg.max_concurrency.max(1))
    }

    pub fn start(&self, anchor: &Anchor, stream_url: &str, live_title: &str) -> Result<AnchorVideo, String> {
        if stream_url.is_empty() {
            return Err("empty stream_url".into());
        }
        let aid = anchor.id.unwrap_or(0);
        if self.is_recording(aid) {
            return Err("already recording".into());
        }
        let cap = self.cfg.max_concurrency.max(1);
        if self.recording_ids().len() >= cap {
            return Err(format!("record slots full ({cap})"));
        }
        let save_root = PathBuf::from(if self.cfg.save_path.is_empty() {
            "./recordings"
        } else {
            &self.cfg.save_path
        });
        let dir = save_root.join(aid.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let batch = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        let video_id = uuid::Uuid::new_v4().simple().to_string();
        let stem = if anchor.anchor_name.is_empty() {
            anchor.web_rid.clone()
        } else {
            anchor.anchor_name.clone()
        };
        let name = format!("{stem}_{batch}_p1.flv");
        let out = dir.join(&name);
        let video = AnchorVideo {
            id: None,
            video_id: video_id.clone(),
            video_name: name,
            start_time: now_str(),
            paragraph: 1,
            duration: "0".into(),
            video_size: String::new(),
            end_time: String::new(),
            batch_number: batch,
            storage_path: out.to_string_lossy().into(),
            live_url: anchor.live_url.clone(),
            source_url: anchor.source_url.clone(),
            anchor_id: aid,
            is_recording: RecordStatus::Recording as i64,
            live_title: live_title.into(),
            analysis_status: 1,
            error_reason: String::new(),
            sec_uid: anchor.sec_uid.clone(),
            platform_type: anchor.platform_value.to_string(),
        };
        let session = Arc::new(RecordSession {
            video_id: video_id.clone(),
            stop: Arc::new(Mutex::new(false)),
            child: Mutex::new(None),
        });
        self.sessions.lock().unwrap().insert(aid, session.clone());
        let slice_sec = self.cfg.slice_minutes() * 60;
        let ffmpeg = self.ffmpeg.clone();
        let stream = stream_url.to_string();
        let reconnect = self.cfg.reconnect;
        let sessions = self.sessions.clone();
        let hook = self.on_finish.lock().unwrap().clone();
        let vid = video_id.clone();
        let out_cb = out.clone();
        thread::spawn(move || {
            run_record(ffmpeg, stream, out, slice_sec, reconnect, session);
            sessions.lock().unwrap().remove(&aid);
            if let Some(h) = hook {
                h(aid, vid, out_cb);
            }
        });
        Ok(video)
    }

    pub fn stop(&self, anchor_id: i64) {
        if let Some(s) = self.sessions.lock().unwrap().remove(&anchor_id) {
            *s.stop.lock().unwrap() = true;
            if let Some(mut child) = s.child.lock().unwrap().take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

pub fn ffmpeg_input_flags(reconnect: bool) -> Vec<&'static str> {
    let mut v = vec![
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-threads",
        "1",
        "-probesize",
        "32768",
        "-analyzeduration",
        "0",
        "-fflags",
        "+discardcorrupt+nobuffer",
    ];
    if reconnect {
        v.extend([
            "-reconnect",
            "1",
            "-reconnect_streamed",
            "1",
            "-reconnect_on_network_error",
            "1",
            "-reconnect_delay_max",
            "4",
        ]);
    }
    v
}

fn run_record(
    ffmpeg: Option<PathBuf>,
    stream: String,
    out: PathBuf,
    slice_sec: u64,
    reconnect: bool,
    session: Arc<RecordSession>,
) {
    if ffmpeg.is_none() {
        let _ = std::fs::write(&out, b"FLV\x01dry-run");
        for _ in 0..15 {
            if *session.stop.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        return;
    }
    let bin = ffmpeg.unwrap();
    let deadline = Instant::now() + Duration::from_secs(slice_sec.max(1));
    let mut paragraph: u32 = 1;
    while !*session.stop.lock().unwrap() && Instant::now() < deadline {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.as_secs() < 2 {
            break;
        }
        let dest = if paragraph == 1 {
            out.clone()
        } else {
            let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("seg");
            let parent = out.parent().unwrap_or_else(|| Path::new("."));
            parent.join(format!("{stem}_r{paragraph}.flv"))
        };
        let flags = ffmpeg_input_flags(reconnect);
        let mut cmd = Command::new(&bin);
        cmd.args(&flags)
            .args(["-y", "-i", &stream, "-c", "copy", "-map", "0", "-sn", "-t"])
            .arg(remain.as_secs().to_string())
            .arg(&dest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::cpu::decorate(&mut cmd, crate::cpu::Role::Ffmpeg);
        match cmd.spawn() {
            Ok(c) => {
                *session.child.lock().unwrap() = Some(c);
                loop {
                    if *session.stop.lock().unwrap() {
                        if let Some(mut ch) = session.child.lock().unwrap().take() {
                            let _ = ch.kill();
                            let _ = ch.wait();
                        }
                        return;
                    }
                    let done = session
                        .child
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|c| c.try_wait().ok().flatten().is_some())
                        .unwrap_or(true);
                    if done {
                        break;
                    }
                    thread::sleep(Duration::from_millis(800));
                }
                let sz = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                if sz < 2048 {
                    break;
                }
            }
            Err(_) => {
                let _ = std::fs::write(&dest, b"FLV\x01dry-run");
                break;
            }
        }
        if *session.stop.lock().unwrap() || Instant::now() >= deadline {
            break;
        }
        paragraph += 1;
        thread::sleep(Duration::from_secs(1));
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
        #[cfg(windows)]
        {
            let p = dir.join(format!("{name}.exe"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
