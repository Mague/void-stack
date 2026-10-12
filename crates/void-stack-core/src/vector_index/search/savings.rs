//! Token-savings bookkeeping for search results.

use chrono::Utc;

use crate::model::Project;

use super::SearchResult;

/// Pure savings calculator: returned tokens, full-file tokens, and the
/// resulting savings pct. Extracted from `record_real_search_savings` so
/// it can be tested without touching the on-disk stats db.
///
/// Approximates tokens as bytes/4. Unique source files are counted once
/// — multiple chunks from the same file don't double-count on the "full"
/// side. Returns `(tokens_full, tokens_returned, savings_pct)`.
pub(crate) fn compute_search_savings(
    project_root: &std::path::Path,
    chunks_and_files: &[(&str, &str)],
) -> (usize, usize, f32) {
    let tokens_returned: usize = chunks_and_files
        .iter()
        .map(|(chunk, _)| chunk.len() / 4)
        .sum();

    let mut unique_files: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_, file) in chunks_and_files {
        unique_files.insert(*file);
    }
    let tokens_full: usize = unique_files
        .iter()
        .filter_map(|p| std::fs::metadata(project_root.join(p)).ok())
        .map(|m| m.len() as usize / 4)
        .sum();

    let savings_pct = if tokens_full > tokens_returned {
        ((1.0 - tokens_returned as f64 / tokens_full as f64) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    };

    (tokens_full, tokens_returned, savings_pct)
}

/// Shared savings recorder used by both `semantic_search` and
/// `graph_rag_search`. Computes via [`compute_search_savings`] and writes
/// one row to the token_savings table.
///
/// Runs on a detached background thread: telemetry involves fs metadata
/// reads plus a stats-DB write and must never add latency to search nor
/// affect its results — failures are silently dropped.
pub(crate) fn record_real_search_savings(
    project: &Project,
    operation: &str,
    results: &[SearchResult],
) {
    let project_path = project.path.clone();
    let project_name = project.name.clone();
    let operation = operation.to_string();
    let pairs: Vec<(String, String)> = results
        .iter()
        .map(|r| (r.chunk.clone(), r.file_path.clone()))
        .collect();

    std::thread::spawn(move || {
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(c, f)| (c.as_str(), f.as_str()))
            .collect();
        let (tokens_full, tokens_returned, savings_pct) =
            compute_search_savings(std::path::Path::new(&project_path), &borrowed);

        crate::stats::record_saving(crate::stats::TokenSavingsRecord {
            timestamp: Utc::now(),
            project: project_name,
            operation,
            lines_original: tokens_full,
            lines_filtered: tokens_returned,
            savings_pct,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_savings_pct_real_file() {
        crate::isolate_test_data_dir();
        // 1000-byte source file, 50-byte returned chunk → ~95% savings.
        let dir = tempfile::tempdir().unwrap();
        let file_rel = "src/example.rs";
        let full_path = dir.path().join(file_rel);
        std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        std::fs::write(&full_path, "x".repeat(1000)).unwrap();

        let chunk = "y".repeat(50);
        let pairs = vec![(chunk.as_str(), file_rel)];
        let (tokens_full, tokens_returned, pct) = compute_search_savings(dir.path(), &pairs);

        assert_eq!(tokens_full, 250, "1000 bytes / 4 = 250 tokens");
        assert_eq!(tokens_returned, 12, "50 bytes / 4 = 12 tokens");
        assert!((pct - 95.0).abs() < 1.0, "expected ~95% savings, got {pct}");
    }

    #[test]
    fn test_savings_unique_files_not_double_counted() {
        crate::isolate_test_data_dir();
        // Two chunks from the same 800-byte file → tokens_full counts the
        // file once, not twice.
        let dir = tempfile::tempdir().unwrap();
        let file_rel = "src/a.rs";
        let full_path = dir.path().join(file_rel);
        std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        std::fs::write(&full_path, "x".repeat(800)).unwrap();

        let c1 = "a".repeat(40);
        let c2 = "b".repeat(40);
        let pairs = vec![(c1.as_str(), file_rel), (c2.as_str(), file_rel)];
        let (tokens_full, _, _) = compute_search_savings(dir.path(), &pairs);
        assert_eq!(tokens_full, 200, "800/4 once, not 400");
    }

    #[test]
    fn test_savings_pct_zero_when_file_missing() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let chunk = "x".repeat(100);
        let pairs = vec![(chunk.as_str(), "ghost.rs")];
        let (tokens_full, _, pct) = compute_search_savings(dir.path(), &pairs);
        assert_eq!(tokens_full, 0);
        assert_eq!(pct, 0.0);
    }

    #[test]
    fn test_savings_empty_results() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let (tokens_full, tokens_returned, pct) = compute_search_savings(dir.path(), &[]);
        assert_eq!(tokens_full, 0);
        assert_eq!(tokens_returned, 0);
        assert_eq!(pct, 0.0);
    }

    #[test]
    fn test_savings_zero_pct_when_chunk_not_smaller_than_file() {
        crate::isolate_test_data_dir();
        // Returned chunk is bigger than the source file — no savings claimed.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tiny.rs"), "x".repeat(40)).unwrap();
        let chunk = "y".repeat(400);
        let pairs = vec![(chunk.as_str(), "tiny.rs")];
        let (tokens_full, tokens_returned, pct) = compute_search_savings(dir.path(), &pairs);
        assert_eq!(tokens_full, 10, "40 bytes / 4");
        assert_eq!(tokens_returned, 100, "400 bytes / 4");
        assert_eq!(pct, 0.0, "no savings when returned >= full");
    }

    #[test]
    fn test_record_real_search_savings_smoke() {
        crate::isolate_test_data_dir();
        // Fire-and-forget telemetry: must neither panic nor block the caller,
        // even for a file that doesn't exist on disk.
        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: format!("savings-fixture-{}", std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let results = vec![SearchResult {
            file_path: "a.rs".to_string(),
            chunk: "fn a() {}".to_string(),
            score: 0.5,
            line_start: 1,
            line_end: 1,
            community_id: None,
        }];
        record_real_search_savings(&project, "unit_test", &results);
        record_real_search_savings(&project, "unit_test", &[]);
    }
}
