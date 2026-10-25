use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::db::Store;
use crate::models::now_str;

const ATTRS: &[&str] = &[
    "path",
    "domain",
    "expires",
    "max-age",
    "secure",
    "httponly",
    "samesite",
    "priority",
    "partitioned",
];

static JAR: OnceLock<Mutex<CookieJar>> = OnceLock::new();
static SINK: Mutex<Option<Arc<Store>>> = Mutex::new(None);
static TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn lock_for_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug, Default)]
pub struct CookieJar {
    map: HashMap<String, String>,
}

impl CookieJar {
    pub fn parse(raw: &str) -> Self {
        let mut jar = Self::default();
        jar.merge(raw);
        jar
    }

    pub fn merge(&mut self, raw: &str) {
        for line in raw.split(['\n', '\r']) {
            let line = line.trim().trim_start_matches("set-cookie:").trim_start_matches("Set-Cookie:");
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut first = true;
            for part in line.split(';') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let (k, v) = match part.split_once('=') {
                    Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
                    None => {
                        first = false;
                        continue;
                    }
                };
                if k.is_empty() {
                    continue;
                }
                let kl = k.to_ascii_lowercase();
                if ATTRS.contains(&kl.as_str()) {
                    first = false;
                    continue;
                }
                if !first && looks_like_attr(&kl) {
                    continue;
                }
                self.map.insert(k.to_string(), v.to_string());
                first = false;
            }
        }
        self.map.retain(|_, v| !v.is_empty());
    }

    pub fn header(&self) -> String {
        let mut keys: Vec<_> = self.map.keys().cloned().collect();
        keys.sort();
        keys.into_iter()
            .filter_map(|k| self.map.get(&k).map(|v| format!("{k}={v}")))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.map
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn has_ttwid(&self) -> bool {
        self.get("ttwid").map(|v| v.len() > 8).unwrap_or(false)
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn keys(&self) -> Vec<String> {
        let mut k: Vec<_> = self.map.keys().cloned().collect();
        k.sort();
        k
    }

    pub fn snapshot(&self) -> String {
        self.header()
    }

    pub fn redacted(&self) -> Value {
        json!({
            "has_ttwid": self.has_ttwid(),
            "keys": self.keys(),
            "ttwid_prefix": self.get("ttwid").map(|v| v.chars().take(8).collect::<String>()).unwrap_or_default(),
            "count": self.map.len(),
        })
    }
}

fn looks_like_attr(k: &str) -> bool {
    ATTRS.iter().any(|a| k.starts_with(a))
}

fn jar() -> &'static Mutex<CookieJar> {
    JAR.get_or_init(|| Mutex::new(CookieJar::default()))
}

pub fn install(store: Arc<Store>) {
    let raw = store.get_web_session("douyin").unwrap_or_default();
    *jar().lock().unwrap() = CookieJar::parse(&raw);
    *SINK.lock().unwrap() = Some(store);
}

pub fn current() -> CookieJar {
    jar().lock().unwrap().clone()
}

pub fn header() -> String {
    jar().lock().unwrap().header()
}

pub fn set_raw(raw: &str) {
    {
        let mut g = jar().lock().unwrap();
        *g = CookieJar::parse(raw);
    }
    persist();
}

pub fn merge_raw(raw: &str) {
    if raw.is_empty() {
        return;
    }
    {
        jar().lock().unwrap().merge(raw);
    }
    persist();
}

pub fn persist() {
    let store = SINK.lock().unwrap().clone();
    if let Some(store) = store {
        let snap = jar().lock().unwrap().snapshot();
        let _ = store.put_web_session("douyin", &snap);
    }
}

pub fn ensure() {
    if current().has_ttwid() {
        return;
    }
    let _ = refresh();
}

pub fn refresh() -> Result<CookieJar, String> {
    let cookie = header();
    let mut req = crate::detector::agent()
        .get("https://live.douyin.com/")
        .set("Referer", "https://live.douyin.com/");
    if !cookie.is_empty() {
        req = req.set("Cookie", &cookie);
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(e.to_string()),
    };
    let set_cookie = resp.header("set-cookie").unwrap_or("").to_string();
    let ms = resp.header("x-ms-token").unwrap_or("").to_string();
    let body = resp.into_string().unwrap_or_default();
    merge_raw(&set_cookie);
    if !ms.is_empty() {
        merge_raw(&format!("msToken={ms}"));
    }
    if let Some(i) = body.find("ttwid=") {
        let slice = &body[i..i.saturating_add(200).min(body.len())];
        merge_raw(slice);
    }
    Ok(current())
}

pub fn status() -> Value {
    let mut v = current().redacted();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("updated_at".into(), json!(now_str()));
        obj.insert("compass".into(), json!(!named_cookie("compass").is_empty()));
        obj.insert("qianchuan".into(), json!(!named_cookie("qianchuan").is_empty()));
    }
    v
}

pub fn named_cookie(platform: &str) -> String {
    for key in [
        format!("LIVE_{}_COOKIE", platform.to_ascii_uppercase()),
        format!("IFUPAN_{}_COOKIE", platform.to_ascii_uppercase()),
    ] {
        if let Ok(v) = std::env::var(&key) {
            if !v.is_empty() {
                return v;
            }
        }
    }
    if platform == "douyin" {
        return header();
    }
    let store = SINK.lock().unwrap().clone();
    store
        .and_then(|s| s.get_web_session(platform).ok())
        .unwrap_or_default()
}

pub fn set_named(platform: &str, raw: &str) {
    if platform == "douyin" || platform.is_empty() {
        set_raw(raw);
        return;
    }
    let store = SINK.lock().unwrap().clone();
    if let Some(store) = store {
        let _ = store.put_web_session(platform, raw);
    }
}
