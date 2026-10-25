use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum PlatformValue {
    Douyin = 0,
    Tiktok = 1,
    Kuaishou = 2,
    Xiaohongshu = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveStatus {
    Offline = 0,
    Live = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordStatus {
    Idle = 0,
    Recording = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum AnalysisStatus {
    NotAnalyzed = 0,
    Pending = 1,
    Processing = 2,
    Completed = 3,
    Failed = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DanmuType {
    Chat = 1,
    Like = 2,
    Enter = 3,
    Follow = 4,
    Gift = 5,
    Online = 6,
    FansClub = 7,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlatformSpec {
    pub url_prefix: String,
    pub bean: String,
    pub platform: String,
    pub platform_name: String,
    pub platform_value: i32,
}

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub serial_number: String,
    pub detection_fre: i64,
    pub save_path: String,
    pub live_source: i64,
    pub limit_type: i64,
    pub limit_value: String,
    pub is_record: i64,
    pub is_subsection: i64,
    pub hide_duration: i64,
    pub hide_size: i64,
    pub live_notice: i64,
    pub online_number: i64,
    pub max_concurrency: usize,
    pub stagger_delay_ms: u64,
    pub reconnect: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            serial_number: "0.1.0".into(),
            detection_fre: 15,
            save_path: String::new(),
            live_source: 0,
            limit_type: 2,
            limit_value: "240".into(),
            is_record: 1,
            is_subsection: 0,
            hide_duration: 0,
            hide_size: 0,
            live_notice: 0,
            online_number: 1,
            max_concurrency: 4,
            stagger_delay_ms: 500,
            reconnect: true,
        }
    }
}

impl ClientConfig {
    pub fn slice_minutes(&self) -> u64 {
        self.limit_value.parse::<u64>().unwrap_or(240).max(1)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Anchor {
    pub id: Option<i64>,
    pub anchor_name: String,
    pub anchor_avatar: String,
    pub anchor_platform: String,
    pub platform_value: i32,
    pub home_url: String,
    pub live_url: String,
    pub app_share_url: String,
    pub live_status: i64,
    pub record_status: i64,
    pub is_auto_record: i64,
    pub sec_uid: String,
    pub web_rid: String,
    pub online_number: String,
    pub batch_number: String,
    pub start_time: String,
    pub add_time: String,
    pub stream_url: String,
    pub source_url: String,
    pub is_barrage_monitoring: i64,
    pub is_auto_upload_cloud: i64,
    pub last_record_time: String,
}

impl Default for Anchor {
    fn default() -> Self {
        Self {
            id: None,
            anchor_name: String::new(),
            anchor_avatar: String::new(),
            anchor_platform: "抖音直播".into(),
            platform_value: 0,
            home_url: String::new(),
            live_url: String::new(),
            app_share_url: String::new(),
            live_status: 0,
            record_status: 0,
            is_auto_record: 0,
            sec_uid: String::new(),
            web_rid: String::new(),
            online_number: "0".into(),
            batch_number: String::new(),
            start_time: String::new(),
            add_time: String::new(),
            stream_url: String::new(),
            source_url: String::new(),
            is_barrage_monitoring: 0,
            is_auto_upload_cloud: 0,
            last_record_time: "2020-01-01 00:00:00".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AnchorVideo {
    pub id: Option<i64>,
    pub video_id: String,
    pub video_name: String,
    pub start_time: String,
    pub paragraph: i64,
    pub duration: String,
    pub video_size: String,
    pub end_time: String,
    pub batch_number: String,
    pub storage_path: String,
    pub live_url: String,
    pub source_url: String,
    pub anchor_id: i64,
    pub is_recording: i64,
    pub live_title: String,
    pub analysis_status: i64,
    pub error_reason: String,
    pub sec_uid: String,
    pub platform_type: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveRoom {
    pub success: bool,
    pub live: bool,
    pub title: String,
    pub nickname: String,
    pub avatar: String,
    pub sec_uid: String,
    pub web_rid: String,
    pub room_id: String,
    pub stream_url: String,
    pub hls_url: String,
    pub online: i64,
    pub cover: String,
    pub error: String,
    pub wss_url: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub qualities: serde_json::Value,
}

impl Default for LiveRoom {
    fn default() -> Self {
        Self {
            success: false,
            live: false,
            title: String::new(),
            nickname: String::new(),
            avatar: String::new(),
            sec_uid: String::new(),
            web_rid: String::new(),
            room_id: String::new(),
            stream_url: String::new(),
            hls_url: String::new(),
            online: 0,
            cover: String::new(),
            error: String::new(),
            wss_url: String::new(),
            qualities: serde_json::Value::Null,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DanmuEvent {
    pub msg_type: i32,
    pub content: String,
    pub nick_name: String,
    pub user_id: String,
    pub gift_name: String,
    pub gift_count: i64,
    pub online: i64,
    pub event_time: String,
    pub msg_id: String,
    pub batch_number: String,
    pub video_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub method: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sec_uid: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CollectResult {
    pub success: bool,
    pub platform_id: String,
    pub raw: serde_json::Value,
    pub error: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisJob {
    pub video_id: String,
    pub storage_path: String,
    pub status: i32,
    pub transcript: String,
    pub report: String,
    pub error: String,
}

pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
