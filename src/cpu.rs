//! Process roles: control plane keeps CPU; ffmpeg/ASR/signer run niced (and pinned when possible).

use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Control,
    Ffmpeg,
    Signer,
    Asr,
}

pub fn nproc() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1)
}

/// Cores held for detect / WSS / SQLite. 0 on small machines so we do not starve ffmpeg.
pub fn reserved_cores() -> usize {
    let n = nproc();
    if n >= 8 {
        2
    } else if n >= 6 {
        1
    } else {
        0
    }
}

pub fn control_cores() -> Vec<usize> {
    (0..reserved_cores()).collect()
}

pub fn worker_cores() -> Vec<usize> {
    let n = nproc();
    let r = reserved_cores();
    if r == 0 || r >= n {
        (0..n).collect()
    } else {
        (r..n).collect()
    }
}

pub fn apply_control_plane() {
    #[cfg(target_os = "linux")]
    pin_self(&control_cores());
    #[cfg(target_os = "macos")]
    set_qos(QOS_USER_INITIATED);
}

pub fn decorate(cmd: &mut Command, role: Role) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let nice = match role {
            Role::Control => 0,
            Role::Signer => 5,
            Role::Ffmpeg => 10,
            Role::Asr => 15,
        };
        let workers = worker_cores();
        let mut cores = [0usize; 64];
        let n = workers.len().min(64);
        cores[..n].copy_from_slice(&workers[..n]);
        let pin_workers = matches!(role, Role::Ffmpeg | Role::Asr | Role::Signer);
        unsafe {
            cmd.pre_exec(move || {
                if nice != 0 {
                    libc::nice(nice);
                }
                #[cfg(target_os = "linux")]
                if pin_workers {
                    pin_self(&cores[..n]);
                }
                let _ = (pin_workers, cores, n);
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let pri = match role {
            Role::Control => 0x00000020,
            Role::Signer => 0x00004000,
            Role::Ffmpeg => 0x00004000,
            Role::Asr => 0x00000040,
        };
        cmd.creation_flags(pri);
    }
}

pub fn status() -> serde_json::Value {
    serde_json::json!({
        "nproc": nproc(),
        "reserved_cores": reserved_cores(),
        "control_cores": control_cores(),
        "worker_cores": worker_cores(),
        "roles": {
            "control": "normal / cores 0..reserved",
            "ffmpeg": "nice 10 / worker cores",
            "signer": "nice 5 / worker cores",
            "asr": "nice 15 / worker cores / one at a time"
        }
    })
}

#[cfg(target_os = "linux")]
fn pin_self(cores: &[usize]) {
    if cores.is_empty() {
        return;
    }
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &c in cores {
            libc::CPU_SET(c, &mut set);
        }
        let _ = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
    }
}

#[cfg(target_os = "macos")]
const QOS_USER_INITIATED: u32 = 0x15;

#[cfg(target_os = "macos")]
fn set_qos(class: u32) {
    extern "C" {
        fn pthread_set_qos_class_self_np(qos: u32, rel: i32) -> i32;
    }
    unsafe {
        let _ = pthread_set_qos_class_self_np(class, 0);
    }
}
