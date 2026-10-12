//! Bounded cache of loaded vector indexes, plus the idle reaper that also
//! releases the embedding model.

use std::collections::HashMap;
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use super::vectors::VectorIndex;
use crate::error::IndexError;
use crate::model::Project;
use crate::vector_index::stats::{index_dir, index_version};

// ── Global caches ──────────────────────────────────────────
//
// Both caches are bounded: a long-lived process (desktop app, MCP server)
// used to keep every index it ever touched plus the ONNX model and its
// arena resident forever — a cross-project search across ~30 indexed
// projects pinned all of them in RAM (multi-GB). Now:
//   * at most `index_cache_capacity()` vector indexes stay resident (LRU);
//   * entries idle for `INDEX_IDLE_TTL` are dropped by the reaper;
//   * the embedding model is dropped after `MODEL_IDLE_TTL` without use
//     and transparently re-created on the next embed call.

/// Cached vector indexes per project (LRU-bounded, idle-evicted).
static INDEX_CACHE: OnceLock<Mutex<HashMap<String, CachedIndex>>> = OnceLock::new();

/// Default number of vector indexes kept resident at once. Searches mostly
/// target the selected project, so a small LRU keeps the hot path cached
/// without pinning every indexed project. Override with
/// `VOID_STACK_INDEX_CACHE=<n>` (min 1).
const DEFAULT_INDEX_CACHE_CAPACITY: usize = 2;

/// A cached index is dropped after this long without a search.
const INDEX_IDLE_TTL: Duration = Duration::from_secs(10 * 60);

/// How often the background reaper checks for idle entries.
const REAPER_INTERVAL: Duration = Duration::from_secs(60);

/// A project's quantized vectors and the index version they were read at.
pub(super) struct CachedIndex {
    pub(super) index: VectorIndex,
    loaded_version: f64,
    last_used: Instant,
}

impl CachedIndex {
    fn load(project: &Project, conn: &rusqlite::Connection) -> Result<Self, IndexError> {
        // Read the version before the rows: an index rewritten while we
        // load bumps it, and the next search reloads.
        let loaded_version = index_version(project).unwrap_or(0.0);
        let index = VectorIndex::load(conn)?;
        if index.is_empty() {
            return Err(IndexError::VectorIndex(format!(
                "the index for '{}' has no embeddings. It may still be building; \
                 otherwise run index_project_codebase with force=true.",
                project.name
            )));
        }
        tracing::debug!(
            "loaded {} vectors for '{}' ({} KB)",
            index.len(),
            project.name,
            index.bytes() / 1024
        );
        Ok(CachedIndex {
            index,
            loaded_version,
            last_used: Instant::now(),
        })
    }
}

/// Whether a search should keep the loaded index resident afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachePolicy {
    /// Keep it in the LRU cache (the project the user is working on).
    Keep,
    /// Use a cached copy if present, otherwise load, search and drop.
    /// Used for fan-out searches over many projects (cross-project
    /// GraphRAG) so one query doesn't pin every index in memory.
    Transient,
}

fn index_cache() -> &'static Mutex<HashMap<String, CachedIndex>> {
    INDEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(project: &Project) -> String {
    index_dir(project).to_string_lossy().to_string()
}

/// Configured LRU capacity (`VOID_STACK_INDEX_CACHE`, default 2, min 1).
pub(crate) fn index_cache_capacity() -> usize {
    parse_capacity(std::env::var("VOID_STACK_INDEX_CACHE").ok().as_deref())
}

fn parse_capacity(raw: Option<&str>) -> usize {
    raw.and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(DEFAULT_INDEX_CACHE_CAPACITY)
}

/// Drop least-recently-used entries until at most `capacity` remain.
fn evict_lru<V>(map: &mut HashMap<String, V>, capacity: usize, last_used: impl Fn(&V) -> Instant) {
    while map.len() > capacity {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, v)| last_used(v))
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
}

/// Drop entries idle for longer than `ttl`. Returns how many were dropped.
fn evict_idle<V>(
    map: &mut HashMap<String, V>,
    ttl: Duration,
    now: Instant,
    last_used: impl Fn(&V) -> Instant,
) -> usize {
    let before = map.len();
    map.retain(|_, v| now.saturating_duration_since(last_used(v)) < ttl);
    before - map.len()
}

/// Start the idle reaper once per process. It wakes every
/// `REAPER_INTERVAL` and drops idle indexes and the idle model.
pub(super) fn ensure_reaper() {
    static REAPER: Once = Once::new();
    REAPER.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("void-index-reaper".to_string())
            .spawn(|| {
                loop {
                    std::thread::sleep(REAPER_INTERVAL);
                    reap_idle(Instant::now());
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start index cache reaper: {}", e);
        }
    });
}

/// One reaper pass (split out so tests can drive it deterministically).
fn reap_idle(now: Instant) {
    if let Ok(mut map) = index_cache().lock() {
        let dropped = evict_idle(&mut map, INDEX_IDLE_TTL, now, |c| c.last_used);
        if dropped > 0 {
            tracing::debug!("released {} idle vector index(es)", dropped);
        }
    }
    super::model::release_if_idle(now);
}

/// Run `f` against `project`'s vector index, loading it if it isn't cached
/// or the on-disk index is newer than the cached one.
///
/// With [`CachePolicy::Keep`] the index stays in the LRU cache afterwards
/// (evicting the least-recently-used entry beyond capacity); with
/// [`CachePolicy::Transient`] a freshly loaded index is dropped as soon as
/// `f` returns.
pub(super) fn with_index<R>(
    project: &Project,
    conn: &rusqlite::Connection,
    policy: CachePolicy,
    f: impl FnOnce(&CachedIndex) -> R,
) -> Result<R, IndexError> {
    let key = cache_key(project);
    let current_version = index_version(project).unwrap_or(0.0);

    let mut map = index_cache()
        .lock()
        .map_err(|e| IndexError::VectorIndex(format!("index cache lock poisoned: {}", e)))?;

    if let Some(cached) = map.get_mut(&key)
        && cached.loaded_version >= current_version
    {
        cached.last_used = Instant::now();
        return Ok(f(cached));
    }
    // Stale or missing: drop any old copy before loading so two versions
    // of the same index never sit in memory at once.
    map.remove(&key);

    match policy {
        CachePolicy::Transient => {
            // Don't hold the global lock while reading from disk.
            drop(map);
            let loaded = CachedIndex::load(project, conn)?;
            Ok(f(&loaded))
        }
        CachePolicy::Keep => {
            let loaded = CachedIndex::load(project, conn)?;
            let out = f(&loaded);
            map.insert(key, loaded);
            evict_lru(&mut map, index_cache_capacity(), |c| c.last_used);
            drop(map);
            ensure_reaper();
            Ok(out)
        }
    }
}

/// Drop the cached index of a project (called after re-indexing).
pub(crate) fn invalidate_index_cache(project: &Project) {
    if let Some(cache) = INDEX_CACHE.get()
        && let Ok(mut map) = cache.lock()
    {
        map.remove(&cache_key(project));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;
    use crate::vector_index::search::vector_rank;
    use std::collections::HashMap;

    fn fixture_project(name: &str, dir: &tempfile::TempDir) -> Project {
        Project {
            name: format!("search-{}-fixture-{}", name, std::process::id()),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        }
    }

    /// Insert a chunk and mirror it into the FTS index; returns the chunk id.
    fn insert_chunk(conn: &rusqlite::Connection, file: &str, text: &str) -> i64 {
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime) \
             VALUES (?1, 1, 10, ?2, 0)",
            rusqlite::params![file, text],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        crate::vector_index::db::fts_replace_file(conn, file).unwrap();
        id
    }

    /// Little-endian f32 blob, matching the on-disk embedding encoding.
    fn le_blob(vals: &[f32]) -> Vec<u8> {
        vals.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn test_invalidate_index_cache_without_loaded_cache_is_noop() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("invalidate", &dir);
        // Must be safe whether or not the process-wide cache was ever
        // initialized, and must stay idempotent.
        invalidate_index_cache(&project);
        invalidate_index_cache(&project);
    }

    // ── Bounded index cache ─────────────────────────────────

    /// Store 8 embedded chunks for `project` and stamp the index version,
    /// mirroring what the indexer writes. Returns (chunk_id, vector).
    fn write_tiny_index(project: &Project) -> Vec<(i64, Vec<f32>)> {
        let conn = crate::vector_index::db::open_meta_db(project).unwrap();
        let mut out = Vec::new();
        for i in 0..8 {
            let v: Vec<f32> = (0..8).map(|j| (((i * 8 + j) as f32) + 1.0).sin()).collect();
            let id = insert_chunk(&conn, &format!("f{i}.rs"), &format!("chunk {i}"));
            conn.execute(
                "UPDATE chunks SET embedding = ?1 WHERE id = ?2",
                rusqlite::params![le_blob(&v), id],
            )
            .unwrap();
            out.push((id, v));
        }
        crate::vector_index::stats::touch_index_version(project).unwrap();
        out
    }

    fn is_cached(project: &Project) -> bool {
        index_cache()
            .lock()
            .unwrap()
            .contains_key(&cache_key(project))
    }

    #[test]
    fn test_vector_rank_keep_caches_and_finds_nearest() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("keep", &dir);
        let rows = write_tiny_index(&project);
        let conn = crate::vector_index::db::open_meta_db(&project).unwrap();

        let hits = vector_rank(&project, &conn, &rows[3].1, 1, CachePolicy::Keep).unwrap();
        assert_eq!(hits[0].0, rows[3].0, "a chunk is its own nearest neighbour");
        assert!((hits[0].1 - 1.0).abs() < 1e-5);
        assert!(is_cached(&project), "Keep leaves the index resident");

        invalidate_index_cache(&project);
        assert!(!is_cached(&project));
    }

    #[test]
    fn test_vector_rank_transient_does_not_cache() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("transient", &dir);
        let rows = write_tiny_index(&project);
        let conn = crate::vector_index::db::open_meta_db(&project).unwrap();

        let hits = vector_rank(&project, &conn, &rows[0].1, 8, CachePolicy::Transient).unwrap();
        assert_eq!(hits.len(), 8);
        assert!(
            !is_cached(&project),
            "a fan-out search must not pin the index in memory"
        );
    }

    #[test]
    fn test_reloads_when_disk_index_is_newer() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("reload", &dir);
        write_tiny_index(&project);
        let conn = crate::vector_index::db::open_meta_db(&project).unwrap();
        with_index(&project, &conn, CachePolicy::Keep, |_| ()).unwrap();

        // Pretend the cached copy predates the on-disk index (another
        // process re-indexed): the next search must reload, not reuse.
        index_cache()
            .lock()
            .unwrap()
            .get_mut(&cache_key(&project))
            .unwrap()
            .loaded_version = 0.0;
        let stamp = with_index(&project, &conn, CachePolicy::Keep, |c| c.loaded_version).unwrap();
        assert!(stamp > 0.0, "stale entry was replaced by a fresh load");
        invalidate_index_cache(&project);
    }

    #[test]
    fn test_index_without_embeddings_is_an_error() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("noemb", &dir);
        let conn = crate::vector_index::db::open_meta_db(&project).unwrap();
        insert_chunk(&conn, "a.rs", "fn a() {}");
        let err = vector_rank(&project, &conn, &[1.0, 0.0], 5, CachePolicy::Keep).unwrap_err();
        assert!(matches!(err, IndexError::VectorIndex(_)), "{err:?}");
    }

    #[test]
    fn test_parse_capacity() {
        assert_eq!(parse_capacity(None), DEFAULT_INDEX_CACHE_CAPACITY);
        assert_eq!(parse_capacity(Some("5")), 5);
        assert_eq!(parse_capacity(Some(" 3 ")), 3);
        assert_eq!(parse_capacity(Some("0")), DEFAULT_INDEX_CACHE_CAPACITY);
        assert_eq!(parse_capacity(Some("lots")), DEFAULT_INDEX_CACHE_CAPACITY);
    }

    #[test]
    fn test_evict_lru_drops_oldest_first() {
        let t0 = Instant::now();
        let mut map: HashMap<String, Instant> = HashMap::new();
        map.insert("a".into(), t0);
        map.insert("b".into(), t0 + Duration::from_secs(2));
        map.insert("c".into(), t0 + Duration::from_secs(1));
        evict_lru(&mut map, 2, |t| *t);
        assert!(!map.contains_key("a"), "least recently used goes first");
        evict_lru(&mut map, 1, |t| *t);
        assert_eq!(map.keys().collect::<Vec<_>>(), vec!["b"]);
        evict_lru(&mut map, 5, |t| *t);
        assert_eq!(map.len(), 1, "under capacity is a no-op");
    }

    #[test]
    fn test_evict_idle_respects_ttl() {
        let now = Instant::now() + Duration::from_secs(600);
        let mut map: HashMap<String, Instant> = HashMap::new();
        map.insert("fresh".into(), now - Duration::from_secs(10));
        map.insert("idle".into(), now - Duration::from_secs(120));
        let dropped = evict_idle(&mut map, Duration::from_secs(60), now, |t| *t);
        assert_eq!(dropped, 1);
        assert!(map.contains_key("fresh") && !map.contains_key("idle"));
    }

    #[test]
    fn test_reap_idle_releases_stale_index() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("reap", &dir);
        write_tiny_index(&project);
        let conn = crate::vector_index::db::open_meta_db(&project).unwrap();
        with_index(&project, &conn, CachePolicy::Keep, |_| ()).unwrap();
        assert!(is_cached(&project));

        reap_idle(Instant::now() + INDEX_IDLE_TTL + Duration::from_secs(1));
        assert!(!is_cached(&project), "idle index is released by the reaper");
    }

    #[test]
    fn test_delete_index_releases_cached_index() {
        crate::isolate_test_data_dir();
        let dir = tempfile::tempdir().unwrap();
        let project = fixture_project("delete", &dir);
        write_tiny_index(&project);
        let conn = crate::vector_index::db::open_meta_db(&project).unwrap();
        with_index(&project, &conn, CachePolicy::Keep, |_| ()).unwrap();
        drop(conn);
        assert!(is_cached(&project));

        crate::vector_index::stats::delete_index(&project).unwrap();
        assert!(
            !is_cached(&project),
            "a deleted index must not keep serving searches"
        );
        assert!(!crate::vector_index::stats::index_exists(&project));
    }
}
