use std::{fs::OpenOptions, io::Write, path::Path, time::Duration};

use anyhow::{anyhow, Result};
use dbus::{
    arg::{self, Variant},
    blocking::Connection,
};

use regex::Regex;

pub fn create_cgroup(
    container_id: &str,
    target_pid: u32,
    mem: Option<String>,
    cpus: Option<f32>,
    pids: Option<i32>,
) -> Result<()> {
    let conn = Connection::new_system()?;
    let proxy = conn.with_proxy(
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        Duration::new(5, 0),
    );

    use super::dbus_systemd::OrgFreedesktopSystemd1Manager;

    let properties = build_properties(target_pid, mem, cpus, pids, container_id)?;
    let _r = proxy.start_transient_unit(
        &format!("rocker-{}.scope", container_id),
        "replace",
        properties,
        Vec::new(),
    )?;

    Ok(())
}

pub fn add_process_to_cgroup(container_id: &str, pid: u32) -> Result<()> {
    // Check if using cgroup v2.
    // https://github.com/opencontainers/runc/blob/master/docs/cgroup-v2.md
    if Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        let path = fetch_cgroup_v2_scope_path(container_id);
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(format!("{}/cgroup.procs", path))?;
        write!(file, "{}", pid)?;
    } else {
        let paths = fetch_cgourp_v1_resource_paths(container_id);
        for path in paths {
            let mut file = OpenOptions::new()
                .write(true)
                .append(true)
                .open(format!("{}/cgroup.procs", path))?;
            write!(file, "{}", pid)?;
        }
    }

    Ok(())
}

fn fetch_cgroup_v2_scope_path(container_id: &str) -> String {
    format!("/sys/fs/cgroup/system.slice/rocker-{}.scope", container_id)
}

fn fetch_cgourp_v1_resource_paths(container_id: &str) -> Vec<String> {
    // Rocker only supports "cpu", "memory", and "pids".
    let resources = ["cpu", "memory", "pids"];
    let mut paths: Vec<String> = Vec::new();
    for r in resources.iter() {
        paths.push(format!(
            "/sys/fs/cgroup/{}/system.slice/rocker-{}.scope",
            r, container_id
        ));
    }
    paths
}

fn build_properties(
    target_pid: u32,
    mem: Option<String>,
    cpus: Option<f32>,
    pids: Option<i32>,
    container_id: &str,
) -> Result<Vec<(&'static str, arg::Variant<Box<dyn arg::RefArg>>)>> {
    let mut vec: Vec<(&str, arg::Variant<Box<dyn arg::RefArg>>)> = Vec::new();
    vec.push(("PIDs", Variant(Box::new(vec![target_pid]))));
    vec.push((
        "Description",
        Variant(Box::new(
            format!("rocker container: {}", container_id).to_string(),
        )),
    ));

    if mem.is_some() {
        vec.push(("MemoryAccounting", Variant(Box::new(true))));
        let mem_bytes = parse_memory_limit(mem.unwrap())?;
        vec.push(("MemoryMax", Variant(Box::new(mem_bytes))));
    }

    if cpus.is_some() {
        vec.push(("CPUAccounting", Variant(Box::new(true))));
        vec.push((
            "CPUQuotaPerSecUSec",
            Variant(Box::new((cpus.unwrap() * 1000000.0).round() as u64)),
        ));
    }

    if pids.is_some() {
        vec.push(("TasksAccounting", Variant(Box::new(true))));
        vec.push(("TasksMax", Variant(Box::new(pids.unwrap() as u64))))
    }

    Ok(vec)
}

fn parse_memory_limit(mem: String) -> Result<u64> {
    let re = Regex::new(r"(\d+)(.*)").unwrap();
    let mut bytes: String = "".to_string();
    let mut unit: String = "".to_string();
    for cap in re.captures_iter(&mem) {
        bytes = cap[1].to_string();
        unit = cap[2].to_string();
    }

    let bytes: &str = &bytes;
    let unit: &str = &unit;

    if bytes == "" {
        return Err(anyhow!("Memory limit format invalid"));
    }

    let bytes: u64 = bytes.parse().unwrap();
    match unit {
        "" => Ok(bytes),
        u => match u {
            "K" | "KB" | "k" | "kb" => Ok(bytes * (1e3 as u64)),
            "M" | "MB" | "m" | "mb" => Ok(bytes * (1e6 as u64)),
            "G" | "GB" | "g" | "gb" => Ok(bytes * (1e9 as u64)),
            "T" | "TB" | "t" | "tb" => Ok(bytes * (1e12 as u64)),
            _ => Err(anyhow!("Invalid memory unit")),
        },
    }
}
