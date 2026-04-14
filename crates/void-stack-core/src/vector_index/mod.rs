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

pub use indexer::{
    IndexJobStatus, find_dependents, get_index_job_status, index_project, index_project_background,
    install_git_hook, is_watching, unwatch_project, watch_project,
};
pub use search::{SearchResult, semantic_search};
pub use stats::{IndexStats, delete_index, get_index_stats, index_exists};
pub use voidignore::{VoidIgnoreResult, generate_voidignore, save_voidignore};

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::chunker::{Chunk, chunk_file, enrich_chunk_with_context};
    use super::db::open_meta_db;
    use super::indexer::{
        cleanup_stale_chunks, collect_indexable_files, find_dependents, read_job, update_job,
    };
    use super::stats::{file_sha256, get_git_changed_files, save_stats};
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
        let job_key = index_project_background(&project, true, None);
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

    // ── Watch mode + git hook ─────────────────────────────────

    #[test]
    fn test_watch_project_and_unwatch() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn a() {}").unwrap();
        let project = crate::model::Project {
            name: "test-watch-project".to_string(),
            path: tmp.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        assert!(!is_watching(&project));
        watch_project(&project).unwrap();
        assert!(is_watching(&project));
        unwatch_project(&project);
        assert!(!is_watching(&project));
    }

    #[test]
    fn test_install_git_hook_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks = tmp.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let project = crate::model::Project {
            name: "test-hook".to_string(),
            path: tmp.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        install_git_hook(&project).unwrap();
        let hook = std::fs::read_to_string(hooks.join("post-commit")).unwrap();
        assert!(hook.contains("void index"));
        // Must be HEAD~1 (diff new commit vs its parent). Plain HEAD would
        // diff the just-made commit against the working tree — empty right
        // after the commit, so nothing would re-index.
        assert!(
            hook.contains("--git-base HEAD~1"),
            "hook should use HEAD~1, got:\n{}",
            hook
        );
        assert!(
            !hook.contains("--git-base HEAD "),
            "hook must not use bare HEAD, got:\n{}",
            hook
        );
    }

    #[test]
    fn test_install_git_hook_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks = tmp.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let project = crate::model::Project {
            name: "test-hook-idem".to_string(),
            path: tmp.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        install_git_hook(&project).unwrap();
        install_git_hook(&project).unwrap();

        let hook = std::fs::read_to_string(hooks.join("post-commit")).unwrap();
        assert_eq!(
            hook.matches("void index").count(),
            1,
            "installing twice should not duplicate the hook line"
        );
    }

    // ── Stale-file cleanup (Bug 2) ────────────────────────────

    #[test]
    fn test_cleanup_removes_files_no_longer_indexable() {
        // Simulate: a.rs and b.rs were indexed. User adds b.rs to
        // .voidignore. On the next incremental run, `current_files` only
        // contains a.rs, so b.rs's chunks must be deleted.
        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: "test-cleanup".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        let conn = open_meta_db(&project).unwrap();
        // Seed two files worth of chunks directly.
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["a.rs", 1i64, 10i64, "fn a() {}", 100.0],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["b.rs", 1i64, 10i64, "fn b() {}", 100.0],
        )
        .unwrap();

        let mut timestamps = std::collections::HashMap::new();
        timestamps.insert("a.rs".to_string(), 100.0);
        timestamps.insert("b.rs".to_string(), 100.0);
        let mut hashes = std::collections::HashMap::new();
        hashes.insert("a.rs".to_string(), "hA".to_string());
        hashes.insert("b.rs".to_string(), "hB".to_string());

        // Only a.rs is indexable now — b.rs got added to .voidignore.
        let current = vec!["a.rs".to_string()];

        let removed = cleanup_stale_chunks(&conn, &current, &mut timestamps, &mut hashes).unwrap();
        assert_eq!(removed, 1, "b.rs should have been cleaned");

        let remaining_files: Vec<String> = conn
            .prepare("SELECT DISTINCT file_path FROM chunks ORDER BY file_path")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(remaining_files, vec!["a.rs".to_string()]);
        assert!(!timestamps.contains_key("b.rs"));
        assert!(!hashes.contains_key("b.rs"));
        assert!(timestamps.contains_key("a.rs"));
    }

    #[test]
    fn test_cleanup_noop_when_all_files_present() {
        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: "test-cleanup-noop".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };

        let conn = open_meta_db(&project).unwrap();
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["a.rs", 1i64, 10i64, "fn a() {}", 100.0],
        )
        .unwrap();

        let mut timestamps = std::collections::HashMap::new();
        timestamps.insert("a.rs".to_string(), 100.0);
        let mut hashes = std::collections::HashMap::new();
        let current = vec!["a.rs".to_string()];
        let removed = cleanup_stale_chunks(&conn, &current, &mut timestamps, &mut hashes).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_install_git_hook_errors_on_non_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: "no-git".to_string(),
            path: tmp.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let err = install_git_hook(&project).unwrap_err();
        assert!(err.contains("Not a git repository"));
    }

    // ── Chunk enrichment with import context ──────────────────

    #[test]
    fn test_enrich_chunk_adds_used_by() {
        let mut chunk = Chunk {
            file_path: "src/service.rs".to_string(),
            text: "pub fn handle() {}".to_string(),
            line_start: 1,
            line_end: 1,
        };
        enrich_chunk_with_context(&mut chunk, &[], &["src/controller.rs".to_string()]);
        assert!(
            chunk.text.contains("// Used by: controller"),
            "enriched text: {}",
            chunk.text
        );
        assert!(chunk.text.contains("pub fn handle()"));
    }

    #[test]
    fn test_enrich_chunk_adds_imports() {
        let mut chunk = Chunk {
            file_path: "lib/widget.dart".to_string(),
            text: "class Widget {}".to_string(),
            line_start: 1,
            line_end: 1,
        };
        enrich_chunk_with_context(
            &mut chunk,
            &["lib/repo.dart".to_string(), "lib/state.dart".to_string()],
            &[],
        );
        assert!(chunk.text.contains("// Imports: repo, state"));
        assert!(chunk.text.contains("class Widget {}"));
    }

    #[test]
    fn test_enrich_chunk_no_op_when_empty() {
        let original = "fn foo() {}".to_string();
        let mut chunk = Chunk {
            file_path: "a.rs".to_string(),
            text: original.clone(),
            line_start: 1,
            line_end: 1,
        };
        enrich_chunk_with_context(&mut chunk, &[], &[]);
        assert_eq!(chunk.text, original);
    }

    #[test]
    fn test_enrich_chunk_caps_at_3_importers() {
        let mut chunk = Chunk {
            file_path: "core.rs".to_string(),
            text: "pub struct Core;".to_string(),
            line_start: 1,
            line_end: 1,
        };
        let importers = vec![
            "alpha.rs".to_string(),
            "beta.rs".to_string(),
            "gamma.rs".to_string(),
            "omega.rs".to_string(),
        ];
        enrich_chunk_with_context(&mut chunk, &[], &importers);
        let used_by_line = chunk.text.lines().next().unwrap();
        assert!(
            used_by_line.contains("alpha")
                && used_by_line.contains("beta")
                && used_by_line.contains("gamma"),
            "first three importers expected in summary: {}",
            used_by_line
        );
        assert!(
            !used_by_line.contains("omega"),
            "4th importer leaked into summary: {}",
            used_by_line
        );
    }

    // ── Dependent file propagation ────────────────────────────

    #[test]
    fn test_find_dependents_python() {
        // Python: the resolver handles `import models` → `models.py` for
        // non-relative imports when the module lives at project root.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("requirements.txt"), "").unwrap();
        std::fs::write(tmp.path().join("models.py"), "class User: pass\n").unwrap();
        std::fs::write(
            tmp.path().join("service.py"),
            "import models\n\ndef handle():\n    pass\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("lonely.py"), "def main(): pass\n").unwrap();

        let deps = find_dependents(tmp.path(), &["models.py".to_string()]);
        assert!(
            deps.contains("service.py"),
            "service.py imports models, expected as dependent. got {:?}",
            deps
        );
        assert!(!deps.contains("lonely.py"), "lonely.py imports nothing");
        assert!(
            !deps.contains("models.py"),
            "changed files should be excluded from dependents"
        );
    }

    #[test]
    fn test_find_dependents_empty_when_no_importers() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("requirements.txt"), "").unwrap();
        std::fs::write(tmp.path().join("standalone.py"), "def main(): pass\n").unwrap();
        let deps = find_dependents(tmp.path(), &["standalone.py".to_string()]);
        assert!(deps.is_empty(), "no importers, got {:?}", deps);
    }

    #[test]
    fn test_find_dependents_no_graph_returns_empty() {
        // No project marker → build_graph returns None → empty set.
        let tmp = tempfile::tempdir().unwrap();
        let deps = find_dependents(tmp.path(), &["a.rs".to_string()]);
        assert!(deps.is_empty());
    }

    // ── SHA-256 content hashing ───────────────────────────────

    #[test]
    fn test_file_sha256_returns_consistent_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("test.rs");
        std::fs::write(&f, b"fn main() {}").unwrap();
        let h1 = file_sha256(&f);
        let h2 = file_sha256(&f);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64, "SHA-256 hex is 64 chars");
    }

    #[test]
    fn test_file_sha256_changes_on_content_change() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("test.rs");
        std::fs::write(&f, b"fn main() {}").unwrap();
        let h1 = file_sha256(&f);
        std::fs::write(&f, b"fn main() { println!(); }").unwrap();
        let h2 = file_sha256(&f);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_file_sha256_missing_file_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist.rs");
        assert!(file_sha256(&missing).is_empty());
    }

    #[test]
    fn test_meta_db_has_file_hash_column_after_open() {
        let tmp = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: "test-hash-column".to_string(),
            path: tmp.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let conn = open_meta_db(&project).unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(chunks)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .flatten()
            .collect();
        assert!(
            cols.contains(&"file_hash".to_string()),
            "file_hash column should be added by migration, got cols={:?}",
            cols
        );
    }

    // ── Git diff change detection ─────────────────────────────

    #[test]
    fn test_get_git_changed_files_non_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = get_git_changed_files(tmp.path(), "HEAD~1");
        assert!(
            result.is_empty(),
            "Non-git dir should return empty vec, got {:?}",
            result
        );
    }

    #[test]
    fn test_get_git_changed_files_with_git_repo() {
        use std::process::Command;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path();
        // init repo
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(p)
            .output()
            .unwrap();
        std::fs::write(p.join("a.rs"), "fn a() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(p)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init", "-q"])
            .current_dir(p)
            .output()
            .unwrap();
        // Modify file without committing
        std::fs::write(p.join("a.rs"), "fn a() { println!(); }").unwrap();
        let changed = get_git_changed_files(p, "HEAD");
        assert!(
            changed.contains(&"a.rs".to_string()),
            "Expected a.rs in changed files, got {:?}",
            changed
        );
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
