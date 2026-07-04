use serde::Serialize;
use std::sync::Mutex;
use sysinfo::{Disks, Networks, System};

pub struct SystemState {
    pub system: Mutex<System>,
    pub networks: Mutex<Networks>,
    pub disks: Mutex<Disks>,
}

impl SystemState {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu();
        system.refresh_memory();

        let networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();

        Self {
            system: Mutex::new(system),
            networks: Mutex::new(networks),
            disks: Mutex::new(disks),
        }
    }
}

#[derive(Serialize)]
pub struct SystemStats {
    pub cpu: f32,
    pub memory: f32,
    pub disk: f32,
    pub net_rx: u64,
    pub net_tx: u64,
}

#[tauri::command]
pub fn get_system_stats(state: tauri::State<'_, SystemState>) -> Result<SystemStats, String> {
    let mut sys = state
        .system
        .lock()
        .map_err(|_| "system lock poisoned".to_string())?;
    let mut networks = state
        .networks
        .lock()
        .map_err(|_| "networks lock poisoned".to_string())?;
    let mut disks = state
        .disks
        .lock()
        .map_err(|_| "disks lock poisoned".to_string())?;

    sys.refresh_cpu();
    sys.refresh_memory();
    networks.refresh();
    disks.refresh();

    let cpu = sys.global_cpu_info().cpu_usage();

    let total_mem = sys.total_memory() as f32;
    let used_mem = sys.used_memory() as f32;
    let memory = if total_mem > 0.0 {
        (used_mem / total_mem) * 100.0
    } else {
        0.0
    };

    let mut disk_total: u64 = 0;
    let mut disk_available: u64 = 0;
    for disk in disks.list() {
        disk_total = disk_total.saturating_add(disk.total_space());
        disk_available = disk_available.saturating_add(disk.available_space());
    }
    let disk_used = disk_total.saturating_sub(disk_available) as f32;
    let disk = if disk_total > 0 {
        (disk_used / disk_total as f32) * 100.0
    } else {
        0.0
    };

    let mut net_rx: u64 = 0;
    let mut net_tx: u64 = 0;
    for (_name, data) in networks.iter() {
        net_rx = net_rx.saturating_add(data.received());
        net_tx = net_tx.saturating_add(data.transmitted());
    }

    Ok(SystemStats {
        cpu,
        memory,
        disk,
        net_rx,
        net_tx,
    })
}
