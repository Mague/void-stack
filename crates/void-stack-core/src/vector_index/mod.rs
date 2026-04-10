//! Vector index for semantic code search.
//!
//! Uses fastembed (BAAI/bge-small-en-v1.5) for embeddings and hnsw_rs for
//! approximate nearest neighbor search. Index persists to disk between sessions.

mod chunker;
pub(crate) mod db;
mod indexer;
mod search;
pub mod stats;
mod voidignore;

// ── Public re-exports (preserve existing API) ──────────────

pub use indexer::{IndexJobStatus, get_index_job_status, index_project, index_project_background};
pub use search::{SearchResult, semantic_search};
pub use stats::{IndexStats, delete_index, get_index_stats, index_exists};
pub use voidignore::{VoidIgnoreResult, generate_voidignore, save_voidignore};

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::chunker::chunk_file;
    use super::db::open_meta_db;
    use super::indexer::{collect_indexable_files, read_job, update_job};
    use super::stats::save_stats;
    use super::voidignore::{generate_voidignore, save_voidignore};
    use super::*;
    use chrono::Utc;
    use rusqlite::Connection;

    #[test]
    fn test_chunk_small_file() {
        let chunks = chunk_file("test.rs", "fn main() {\n    println!(\"hello\");\n}");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("test.rs"));
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks[0].line_end, 3);
    }

    #[test]
    fn test_chunk_empty_file() {
        let chunks = chunk_file("empty.rs", "");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_large_file() {
        let lines: Vec<String> = (0..120).map(|i| format!("line {}", i)).collect();
        let content = lines.join("\n");
        let chunks = chunk_file("big.rs", &content);
        assert!(chunks.len() >= 2);

        // All lines should be covered
        let total_lines: usize = chunks.iter().map(|c| c.line_end - c.line_start + 1).sum();
        assert!(total_lines >= 120);

        // Each chunk should have the file path prefix
        for chunk in &chunks {
            assert!(chunk.text.contains("big.rs"));
        }
    }

    #[test]
    fn test_chunk_respects_blank_lines() {
        let mut lines = Vec::new();
        for i in 0..50 {
            lines.push(format!("code line {}", i));
        }
        lines[35] = String::new(); // blank line near chunk boundary
        let content = lines.join("\n");
        let chunks = chunk_file("test.go", &content);
        // Should have at least 1 chunk
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunk_min_lines_threshold() {
        let content = "a\nb\nc";
        let chunks = chunk_file("tiny.rs", content);
        // 3 lines is below MIN_CHUNK_LINES, should still produce 1 chunk
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_meta_db_creation() {
        let dir = tempfile::tempdir().unwrap();
        let _project = crate::model::Project {
            name: "test-project".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        // We can't easily test the full path without mocking dirs, but we can test the DB init
        let db_path = dir.path().join("test_meta.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                text TEXT NOT NULL,
                mtime REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chunk_order (
                hnsw_id INTEGER PRIMARY KEY,
                chunk_id INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS stats (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .unwrap();

        // Insert and read back
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["src/main.rs", 1i64, 10i64, "fn main() {}", 1000.0],
        ).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_index_stats_serialization() {
        let stats = IndexStats {
            files_indexed: 42,
            chunks_total: 300,
            model: "BAAI/bge-small-en-v1.5".to_string(),
            size_mb: 15.5,
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("42"));
        assert!(json.contains("bge-small"));
        let parsed: IndexStats = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.files_indexed, 42);
    }

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult {
            file_path: "src/main.rs".to_string(),
            chunk: "fn main() {}".to_string(),
            score: 0.95,
            line_start: 1,
            line_end: 10,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("main.rs"));
        assert!(json.contains("0.95"));
    }

    #[test]
    fn test_code_extensions_coverage() {
        use super::indexer::CODE_EXTENSIONS;
        assert!(CODE_EXTENSIONS.contains(&"rs"));
        assert!(CODE_EXTENSIONS.contains(&"go"));
        assert!(CODE_EXTENSIONS.contains(&"py"));
        assert!(CODE_EXTENSIONS.contains(&"ts"));
        assert!(CODE_EXTENSIONS.contains(&"dart"));
        assert!(CODE_EXTENSIONS.contains(&"proto"));
    }

    // ── .voidignore generator ──────────────────────────────────

    #[test]
    fn test_generate_voidignore_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let result = generate_voidignore(dir.path());
        assert!(result.content.contains("target/"));
        assert!(result.content.contains("**/*.pb.rs"));
        assert!(result.content.contains("Cargo.lock"));
        assert!(result.content.contains("# Rust specifics"));
    }

    #[test]
    fn test_generate_voidignore_always_has_universal_exclusions() {
        let dir = tempfile::tempdir().unwrap();
        let result = generate_voidignore(dir.path());
        assert!(result.content.contains("node_modules/"));
        assert!(result.content.contains("__pycache__/"));
        assert!(result.content.contains(".next/"));
        assert!(result.content.contains(".nuxt/"));
        assert!(result.content.contains(".dart_tool/"));
        assert!(result.content.contains("venv/"));
        assert!(result.content.contains("**/*.g.dart"));
        assert!(result.content.contains("**/*.pb.go"));
    }

    #[test]
    fn test_save_voidignore() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_voidignore(dir.path(), "test\n").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "test\n");
    }

    // ── Hardcoded exclusions in indexer ─────────────────────────

    #[test]
    fn test_indexer_skips_generated_files() {
        let dir = tempfile::tempdir().unwrap();
        // Create source files (one real, several generated)
        std::fs::write(dir.path().join("lib.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("service.pb.rs"), "// generated").unwrap();
        std::fs::write(dir.path().join("model_pb2.py"), "# generated").unwrap();
        std::fs::write(dir.path().join("api.pb.go"), "// generated").unwrap();
        std::fs::write(dir.path().join("model.g.dart"), "// generated").unwrap();

        let files = collect_indexable_files(dir.path());
        assert_eq!(files.len(), 1, "Only lib.rs should be indexed");
        assert!(files[0].contains("lib.rs"));
    }

    #[test]
    fn test_indexer_skips_extra_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        // Directories that should be skipped
        for dirname in &[".next", ".nuxt", "venv", ".dart_tool", "coverage"] {
            let sub = dir.path().join(dirname);
            std::fs::create_dir(&sub).unwrap();
            std::fs::write(sub.join("file.rs"), "fn x() {}").unwrap();
        }

        let files = collect_indexable_files(dir.path());
        assert_eq!(files.len(), 1, "Only main.rs should be indexed");
    }

    #[test]
    fn test_reindex_respects_new_voidignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("generated.rs"), "fn gen() {}").unwrap();

        // First index: both files collected
        let files1 = collect_indexable_files(dir.path());
        assert_eq!(files1.len(), 2);

        // Now add a .voidignore excluding generated.rs
        std::fs::write(dir.path().join(".voidignore"), "generated.rs\n").unwrap();

        // Re-index: should only include main.rs
        let files2 = collect_indexable_files(dir.path());
        assert_eq!(files2.len(), 1);
        assert!(files2[0].contains("main.rs"));
    }

    // ── Function-aware chunking ────────────────────────────────

    #[test]
    fn test_dart_function_not_split() {
        let mut body = vec!["void _handleUnpublish(ServiceModel service) async {".to_string()];
        for i in 0..78 {
            body.push(format!("  // line {}", i));
        }
        body.push("}".to_string());
        let content = body.join("\n");
        let chunks = chunk_file("lib/screens/marketplace.dart", &content);
        assert_eq!(chunks.len(), 1, "80-line function should be a single chunk");
        assert!(chunks[0].text.contains("_handleUnpublish"));
        assert!(chunks[0].text.contains("line 77"));
    }

    #[test]
    fn test_large_function_repeats_signature_in_continuation() {
        let mut lines = vec!["void bigMethod(BuildContext context) async {".to_string()];
        for i in 0..198 {
            lines.push(format!("  await step{}();", i));
        }
        lines.push("}".to_string());
        let content = lines.join("\n");
        let chunks = chunk_file("lib/widget.dart", &content);
        assert!(
            chunks.len() >= 2,
            "200-line function should produce multiple chunks"
        );
        for chunk in &chunks[1..] {
            assert!(
                chunk.text.contains("bigMethod"),
                "Continuation chunk missing signature context"
            );
        }
    }

    #[test]
    fn test_two_rust_functions_two_chunks() {
        let content = "fn foo() {\n  let x = 1;\n  let y = 2;\n  let z = 3;\n}\n\nfn bar() {\n  let a = 4;\n  let b = 5;\n  let c = 6;\n}";
        let chunks = chunk_file("src/lib.rs", content);
        assert_eq!(chunks.len(), 2, "Two functions should produce two chunks");
        assert!(chunks[0].text.contains("fn foo"));
        assert!(chunks[1].text.contains("fn bar"));
    }

    #[test]
    fn test_fallback_for_unknown_extension() {
        let lines: Vec<String> = (0..120).map(|i| format!("key_{} = {}", i, i)).collect();
        let content = lines.join("\n");
        let chunks = chunk_file("config.toml", &content);
        assert!(!chunks.is_empty(), "Fallback should still produce chunks");
    }

    #[test]
    fn test_python_functions_chunked() {
        let content =
            "import os\n\ndef hello():\n    print('hi')\n\ndef world():\n    print('world')\n";
        let chunks = chunk_file("app.py", content);
        assert!(
            chunks.len() >= 2,
            "Two python functions should be separate chunks"
        );
        assert!(chunks.iter().any(|c| c.text.contains("def hello")));
        assert!(chunks.iter().any(|c| c.text.contains("def world")));
    }

    #[test]
    fn test_go_functions_chunked() {
        let content = "package main\n\nfunc Foo() {\n\treturn\n}\n\nfunc Bar() {\n\treturn\n}\n";
        let chunks = chunk_file("main.go", content);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().any(|c| c.text.contains("func Foo")));
        assert!(chunks.iter().any(|c| c.text.contains("func Bar")));
    }

    // ── Background Indexing Tests ─────────────────────────────────────────────

    #[test]
    fn test_index_job_status_running() {
        let project = crate::model::Project {
            name: "test-project".to_string(),
            path: "F:\\workspace\\test".to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        // Should return None when no job exists
        let status = get_index_job_status(&project);
        assert!(status.is_none(), "Should be None when no job exists");

        // Start background indexing (without actual files to avoid real indexing)
        let job_key = index_project_background(&project, true);
        assert_eq!(job_key, project.path, "Job key should be project path");

        // Should now return Running status
        let status = get_index_job_status(&project);
        match status {
            Some(IndexJobStatus::Running {
                files_processed,
                files_total,
            }) => {
                assert_eq!(files_processed, 0, "Initial files_processed should be 0");
                assert_eq!(files_total, 0, "Initial files_total should be 0");
            }
            _ => panic!("Expected Running status after starting job"),
        }
    }

    #[test]
    fn test_get_index_job_status_none_for_unknown_project() {
        let project = crate::model::Project {
            name: "nonexistent-project".to_string(),
            path: "F:\\workspace\\nonexistent".to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        let status = get_index_job_status(&project);
        assert!(status.is_none(), "Should be None for project with no jobs");
    }

    #[test]
    fn test_update_job_works_normally() {
        let key = "test_update_job_normal";
        update_job(
            key,
            IndexJobStatus::Running {
                files_processed: 5,
                files_total: 10,
            },
        );
        let status = read_job(key);
        match status {
            Some(IndexJobStatus::Running {
                files_processed,
                files_total,
            }) => {
                assert_eq!(files_processed, 5);
                assert_eq!(files_total, 10);
            }
            _ => panic!("Expected Running status"),
        }

        // Transition to Completed
        update_job(
            key,
            IndexJobStatus::Completed {
                stats: IndexStats {
                    files_indexed: 10,
                    chunks_total: 50,
                    model: "test".to_string(),
                    size_mb: 1.0,
                    created_at: Utc::now(),
                },
            },
        );
        let status = read_job(key);
        assert!(matches!(status, Some(IndexJobStatus::Completed { .. })));
    }

    #[test]
    fn test_update_job_recovers_from_poison() {
        // Poison the mutex by panicking inside a lock
        let _ = std::panic::catch_unwind(|| {
            let _guard = super::indexer::job_registry().lock().unwrap();
            panic!("intentional poison");
        });

        // update_job should still work despite poisoned mutex
        let key = "test_poison_recovery";
        update_job(
            key,
            IndexJobStatus::Failed {
                error: "test error".to_string(),
            },
        );

        // read_job should also recover
        let status = read_job(key);
        match status {
            Some(IndexJobStatus::Failed { error }) => {
                assert_eq!(error, "test error");
            }
            _ => panic!("Expected Failed status after poison recovery"),
        }
    }

    #[test]
    fn test_get_index_stats_returns_disk_stats_when_index_complete() {
        // Create a project with a real temp directory and index stats on disk
        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: "test-stats-disk".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        // Write stats to disk via meta DB
        let idx = super::stats::index_dir(&project);
        let _ = std::fs::create_dir_all(&idx);
        let conn = open_meta_db(&project).unwrap();
        let stats = IndexStats {
            files_indexed: 42,
            chunks_total: 200,
            model: "BAAI/bge-small-en-v1.5".to_string(),
            size_mb: 5.0,
            created_at: Utc::now(),
        };
        save_stats(&conn, &stats).unwrap();

        // Verify get_index_stats reads from disk correctly
        let disk_stats = get_index_stats(&project).unwrap();
        assert!(disk_stats.is_some(), "Should find stats on disk");
        let disk_stats = disk_stats.unwrap();
        assert_eq!(disk_stats.files_indexed, 42);
        assert_eq!(disk_stats.chunks_total, 200);
    }
}
