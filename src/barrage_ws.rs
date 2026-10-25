use std::io::Read;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tungstenite::client::IntoClientRequest;
use tungstenite::http::header::{COOKIE, ORIGIN, USER_AGENT};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use crate::models::DanmuEvent;
use crate::webcast::{encode_ack, encode_heartbeat, events_from_bytes};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

const HOSTS: &[&str] = &[
    "webcast5-ws-web-lf.douyin.com",
    "webcast5-ws-web-lq.douyin.com",
    "webcast3-ws-web-lf.douyin.com",
];

#[derive(Clone, Debug)]
pub struct BarrageStart {
    pub anchor_id: i64,
    pub room_id: String,
    pub wss_url: String,
    pub video_id: String,
    pub batch: String,
}

pub fn run_loop(req: BarrageStart, stop: Arc<AtomicBool>, sink: Arc<dyn Fn(DanmuEvent) + Send + Sync>) {
    let mut backoff = 1u64;
    let mut host_i = 0usize;
    let mut signed: Option<String> = None;
    while !stop.load(Ordering::SeqCst) {
        crate::session::ensure();
        let cookie = crate::session::header();
        let url = if req.wss_url.starts_with("wss://") {
            req.wss_url.clone()
        } else if let Some(u) = signed.clone() {
            u
        } else if crate::sign::configured() {
            match crate::sign::sign_wss(&req.room_id) {
                Ok(u) => {
                    eprintln!("barrage signed {}", crate::sign::redact_wss(&u));
                    signed = Some(u.clone());
                    u
                }
                Err(e) => {
                    eprintln!("barrage sign err: {e}");
                    build_wss(HOSTS[host_i % HOSTS.len()], &req.room_id)
                }
            }
        } else {
            build_wss(HOSTS[host_i % HOSTS.len()], &req.room_id)
        };
        eprintln!("barrage connecting {}", crate::sign::redact_wss(&url));
        match open_socket(&url, &cookie) {
            Ok(mut ws) => {
                eprintln!("barrage socket ok");
                backoff = 1;
                set_read_timeout(&mut ws, Duration::from_secs(12));
                let mut idle_pings = 0u32;
                while !stop.load(Ordering::SeqCst) {
                    match ws.read() {
                        Ok(Message::Binary(buf)) => {
                            idle_pings = 0;
                            let (resp, events) = events_from_bytes(&buf, &req.video_id, &req.batch);
                            for ev in events {
                                sink(ev);
                            }
                            if resp.need_ack {
                                let ack = encode_ack(&resp.internal_ext);
                                let _ = ws.send(Message::Binary(ack));
                            }
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = ws.send(Message::Pong(p));
                        }
                        Ok(Message::Pong(_)) | Ok(Message::Text(_)) => {}
                        Ok(Message::Close(c)) => {
                            eprintln!("barrage close {c:?}");
                            break;
                        }
                        Ok(Message::Frame(_)) => break,
                        Err(tungstenite::Error::Io(e))
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            idle_pings += 1;
                            let _ = ws.send(Message::Binary(encode_heartbeat()));
                            let _ = ws.send(Message::Ping(Vec::new()));
                            if idle_pings > 8 {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
            Err(e) => {
                eprintln!("barrage socket err: {e}");
                if e.contains("200") {
                    signed = None;
                    crate::sign::invalidate(&req.room_id);
                }
                host_i = host_i.wrapping_add(1);
                if poll_http(&req, &stop, &sink) {
                    backoff = 1;
                    continue;
                }
            }
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_secs(backoff));
        backoff = (backoff * 2).min(30);
    }
}

fn build_wss(host: &str, room_id: &str) -> String {
    format!(
        "wss://{host}/webcast/im/push/v2/?app_name=douyin_web&version_code=180800&webcast_sdk_version=1.0.14-beta.0&update_version_code=1.0.14-beta.0&compress=gzip&device_platform=web&cookie_enabled=true&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=Win32&browser_name=Mozilla&browser_online=true&tz_name=Asia/Shanghai&aid=6383&live_id=1&heartbeatDuration=0&room_id={room_id}"
    )
}

fn open_socket(
    url: &str,
    cookie: &str,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let mut req = url.into_client_request().map_err(|e| e.to_string())?;
    let headers = req.headers_mut();
    headers.insert(
        USER_AGENT,
        UA.parse()
            .map_err(|e: tungstenite::http::header::InvalidHeaderValue| e.to_string())?,
    );
    headers.insert(
        ORIGIN,
        "https://live.douyin.com"
            .parse()
            .map_err(|e: tungstenite::http::header::InvalidHeaderValue| e.to_string())?,
    );
    headers.insert(
        "Referer",
        "https://live.douyin.com/"
            .parse()
            .map_err(|e: tungstenite::http::header::InvalidHeaderValue| e.to_string())?,
    );
    if !cookie.is_empty() {
        headers.insert(
            COOKIE,
            cookie
                .parse()
                .map_err(|e: tungstenite::http::header::InvalidHeaderValue| e.to_string())?,
        );
    }
    let (ws, _) = connect(req).map_err(|e| e.to_string())?;
    Ok(ws)
}

/// HTTP IM fetch when WSS handshake is rejected (often HTTP 200 without 101).
fn poll_http(
    req: &BarrageStart,
    stop: &Arc<AtomicBool>,
    sink: &Arc<dyn Fn(DanmuEvent) + Send + Sync>,
) -> bool {
    if req.room_id.is_empty() {
        return false;
    }
    eprintln!("barrage http poll room_id={}", req.room_id);
    let mut cursor = String::new();
    let mut ok = false;
    for _ in 0..20 {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let url = format!(
            "https://live.douyin.com/webcast/im/fetch/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&identity=audience&resp_content_type=protobuf&last_rtt=-1&room_id={}&cursor={}",
            req.room_id,
            urlencoding_lite(&cursor)
        );
        crate::session::ensure();
        let cookie = crate::session::header();
        let mut call = crate::detector::agent()
            .get(&url)
            .set("Referer", "https://live.douyin.com/")
            .set("Origin", "https://live.douyin.com")
            .set("Accept", "application/x-protobuf, */*");
        if !cookie.is_empty() {
            call = call.set("Cookie", &cookie);
        }
        let resp = match call.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                eprintln!("barrage http poll status {code}");
                let _ = r;
                break;
            }
            Err(e) => {
                eprintln!("barrage http poll err: {e}");
                break;
            }
        };
        let status = resp.status();
        let mut bytes = Vec::new();
        let _ = resp.into_reader().read_to_end(&mut bytes);
        if status != 200 {
            eprintln!("barrage http poll status {status}");
            break;
        }
        if bytes.is_empty() {
            thread::sleep(Duration::from_millis(800));
            continue;
        }
        ok = true;
        let (im, events) = events_from_bytes(&bytes, &req.video_id, &req.batch);
        if !im.cursor.is_empty() {
            cursor = im.cursor.clone();
        }
        if !events.is_empty() {
            eprintln!("barrage http poll {} events", events.len());
        }
        for ev in events {
            sink(ev);
        }
        thread::sleep(Duration::from_millis(800));
    }
    ok
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn set_read_timeout(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>, d: Duration) {
    match ws.get_ref() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(d));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.sock.set_read_timeout(Some(d));
        }
        _ => {}
    }
}
