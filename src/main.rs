use std::sync::Arc;

use clap::{Parser, Subcommand};
use live_record_analysis::api::{self, App};
use live_record_analysis::pipeline::Pipeline;

#[derive(Parser)]
#[command(name = "live-record", about = "直播录制分析")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 识别 URL 并探测是否在播
    Probe { url: String },
    /// 写入主播
    Add {
        url: String,
        #[arg(long)]
        detect: bool,
    },
    /// 列出主播
    List,
    /// 本地 REST（对齐 HttpListenerServer）
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 5001)]
        port: u16,
    },
    /// 常驻弹幕 WSS（打印 JSON 行）
    Barrage { url: String },
    /// 网页 cookie：查看 / 写入 / 刷新 ttwid（--platform compass|qianchuan|douyin）
    Session {
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        refresh: bool,
        #[arg(long, default_value = "douyin")]
        platform: String,
    },
    /// 口播 + 弹幕复盘
    Analyze {
        path: String,
        #[arg(long, default_value = "")]
        title: String,
    },
    /// 探测 + 巨量/千川采集
    Collect { url: String },
    /// 添加主播并启动调度（自动录制 + 弹幕 + 下播复盘）
    Watch { url: String },
}

fn main() {
    live_record_analysis::cpu::apply_control_plane();
    let cli = Cli::parse();
    let pipe = Pipeline::default_paths().expect("open db");
    match cli.cmd {
        Cmd::Probe { url } => {
            let v = pipe.probe(&url).expect("probe");
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
        }
        Cmd::Add { url, detect } => {
            let a = pipe.add(&url).expect("add");
            println!("{}", serde_json::to_string_pretty(&a).unwrap());
            if detect {
                pipe.scheduler.start_detection();
                eprintln!("detection started, ctrl+c to stop");
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
        Cmd::List => {
            let rows = pipe.store.list_anchors().expect("list");
            println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        }
        Cmd::Serve { host, port } => {
            let app = Arc::new(App {
                store: pipe.store.clone(),
                scheduler: pipe.scheduler.clone(),
                token: std::sync::Mutex::new(String::new()),
            });
            api::serve(&host, port, app).expect("serve");
        }
        Cmd::Session { set, refresh, platform } => {
            if let Some(raw) = set {
                live_record_analysis::session::set_named(&platform, &raw);
            }
            if refresh && platform == "douyin" {
                match live_record_analysis::session::refresh() {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
            }
            println!("{}", serde_json::to_string_pretty(&live_record_analysis::session::status()).unwrap());
        }
        Cmd::Analyze { path, title } => {
            let job = live_record_analysis::analysis::AnalysisPipeline::default().run(&path, &title);
            println!("{}", serde_json::to_string_pretty(&job).unwrap());
        }
        Cmd::Collect { url } => {
            let room = live_record_analysis::detector::detect(&url);
            let mut a = live_record_analysis::models::Anchor {
                source_url: url.clone(),
                live_url: url,
                sec_uid: room.sec_uid.clone(),
                web_rid: room.web_rid.clone(),
                id: Some(0),
                ..live_record_analysis::models::Anchor::default()
            };
            a.anchor_name = room.nickname.clone();
            let rows = live_record_analysis::collectors::collect_all(&a, Some(&room));
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({"room": room, "collect": rows})).unwrap()
            );
        }
        Cmd::Watch { url } => {
            let a = pipe.add(&url).expect("add");
            println!("{}", serde_json::to_string_pretty(&a).unwrap());
            pipe.scheduler.start_detection();
            eprintln!("watching, ctrl+c to stop");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        Cmd::Barrage { url } => {
            use live_record_analysis::barrage::BarrageHub;
            use live_record_analysis::barrage_ws::BarrageStart;
            use live_record_analysis::detector::detect;
            let room = detect(&url);
            if room.room_id.is_empty() && room.wss_url.is_empty() {
                eprintln!("no room_id/wss_url: {}", room.error);
                std::process::exit(1);
            }
            let hub = BarrageHub::default();
            hub.start(
                BarrageStart {
                    anchor_id: 0,
                    room_id: room.room_id,
                    wss_url: room.wss_url,
                    video_id: "live".into(),
                    batch: "cli".into(),
                },
                |ev| {
                    println!("{}", serde_json::to_string(&ev).unwrap_or_default());
                },
            );
            eprintln!("barrage connected, ctrl+c to stop");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
}
