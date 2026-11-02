//! Host memory figures for the Performance screen and F3.

use std::sync::OnceLock;

/// Total physical memory in MiB.
#[must_use]
pub fn total_memory_mb() -> u32 {
    static T: OnceLock<u32> = OnceLock::new();
    *T.get_or_init(|| {
        let mut s = sysinfo::System::new();
        s.refresh_memory();
        (s.total_memory() / (1024 * 1024)) as u32
    })
}

/// Resident memory of this process in MiB.
#[must_use]
pub fn process_memory_mb() -> f32 {
    memory_stats::memory_stats().map_or(0.0, |m| m.physical_mem as f32 / (1024.0 * 1024.0))
}
