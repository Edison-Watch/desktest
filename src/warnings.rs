//! Resource usage warnings for heavyweight commands.
//!
//! These are static, informational warnings printed to stderr before
//! resource-intensive operations begin. Suppressed by `--quiet`.

use crate::config::Config;

const DEFAULT_MEMORY_BYTES: i64 = 4 * 1024 * 1024 * 1024; // 4 GB
const DEFAULT_CPU_CORES: i64 = 4_000_000_000; // 4 nano-CPU cores

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
    let mem = config.container_memory_bytes.unwrap_or(DEFAULT_MEMORY_BYTES);
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

/// Warn before running a test suite.
pub fn warn_suite_resources(config: &Config, test_count: usize) {
    let mem = config.container_memory_bytes.unwrap_or(DEFAULT_MEMORY_BYTES);
    let cpus = config.container_nano_cpus.unwrap_or(DEFAULT_CPU_CORES);
    eprintln!(
        "Warning: Running {} test(s) sequentially — each allocates {} memory and {} CPU cores.",
        test_count,
        format_gb(mem),
        format_cores(cpus),
    );
}

/// Warn before running init-macos (golden image provisioning).
pub fn warn_init_macos_resources() {
    eprintln!(
        "Warning: This will download and provision a macOS VM image (~10-20 GB disk, may take 30-60 minutes on first run)."
    );
}
