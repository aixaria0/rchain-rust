//! The sampled resource snapshot and its watchdog.
//!
//! The snapshot is read by the status surface (A1) and will gate admission (units B/C). The watchdog is a
//! dedicated `std::thread`: a signal carried by the executor it protects is not a signal, and a stale
//! sample is reported as `unavailable`, never as `normal`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{cgroup, colocation, disk, meminfo};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceSnapshot {
    /// `normal` when the host facts were read; `unavailable` when the memory facts could not be. The
    /// pressure states (`throttled`/`critical`) arrive with unit C.
    pub state: &'static str,
    pub mem_available_mb: u64,
    pub mem_total_mb: u64,
    pub cgroup_version: &'static str,
    pub cgroup_limit_mb: Option<u64>,
    pub cgroup_current_mb: Option<u64>,
    pub cgroup_oom_kills: u64,
    /// `(quota, period)` from `cpu.max`/`cpu.cfs_quota_us`.
    pub cpu_quota: Option<(u64, u64)>,
    pub data_dir_free_mb: Option<u64>,
    pub rnode_processes: usize,
    /// Milliseconds since the Unix epoch at the sample.
    pub sampled_at_ms: u64,
    /// Which readers contributed, e.g. `meminfo+cgroup+disk`.
    pub source: String,
}

impl Default for ResourceSnapshot {
    fn default() -> Self {
        ResourceSnapshot {
            state: "unavailable",
            mem_available_mb: 0,
            mem_total_mb: 0,
            cgroup_version: "unavailable",
            cgroup_limit_mb: None,
            cgroup_current_mb: None,
            cgroup_oom_kills: 0,
            cpu_quota: None,
            data_dir_free_mb: None,
            rnode_processes: 0,
            sampled_at_ms: 0,
            source: "none".to_string(),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn gather(data_dir: &Path) -> ResourceSnapshot {
    let mem = meminfo::read_meminfo().ok();
    let cg = cgroup::read_cgroup_facts();
    let disk = disk::data_dir_disk_facts(data_dir).ok();
    let mut source = Vec::new();
    if mem.is_some() {
        source.push("meminfo");
    }
    if cg.version != cgroup::CgroupVersion::Unavailable {
        source.push("cgroup");
    }
    if disk.is_some() {
        source.push("disk");
    }
    ResourceSnapshot {
        state: if mem.is_some() {
            "normal"
        } else {
            "unavailable"
        },
        mem_available_mb: mem.map(|m| m.available_kb / 1024).unwrap_or(0),
        mem_total_mb: mem.map(|m| m.total_kb / 1024).unwrap_or(0),
        cgroup_version: cg.version.as_str(),
        cgroup_limit_mb: cg.memory_max.map(|b| b / (1024 * 1024)),
        cgroup_current_mb: cg.memory_current.map(|b| b / (1024 * 1024)),
        cgroup_oom_kills: cg.oom_kill.unwrap_or(0),
        cpu_quota: cg.cpu_quota,
        data_dir_free_mb: disk.map(|d| d.free_bytes / (1024 * 1024)),
        rnode_processes: colocation::count_rnode_processes(Path::new("/proc")),
        sampled_at_ms: now_ms(),
        source: if source.is_empty() {
            "none".to_string()
        } else {
            source.join("+")
        },
    }
}

struct Inner {
    snapshot: RwLock<ResourceSnapshot>,
    data_dir: PathBuf,
}

/// A snapshot that a dedicated thread refreshes; the thread stops once the guard is dropped.
pub struct ResourceGuard {
    inner: Arc<Inner>,
}

impl ResourceGuard {
    pub fn start(data_dir: PathBuf, interval: Duration) -> Result<Arc<Self>, String> {
        let inner = Arc::new(Inner {
            snapshot: RwLock::new(gather(&data_dir)),
            data_dir,
        });
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("resource-watchdog".to_string())
            .spawn(move || loop {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let snapshot = gather(&inner.data_dir);
                if let Ok(mut current) = inner.snapshot.write() {
                    *current = snapshot;
                }
                drop(inner);
                std::thread::sleep(interval);
            })
            .map_err(|e| format!("spawn the resource watchdog: {e}"))?;
        Ok(Arc::new(ResourceGuard { inner }))
    }

    /// A fixed snapshot for tests and callers that have no host to sample.
    pub fn inert(snapshot: ResourceSnapshot) -> Arc<Self> {
        Arc::new(ResourceGuard {
            inner: Arc::new(Inner {
                snapshot: RwLock::new(snapshot),
                data_dir: PathBuf::new(),
            }),
        })
    }

    pub fn snapshot(&self) -> ResourceSnapshot {
        self.inner
            .snapshot
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn sample_now(&self) -> ResourceSnapshot {
        gather(&self.inner.data_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_names_its_source_and_age() {
        let dir = std::env::temp_dir();
        let snapshot = gather(&dir);
        assert!(snapshot.sampled_at_ms > 0, "the sample carries a time");
        assert!(!snapshot.source.is_empty(), "the source is named");
        #[cfg(unix)]
        assert!(
            snapshot.source.contains("disk"),
            "the data dir is statvfs'd on unix"
        );
    }

    #[test]
    fn an_inert_guard_returns_its_snapshot() {
        let snapshot = ResourceSnapshot {
            state: "normal",
            mem_available_mb: 512,
            source: "test".to_string(),
            ..Default::default()
        };
        let guard = ResourceGuard::inert(snapshot.clone());
        assert_eq!(guard.snapshot(), snapshot);
    }
}
