pub mod analysis;
pub mod api;
pub mod barrage;
pub mod barrage_ws;
pub mod collectors;
pub mod crypto;
pub mod db;
pub mod detector;
pub mod models;
pub mod pipeline;
pub mod platforms;
pub mod pool;
pub mod proto_wire;
pub mod recorder;
pub mod scheduler;
pub mod session;
pub mod sign;
pub mod webcast;

pub use models::{Anchor, ClientConfig, LiveRoom, PlatformValue};
pub use pipeline::Pipeline;
pub use platforms::match_url;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::barrage::ingest;
    use crate::crypto::machine_code;
    use serde_json::json;

    #[test]
    fn url_douyin_rid() {
        let p = match_url("https://live.douyin.com/123456").unwrap();
        assert_eq!(p.spec.bean, "DouYinLive");
        assert_eq!(p.web_rid, "123456");
        assert_eq!(p.spec.platform_value, 0);
    }

    #[test]
    fn url_share_home() {
        assert_eq!(
            match_url("https://v.douyin.com/abc/").unwrap().spec.bean,
            "DouYinAppShareLive"
        );
        let home = match_url("https://www.douyin.com/user/MS4wLjABxxxx").unwrap();
        assert_eq!(home.spec.bean, "DouYinHomeLive");
        assert!(home.sec_uid.starts_with("MS4"));
    }

    #[test]
    fn url_other() {
        assert_eq!(
            match_url("https://www.tiktok.com/@foo").unwrap().spec.platform_value,
            1
        );
        assert_eq!(
            match_url("https://live.kuaishou.com/u/bar").unwrap().spec.platform_value,
            2
        );
        assert_eq!(
            match_url("https://www.redelight.cn/live/x").unwrap().spec.platform_value,
            4
        );
        assert!(match_url("https://example.com/live").is_err());
    }

    #[test]
    fn store_roundtrip() {
        let _g = crate::session::lock_for_test();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.sqlite");
        let pipe = Pipeline::new(db.to_str().unwrap(), dir.path().to_str().unwrap()).unwrap();
        let a = pipe.add("https://live.douyin.com/999").unwrap();
        assert!(a.id.is_some());
        let rows = pipe.store.list_anchors().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].web_rid, "999");
        assert_eq!(pipe.store.get_config().unwrap().slice_minutes(), 240);
    }

    #[test]
    fn barrage_map() {
        let ev = ingest(
            &json!({"method":"WebcastChatMessage","content":"hi","user":{"nickname":"n","id":"1"}}),
            "vid",
            "batch",
        );
        assert_eq!(ev.msg_type, 1);
        let ev2 = ingest(&json!({"method":"WebcastGiftMessage","gift_name":"rose","gift_count":2}), "vid", "batch");
        assert_eq!(ev2.msg_type, 5);
    }

    #[test]
    fn machine_code_stable() {
        let a = machine_code("cpu", "disk", "board");
        let b = machine_code("cpu", "disk", "board");
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn api_routes() {
        let _g = crate::session::lock_for_test();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.sqlite");
        let pipe = Pipeline::new(db.to_str().unwrap(), dir.path().to_str().unwrap()).unwrap();
        let app = api::App {
            store: pipe.store.clone(),
            scheduler: pipe.scheduler.clone(),
            token: std::sync::Mutex::new(String::new()),
        };
        let r = app.handle("GET", "/api/system/getMachineCode", &Default::default(), json!({}));
        assert_eq!(r["code"], 0);
        let r = app.handle(
            "POST",
            "/api/anchorinfo/AddOrUpdateAnchor",
            &Default::default(),
            json!({"url":"https://live.douyin.com/1"}),
        );
        assert_eq!(r["code"], 0);
        let missing = app.handle("GET", "/nope", &Default::default(), json!({}));
        assert_eq!(missing["code"], -9);
        let st = app.handle("GET", "/api/system/status", &Default::default(), json!({}));
        assert_eq!(st["code"], 0);
        assert_eq!(st["data"]["max_concurrency"], 10);
        let s = app.handle(
            "POST",
            "/api/config/session",
            &Default::default(),
            json!({"cookie":"ttwid=abcdefghijk; msToken=zzz"}),
        );
        assert_eq!(s["code"], 0);
        assert_eq!(s["data"]["has_ttwid"], true);
        let g = app.handle("GET", "/api/config/session", &Default::default(), json!({}));
        assert_eq!(g["data"]["has_ttwid"], true);
    }

    #[test]
    fn cookie_jar_parse_and_merge() {
        let mut j = crate::session::CookieJar::parse("ttwid=abcdefghijk; Path=/; Domain=.douyin.com");
        assert!(j.has_ttwid());
        assert_eq!(j.get("ttwid"), Some("abcdefghijk"));
        assert!(j.get("Path").is_none());
        j.merge("msToken=hello; HttpOnly");
        assert_eq!(j.get("msToken"), Some("hello"));
        let h = j.header();
        assert!(h.contains("ttwid=abcdefghijk"));
        assert!(h.contains("msToken=hello"));
        assert!(!h.to_ascii_lowercase().contains("httponly"));
    }

    #[test]
    fn session_roundtrip_store() {
        let _g = crate::session::lock_for_test();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.sqlite");
        let pipe = Pipeline::new(db.to_str().unwrap(), dir.path().to_str().unwrap()).unwrap();
        crate::session::set_raw("ttwid=storedttwidvalue");
        let raw = pipe.store.get_web_session("douyin").unwrap();
        assert!(raw.contains("ttwid=storedttwidvalue"));
        crate::session::install(pipe.store.clone());
        assert!(crate::session::current().has_ttwid());
    }

    #[test]
    fn pool_caps_inflight() {
        let peak = crate::pool::peak_inflight(8, 3, 30);
        assert!(peak <= 3, "peak={peak}");
        assert!(peak >= 1);
    }

    #[test]
    fn sign_stub_md5() {
        assert!(crate::sign::user_unique_id().len() >= 19);
        let params = crate::sign::stub_params("7695952605281110819", "2477807613760054152");
        assert_eq!(
            crate::sign::x_ms_stub(&params),
            "92b54bea64f3c5a3c309d1520cd5e92d"
        );
        let u = format!(
            "{}&signature=abc",
            crate::sign::unsigned_wss("1", "2")
        );
        assert!(!crate::sign::redact_wss(&u).contains("abc"));
    }

    #[test]
    fn pick_flv_prefers_hd1() {
        let m = json!({"FULL_HD1":"http://a.flv","HD1":"http://b.flv","SD1":"http://c.flv"});
        assert_eq!(crate::detector::pick_flv(&m), "http://b.flv");
        std::env::set_var("LIVE_QUALITY", "SD1");
        assert_eq!(crate::detector::pick_flv(&m), "http://c.flv");
        std::env::remove_var("LIVE_QUALITY");
    }

    #[test]
    fn ffmpeg_reconnect_flags() {
        let flags = crate::recorder::ffmpeg_input_flags(true);
        assert!(flags.contains(&"-reconnect"));
        assert!(flags.contains(&"-reconnect_streamed"));
        assert!(flags.contains(&"-threads"));
        assert!(!flags.contains(&"-reconnect_at_eof"));
        let off = crate::recorder::ffmpeg_input_flags(false);
        assert!(!off.contains(&"-reconnect"));
    }

    #[test]
    fn webcast_gzip_chat_roundtrip() {
        let buf = crate::webcast::encode_chat_frame("alice", "hello", true);
        let (resp, events) = crate::webcast::events_from_frame(&buf, "vid", "batch");
        assert!(resp.need_ack);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].nick_name, "alice");
        assert_eq!(events[0].content, "hello");
        assert_eq!(events[0].msg_type, 1);
        assert_eq!(events[0].video_id, "vid");
        assert_eq!(events[0].method, "WebcastChatMessage");
        let gift = crate::webcast::encode_gift_frame("bob", "rose", 3, false);
        let (_, ge) = crate::webcast::events_from_frame(&gift, "vid", "batch");
        assert_eq!(ge.len(), 1);
        assert_eq!(ge[0].msg_type, 5);
        assert_eq!(ge[0].gift_name, "rose");
        assert_eq!(ge[0].gift_count, 3);
        let junk = crate::webcast::encode_unknown_frame("WebcastInteractEffectMessage", false);
        let (_, je) = crate::webcast::events_from_frame(&junk, "vid", "batch");
        assert!(je.is_empty());
        let st = crate::webcast::danmu_stats(&ge);
        assert_eq!(st["gift"], 1);
    }

    #[test]
    fn collector_metrics_and_xhs_url() {
        let _g = crate::session::lock_for_test();
        let v = json!({"data":{"gmv": 12.5, "watch_uv": 9, "foo": 1}});
        let m = crate::collectors::pick_metrics(&v);
        assert_eq!(m["gmv"], 12.5);
        assert_eq!(m["watch_uv"], 9);
        assert!(m.get("foo").is_none());
        let p = crate::platforms::match_url("https://www.xiaohongshu.com/livestream/abc").unwrap();
        assert_eq!(p.spec.platform_value, 4);
        assert_eq!(p.username, "abc");
        let job = crate::analysis::AnalysisPipeline::default().run("/no/such.flv", "t");
        assert!(job.report.contains("stats"));
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.sqlite");
        let pipe = Pipeline::new(db.to_str().unwrap(), dir.path().to_str().unwrap()).unwrap();
        pipe.store
            .insert_collect(1, "juliang", false, "{}", "no compass cookie")
            .unwrap();
        let rows = pipe.store.latest_collects(1).unwrap();
        assert_eq!(rows.len(), 1);
        pipe.store
            .insert_danmu("vid1", "b", 1, "hi", "n", "1", "t", "{\"method\":\"WebcastChatMessage\",\"content\":\"hi\"}")
            .unwrap();
        let fake = dir.path().join("x.flv");
        let _ = std::fs::write(&fake, b"FLV");
        let job = crate::analysis::finalize_recording(pipe.store.as_ref(), "vid1", fake.to_str().unwrap(), "t");
        assert!(job.report.contains("chat"));
    }

    #[test]
    fn barrage_hub_start_stop() {
        let hub = crate::barrage::BarrageHub::default();
        hub.start(
            crate::barrage_ws::BarrageStart {
                anchor_id: 9,
                room_id: String::new(),
                wss_url: String::new(),
                video_id: "v".into(),
                batch: "b".into(),
            },
            |_| {},
        );
        assert!(hub.active().is_empty());
        hub.stop(9);
        hub.stop_all();
    }
}
