    use super::*;

    // ── Priority ordering ───────────────────────────────────────────────

    #[test]
    fn priority_ordering() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Medium);
        assert!(Priority::Medium > Priority::Low);
    }

    #[test]
    fn build_job_sorts_by_priority() {
        let low = FcBuildJob {
            priority: Priority::Low,
            path: PathBuf::from("a.ttf"),
            font_index: None,
            guessed_family: "a".into(),
        };
        let critical = FcBuildJob {
            priority: Priority::Critical,
            path: PathBuf::from("b.ttf"),
            font_index: None,
            guessed_family: "b".into(),
        };
        let mut jobs = vec![low.clone(), critical.clone()];
        jobs.sort();
        assert_eq!(jobs[0].priority, Priority::Low);
        assert_eq!(jobs[1].priority, Priority::Critical);
    }

    // ── Scout priority assignment ───────────────────────────────────────

    #[test]
    fn assign_scout_priority_common_family_gets_high() {
        use crate::OperatingSystem;
        let common = config::tokenize_common_families(OperatingSystem::MacOS);

        let tokens = tokenize_all("Helvetica");
        assert_eq!(assign_scout_priority(&tokens, &common), Priority::High);

        let tokens = tokenize_all("TimesNewRoman-Bold");
        assert_eq!(assign_scout_priority(&tokens, &common), Priority::High);
    }

    #[test]
    fn assign_scout_priority_unknown_font_gets_low() {
        use crate::OperatingSystem;
        let common = config::tokenize_common_families(OperatingSystem::MacOS);

        let tokens = tokenize_all("SomeObscureFont");
        assert_eq!(assign_scout_priority(&tokens, &common), Priority::Low);
    }

    #[test]
    fn assign_scout_priority_follows_injected_families() {
        // The whole point of FcScanConfig: an embedder whose detected UI
        // font is "Cantarell" (GNOME; in no built-in table) injects it
        // and its files outrank everything - including families the OS
        // defaults would have boosted.
        let config = config::FcScanConfig {
            font_dirs: Vec::new(),
            priority_families: vec!["Cantarell".to_string()],
        };
        let sets = config.priority_token_sets();

        let tokens = tokenize_all("Cantarell-Bold");
        assert_eq!(assign_scout_priority(&tokens, &sets), Priority::High);

        // Arial is High under every built-in table, but this embedder
        // did not ask for it.
        let tokens = tokenize_all("Arial");
        assert_eq!(assign_scout_priority(&tokens, &sets), Priority::Low);
    }

    // ── find_family_paths ───────────────────────────────────────────────

    #[test]
    fn find_family_paths_exact_match() {
        let mut known = BTreeMap::new();
        known.insert("arial".to_string(), vec![PathBuf::from("/fonts/Arial.ttf")]);
        known.insert(
            "helvetica".to_string(),
            vec![PathBuf::from("/fonts/Helvetica.ttf")],
        );

        let paths = find_family_paths("helvetica", &known);
        assert_eq!(paths.len(), 1);
        assert!(paths.contains(&PathBuf::from("/fonts/Helvetica.ttf")));
    }

    #[test]
    fn find_family_paths_fuzzy_substring() {
        let mut known = BTreeMap::new();
        known.insert("arial".to_string(), vec![PathBuf::from("/fonts/Arial.ttf")]);
        known.insert(
            "arialnarrow".to_string(),
            vec![PathBuf::from("/fonts/ArialNarrow.ttf")],
        );
        known.insert(
            "helvetica".to_string(),
            vec![PathBuf::from("/fonts/Helvetica.ttf")],
        );

        // "arial" matches both "arial" (exact) and "arialnarrow" (contains)
        let paths = find_family_paths("arial", &known);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&PathBuf::from("/fonts/Arial.ttf")));
        assert!(paths.contains(&PathBuf::from("/fonts/ArialNarrow.ttf")));
    }

    #[test]
    fn find_family_paths_no_match() {
        let mut known = BTreeMap::new();
        known.insert("arial".to_string(), vec![PathBuf::from("/fonts/Arial.ttf")]);

        let paths = find_family_paths("courier", &known);
        assert!(paths.is_empty());
    }

    // ── find_incomplete_paths ───────────────────────────────────────────

    #[test]
    fn find_incomplete_paths_filters_completed() {
        let mut known = BTreeMap::new();
        known.insert(
            "arial".to_string(),
            vec![
                PathBuf::from("/fonts/Arial.ttf"),
                PathBuf::from("/fonts/ArialBold.ttf"),
            ],
        );

        let mut completed = HashSet::new();
        completed.insert(PathBuf::from("/fonts/Arial.ttf"));

        let incomplete = find_incomplete_paths(&["arial".to_string()], &known, &completed);

        assert_eq!(incomplete.len(), 1);
        assert_eq!(incomplete[0].0, PathBuf::from("/fonts/ArialBold.ttf"));
    }

    // ── family_exists_in_patterns ───────────────────────────────────────

    #[test]
    fn family_exists_by_name() {
        let pattern = FcPattern {
            name: Some("Arial".to_string()),
            ..Default::default()
        };
        assert!(family_exists_in_patterns("arial", [&pattern].into_iter()));
    }

    #[test]
    fn family_exists_by_family_field() {
        let pattern = FcPattern {
            family: Some("Helvetica Neue".to_string()),
            ..Default::default()
        };
        assert!(family_exists_in_patterns(
            "helveticaneue",
            [&pattern].into_iter()
        ));
    }

    #[test]
    fn family_not_found() {
        let pattern = FcPattern {
            name: Some("Arial".to_string()),
            ..Default::default()
        };
        assert!(!family_exists_in_patterns(
            "courier",
            [&pattern].into_iter()
        ));
    }

/// Helper: tokenize a stem into all lowercase tokens.
    fn tokenize_all(stem: &str) -> Vec<String> {
        crate::config::tokenize_lowercase(stem)
    }
