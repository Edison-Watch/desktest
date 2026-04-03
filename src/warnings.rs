//! Resource usage warnings for heavyweight commands.
//!
//! These are static, informational warnings printed to stderr before
//! resource-intensive operations begin. Suppressed by `--quiet`.

use crate::config::Config;
use crate::task::AppConfig;

const DEFAULT_MEMORY_BYTES: i64 = 4 * 1024 * 1024 * 1024; // 4 GB
const DEFAULT_CPU_CORES: i64 = 4_000_000_000; // 4 CPU cores (in nano-CPUs)

fn format_gb(bytes: i64) -> String {
    let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if (gb - gb.round()).abs() < 0.01 {
        format!("{:.0} GB", gb)
    } else {
        format!("{:.1} GB", gb)
    }
}

fn format_cores(nano_cpus: i64) -> String {
    let cores = nano_cpus as f64 / 1_000_000_000.0;
    if (cores - cores.round()).abs() < 0.01 {
        format!("{:.0}", cores)
    } else {
        format!("{:.1}", cores)
    }
}

/// Warn before creating a Docker container.
pub fn warn_docker_resources(config: &Config) {
    let mem = config
        .container_memory_bytes
        .unwrap_or(DEFAULT_MEMORY_BYTES);
    let cpus = config.container_nano_cpus.unwrap_or(DEFAULT_CPU_CORES);
    eprintln!(
        "Warning: This will allocate a Docker container with {} memory and {} CPU cores.",
        format_gb(mem),
        format_cores(cpus),
    );
}

/// Warn before creating a Tart VM.
pub fn warn_tart_resources() {
    eprintln!(
        "Warning: This will clone a macOS VM (~10+ GB disk). \
         Max 2 VMs can run simultaneously per host (Apple Virtualization license)."
    );
}

/// Warn before running a test suite, with platform-aware resource details.
pub fn warn_suite_resources(config: &Config, apps: &[&AppConfig]) {
    let mut docker_count = 0usize;
    let mut tart_count = 0usize;
    let mut native_count = 0usize;

    for app in apps {
        match app {
            AppConfig::MacosTart { .. } => tart_count += 1,
            AppConfig::MacosNative { .. } => native_count += 1,
            _ => docker_count += 1,
        }
    }

    let total = apps.len();

    if docker_count > 0 && tart_count == 0 {
        // All Docker (possibly some native)
        let mem = config
            .container_memory_bytes
            .unwrap_or(DEFAULT_MEMORY_BYTES);
        let cpus = config.container_nano_cpus.unwrap_or(DEFAULT_CPU_CORES);
        eprintln!(
            "Warning: Running {} test(s) sequentially — each Docker test allocates {} memory and {} CPU cores.",
            total,
            format_gb(mem),
            format_cores(cpus),
        );
    } else if tart_count > 0 && docker_count == 0 {
        // All Tart (possibly some native)
        eprintln!(
            "Warning: Running {} test(s) sequentially — each Tart test clones a macOS VM (~10+ GB disk).",
            total,
        );
    } else if docker_count > 0 && tart_count > 0 {
        // Mixed suite
        let mem = config
            .container_memory_bytes
            .unwrap_or(DEFAULT_MEMORY_BYTES);
        let cpus = config.container_nano_cpus.unwrap_or(DEFAULT_CPU_CORES);
        eprintln!(
            "Warning: Running {} test(s) sequentially — {} Docker ({} memory, {} CPU cores each), {} Tart (~10+ GB disk each){}.",
            total,
            docker_count,
            format_gb(mem),
            format_cores(cpus),
            tart_count,
            if native_count > 0 {
                format!(", {} native", native_count)
            } else {
                String::new()
            },
        );
    } else {
        // All native — lightweight, just note the count
        eprintln!(
            "Warning: Running {} native macOS test(s) sequentially.",
            total,
        );
    }
}

/// Warn before running init-windows (golden image provisioning).
pub fn warn_init_windows_resources() {
    eprintln!(
        "Warning: This will install Windows 11 from ISO and provision dependencies \
         (~40-60 GB disk, may take 30-60 minutes)."
    );
}

/// Warn before running init-macos (golden image provisioning).
pub fn warn_init_macos_resources() {
    eprintln!(
        "Warning: This will download and provision a macOS VM image (~10-20 GB disk, may take 30-60 minutes on first run)."
    );
}
