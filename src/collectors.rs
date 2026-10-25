use serde_json::{json, Value};

use crate::models::{Anchor, CollectResult, LiveRoom, now_str};

const COMPASS_COMMENT: &str =
    "https://compass.jinritemai.com/compass_api/content_live/author/live_screen/review_comment";
const COMPASS_TREND: &str =
    "https://compass.jinritemai.com/compass_api/author/live/live_screen/blend_trend_v2";
const COMPASS_HISTORY: &str =
    "https://buyin.jinritemai.com/compass_api/content_live/author/live_detail/history_live";
const QIANCHUAN_LIVE: &str = "https://qianchuan.jinritemai.com/ad/api/live/room/stat";

pub fn auth_status() -> Value {
    let compass = crate::session::named_cookie("compass");
    let qianchuan = crate::session::named_cookie("qianchuan");
    json!({
        "douyin": crate::session::status(),
        "compass": {
            "authorized": !compass.is_empty(),
            "cookie_chars": compass.len(),
            "login": "https://compass.jinritemai.com",
            "set": "session --platform compass --set '<cookie>'",
        },
        "qianchuan": {
            "authorized": !qianchuan.is_empty(),
            "cookie_chars": qianchuan.len(),
            "login": "https://qianchuan.jinritemai.com",
            "set": "session --platform qianchuan --set '<cookie>'",
        }
    })
}

pub fn collect_all(anchor: &Anchor, room: Option<&LiveRoom>) -> Vec<CollectResult> {
    vec![
        live_meta(anchor, room),
        juliang(anchor, room),
        qianchuan(anchor, room),
    ]
}

fn live_meta(anchor: &Anchor, room: Option<&LiveRoom>) -> CollectResult {
    match room {
        None => CollectResult {
            success: false,
            platform_id: "anchor_live".into(),
            raw: json!({}),
            error: "no room".into(),
        },
        Some(r) => CollectResult {
            success: true,
            platform_id: "anchor_live".into(),
            raw: json!({
                "secUid": r.sec_uid,
                "webRid": r.web_rid,
                "roomId": r.room_id,
                "title": r.title,
                "online": r.online,
                "live": r.live,
                "streamUrl": r.stream_url,
                "hlsUrl": r.hls_url,
                "qualities": r.qualities,
                "anchorId": anchor.id,
                "collectTime": now_str(),
            }),
            error: String::new(),
        },
    }
}

fn juliang(anchor: &Anchor, room: Option<&LiveRoom>) -> CollectResult {
    let cookie = crate::session::named_cookie("compass");
    let room_id = room.map(|r| r.room_id.clone()).unwrap_or_default();
    let sec = if anchor.sec_uid.is_empty() {
        room.map(|r| r.sec_uid.clone()).unwrap_or_default()
    } else {
        anchor.sec_uid.clone()
    };
    if cookie.is_empty() {
        return CollectResult {
            success: false,
            platform_id: "juliang".into(),
            raw: json!({
                "authorized": false,
                "login": "https://compass.jinritemai.com",
                "hint": "session --platform compass --set '<compass cookie>'",
                "apis": [COMPASS_COMMENT, COMPASS_TREND, COMPASS_HISTORY],
            }),
            error: "no compass cookie".into(),
        };
    }
    let body = json!({
        "room_id": room_id,
        "roomId": room_id,
        "author_id": sec,
        "sec_uid": sec,
        "secUid": sec,
    });
    let comment = post_json(COMPASS_COMMENT, &cookie, "https://compass.jinritemai.com/", &body);
    let trend = post_json(COMPASS_TREND, &cookie, "https://compass.jinritemai.com/", &body);
    let history = post_json(COMPASS_HISTORY, &cookie, "https://buyin.jinritemai.com/", &body);
    let metrics = merge_metrics(vec![&comment.1, &trend.1, &history.1]);
    let ok = comment.0 == 200 || trend.0 == 200 || history.0 == 200;
    CollectResult {
        success: ok,
        platform_id: "juliang".into(),
        raw: json!({
            "authorized": true,
            "secUid": sec,
            "roomId": room_id,
            "metrics": metrics,
            "review_comment": { "http": comment.0, "data": clip(&comment.1) },
            "blend_trend": { "http": trend.0, "data": clip(&trend.1) },
            "history_live": { "http": history.0, "data": clip(&history.1) },
        }),
        error: if ok { String::new() } else { format!("http {}/{}/{}", comment.0, trend.0, history.0) },
    }
}

fn qianchuan(anchor: &Anchor, room: Option<&LiveRoom>) -> CollectResult {
    let cookie = crate::session::named_cookie("qianchuan");
    let room_id = room.map(|r| r.room_id.clone()).unwrap_or_default();
    if cookie.is_empty() {
        return CollectResult {
            success: false,
            platform_id: "qianchuan".into(),
            raw: json!({
                "authorized": false,
                "login": "https://qianchuan.jinritemai.com",
                "hint": "session --platform qianchuan --set '<qianchuan cookie>'",
            }),
            error: "no qianchuan cookie".into(),
        };
    }
    let body = json!({
        "room_id": room_id,
        "aweme_id": room_id,
        "sec_uid": anchor.sec_uid,
    });
    let (code, data) = post_json(QIANCHUAN_LIVE, &cookie, "https://qianchuan.jinritemai.com/", &body);
    CollectResult {
        success: code == 200,
        platform_id: "qianchuan".into(),
        raw: json!({
            "authorized": true,
            "http": code,
            "metrics": pick_metrics(&data),
            "data": clip(&data),
        }),
        error: if code == 200 { String::new() } else { format!("http {code}") },
    }
}

fn post_json(url: &str, cookie: &str, referer: &str, body: &Value) -> (u16, Value) {
    let mut req = crate::detector::agent()
        .post(url)
        .set("Content-Type", "application/json")
        .set("Referer", referer)
        .set("Origin", referer.trim_end_matches('/'));
    if !cookie.is_empty() {
        req = req.set("Cookie", cookie);
    }
    match req.send_json(body.clone()) {
        Ok(resp) => {
            let code = resp.status();
            let v = resp.into_json::<Value>().unwrap_or(Value::Null);
            (code, v)
        }
        Err(ureq::Error::Status(code, resp)) => {
            let v = resp.into_json::<Value>().unwrap_or(Value::Null);
            (code, v)
        }
        Err(_) => (0, Value::Null),
    }
}

pub fn pick_metrics(v: &Value) -> Value {
    let mut out = serde_json::Map::new();
    walk_metrics(v, &mut out, 0);
    Value::Object(out)
}

fn walk_metrics(v: &Value, out: &mut serde_json::Map<String, Value>, depth: usize) {
    if depth > 6 || out.len() > 40 {
        return;
    }
    match v {
        Value::Object(m) => {
            for (k, val) in m {
                let lk = k.to_ascii_lowercase();
                if is_kpi(&lk) && (val.is_number() || val.is_string()) {
                    out.entry(k.clone()).or_insert(val.clone());
                }
                walk_metrics(val, out, depth + 1);
            }
        }
        Value::Array(a) => {
            for x in a.iter().take(20) {
                walk_metrics(x, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn is_kpi(k: &str) -> bool {
    [
        "gmv", "uv", "pv", "watch", "comment", "pay", "click", "show", "impress",
        "convert", "order", "sales", "online", "duration", "refund", "ctr", "cvr",
        "roi", "cost", "stat_cost", "pay_amt", "live_watch",
    ]
    .iter()
    .any(|p| k.contains(p))
}

fn merge_metrics(vs: Vec<&Value>) -> Value {
    let mut out = serde_json::Map::new();
    for v in vs {
        if let Value::Object(m) = pick_metrics(v) {
            for (k, val) in m {
                out.entry(k).or_insert(val);
            }
        }
    }
    Value::Object(out)
}

fn clip(v: &Value) -> Value {
    let s = v.to_string();
    if s.len() <= 4000 {
        v.clone()
    } else {
        json!({"truncated": true, "chars": s.len()})
    }
}
