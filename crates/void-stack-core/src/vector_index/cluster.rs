//! Leiden community clustering over the vector index.
//!
//! Builds a similarity graph from chunk embeddings (cosine > threshold) and
//! runs the Leiden algorithm to detect communities. Persists assignments to a
//! `communities` table so semantic_search can attach `community_id` per result.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rusqlite::Connection;

use super::db::open_meta_db;
use super::stats::index_exists;
use crate::error::IndexError;
use crate::model::Project;

/// Cosine similarity threshold for adding an edge between two chunks.
pub const SIMILARITY_THRESHOLD: f32 = 0.72;

/// Maximum Leiden iterations.
const MAX_LEIDEN_ITER: usize = 100;

/// Cap on the number of chunk embeddings considered when building the
/// similarity graph. Larger projects are evenly subsampled. The 500-chunk
/// cap keeps the O(n²) cosine pass at ~125k comparisons so clustering
/// finishes well under the MCP tool timeout (10s on a warm machine vs.
/// >4 min at 2,000 chunks).
const MAX_CHUNKS_FOR_CLUSTERING: usize = 500;

// ── Background job registry ────────────────────────────────

/// Public state machine for the (single, process-wide) clustering job.
#[derive(Clone, Debug)]
pub enum ClusterJobState {
    Idle,
    Running,
    Completed { communities: usize },
    Failed(String),
}

static CLUSTER_JOB: OnceLock<Mutex<ClusterJobState>> = OnceLock::new();

fn cluster_job() -> &'static Mutex<ClusterJobState> {
    CLUSTER_JOB.get_or_init(|| Mutex::new(ClusterJobState::Idle))
}

/// Spawn a background thread that runs [`cluster_project`] and writes the
/// outcome into the shared job state. Returns immediately. A second call
/// while a run is in flight is rejected so callers can't accidentally
/// double-schedule the same expensive job.
pub fn cluster_project_background(project: &Project) -> Result<(), IndexError> {
    {
        let mut state = cluster_job()
            .lock()
            .map_err(|e| IndexError::Other(format!("cluster job state poisoned: {e}")))?;
        if matches!(*state, ClusterJobState::Running) {
            return Err(IndexError::Other(
                "clustering already in progress".to_string(),
            ));
        }
        *state = ClusterJobState::Running;
    }
    let project = project.clone();
    std::thread::spawn(move || {
        let result = cluster_project(&project);
        // Recover from a poisoned mutex so a panic in one job doesn't
        // permanently lock everyone else out.
        let mut state = match cluster_job().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *state = match result {
            Ok(stats) => ClusterJobState::Completed {
                communities: stats.communities,
            },
            Err(e) => ClusterJobState::Failed(e.to_string()),
        };
    });
    Ok(())
}

/// Read the current clustering job state. `Idle` is returned when nothing
/// has run yet in this process.
pub fn get_cluster_job_state() -> ClusterJobState {
    match cluster_job().lock() {
        Ok(g) => g.clone(),
        Err(p) => p.into_inner().clone(),
    }
}

#[derive(Debug, Clone)]
pub struct ClusterStats {
    pub chunks_total: usize,
    pub communities: usize,
    pub largest_community_size: usize,
}

/// Run clustering end-to-end: build graph → Leiden → persist.
pub fn cluster_project(project: &Project) -> Result<ClusterStats, IndexError> {
    if !index_exists(project) {
        return Err(IndexError::IndexNotFound(project.name.clone()));
    }

    let conn = open_meta_db(project)?;
    ensure_communities_table(&conn)?;

    let all = load_chunk_ids_with_embeddings(&conn)?;
    let edges = similarity_graph(&sample_for_graph(&all));
    if edges.is_empty() {
        return Err(IndexError::Other(format!(
            "no similarity edges above threshold {} found — \
             the index may be empty or contain only dissimilar chunks",
            SIMILARITY_THRESHOLD
        )));
    }

    let mut communities = run_leiden(&edges)?;
    // Leiden only sees the sampled chunks that have at least one edge;
    // everything else (the rest of a large project, isolated chunks) joins
    // the community whose centroid it is closest to. Before this, projects
    // over 500 chunks came back almost entirely "Unclustered".
    assign_by_centroid(&all, &mut communities);

    let chunks_total = communities.len();
    let mut size_by_community: HashMap<usize, usize> = HashMap::new();
    for &c in communities.values() {
        *size_by_community.entry(c).or_insert(0) += 1;
    }
    let community_count = size_by_community.len();
    let largest_size = size_by_community.values().copied().max().unwrap_or(0);

    save_communities(&conn, &communities)?;

    Ok(ClusterStats {
        chunks_total,
        communities: community_count,
        largest_community_size: largest_size,
    })
}

/// Cosine-similarity graph over the chunk embeddings in `conn`.
/// Only processes up to [`MAX_CHUNKS_FOR_CLUSTERING`] embeddings; projects
/// larger than that are sampled by taking every Nth chunk so the returned
/// graph stays manageable.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_similarity_graph(
    conn: &Connection,
) -> Result<Vec<(i64, i64, f32)>, IndexError> {
    let all = load_chunk_ids_with_embeddings(conn)?;
    Ok(similarity_graph(&sample_for_graph(&all)))
}

/// Every Nth chunk, at most [`MAX_CHUNKS_FOR_CLUSTERING`].
fn sample_for_graph(all: &[(i64, Vec<f32>)]) -> Vec<&(i64, Vec<f32>)> {
    if all.len() <= MAX_CHUNKS_FOR_CLUSTERING {
        return all.iter().collect();
    }
    let step = all.len() / MAX_CHUNKS_FOR_CLUSTERING;
    all.iter()
        .step_by(step.max(1))
        .take(MAX_CHUNKS_FOR_CLUSTERING)
        .collect()
}

/// Edges between chunks whose cosine exceeds [`SIMILARITY_THRESHOLD`].
fn similarity_graph(chunks: &[&(i64, Vec<f32>)]) -> Vec<(i64, i64, f32)> {
    let n = chunks.len();
    let mut edges = Vec::new();

    for i in 0..n {
        let (id_a, emb_a) = (chunks[i].0, &chunks[i].1);
        for chunk_b in chunks.iter().skip(i + 1) {
            let (id_b, emb_b) = (chunk_b.0, &chunk_b.1);
            let sim = cosine_similarity(emb_a, emb_b);
            if sim > SIMILARITY_THRESHOLD {
                edges.push((id_a, id_b, sim));
            }
        }
    }

    edges
}

/// Smallest community whose centroid may attract other chunks — a pair
/// of near-duplicates is too narrow to stand for a topic.
const MIN_CENTROID_MEMBERS: usize = 3;
/// A chunk joins the nearest centroid only above this cosine; below, it
/// stays unclustered rather than pollute a community.
pub const CENTROID_FLOOR: f32 = 0.5;

/// Assign every chunk without a community to the community with the
/// nearest centroid (mean of the members' normalized embeddings), if that
/// centroid is close enough. Existing assignments are kept.
pub(crate) fn assign_by_centroid(all: &[(i64, Vec<f32>)], communities: &mut HashMap<i64, usize>) {
    let dim = all.first().map(|(_, e)| e.len()).unwrap_or(0);
    if dim == 0 || communities.is_empty() {
        return;
    }
    let mut sums: HashMap<usize, (Vec<f32>, usize)> = HashMap::new();
    for (id, emb) in all {
        if emb.len() != dim {
            continue;
        }
        if let Some(c) = communities.get(id) {
            let entry = sums.entry(*c).or_insert_with(|| (vec![0.0; dim], 0));
            let norm = l2(emb);
            if norm > 0.0 {
                for (acc, v) in entry.0.iter_mut().zip(emb) {
                    *acc += v / norm;
                }
                entry.1 += 1;
            }
        }
    }
    let centroids: Vec<(usize, Vec<f32>)> = sums
        .into_iter()
        .filter(|(_, (_, n))| *n >= MIN_CENTROID_MEMBERS)
        .map(|(c, (sum, _))| {
            let norm = l2(&sum);
            (
                c,
                sum.into_iter()
                    .map(|v| v / norm.max(f32::EPSILON))
                    .collect(),
            )
        })
        .collect();
    if centroids.is_empty() {
        return;
    }
    for (id, emb) in all {
        if communities.contains_key(id) || emb.len() != dim {
            continue;
        }
        let best = centroids
            .iter()
            .map(|(c, cen)| (*c, cosine_similarity(emb, cen)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((c, sim)) = best
            && sim >= CENTROID_FLOOR
        {
            communities.insert(*id, c);
        }
    }
}

fn l2(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// Keep communities current after an index run: a full rebuild (or an
/// index that was never clustered) clusters from scratch; an incremental
/// run drops assignments of deleted chunks and gives new chunks the
/// community of their nearest centroid. Never fails the index run.
pub(crate) fn refresh_after_index(project: &Project, conn: &Connection, full: bool) {
    let has_rows = ensure_communities_table(conn).is_ok()
        && conn
            .query_row("SELECT COUNT(*) FROM communities", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|n| n > 0)
            .unwrap_or(false);
    if full || !has_rows {
        if let Err(e) = cluster_project(project) {
            tracing::debug!("clustering after index skipped: {e}");
        }
        return;
    }
    let result = (|| -> Result<(), IndexError> {
        conn.execute(
            "DELETE FROM communities WHERE chunk_id NOT IN (SELECT id FROM chunks)",
            [],
        )
        .map_err(IndexError::from)?;
        let all = load_chunk_ids_with_embeddings(conn)?;
        let mut communities = load_communities(conn)?;
        let before = communities.len();
        assign_by_centroid(&all, &mut communities);
        if communities.len() != before {
            save_communities(conn, &communities)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        tracing::debug!("community refresh after incremental index skipped: {e}");
    }
}

/// Run Leiden on the similarity graph. Returns chunk_id → community_id where
/// community_id 0 corresponds to the largest community.
pub(crate) fn run_leiden(edges: &[(i64, i64, f32)]) -> Result<HashMap<i64, usize>, IndexError> {
    use fa_leiden_cd::{Graph, TrivialModularityOptimizer};

    if edges.is_empty() {
        return Ok(HashMap::new());
    }

    let mut g: Graph<i64, ()> = Graph::new();
    let mut id_to_idx: HashMap<i64, usize> = HashMap::new();

    for (a, b, _) in edges {
        id_to_idx.entry(*a).or_insert_with(|| g.add_node(*a));
        id_to_idx.entry(*b).or_insert_with(|| g.add_node(*b));
    }

    for (a, b, weight) in edges {
        let ia = id_to_idx
            .get(a)
            .copied()
            .ok_or_else(|| IndexError::Other(format!("missing node index for chunk {}", a)))?;
        let ib = id_to_idx
            .get(b)
            .copied()
            .ok_or_else(|| IndexError::Other(format!("missing node index for chunk {}", b)))?;
        g.add_edge(ia, ib, (), *weight);
    }

    let mut optimizer = TrivialModularityOptimizer {
        parallel_scale: 1024,
        tol: 1e-4,
    };
    let hierarchy = g.leiden(Some(MAX_LEIDEN_ITER), &mut optimizer);

    let idx_to_id: HashMap<usize, i64> = id_to_idx.iter().map(|(id, idx)| (*idx, *id)).collect();

    // Walk the hierarchy: each top-level node is one community.
    let mut raw_assignments: HashMap<i64, usize> = HashMap::new();
    let mut sizes: Vec<(usize, usize)> = Vec::new();

    for (community_id, node) in hierarchy.node_data_slice().iter().enumerate() {
        let members = RefCell::new(Vec::<usize>::new());
        node.collect_nodes(&|idx| members.borrow_mut().push(idx));
        let collected = members.into_inner();
        sizes.push((community_id, collected.len()));
        for idx in collected {
            if let Some(chunk_id) = idx_to_id.get(&idx) {
                raw_assignments.insert(*chunk_id, community_id);
            }
        }
    }

    // Re-rank so the largest community is id 0.
    sizes.sort_by_key(|b| std::cmp::Reverse(b.1));
    let remap: HashMap<usize, usize> = sizes
        .into_iter()
        .enumerate()
        .map(|(new_id, (old_id, _))| (old_id, new_id))
        .collect();

    let assignments = raw_assignments
        .into_iter()
        .map(|(chunk_id, old_id)| (chunk_id, remap.get(&old_id).copied().unwrap_or(old_id)))
        .collect();

    Ok(assignments)
}

/// Persist (chunk_id, community_id, community_size) tuples. Replaces any prior
/// rows so a re-cluster always reflects the latest run.
pub(crate) fn save_communities(
    conn: &Connection,
    communities: &HashMap<i64, usize>,
) -> Result<(), IndexError> {
    ensure_communities_table(conn)?;

    let mut size_by_community: HashMap<usize, usize> = HashMap::new();
    for &c in communities.values() {
        *size_by_community.entry(c).or_insert(0) += 1;
    }

    let tx = conn.unchecked_transaction().map_err(IndexError::from)?;
    tx.execute("DELETE FROM communities", [])
        .map_err(IndexError::from)?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO communities (chunk_id, community_id, community_size) \
                 VALUES (?1, ?2, ?3)",
            )
            .map_err(IndexError::from)?;
        for (chunk_id, community_id) in communities {
            let size = size_by_community.get(community_id).copied().unwrap_or(0);
            stmt.execute(rusqlite::params![
                *chunk_id,
                *community_id as i64,
                size as i64,
            ])
            .map_err(IndexError::from)?;
        }
    }

    tx.commit().map_err(IndexError::from)?;
    Ok(())
}

/// Load community assignments. Returns an empty map if the table is missing.
pub(crate) fn load_communities(conn: &Connection) -> Result<HashMap<i64, usize>, IndexError> {
    ensure_communities_table(conn)?;
    let mut stmt = conn
        .prepare("SELECT chunk_id, community_id FROM communities")
        .map_err(IndexError::from)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as usize))
        })
        .map_err(IndexError::from)?;
    let mut map = HashMap::new();
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    Ok(map)
}

/// Community ids for just `chunk_ids` — the search path only needs the
/// handful of chunks it returns, not the whole table.
pub(crate) fn load_communities_for(
    conn: &Connection,
    chunk_ids: &[i64],
) -> Result<HashMap<i64, usize>, IndexError> {
    let mut map = HashMap::new();
    if chunk_ids.is_empty() {
        return Ok(map);
    }
    ensure_communities_table(conn)?;
    // SQLite caps bound parameters at 999; search results are far below.
    for batch in chunk_ids.chunks(900) {
        let placeholders = vec!["?"; batch.len()].join(",");
        let sql = format!(
            "SELECT chunk_id, community_id FROM communities WHERE chunk_id IN ({})",
            placeholders
        );
        let mut stmt = conn.prepare(&sql).map_err(IndexError::from)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(batch.iter()), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .map_err(IndexError::from)?;
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
    }
    Ok(map)
}

fn ensure_communities_table(conn: &Connection) -> Result<(), IndexError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS communities (
            chunk_id INTEGER PRIMARY KEY,
            community_id INTEGER NOT NULL,
            community_size INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_communities_id ON communities(community_id);",
    )
    .map_err(IndexError::from)
}

fn load_chunk_ids_with_embeddings(conn: &Connection) -> Result<Vec<(i64, Vec<f32>)>, IndexError> {
    let mut stmt = conn
        .prepare("SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL")
        .map_err(IndexError::from)?;
    let rows = stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let blob: Option<Vec<u8>> = row.get(1)?;
            Ok((id, blob))
        })
        .map_err(IndexError::from)?;

    let mut out = Vec::new();
    for row in rows.flatten() {
        if let Some(blob) = row.1 {
            let emb = super::db::bytes_to_f32_vec(&blob);
            if !emb.is_empty() {
                out.push((row.0, emb));
            }
        }
    }
    Ok(out)
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn open_in_memory() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        // Mimic schema fragments cluster.rs depends on (chunks.id + embedding).
        conn.execute_batch(
            "CREATE TABLE chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                text TEXT NOT NULL,
                mtime REAL NOT NULL,
                embedding BLOB,
                file_hash TEXT NOT NULL DEFAULT ''
            );",
        )
        .expect("create chunks");
        conn
    }

    fn f32_vec_to_bytes(v: &[f32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(v.len() * 4);
        for &x in v {
            out.extend_from_slice(&x.to_le_bytes());
        }
        out
    }

    fn insert_chunk(conn: &Connection, file: &str, embedding: &[f32]) -> i64 {
        conn.execute(
            "INSERT INTO chunks (file_path, line_start, line_end, text, mtime, embedding) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                file,
                1i64,
                10i64,
                "code",
                0.0f64,
                f32_vec_to_bytes(embedding)
            ],
        )
        .expect("insert chunk");
        conn.last_insert_rowid()
    }

    #[test]
    fn test_assign_by_centroid_fills_unclustered_chunks() {
        crate::isolate_test_data_dir();
        // Two topics along x and y; ids 1-3 / 4-6 clustered, 7-9 not.
        let all: Vec<(i64, Vec<f32>)> = vec![
            (1, vec![1.0, 0.05, 0.0]),
            (2, vec![0.95, 0.1, 0.0]),
            (3, vec![1.0, 0.0, 0.05]),
            (4, vec![0.0, 1.0, 0.05]),
            (5, vec![0.1, 0.95, 0.0]),
            (6, vec![0.0, 1.0, 0.0]),
            (7, vec![0.9, 0.2, 0.0]), // near topic 0
            (8, vec![0.2, 0.9, 0.0]), // near topic 1
            (9, vec![0.0, 0.0, 1.0]), // far from both: stays out
        ];
        let mut communities: HashMap<i64, usize> = [(1, 0), (2, 0), (3, 0), (4, 1), (5, 1), (6, 1)]
            .into_iter()
            .collect();
        assign_by_centroid(&all, &mut communities);
        assert_eq!(communities.get(&7), Some(&0));
        assert_eq!(communities.get(&8), Some(&1));
        assert_eq!(communities.get(&9), None, "below CENTROID_FLOOR");
        assert_eq!(communities.get(&1), Some(&0), "existing kept");
    }

    #[test]
    fn test_assign_by_centroid_ignores_tiny_communities() {
        crate::isolate_test_data_dir();
        let all: Vec<(i64, Vec<f32>)> = vec![
            (1, vec![1.0, 0.0]),
            (2, vec![1.0, 0.0]),
            (3, vec![1.0, 0.01]),
        ];
        let mut communities: HashMap<i64, usize> = [(1, 0), (2, 0)].into_iter().collect();
        assign_by_centroid(&all, &mut communities);
        assert_eq!(
            communities.get(&3),
            None,
            "a 2-member community attracts nobody"
        );
    }

    #[test]
    fn test_cosine_similarity_identical_vectors() {
        crate::isolate_test_data_dir();
        let a = vec![1.0_f32, 2.0, 3.0];
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-6, "expected ~1.0 got {}", sim);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        crate::isolate_test_data_dir();
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero_vector_returns_zero() {
        crate::isolate_test_data_dir();
        let a = vec![0.0_f32; 4];
        let b = vec![1.0_f32; 4];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_build_similarity_graph_empty_when_no_chunks() {
        crate::isolate_test_data_dir();
        let conn = open_in_memory();
        let edges = build_similarity_graph(&conn).expect("build graph");
        assert!(edges.is_empty());
    }

    #[test]
    fn test_build_similarity_graph_respects_threshold() {
        crate::isolate_test_data_dir();
        let conn = open_in_memory();
        // Two near-identical embeddings + one orthogonal → only one edge.
        let id_a = insert_chunk(&conn, "a.rs", &[1.0, 0.0, 0.0]);
        let id_b = insert_chunk(&conn, "b.rs", &[0.99, 0.01, 0.0]);
        let _id_c = insert_chunk(&conn, "c.rs", &[0.0, 0.0, 1.0]);
        let edges = build_similarity_graph(&conn).expect("build graph");
        assert_eq!(edges.len(), 1, "expected one edge above threshold");
        let (u, v, _w) = edges[0];
        assert!((u == id_a && v == id_b) || (u == id_b && v == id_a));
    }

    #[test]
    fn test_run_leiden_empty_graph() {
        crate::isolate_test_data_dir();
        let result = run_leiden(&[]).expect("run leiden empty");
        assert!(result.is_empty());
    }

    #[test]
    fn test_run_leiden_two_clusters() {
        crate::isolate_test_data_dir();
        // Two well-separated cliques: {1,2,3} and {4,5,6}.
        let edges = vec![
            (1i64, 2, 0.9_f32),
            (1, 3, 0.9),
            (2, 3, 0.9),
            (4, 5, 0.9),
            (4, 6, 0.9),
            (5, 6, 0.9),
        ];
        let assignments = run_leiden(&edges).expect("run leiden");
        // Every input chunk should be assigned.
        for id in [1i64, 2, 3, 4, 5, 6] {
            assert!(assignments.contains_key(&id), "missing id {}", id);
        }
        // Two cliques → at least 2 distinct community ids.
        let unique: std::collections::HashSet<usize> = assignments.values().copied().collect();
        assert!(
            unique.len() >= 2,
            "expected ≥2 communities, got {:?}",
            unique
        );
    }

    #[test]
    fn test_save_and_load_communities_roundtrip() {
        crate::isolate_test_data_dir();
        let conn = open_in_memory();
        let id_a = insert_chunk(&conn, "a.rs", &[1.0, 0.0]);
        let id_b = insert_chunk(&conn, "b.rs", &[1.0, 0.0]);
        let id_c = insert_chunk(&conn, "c.rs", &[0.0, 1.0]);

        let mut input = HashMap::new();
        input.insert(id_a, 0usize);
        input.insert(id_b, 0);
        input.insert(id_c, 1);

        save_communities(&conn, &input).expect("save");
        let loaded = load_communities(&conn).expect("load");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.get(&id_a).copied(), Some(0));
        assert_eq!(loaded.get(&id_b).copied(), Some(0));
        assert_eq!(loaded.get(&id_c).copied(), Some(1));
    }

    #[test]
    fn test_load_communities_for_only_returns_requested_ids() {
        crate::isolate_test_data_dir();
        let conn = Connection::open_in_memory().unwrap();
        ensure_communities_table(&conn).unwrap();
        for (chunk, community) in [(1i64, 7i64), (2, 7), (3, 9)] {
            conn.execute(
                "INSERT INTO communities (chunk_id, community_id, community_size) VALUES (?1, ?2, 1)",
                rusqlite::params![chunk, community],
            )
            .unwrap();
        }
        let got = load_communities_for(&conn, &[3, 1, 42]).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got.get(&1), Some(&7));
        assert_eq!(got.get(&3), Some(&9));
        assert!(load_communities_for(&conn, &[]).unwrap().is_empty());
    }

    #[test]
    fn test_load_communities_empty_when_table_missing() {
        crate::isolate_test_data_dir();
        let conn = Connection::open_in_memory().expect("conn");
        // No chunks table here at all — load should still succeed with empty map.
        let loaded = load_communities(&conn).expect("load");
        assert!(loaded.is_empty());
    }

    /// The cluster job state is process-global: tests that touch it run one
    /// at a time, or one test's Running/Idle reset races another's assert.
    fn job_state_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_cluster_background_returns_immediately() {
        let _serial = job_state_lock();
        crate::isolate_test_data_dir();
        // `cluster_project_background` should never block — the actual work
        // runs on a spawned thread. The job will fail (index missing for
        // this synthetic project) but the *scheduling* call must return
        // within a very small budget. Reset state first so a previous test
        // run doesn't leave us in Running.
        *cluster_job().lock().unwrap() = ClusterJobState::Idle;

        let dir = tempfile::tempdir().unwrap();
        let project = crate::model::Project {
            name: "test-cluster-bg".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let start = std::time::Instant::now();
        cluster_project_background(&project).expect("schedule background cluster");
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 100,
            "background scheduling should be near-instant, took {:?}",
            elapsed
        );
        // Let the spawned job settle before releasing the lock, so its final
        // state write can't land in the middle of another test.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while matches!(get_cluster_job_state(), ClusterJobState::Running)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        *cluster_job().lock().unwrap() = ClusterJobState::Idle;
    }

    #[test]
    fn test_cluster_double_start_blocked() {
        let _serial = job_state_lock();
        crate::isolate_test_data_dir();
        // Force the registry into Running so the guard fires deterministically
        // — relying on the actual spawned thread to still be Running is racy.
        *cluster_job().lock().unwrap() = ClusterJobState::Running;

        let project = crate::model::Project {
            name: "test-cluster-double".to_string(),
            path: "F:\\nope".to_string(),
            description: String::new(),
            project_type: None,
            tags: vec![],
            services: vec![],
            hooks: None,
        };
        let err = cluster_project_background(&project).expect_err("second call must error");
        assert!(err.to_string().contains("already in progress"), "got {err}");

        // Reset so we don't leak Running state to later tests.
        *cluster_job().lock().unwrap() = ClusterJobState::Idle;
    }

    #[test]
    fn test_get_cluster_job_state_default_idle() {
        let _serial = job_state_lock();
        crate::isolate_test_data_dir();
        // Reset and verify the default observable state.
        *cluster_job().lock().unwrap() = ClusterJobState::Idle;
        assert!(matches!(get_cluster_job_state(), ClusterJobState::Idle));
    }

    #[test]
    fn test_save_communities_overwrites_prior_run() {
        crate::isolate_test_data_dir();
        let conn = open_in_memory();
        let id = insert_chunk(&conn, "a.rs", &[1.0]);
        let mut first = HashMap::new();
        first.insert(id, 5usize);
        save_communities(&conn, &first).unwrap();

        let mut second = HashMap::new();
        second.insert(id, 0usize);
        save_communities(&conn, &second).unwrap();

        let loaded = load_communities(&conn).unwrap();
        assert_eq!(loaded.get(&id).copied(), Some(0));
    }
}
