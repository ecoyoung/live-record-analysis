use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{Anchor, AnchorVideo, ClientConfig};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS anchor_info (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  anchor_name TEXT NOT NULL,
  anchor_avatar TEXT NOT NULL DEFAULT '',
  anchor_platform TEXT NOT NULL,
  platform_value INTEGER NOT NULL DEFAULT 0,
  home_url TEXT,
  live_url TEXT,
  app_share_url TEXT,
  live_status INTEGER DEFAULT 0,
  record_status INTEGER DEFAULT 0,
  is_auto_record INTEGER DEFAULT 0,
  sec_uid TEXT,
  web_rid TEXT,
  online_number TEXT,
  batch_number TEXT,
  start_time TEXT,
  add_time TEXT,
  stream_url TEXT,
  source_url TEXT,
  is_barrage_monitoring INTEGER NOT NULL DEFAULT 0,
  is_auto_upload_cloud INTEGER NOT NULL DEFAULT 0,
  last_record_time TEXT NOT NULL DEFAULT '2020-01-01 00:00:00'
);
CREATE TABLE IF NOT EXISTS anchor_video (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  video_id TEXT NOT NULL,
  video_name TEXT NOT NULL,
  start_time TEXT NOT NULL,
  paragraph INTEGER NOT NULL DEFAULT 1,
  duration TEXT NOT NULL DEFAULT '0',
  vedio_sizie TEXT,
  end_time TEXT,
  batch_number TEXT,
  storage_path TEXT,
  live_url TEXT,
  source_url TEXT,
  anchor_id INTEGER,
  is_recording INTEGER,
  live_title TEXT,
  analysis_status INTEGER DEFAULT 0,
  error_reason TEXT,
  sec_uid TEXT,
  platform_type TEXT,
  user_id TEXT NOT NULL DEFAULT '0'
);
CREATE TABLE IF NOT EXISTS config (
  id INTEGER PRIMARY KEY,
  serial_number TEXT NOT NULL,
  detection_fre INTEGER NOT NULL,
  save_path TEXT,
  live_source INTEGER NOT NULL DEFAULT 0,
  limit_type INTEGER NOT NULL,
  limit_value TEXT NOT NULL,
  is_rocord INTEGER NOT NULL,
  is_subection INTEGER NOT NULL,
  hide_duration INTEGER NOT NULL,
  hide_size INTEGER NOT NULL,
  live_notice INTEGER NOT NULL,
  online_number INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS danmu (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  video_id TEXT,
  batch_number TEXT,
  msg_type INTEGER,
  content TEXT,
  nick_name TEXT,
  user_id TEXT,
  event_time TEXT,
  payload TEXT
);
CREATE TABLE IF NOT EXISTS web_session (
  platform TEXT PRIMARY KEY,
  cookie TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS collect_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  anchor_id INTEGER,
  platform_id TEXT,
  success INTEGER,
  raw TEXT,
  error TEXT,
  collected_at TEXT
);
"#;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;
             PRAGMA temp_store=MEMORY;
             PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM config", [], |r| r.get(0))?;
        if n == 0 {
            conn.execute(
                "INSERT INTO config (id, serial_number, detection_fre, save_path, live_source,
                 limit_type, limit_value, is_rocord, is_subection, hide_duration, hide_size,
                 live_notice, online_number) VALUES (1, '0.1.0', 15, '', 0, 2, '240', 1, 0, 0, 0, 0, 1)",
                [],
            )?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_config(&self) -> rusqlite::Result<ClientConfig> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT serial_number, detection_fre, save_path, live_source, limit_type,
            limit_value, is_rocord, is_subection, hide_duration, hide_size, live_notice, online_number
            FROM config WHERE id=1", [], |r| {
            Ok(ClientConfig {
                serial_number: r.get(0)?,
                detection_fre: r.get(1)?,
                save_path: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                live_source: r.get(3)?,
                limit_type: r.get(4)?,
                limit_value: r.get(5)?,
                is_record: r.get(6)?,
                is_subsection: r.get(7)?,
                hide_duration: r.get(8)?,
                hide_size: r.get(9)?,
                live_notice: r.get(10)?,
                online_number: r.get(11)?,
                ..ClientConfig::default()
            })
        })
    }

    pub fn save_config(&self, cfg: &ClientConfig) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE config SET serial_number=?, detection_fre=?, save_path=?, live_source=?,
             limit_type=?, limit_value=?, is_rocord=?, is_subection=?, hide_duration=?,
             hide_size=?, live_notice=?, online_number=? WHERE id=1",
            params![
                cfg.serial_number,
                cfg.detection_fre,
                cfg.save_path,
                cfg.live_source,
                cfg.limit_type,
                cfg.limit_value,
                cfg.is_record,
                cfg.is_subsection,
                cfg.hide_duration,
                cfg.hide_size,
                cfg.live_notice,
                cfg.online_number
            ],
        )?;
        Ok(())
    }

    pub fn upsert_anchor(&self, a: &mut Anchor) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        if let Some(id) = a.id {
            conn.execute(
                "UPDATE anchor_info SET anchor_name=?, anchor_avatar=?, anchor_platform=?,
                 platform_value=?, home_url=?, live_url=?, app_share_url=?, live_status=?,
                 record_status=?, is_auto_record=?, sec_uid=?, web_rid=?, online_number=?,
                 batch_number=?, start_time=?, stream_url=?, source_url=?, is_barrage_monitoring=?,
                 is_auto_upload_cloud=?, last_record_time=? WHERE id=?",
                params![
                    a.anchor_name,
                    a.anchor_avatar,
                    a.anchor_platform,
                    a.platform_value,
                    a.home_url,
                    a.live_url,
                    a.app_share_url,
                    a.live_status,
                    a.record_status,
                    a.is_auto_record,
                    a.sec_uid,
                    a.web_rid,
                    a.online_number,
                    a.batch_number,
                    a.start_time,
                    a.stream_url,
                    a.source_url,
                    a.is_barrage_monitoring,
                    a.is_auto_upload_cloud,
                    a.last_record_time,
                    id
                ],
            )?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO anchor_info (anchor_name, anchor_avatar, anchor_platform, platform_value,
                 home_url, live_url, app_share_url, live_status, record_status, is_auto_record,
                 sec_uid, web_rid, online_number, batch_number, start_time, add_time, stream_url,
                 source_url, is_barrage_monitoring, is_auto_upload_cloud, last_record_time)
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    a.anchor_name,
                    a.anchor_avatar,
                    a.anchor_platform,
                    a.platform_value,
                    a.home_url,
                    a.live_url,
                    a.app_share_url,
                    a.live_status,
                    a.record_status,
                    a.is_auto_record,
                    a.sec_uid,
                    a.web_rid,
                    a.online_number,
                    a.batch_number,
                    a.start_time,
                    a.add_time,
                    a.stream_url,
                    a.source_url,
                    a.is_barrage_monitoring,
                    a.is_auto_upload_cloud,
                    a.last_record_time
                ],
            )?;
            let id = conn.last_insert_rowid();
            a.id = Some(id);
            Ok(id)
        }
    }

    fn map_anchor(r: &rusqlite::Row<'_>) -> rusqlite::Result<Anchor> {
        Ok(Anchor {
            id: Some(r.get(0)?),
            anchor_name: r.get(1)?,
            anchor_avatar: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            anchor_platform: r.get(3)?,
            platform_value: r.get(4)?,
            home_url: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            live_url: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            app_share_url: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            live_status: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
            record_status: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
            is_auto_record: r.get::<_, Option<i64>>(10)?.unwrap_or(0),
            sec_uid: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
            web_rid: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
            online_number: r.get::<_, Option<String>>(13)?.unwrap_or_else(|| "0".into()),
            batch_number: r.get::<_, Option<String>>(14)?.unwrap_or_default(),
            start_time: r.get::<_, Option<String>>(15)?.unwrap_or_default(),
            add_time: r.get::<_, Option<String>>(16)?.unwrap_or_default(),
            stream_url: r.get::<_, Option<String>>(17)?.unwrap_or_default(),
            source_url: r.get::<_, Option<String>>(18)?.unwrap_or_default(),
            is_barrage_monitoring: r.get::<_, Option<i64>>(19)?.unwrap_or(0),
            is_auto_upload_cloud: r.get::<_, Option<i64>>(20)?.unwrap_or(0),
            last_record_time: r.get::<_, Option<String>>(21)?.unwrap_or_default(),
        })
    }

    pub fn list_anchors(&self) -> rusqlite::Result<Vec<Anchor>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, anchor_name, anchor_avatar, anchor_platform, platform_value, home_url,
             live_url, app_share_url, live_status, record_status, is_auto_record, sec_uid, web_rid,
             online_number, batch_number, start_time, add_time, stream_url, source_url,
             is_barrage_monitoring, is_auto_upload_cloud, last_record_time
             FROM anchor_info ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], Self::map_anchor)?;
        rows.collect()
    }

    pub fn get_anchor(&self, id: i64) -> rusqlite::Result<Option<Anchor>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, anchor_name, anchor_avatar, anchor_platform, platform_value, home_url,
             live_url, app_share_url, live_status, record_status, is_auto_record, sec_uid, web_rid,
             online_number, batch_number, start_time, add_time, stream_url, source_url,
             is_barrage_monitoring, is_auto_upload_cloud, last_record_time
             FROM anchor_info WHERE id=?",
            [id],
            Self::map_anchor,
        )
        .optional()
    }

    pub fn delete_anchor(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM anchor_info WHERE id=?", [id])?;
        Ok(())
    }

    pub fn insert_video(&self, v: &AnchorVideo) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO anchor_video (video_id, video_name, start_time, paragraph, duration,
             vedio_sizie, end_time, batch_number, storage_path, live_url, source_url, anchor_id,
             is_recording, live_title, analysis_status, error_reason, sec_uid, platform_type)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                v.video_id,
                v.video_name,
                v.start_time,
                v.paragraph,
                v.duration,
                v.video_size,
                v.end_time,
                v.batch_number,
                v.storage_path,
                v.live_url,
                v.source_url,
                v.anchor_id,
                v.is_recording,
                v.live_title,
                v.analysis_status,
                v.error_reason,
                v.sec_uid,
                v.platform_type
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_videos(&self, anchor_id: Option<i64>) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut out = Vec::new();
        if let Some(id) = anchor_id {
            let mut stmt = conn.prepare(
                "SELECT video_id, video_name, start_time, storage_path, is_recording, analysis_status
                 FROM anchor_video WHERE anchor_id=? ORDER BY id DESC",
            )?;
            let rows = stmt.query_map([id], |r| {
                Ok(serde_json::json!({
                    "video_id": r.get::<_, String>(0)?,
                    "video_name": r.get::<_, String>(1)?,
                    "start_time": r.get::<_, String>(2)?,
                    "storage_path": r.get::<_, Option<String>>(3)?,
                    "is_recording": r.get::<_, Option<i64>>(4)?,
                    "analysis_status": r.get::<_, Option<i64>>(5)?,
                }))
            })?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT video_id, video_name, start_time, storage_path, is_recording, analysis_status
                 FROM anchor_video ORDER BY id DESC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(serde_json::json!({
                    "video_id": r.get::<_, String>(0)?,
                    "video_name": r.get::<_, String>(1)?,
                    "start_time": r.get::<_, String>(2)?,
                    "storage_path": r.get::<_, Option<String>>(3)?,
                    "is_recording": r.get::<_, Option<i64>>(4)?,
                    "analysis_status": r.get::<_, Option<i64>>(5)?,
                }))
            })?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn insert_danmu(
        &self,
        video_id: &str,
        batch: &str,
        msg_type: i32,
        content: &str,
        nick: &str,
        user_id: &str,
        event_time: &str,
        payload: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO danmu (video_id, batch_number, msg_type, content, nick_name, user_id,
             event_time, payload) VALUES (?,?,?,?,?,?,?,?)",
            params![video_id, batch, msg_type, content, nick, user_id, event_time, payload],
        )?;
        Ok(())
    }

    pub fn finish_video(&self, video_id: &str, analysis_status: i64, error_reason: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE anchor_video SET is_recording=0, end_time=?, analysis_status=?, error_reason=?
             WHERE video_id=?",
            params![crate::models::now_str(), analysis_status, error_reason, video_id],
        )?;
        Ok(())
    }

    pub fn video_path(&self, video_id: &str) -> rusqlite::Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT storage_path, live_title FROM anchor_video WHERE video_id=? LIMIT 1",
            [video_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())),
        )
        .optional()
    }

    pub fn list_danmu_payloads(&self, video_id: &str) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT payload FROM danmu WHERE video_id=? ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([video_id], |r| r.get::<_, Option<String>>(0))?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    pub fn insert_collect(
        &self,
        anchor_id: i64,
        platform_id: &str,
        success: bool,
        raw: &str,
        error: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO collect_log (anchor_id, platform_id, success, raw, error, collected_at)
             VALUES (?,?,?,?,?,?)",
            params![
                anchor_id,
                platform_id,
                if success { 1 } else { 0 },
                raw,
                error,
                crate::models::now_str()
            ],
        )?;
        Ok(())
    }

    pub fn latest_collects(&self, anchor_id: i64) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT platform_id, success, raw, error, collected_at FROM collect_log
             WHERE anchor_id=? ORDER BY id DESC LIMIT 30",
        )?;
        let rows = stmt.query_map([anchor_id], |r| {
            Ok(serde_json::json!({
                "platform_id": r.get::<_, String>(0)?,
                "success": r.get::<_, i64>(1)?,
                "raw": serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(2)?).unwrap_or(serde_json::Value::Null),
                "error": r.get::<_, String>(3)?,
                "collected_at": r.get::<_, String>(4)?,
            }))
        })?;
        rows.collect()
    }

    pub fn get_web_session(&self, platform: &str) -> rusqlite::Result<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT cookie FROM web_session WHERE platform=?1",
            params![platform],
            |r| r.get(0),
        )
        .optional()
        .map(|v| v.unwrap_or_default())
    }

    pub fn put_web_session(&self, platform: &str, cookie: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO web_session (platform, cookie, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(platform) DO UPDATE SET cookie=excluded.cookie, updated_at=excluded.updated_at",
            params![platform, cookie, crate::models::now_str()],
        )?;
        Ok(())
    }
}
