use serde::Serialize;
use std::time::Duration;
use tauri::AppHandle;

#[derive(Serialize)]
pub struct RemoteStats {
    pub cpu: f32,
    pub memory: f32,
    pub disk: f32,
    pub net_rx: u64,
    pub net_tx: u64,
    pub cpu_total: u64,
    pub cpu_idle: u64,
}

fn build_stats_command() -> String {
    "echo '>>>CPU'; head -1 /proc/stat; echo '>>>MEM'; grep -E '^(MemTotal|MemAvailable|MemFree|Buffers|Cached):' /proc/meminfo; echo '>>>DISK'; df -P / | tail -1; echo '>>>NET'; grep -E '^ *(eth|ens|eno|enp|wlan|wl)[0-9]' /proc/net/dev 2>/dev/null || cat /proc/net/dev | tail -n +3".to_string()
}

fn parse_cpu_line(line: &str) -> Option<(u64, u64)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let user: u64 = parts.get(1)?.parse().ok()?;
    let nice: u64 = parts.get(2)?.parse().ok()?;
    let system: u64 = parts.get(3)?.parse().ok()?;
    let idle: u64 = parts.get(4)?.parse().ok()?;
    let iowait: u64 = parts.get(5).unwrap_or(&"0").parse().unwrap_or(0);
    let irq: u64 = parts.get(6).unwrap_or(&"0").parse().unwrap_or(0);
    let softirq: u64 = parts.get(7).unwrap_or(&"0").parse().unwrap_or(0);
    let steal: u64 = parts.get(8).unwrap_or(&"0").parse().unwrap_or(0);

    let idle_total = idle + iowait;
    let non_idle = user + nice + system + irq + softirq + steal;
    let total = idle_total + non_idle;
    Some((total, idle_total))
}

fn parse_stats(output: &str) -> Result<RemoteStats, String> {
    let mut lines = output.lines();
    let mut section = "";

    let mut cpu_total = 0;
    let mut cpu_idle = 0;

    let mut mem_total = 0.0;
    let mut mem_avail = 0.0;
    let mut mem_free = 0.0;
    let mut mem_buffers = 0.0;
    let mut mem_cached = 0.0;
    let mut has_avail = false;

    let mut disk_pct = 0.0;

    let mut rx_bytes = 0;
    let mut tx_bytes = 0;

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with(">>>") {
            section = trimmed;
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }

        match section {
            ">>>CPU" => {
                if trimmed.starts_with("cpu ") {
                    if let Some((t, i)) = parse_cpu_line(trimmed) {
                        cpu_total = t;
                        cpu_idle = i;
                    }
                }
            }
            ">>>MEM" => {
                if trimmed.starts_with("MemTotal:") {
                    if let Some(val) = trimmed.split_whitespace().nth(1) {
                        mem_total = val.parse::<f64>().unwrap_or(0.0);
                    }
                } else if trimmed.starts_with("MemAvailable:") {
                    if let Some(val) = trimmed.split_whitespace().nth(1) {
                        mem_avail = val.parse::<f64>().unwrap_or(0.0);
                        has_avail = true;
                    }
                } else if trimmed.starts_with("MemFree:") {
                    if let Some(val) = trimmed.split_whitespace().nth(1) {
                        mem_free = val.parse::<f64>().unwrap_or(0.0);
                    }
                } else if trimmed.starts_with("Buffers:") {
                    if let Some(val) = trimmed.split_whitespace().nth(1) {
                        mem_buffers = val.parse::<f64>().unwrap_or(0.0);
                    }
                } else if trimmed.starts_with("Cached:") {
                    if let Some(val) = trimmed.split_whitespace().nth(1) {
                        mem_cached = val.parse::<f64>().unwrap_or(0.0);
                    }
                }
            }
            ">>>DISK" => {
                if !trimmed.starts_with("Filesystem") && !trimmed.starts_with("Use%") {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let cap_str = parts[4].replace("%", "");
                        disk_pct = cap_str.parse::<f32>().unwrap_or(0.0);
                    }
                }
            }
            ">>>NET" => {
                if trimmed.contains("|") {
                    continue;
                }
                let clean_line = trimmed.replace(":", " ");
                let clean_parts: Vec<&str> = clean_line.split_whitespace().collect();
                if clean_parts.len() >= 10 {
                    let r = clean_parts[1].parse::<u64>().unwrap_or(0);
                    let t = clean_parts[9].parse::<u64>().unwrap_or(0);
                    rx_bytes += r;
                    tx_bytes += t;
                }
            }
            _ => {}
        }
    }

    if !has_avail {
        mem_avail = mem_free + mem_buffers + mem_cached;
    }
    let mem_used = if mem_total > 0.0 {
        mem_total - mem_avail
    } else {
        0.0
    };
    let mem_pct = if mem_total > 0.0 {
        (mem_used / mem_total * 100.0) as f32
    } else {
        0.0
    };

    Ok(RemoteStats {
        cpu: 0.0, // Calculated on frontend now
        memory: mem_pct,
        disk: disk_pct,
        net_rx: rx_bytes,
        net_tx: tx_bytes,
        cpu_total: cpu_total,
        cpu_idle: cpu_idle,
    })
}

pub async fn get_remote_stats_runtime(
    shared_session_slot: &crate::ssh::SharedSshSessionSlot,
) -> Result<RemoteStats, String> {
    let shared_session = shared_session_slot
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "SSH session not available for monitoring".to_string())?;

    let mut channel = {
        let session = shared_session.lock().await;
        session
            .channel_open_session()
            .await
            .map_err(|e| format!("Channel open failed: {}", e))?
    };

    let cmd = build_stats_command();
    channel
        .exec(true, cmd.as_bytes())
        .await
        .map_err(|e| format!("Exec failed: {}", e))?;

    let mut output = String::new();
    let timeout = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            match channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    output.push_str(&String::from_utf8_lossy(&data));
                }
                Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                    output.push_str(&String::from_utf8_lossy(&data));
                }
                Some(russh::ChannelMsg::ExitStatus { .. })
                | Some(russh::ChannelMsg::Eof)
                | Some(russh::ChannelMsg::Close)
                | None => break,
                _ => {}
            }
        }
    })
    .await;

    if timeout.is_err() {
        let _ = channel.close().await;
        return Err("Remote stats timeout".to_string());
    }

    let _ = channel.close().await;
    drop(shared_session);

    parse_stats(&output)
}

#[allow(dead_code)]
pub async fn get_remote_stats_legacy(
    shared_session_slot: &crate::ssh::SharedSshSessionSlot,
) -> Result<RemoteStats, String> {
    get_remote_stats_runtime(shared_session_slot).await
}

#[tauri::command]
pub async fn get_remote_stats(
    _app_handle: AppHandle,
    supervisor: tauri::State<'_, crate::session::supervisor::SessionSupervisor>,
    _ssh_state: tauri::State<'_, crate::ssh::SshAppState>,
    session_id: String,
) -> Result<RemoteStats, String> {
    supervisor.get_remote_stats(session_id).await
}
