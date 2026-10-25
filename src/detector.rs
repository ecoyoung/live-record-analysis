use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use crate::models::{LiveRoom, PlatformValue};
use crate::platforms::{match_url, ParsedLiveUrl};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub(crate) fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .user_agent(UA)
            .timeout(Duration::from_secs(8))
            .max_idle_connections(16)
            .max_idle_connections_per_host(8)
            .build()
    })
}

fn http_get(url: &str) -> Result<(u16, String, String), String> {
    crate::session::ensure();
    let cookie = crate::session::header();
    let mut req = agent().get(url);
    if url.contains("douyin.com") {
        req = req
            .set("Referer", "https://live.douyin.com/")
            .set("Origin", "https://live.douyin.com");
    } else if url.contains("kuaishou.com") {
        req = req
            .set("Referer", "https://live.kuaishou.com/")
            .set("Origin", "https://live.kuaishou.com");
    } else if url.contains("tiktok.com") {
        req = req
            .set("Referer", "https://www.tiktok.com/")
            .set("Origin", "https://www.tiktok.com");
    } else if url.contains("xiaohongshu.com") || url.contains("redelight.cn") {
        req = req
            .set("Referer", "https://www.xiaohongshu.com/")
            .set("Origin", "https://www.xiaohongshu.com");
    }
    if !cookie.is_empty() {
        req = req.set("Cookie", &cookie);
    }
    match req.call() {
        Ok(resp) => {
            let status = resp.status();
            let final_url = resp.get_url().to_string();
            absorb_session_headers(&resp);
            let body = resp.into_string().unwrap_or_default();
            Ok((status, body, final_url))
        }
        Err(ureq::Error::Status(code, resp)) => {
            absorb_session_headers(&resp);
            let body = resp.into_string().unwrap_or_default();
            Ok((code, body, url.to_string()))
        }
        Err(e) => Err(e.to_string()),
    }
}

fn absorb_session_headers(resp: &ureq::Response) {
    if let Some(c) = resp.header("set-cookie") {
        crate::session::merge_raw(c);
    }
    if let Some(t) = resp
        .header("x-ms-token")
        .or_else(|| resp.header("X-Ms-Token"))
    {
        crate::session::merge_raw(&format!("msToken={t}"));
    }
}

pub fn detect(url: &str) -> LiveRoom {
    let parsed = match match_url(url) {
        Ok(p) => p,
        Err(e) => {
            return LiveRoom {
                error: e,
                ..LiveRoom::default()
            }
        }
    };
    detect_parsed(&parsed)
}

pub fn detect_parsed(parsed: &ParsedLiveUrl) -> LiveRoom {
    match parsed.spec.platform_value {
        x if x == PlatformValue::Douyin as i32 => detect_douyin(parsed),
        x if x == PlatformValue::Kuaishou as i32 => detect_kuaishou(parsed),
        x if x == PlatformValue::Tiktok as i32 => detect_tiktok(parsed),
        x if x == PlatformValue::Xiaohongshu as i32 => detect_xiaohongshu(parsed),
        _ => LiveRoom {
            error: "unknown platform".into(),
            ..LiveRoom::default()
        },
    }
}

fn detect_douyin(parsed: &ParsedLiveUrl) -> LiveRoom {
    let mut web_rid = parsed.web_rid.clone();
    if web_rid.is_empty() {
        if let Ok((_, body, _)) = http_get(&parsed.source_url) {
            if let Some(i) = body.find("\"web_rid\"") {
                let slice = &body[i..i.saturating_add(40).min(body.len())];
                web_rid = slice
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect();
            }
        }
        if web_rid.is_empty() {
            return LiveRoom {
                error: "no web_rid".into(),
                sec_uid: parsed.sec_uid.clone(),
                ..LiveRoom::default()
            };
        }
    }
    let api = format!(
        "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&enter_from=web_live&cookie_enabled=true&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome&browser_version=120.0.0.0&web_rid={web_rid}"
    );
    let (code, body, _) = match http_get(&api) {
        Ok(v) => v,
        Err(e) => {
            return LiveRoom {
                web_rid,
                error: e,
                ..LiveRoom::default()
            }
        }
    };
    if code != 200 {
        return LiveRoom {
            web_rid,
            error: format!("enter http {code}"),
            ..LiveRoom::default()
        };
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            if crate::session::refresh().is_ok() {
                if let Ok((code2, body2, _)) = http_get(&api) {
                    if code2 == 200 {
                        if let Ok(v) = serde_json::from_str::<Value>(&body2) {
                            return finish_douyin(parsed, web_rid, v);
                        }
                    }
                }
            }
            return LiveRoom {
                web_rid,
                error: "bad json".into(),
                ..LiveRoom::default()
            };
        }
    };
    finish_douyin(parsed, web_rid, payload)
}

fn finish_douyin(parsed: &ParsedLiveUrl, web_rid: String, payload: Value) -> LiveRoom {
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    let room = data
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .cloned()
        .or_else(|| data.get("room").cloned())
        .unwrap_or(Value::Null);
    let status = room.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
    let owner = room
        .get("owner")
        .cloned()
        .or_else(|| data.get("user").cloned())
        .unwrap_or(Value::Null);
    let stream = room.get("stream_url").cloned().unwrap_or(Value::Null);
    let flv_map = stream.get("flv_pull_url").cloned().unwrap_or(Value::Null);
    let flv = pick_flv(&flv_map);
    let hls = stream
        .get("hls_pull_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let avatar = owner
        .get("avatar_thumb")
        .and_then(|a| a.get("url_list"))
        .and_then(|u| u.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let online = room
        .get("user_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    LiveRoom {
        success: true,
        live: status == 2 && (!flv.is_empty() || !hls.is_empty()),
        title: room
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        nickname: owner
            .get("nickname")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        avatar,
        sec_uid: owner
            .get("sec_uid")
            .and_then(|v| v.as_str())
            .unwrap_or(&parsed.sec_uid)
            .to_string(),
        web_rid,
        room_id: room
            .get("id_str")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| room.get("id").and_then(|v| v.as_i64()).map(|n| n.to_string()))
            .unwrap_or_default(),
        stream_url: flv,
        hls_url: hls,
        online,
        cover: String::new(),
        error: String::new(),
        wss_url: crate::webcast::find_wss(&payload).unwrap_or_default(),
        qualities: flv_map,
    }
}

pub fn pick_flv(flv_map: &Value) -> String {
    let prefer = std::env::var("LIVE_QUALITY").unwrap_or_else(|_| "HD1".into());
    let order = match prefer.to_ascii_uppercase().as_str() {
        "FULL_HD1" | "FULL" | "ORIGIN" => ["FULL_HD1", "HD1", "SD1", "SD2"],
        "SD1" | "SD" => ["SD1", "HD1", "SD2", "FULL_HD1"],
        _ => ["HD1", "SD1", "FULL_HD1", "SD2"],
    };
    order
        .iter()
        .find_map(|k| flv_map.get(*k).and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

fn detect_kuaishou(parsed: &ParsedLiveUrl) -> LiveRoom {
    let user = parsed.username.clone();
    let api = format!(
        "https://live.kuaishou.com/live_api/liveroom/livedetail?principalId={user}"
    );
    let mut room = LiveRoom {
        web_rid: user.clone(),
        nickname: user.clone(),
        ..LiveRoom::default()
    };
    if let Ok((code, body, _)) = http_get(&api) {
        if code == 200 {
            if let Ok(v) = serde_json::from_str::<Value>(&body) {
                fill_from_json(&mut room, &v);
            }
        }
    }
    if room.stream_url.is_empty() {
        if let Ok((code, body, _)) = http_get(&parsed.source_url) {
            room.success = code == 200;
            fill_from_html(&mut room, &body);
        }
    } else {
        room.success = true;
    }
    room.live = room.live || !room.stream_url.is_empty() || !room.hls_url.is_empty();
    room
}

fn detect_tiktok(parsed: &ParsedLiveUrl) -> LiveRoom {
    let user = parsed.username.clone();
    let api = format!(
        "https://webcast.tiktok.com/webcast/room/web/enter/?aid=1988&app_name=tiktok_web&live_id=12&device_platform=web&language=en-US&enter_from=web_live&cookie_enabled=true&screen_width=1920&screen_height=1080&unique_id={user}&uniqueId={user}"
    );
    let mut room = LiveRoom {
        web_rid: user.clone(),
        nickname: user.clone(),
        ..LiveRoom::default()
    };
    if let Ok((code, body, _)) = http_get(&api) {
        if code == 200 {
            if let Ok(v) = serde_json::from_str::<Value>(&body) {
                fill_from_json(&mut room, &v);
            }
        }
    }
    if room.stream_url.is_empty() {
        let page = format!("https://www.tiktok.com/@{user}/live");
        if let Ok((code, body, _)) = http_get(&page) {
            room.success = code == 200;
            fill_from_html(&mut room, &body);
        }
    } else {
        room.success = true;
    }
    room.live = room.live || !room.stream_url.is_empty() || !room.hls_url.is_empty();
    room
}

fn detect_xiaohongshu(parsed: &ParsedLiveUrl) -> LiveRoom {
    let id = if parsed.username.is_empty() {
        parsed.source_url.clone()
    } else {
        parsed.username.clone()
    };
    let mut room = LiveRoom {
        web_rid: id.clone(),
        ..LiveRoom::default()
    };
    if let Ok((code, body, _)) = http_get(&parsed.source_url) {
        room.success = code == 200;
        fill_from_html(&mut room, &body);
        if let Ok(v) = serde_json::from_str::<Value>(&body) {
            fill_from_json(&mut room, &v);
        }
    }
    room.live = room.live || !room.stream_url.is_empty() || !room.hls_url.is_empty();
    room
}

fn fill_from_json(room: &mut LiveRoom, v: &Value) {
    if let Some(s) = first_str(v, &["nickname", "userName", "anchorName", "name"]) {
        if room.nickname.is_empty() {
            room.nickname = s;
        }
    }
    if let Some(s) = first_str(v, &["title", "liveTitle", "caption"]) {
        room.title = s;
    }
    if let Some(s) = first_str(v, &["roomId", "room_id", "liveStreamId", "liveId"]) {
        room.room_id = s;
    }
    if let Some(n) = first_i64(v, &["userCount", "user_count", "onlineCount", "watchingCount"]) {
        room.online = n;
    }
    let (flv, hls) = media_from_value(v);
    if room.stream_url.is_empty() {
        room.stream_url = flv;
    }
    if room.hls_url.is_empty() {
        room.hls_url = hls;
    }
    if room.wss_url.is_empty() {
        room.wss_url = crate::webcast::find_wss(v).unwrap_or_default();
    }
    if v.to_string().contains("\"living\":true") || v.to_string().contains("\"status\":2") {
        room.live = true;
    }
}

fn fill_from_html(room: &mut LiveRoom, body: &str) {
    let (flv, hls) = media_from_html(body);
    if room.stream_url.is_empty() {
        room.stream_url = flv;
    }
    if room.hls_url.is_empty() {
        room.hls_url = hls;
    }
    if body.contains("\"living\":true") || body.contains("liveStreamId") || body.contains("PlayUrl") {
        room.live = true;
    }
    if let Some(i) = body.find("\"nickname\":\"") {
        let s = &body[i + 12..];
        room.nickname = s.split('"').next().unwrap_or("").to_string();
    }
}

fn media_from_html(body: &str) -> (String, String) {
    let mut flv = String::new();
    let mut hls = String::new();
    for part in body.split('"') {
        if part.starts_with("http") && part.contains(".flv") && flv.is_empty() {
            flv = part.replace("\\u002F", "/").replace("\\/", "/");
        }
        if part.starts_with("http") && (part.contains(".m3u8") || part.contains("hls")) && hls.is_empty() {
            hls = part.replace("\\u002F", "/").replace("\\/", "/");
        }
    }
    (flv, hls)
}

fn media_from_value(v: &Value) -> (String, String) {
    let mut flv = String::new();
    let mut hls = String::new();
    walk_media(v, &mut flv, &mut hls);
    (flv, hls)
}

fn walk_media(v: &Value, flv: &mut String, hls: &mut String) {
    match v {
        Value::String(s) => {
            if flv.is_empty() && s.contains(".flv") {
                *flv = s.clone();
            }
            if hls.is_empty() && (s.contains(".m3u8") || s.contains("hls")) && s.starts_with("http") {
                *hls = s.clone();
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk_media(x, flv, hls)),
        Value::Object(m) => m.values().for_each(|x| walk_media(x, flv, hls)),
        _ => {}
    }
}

fn first_str(v: &Value, keys: &[&str]) -> Option<String> {
    match v {
        Value::Object(m) => {
            for k in keys {
                if let Some(s) = m.get(*k).and_then(|x| x.as_str()) {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
            m.values().find_map(|x| first_str(x, keys))
        }
        Value::Array(a) => a.iter().find_map(|x| first_str(x, keys)),
        _ => None,
    }
}

fn first_i64(v: &Value, keys: &[&str]) -> Option<i64> {
    match v {
        Value::Object(m) => {
            for k in keys {
                if let Some(n) = m.get(*k).and_then(|x| x.as_i64()) {
                    return Some(n);
                }
            }
            m.values().find_map(|x| first_i64(x, keys))
        }
        Value::Array(a) => a.iter().find_map(|x| first_i64(x, keys)),
        _ => None,
    }
}
