//! Local Douyin WSS signature: MD5 X-MS-STUB + `fp.data` frontierSign (Node).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SDK: &str = "1.0.12";
const UA_VER: &str = "5.0+(Windows+NT+10.0%3b+Win64%3b+x64)+AppleWebKit%2f537.36+(KHTML%2c+like+Gecko)+Chrome%2f126.0.0.0+Safari%2f537.36";
const CACHE_TTL: Duration = Duration::from_secs(300);

fn cache() -> &'static Mutex<HashMap<String, (Instant, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

struct SignerProc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

fn daemon() -> &'static Mutex<Option<SignerProc>> {
    static D: OnceLock<Mutex<Option<SignerProc>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(None))
}

pub fn configured() -> bool {
    fp_path().is_some() && which("node").is_some() && (frontier_server().is_some() || frontier_script().is_some())
}

pub fn user_unique_id() -> String {
    let n = uuid::Uuid::new_v4().as_u128();
    format!("{}", 1_000_000_000_000_000_000u128 + (n % 8_999_999_999_999_999_999u128))
}

pub fn stub_params(room_id: &str, user_unique_id: &str) -> String {
    format!(
        "live_id=1,aid=6383,version_code=180800,webcast_sdk_version={SDK},room_id={room_id},sub_room_id=,sub_channel_id=,did_rule=3,user_unique_id={user_unique_id},device_platform=web,device_type=,ac=,identity=audience"
    )
}

pub fn x_ms_stub(params: &str) -> String {
    format!("{:x}", md5::compute(params.as_bytes()))
}

pub fn unsigned_wss(room_id: &str, user_unique_id: &str) -> String {
    format!(
        "wss://webcast5-ws-web-lf.douyin.com/webcast/im/push/v2/?app_name=douyin_web&version_code=180800&webcast_sdk_version={SDK}&update_version_code={SDK}&compress=gzip&device_platform=web&cookie_enabled=true&screen_width=1440&screen_height=960&browser_language=zh-CN&browser_platform=Win32&browser_name=Mozilla&browser_version={UA_VER}&browser_online=true&tz_name=Asia/Shanghai&host=https://live.douyin.com&aid=6383&live_id=1&did_rule=3&endpoint=live_pc&support_wrds=1&user_unique_id={user_unique_id}&im_path=/webcast/im/fetch/&identity=audience&room_id={room_id}&heartbeatDuration=0"
    )
}

pub fn sign_wss(room_id: &str) -> Result<String, String> {
    if room_id.is_empty() {
        return Err("room_id required".into());
    }
    if let Ok(c) = cache().lock() {
        if let Some((t, url)) = c.get(room_id) {
            if t.elapsed() < CACHE_TTL {
                return Ok(url.clone());
            }
        }
    }
    let uid = user_unique_id();
    let stub = x_ms_stub(&stub_params(room_id, &uid));
    let sig = sign_stub(&stub)?;
    let url = format!("{}&signature={sig}", unsigned_wss(room_id, &uid));
    if let Ok(mut c) = cache().lock() {
        c.insert(room_id.to_string(), (Instant::now(), url.clone()));
    }
    Ok(url)
}

fn sign_stub(stub: &str) -> Result<String, String> {
    match sign_via_daemon(stub) {
        Ok(s) if s.len() >= 8 => Ok(s),
        _ => sign_oneshot(stub),
    }
}

fn sign_via_daemon(stub: &str) -> Result<String, String> {
    let mut slot = daemon().lock().map_err(|e| e.to_string())?;
    if slot.as_mut().map(|p| p.child.try_wait().ok().flatten().is_some()).unwrap_or(true) {
        *slot = Some(spawn_daemon()?);
    }
    let proc = slot.as_mut().ok_or_else(|| "signer spawn failed".to_string())?;
    proc.stdin
        .write_all(format!("{stub}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    proc.stdin.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    proc.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
    let sig = line.trim().to_string();
    if sig.len() < 8 {
        *slot = None;
        return Err("empty daemon signature".into());
    }
    Ok(sig)
}

fn spawn_daemon() -> Result<SignerProc, String> {
    let fp = fp_path().ok_or_else(|| "fp.data not found".to_string())?;
    let script = frontier_server().ok_or_else(|| "scripts/frontier_sign_server.js not found".to_string())?;
    let node = which("node").ok_or_else(|| "node not on PATH".to_string())?;
    let mut child = Command::new(node)
        .arg(&script)
        .arg(&fp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().ok_or_else(|| "no stdin".to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;
    Ok(SignerProc {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

fn sign_oneshot(stub: &str) -> Result<String, String> {
    let fp = fp_path().ok_or_else(|| "fp.data not found (copy fp/fp.data to data/fp.data)".to_string())?;
    let script = frontier_script().ok_or_else(|| "scripts/frontier_sign.js not found".to_string())?;
    let node = which("node").ok_or_else(|| "node not on PATH".to_string())?;
    let out = Command::new(node)
        .arg(&script)
        .arg(stub)
        .arg(&fp)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "frontier_sign: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let sig = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sig.len() < 8 {
        return Err("empty local signature".into());
    }
    Ok(sig)
}

pub fn invalidate(room_id: &str) {
    if let Ok(mut c) = cache().lock() {
        c.remove(room_id);
    }
}

pub fn redact_wss(url: &str) -> String {
    match url.split_once("signature=") {
        Some((head, rest)) => {
            let sig: String = rest.chars().take_while(|c| *c != '&').collect();
            format!("{head}signature=<{} chars>", sig.len())
        }
        None => url.to_string(),
    }
}

fn fp_path() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    for key in ["LIVE_FP", "IFUPAN_FP"] {
        if let Ok(p) = std::env::var(key) {
            cands.push(PathBuf::from(p));
        }
    }
    cands.extend([
        PathBuf::from("data/fp.data"),
        PathBuf::from("./data/fp.data"),
        PathBuf::from("fp.data"),
        PathBuf::from("../extracted/app/fp/fp.data"),
    ]);
    cands.into_iter().find(|p| p.is_file())
}

fn frontier_server() -> Option<PathBuf> {
    script_cand("scripts/frontier_sign_server.js")
}

fn frontier_script() -> Option<PathBuf> {
    script_cand("scripts/frontier_sign.js")
}

fn script_cand(rel: &str) -> Option<PathBuf> {
    let mut cands = vec![PathBuf::from(rel), PathBuf::from(format!("./{rel}"))];
    if let Ok(m) = std::env::var("CARGO_MANIFEST_DIR") {
        cands.push(PathBuf::from(m).join(rel));
    }
    cands.into_iter().find(|p| p.is_file())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}
