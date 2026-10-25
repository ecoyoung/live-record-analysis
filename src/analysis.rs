use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::models::{AnalysisJob, AnalysisStatus, DanmuEvent};

const ARK_CHAT: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";

pub struct AnalysisPipeline {
    pub ark_key: String,
    pub model: String,
}

pub fn finalize_recording(store: &crate::db::Store, video_id: &str, path: &str, title: &str) -> AnalysisJob {
    let payloads = store.list_danmu_payloads(video_id).unwrap_or_default();
    let events: Vec<DanmuEvent> = payloads
        .iter()
        .filter_map(|p| serde_json::from_str::<serde_json::Value>(p).ok())
        .map(|v| crate::barrage::ingest(&v, video_id, "replay"))
        .collect();
    let sidecar = Path::new(path).with_extension("danmu.jsonl");
    let _ = std::fs::write(
        &sidecar,
        payloads.into_iter().map(|p| p + "\n").collect::<String>(),
    );
    let job = AnalysisPipeline::default().run_with_danmu(path, title, &events);
    let report_path = Path::new(path).with_extension("report.json");
    let _ = std::fs::write(&report_path, &job.report);
    let _ = store.finish_video(video_id, job.status as i64, &job.report.chars().take(4000).collect::<String>());
    job
}

impl Default for AnalysisPipeline {
    fn default() -> Self {
        Self {
            ark_key: std::env::var("ARK_API_KEY").unwrap_or_default(),
            model: std::env::var("ARK_MODEL").unwrap_or_else(|_| "doubao-pro-32k".into()),
        }
    }
}

impl AnalysisPipeline {
    pub fn run(&self, video_path: &str, title: &str) -> AnalysisJob {
        self.run_with_danmu(video_path, title, &[])
    }

    pub fn run_with_danmu(&self, video_path: &str, title: &str, danmu: &[DanmuEvent]) -> AnalysisJob {
        let video_id = Path::new(video_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
            .to_string();
        let mut job = AnalysisJob {
            video_id,
            storage_path: video_path.into(),
            status: AnalysisStatus::Processing as i32,
            transcript: String::new(),
            report: String::new(),
            error: String::new(),
        };
        let wav = Path::new(video_path).with_extension("wav");
        if Path::new(video_path).exists() {
            extract_audio(video_path, &wav);
        }
        let extra = load_sidecar_danmu(video_path);
        let events: Vec<DanmuEvent> = if danmu.is_empty() { extra } else { danmu.to_vec() };
        job.transcript = transcribe(&wav);
        if !job.transcript.is_empty() {
            let _ = std::fs::write(Path::new(video_path).with_extension("asr.txt"), &job.transcript);
        }
        let stats = crate::webcast::danmu_stats(&events);
        let local = local_report(title, &job.transcript, &stats, &events);
        let llm = self.diagnose(&job.transcript, title, &stats);
        job.report = json!({
            "title": title,
            "stats": stats,
            "local": local,
            "llm": llm,
        })
        .to_string();
        if job.transcript.is_empty() && events.is_empty() {
            job.error = "no transcript and no danmu".into();
            job.status = AnalysisStatus::Failed as i32;
        } else {
            job.status = AnalysisStatus::Completed as i32;
        }
        job
    }

    fn diagnose(&self, transcript: &str, title: &str, stats: &serde_json::Value) -> serde_json::Value {
        if self.ark_key.is_empty() {
            return json!({"note": "no ARK_API_KEY"});
        }
        let clipped: String = transcript.chars().take(8000).collect();
        let prompt = format!(
            "你是直播复盘助手。根据口播、弹幕统计，输出 JSON：highlights[], issues[], script[], next_actions[]。\n标题:{title}\n统计:{stats}\n口播:\n{clipped}"
        );
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": "只输出 JSON。"},
                {"role": "user", "content": prompt}
            ]
        });
        match ureq::post(ARK_CHAT)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {}", self.ark_key))
            .send_json(body)
        {
            Ok(resp) => resp
                .into_json::<serde_json::Value>()
                .ok()
                .and_then(|v| {
                    v["choices"][0]["message"]["content"]
                        .as_str()
                        .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap_or(json!({"text": s})))
                })
                .unwrap_or(json!({"note": "empty ark"})),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

static ASR_LOCK: Mutex<()> = Mutex::new(());

fn transcribe(wav: &Path) -> String {
    let _g = ASR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !wav.exists() {
        return String::new();
    }
    let sz = std::fs::metadata(wav).map(|m| m.len()).unwrap_or(0);
    if sz < 4096 {
        return String::new();
    }
    let bins = [
        std::env::var("WHISPER_BIN").unwrap_or_default(),
        "whisper-cli".into(),
        "whisper".into(),
        "whisper.cpp".into(),
    ];
    for bin in bins {
        if bin.is_empty() {
            continue;
        }
        let txt = wav.with_extension("txt");
        if run_limited(
            Command::new(&bin)
                .args(["-t", "2", "-p", "1", "-l", "zh", "-nt", "-f"])
                .arg(wav)
                .arg("-otxt")
                .arg("-of")
                .arg(wav.with_extension("")),
            Duration::from_secs(180),
        ) && txt.exists()
        {
            let s = std::fs::read_to_string(txt).unwrap_or_default();
            if !s.trim().is_empty() {
                return s;
            }
        }
        if let Some(s) = run_limited_stdout(
            Command::new(&bin)
                .args(["--language", "zh", "--model", "base", "--output_format", "txt", "--threads", "2"])
                .arg(wav),
            Duration::from_secs(180),
        ) {
            if !s.is_empty() {
                return s;
            }
        }
    }
    String::new()
}

fn run_limited(cmd: &mut Command, limit: Duration) -> bool {
    let mut child = match cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return false,
        }
    }
}

fn run_limited_stdout(cmd: &mut Command, limit: Duration) -> Option<String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => {
                let mut buf = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = std::io::Read::read_to_end(&mut out, &mut buf);
                }
                let s = String::from_utf8_lossy(&buf).trim().to_string();
                return if s.is_empty() { None } else { Some(s) };
            }
            Ok(Some(_)) => return None,
            Ok(None) if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return None,
        }
    }
}

fn extract_audio(video: &str, wav: &Path) {
    let _ = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-threads",
            "1",
            "-i",
            video,
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(wav)
        .status();
}

fn load_sidecar_danmu(video_path: &str) -> Vec<DanmuEvent> {
    let p = Path::new(video_path).with_extension("danmu.jsonl");
    let Ok(text) = std::fs::read_to_string(p) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|v| crate::barrage::ingest(&v, "video", "sidecar"))
        .collect()
}

fn local_report(title: &str, transcript: &str, stats: &serde_json::Value, events: &[DanmuEvent]) -> serde_json::Value {
    let chat = stats.get("chat").and_then(|v| v.as_u64()).unwrap_or(0);
    let gift = stats.get("gift").and_then(|v| v.as_u64()).unwrap_or(0);
    let peak = stats.get("peak_online").and_then(|v| v.as_i64()).unwrap_or(0);
    let mut highlights = Vec::new();
    let mut issues = Vec::new();
    if gift > 0 {
        highlights.push(format!("礼物 {gift} 笔"));
    }
    if peak > 0 {
        highlights.push(format!("在线峰值 {peak}"));
    }
    if chat == 0 {
        issues.push("几乎没有评论文本，检查弹幕解析或场次是否过冷".to_string());
    }
    if transcript.is_empty() {
        issues.push("没有口播转写：安装 whisper 或设置 WHISPER_BIN".to_string());
    }
    let top: Vec<String> = events
        .iter()
        .filter(|e| e.msg_type == 1 && !e.content.is_empty())
        .take(12)
        .map(|e| format!("{}: {}", e.nick_name, e.content))
        .collect();
    json!({
        "title": title,
        "highlights": highlights,
        "issues": issues,
        "sample_chat": top,
        "next_actions": [
            "把高频评论做成固定话术卡",
            "礼物峰值前后 60 秒单独切片复盘"
        ],
    })
}
