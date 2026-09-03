#[cfg(test)]
mod scan_config_tests {
    use super::*;

    /// Verify new() delegates to explicit OS defaults.
    #[test]
    fn new_delegates_to_os_defaults() {
        let registry = FcFontRegistry::new();
        assert_eq!(
            registry.scan_config,
            FcScanConfig::os_defaults(OperatingSystem::current()),
        );
        assert!(!registry.scan_dirs().is_empty());
    }

    /// Verify empty config scans nothing.
    #[test]
    fn empty_config_scout_scans_nothing() {
        let registry = FcFontRegistry::new_with_config(FcScanConfig::empty());
        assert!(registry.scan_dirs().is_empty());

        registry.scout_thread();

        assert!(registry.is_scan_complete());
        let known = registry
            .known_paths
            .read()
            .expect("known_paths lock poisoned");
        assert!(known.is_empty(), "expected no known paths");
        drop(known);
        let queue = registry.build_queue.lock().expect("queue lock poisoned");
        assert!(queue.is_empty(), "expected no build jobs");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod spawn_gating_tests {
    use super::*;

    /// Names of live threads in this process, read from `/proc/self/task`.
    fn thread_names() -> Vec<String> {
        let Ok(entries) = std::fs::read_dir("/proc/self/task") else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
            .map(|s| s.trim().to_string())
            .collect()
    }

    /// Verify spawning is gated on single-thread-unsafe-locks feature.
    #[test]
    fn spawning_is_gated_on_the_lock_implementation() {
        let registry = FcFontRegistry::new();
        registry.spawn_scout_and_builders();

        // Poll for thread creation.
        let mut saw_rfc_thread = false;
        for _ in 0..100 {
            if thread_names().iter().any(|n| n.starts_with("rfc-font-")) {
                saw_rfc_thread = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        if cfg!(feature = "single-thread-unsafe-locks") {
            assert!(!saw_rfc_thread, "threads spawned with unsafe locks");
        } else {
            assert!(saw_rfc_thread, "no threads spawned");
        }
    }
}
