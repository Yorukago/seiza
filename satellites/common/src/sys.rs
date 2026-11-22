//! Readings of how the machine is doing, for the bar and the widgets.

/// (total, idle) CPU time counters; two readings give the load in between.
pub fn read_cpu_counters() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let n: Vec<u64> = stat.lines().next()?.split_whitespace().skip(1).take(8).filter_map(|v| v.parse().ok()).collect();
    (n.len() == 8).then(|| (n.iter().sum(), n[3] + n[4])) // idle + iowait
}

pub fn read_ram_pct() -> Option<u32> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = |key: &str| info.lines().find(|l| l.starts_with(key))?.split_whitespace().nth(1)?.parse::<f64>().ok();
    Some((100.0 * (1.0 - kb("MemAvailable:")? / kb("MemTotal:")?)).round() as u32)
}

/// CPU temperature in Celsius: the first hwmon chip that looks like a CPU.
pub fn read_temp() -> Option<u32> {
    for e in std::fs::read_dir("/sys/class/hwmon").ok()?.filter_map(Result::ok) {
        let name = std::fs::read_to_string(e.path().join("name")).unwrap_or_default();
        if ["k10temp", "coretemp", "zenpower", "cpu_thermal"].contains(&name.trim()) {
            if let Ok(t) = std::fs::read_to_string(e.path().join("temp1_input")) {
                return Some(t.trim().parse::<u32>().ok()? / 1000);
            }
        }
    }
    None
}

/// (percent, charging) of the first battery, if the machine has one.
pub fn read_battery() -> Option<(u32, bool)> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?.filter_map(Result::ok).find(|e| e.file_name().to_string_lossy().starts_with("BAT"))?.path();
    let pct = std::fs::read_to_string(dir.join("capacity")).ok()?.trim().parse().ok()?;
    let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
    Some((pct, status.trim() == "Charging"))
}

pub fn read_load() -> Option<f32> {
    std::fs::read_to_string("/proc/loadavg").ok()?.split_whitespace().next()?.parse().ok()
}
