//! Offline benchmark for search-backend and embedding-model decisions.
//!
//! Built only with the `bench` feature (`cargo run --release --example
//! vector_bench --features bench -- <cmd> <project>...`). Reads existing
//! indexes, never writes them.
//!
//! * `backend`: production search vs exact f32 vs exact int8 over the
//!   stored embeddings — latency, recall@10 against exact search, memory.
//! * `quality`: retrieval quality per embedding model on an eval set mined
//!   from git history (commit subject → files the commit touched), vector
//!   only and hybrid (production FTS5 + RRF).

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::model::Project;

use super::search::{
    CachePolicy, IDENTIFIER_LEXICAL_BOOST, RRF_K, embed_texts, lexical_rank, rrf_fuse, vector_rank,
};

const TOP_K: usize = 10;

/// Every embedded chunk of an index, by id.
struct Corpus {
    ids: Vec<i64>,
    files: Vec<String>,
    texts: Vec<String>,
    embs: Vec<Vec<f32>>,
}

struct EvalQuery {
    text: String,
    relevant: HashSet<String>,
}

pub fn run(cmd: &str, projects: &[Project], models: &[String]) -> Result<(), String> {
    for project in projects {
        let conn = super::db::open_meta_db(project).map_err(|e| e.to_string())?;
        let corpus = load_corpus(&conn)?;
        let evals = eval_set(project, &corpus);
        println!(
            "\n## {} — {} chunks, {} files, {} eval queries\n",
            project.name,
            corpus.ids.len(),
            corpus.files.iter().collect::<HashSet<_>>().len(),
            evals.len()
        );
        match cmd {
            "backend" => backend(project, &conn, &corpus, &evals)?,
            "quality" => quality(&conn, &corpus, &evals, models)?,
            "evalset" => {
                for q in evals.iter().take(15) {
                    println!("- {} → {:?}", q.text, q.relevant);
                }
            }
            other => return Err(format!("unknown command {other}")),
        }
    }
    Ok(())
}

fn load_corpus(conn: &rusqlite::Connection) -> Result<Corpus, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, file_path, text, embedding FROM chunks \
             WHERE embedding IS NOT NULL ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<Vec<u8>>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut c = Corpus {
        ids: vec![],
        files: vec![],
        texts: vec![],
        embs: vec![],
    };
    for (id, file, text, blob) in rows.flatten() {
        let Some(blob) = blob else { continue };
        c.ids.push(id);
        c.files.push(file.replace('\\', "/"));
        c.texts.push(text);
        c.embs.push(normalized(
            blob.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect(),
        ));
    }
    Ok(c)
}

fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// Commit subjects as queries, the indexed files they touched as answers.
fn eval_set(project: &Project, corpus: &Corpus) -> Vec<EvalQuery> {
    let indexed: HashSet<&str> = corpus.files.iter().map(String::as_str).collect();
    let root = crate::runner::local::strip_win_prefix(&project.path);
    let out = std::process::Command::new("git")
        .args([
            "-C",
            &root,
            "log",
            "--no-merges",
            "-n",
            "600",
            "--name-only",
        ])
        .arg("--format=%x1e%s")
        .output();
    let Ok(out) = out else { return vec![] };
    let log = String::from_utf8_lossy(&out.stdout);
    let mut seen = HashSet::new();
    let mut evals = Vec::new();
    for entry in log.split('\u{1e}').skip(1) {
        let mut lines = entry.lines();
        let Some(subject) = lines.next() else {
            continue;
        };
        let text = strip_conventional(subject);
        if text.split_whitespace().count() < 4 || !seen.insert(text.to_lowercase()) {
            continue;
        }
        let relevant: HashSet<String> = lines
            .map(|l| l.trim().replace('\\', "/"))
            .filter(|f| indexed.contains(f.as_str()))
            .filter(|f| !is_noise_file(f))
            .collect();
        if relevant.is_empty() || relevant.len() > 6 {
            continue;
        }
        evals.push(EvalQuery { text, relevant });
        if evals.len() == 120 {
            break;
        }
    }
    evals
}

fn strip_conventional(subject: &str) -> String {
    let s = subject.trim();
    match s.split_once(": ") {
        Some((head, rest))
            if head.len() < 40
                && !head.contains(' ')
                && head.chars().next().is_some_and(|c| c.is_ascii_lowercase()) =>
        {
            rest.trim().to_string()
        }
        _ => s.to_string(),
    }
}

fn is_noise_file(f: &str) -> bool {
    let lower = f.to_lowercase();
    lower.ends_with("changelog.md")
        || lower.ends_with("readme.md")
        || lower.ends_with("readme.es.md")
        || lower.ends_with("board.md")
        || lower.ends_with(".lock")
        || lower.ends_with("cargo.toml")
        || lower.ends_with("package.json")
}

// ---------------------------------------------------------------------------
// backend

fn exact_top(embs: &[Vec<f32>], q: &[f32], k: usize) -> Vec<usize> {
    let mut scored: Vec<(f32, usize)> = embs
        .iter()
        .enumerate()
        .map(|(i, e)| (dot(e, q), i))
        .collect();
    let k = k.min(scored.len());
    if k == 0 {
        return vec![];
    }
    scored.select_nth_unstable_by(k - 1, |a, b| b.0.total_cmp(&a.0));
    scored.truncate(k);
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, i)| i).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

struct Int8Index {
    dim: usize,
    codes: Vec<i8>,
    scales: Vec<f32>,
}

impl Int8Index {
    fn build(embs: &[Vec<f32>]) -> Self {
        let dim = embs.first().map_or(0, Vec::len);
        let mut codes = Vec::with_capacity(embs.len() * dim);
        let mut scales = Vec::with_capacity(embs.len());
        for e in embs {
            let m = e.iter().fold(0f32, |m, x| m.max(x.abs())).max(1e-12);
            scales.push(m / 127.0);
            codes.extend(e.iter().map(|x| (x / m * 127.0).round() as i8));
        }
        Self { dim, codes, scales }
    }

    fn top(&self, q: &[f32], k: usize) -> Vec<usize> {
        let qm = q.iter().fold(0f32, |m, x| m.max(x.abs())).max(1e-12);
        let qc: Vec<i32> = q.iter().map(|x| (x / qm * 127.0).round() as i32).collect();
        let mut scored: Vec<(f32, usize)> = self
            .codes
            .chunks_exact(self.dim)
            .enumerate()
            .map(|(i, row)| {
                let s: i32 = row.iter().zip(&qc).map(|(a, b)| *a as i32 * b).sum();
                (s as f32 * self.scales[i], i)
            })
            .collect();
        let k = k.min(scored.len());
        if k == 0 {
            return vec![];
        }
        scored.select_nth_unstable_by(k - 1, |a, b| b.0.total_cmp(&a.0));
        scored.truncate(k);
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    fn bytes(&self) -> usize {
        self.codes.len() + self.scales.len() * 4
    }
}

fn recall(found: &[usize], truth: &[usize]) -> f64 {
    let t: HashSet<_> = truth.iter().collect();
    found.iter().filter(|i| t.contains(i)).count() as f64 / truth.len().max(1) as f64
}

fn backend(
    project: &Project,
    conn: &rusqlite::Connection,
    corpus: &Corpus,
    evals: &[EvalQuery],
) -> Result<(), String> {
    let mut queries: Vec<String> = evals.iter().map(|e| e.text.clone()).collect();
    if queries.len() < 20 {
        queries.extend(GENERIC_QUERIES.iter().map(|s| s.to_string()));
    }
    let qembs: Vec<Vec<f32>> = embed_texts(&queries)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(normalized)
        .collect();
    let n = corpus.embs.len();
    let dim = corpus.embs.first().map_or(0, Vec::len);
    let pos_of: HashMap<i64, usize> = corpus
        .ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();

    let truth: Vec<Vec<usize>> = qembs
        .iter()
        .map(|q| exact_top(&corpus.embs, q, TOP_K))
        .collect();

    // Exact f32 (the vectors are already in memory, as they would be after
    // one SQLite read at load).
    let t = Instant::now();
    let reps = 5;
    for _ in 0..reps {
        for q in &qembs {
            std::hint::black_box(exact_top(&corpus.embs, q, TOP_K));
        }
    }
    let f32_us = t.elapsed().as_micros() as f64 / (reps * qembs.len()) as f64;

    // int8
    let t = Instant::now();
    let int8 = Int8Index::build(&corpus.embs);
    let int8_build_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let mut int8_recall = 0.0;
    for _ in 0..reps {
        int8_recall = 0.0;
        for (q, tr) in qembs.iter().zip(&truth) {
            int8_recall += recall(&int8.top(q, TOP_K), tr);
        }
    }
    let int8_us = t.elapsed().as_micros() as f64 / (reps * qembs.len()) as f64;
    int8_recall /= qembs.len() as f64;
    // int8 shortlist of 4k, re-scored exactly.
    let mut rescored_recall = 0.0;
    let t = Instant::now();
    for (q, tr) in qembs.iter().zip(&truth) {
        let mut short: Vec<(f32, usize)> = int8
            .top(q, TOP_K * 4)
            .into_iter()
            .map(|i| (dot(&corpus.embs[i], q), i))
            .collect();
        short.sort_by(|a, b| b.0.total_cmp(&a.0));
        let found: Vec<usize> = short.into_iter().take(TOP_K).map(|(_, i)| i).collect();
        rescored_recall += recall(&found, tr);
    }
    let rescored_us = t.elapsed().as_micros() as f64 / qembs.len() as f64;
    rescored_recall /= qembs.len() as f64;

    // Production path: first call loads, the rest hit the cache.
    let t = Instant::now();
    vector_rank(project, conn, &qembs[0], TOP_K, CachePolicy::Keep).map_err(|e| e.to_string())?;
    let prod_load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let mut prod_recall = 0.0;
    for (q, tr) in qembs.iter().zip(&truth) {
        let got =
            vector_rank(project, conn, q, TOP_K, CachePolicy::Keep).map_err(|e| e.to_string())?;
        let found: Vec<usize> = got
            .iter()
            .filter_map(|(id, _)| pos_of.get(id).copied())
            .collect();
        prod_recall += recall(&found, tr);
    }
    let prod_us = t.elapsed().as_micros() as f64 / qembs.len() as f64;
    prod_recall /= qembs.len() as f64;

    // Loading the f32 matrix from SQLite (what an exact backend does at load).
    let t = Instant::now();
    let reloaded = load_corpus(conn)?;
    let sqlite_load_ms = t.elapsed().as_secs_f64() * 1000.0;
    drop(reloaded);

    let mb = |b: usize| b as f64 / 1_048_576.0;
    println!("{} queries, {n} vectors x {dim}\n", qembs.len());
    println!("| backend | load | per query | recall@10 | vectors in RAM |");
    println!("|---|---|---|---|---|");
    println!(
        "| production | {prod_load_ms:.0} ms | {prod_us:.0} µs | {:.3} | — |",
        prod_recall
    );
    println!(
        "| exact f32 | {sqlite_load_ms:.0} ms (SQLite) | {f32_us:.0} µs | 1.000 | {:.1} MB |",
        mb(n * dim * 4)
    );
    println!(
        "| exact int8 | +{int8_build_ms:.0} ms | {int8_us:.0} µs | {:.3} | {:.1} MB |",
        int8_recall,
        mb(int8.bytes())
    );
    println!(
        "| int8 + f32 rescore of top {} | — | {rescored_us:.0} µs | {:.3} | {:.1} MB + f32 rows |",
        TOP_K * 4,
        rescored_recall,
        mb(int8.bytes())
    );
    Ok(())
}

const GENERIC_QUERIES: &[&str] = &[
    "database connection setup",
    "error handling and retries",
    "parse configuration file",
    "authentication and login",
    "render the user interface",
    "write output to disk",
    "unit tests for the parser",
    "command line arguments",
    "http request handler",
    "cache invalidation",
];

// ---------------------------------------------------------------------------
// quality

struct Variant {
    label: String,
    model: Option<fastembed::EmbeddingModel>,
    query_prefix: &'static str,
    passage_prefix: &'static str,
}

fn variant(name: &str) -> Result<Variant, String> {
    use fastembed::EmbeddingModel as M;
    let (model, q, p) = match name {
        "stored" => (None, "", ""),
        "bge" => (Some(M::BGESmallENV15), "", ""),
        "bge-instr" => (
            Some(M::BGESmallENV15),
            "Represent this sentence for searching relevant passages: ",
            "",
        ),
        "bge-q" => (Some(M::BGESmallENV15Q), "", ""),
        "e5-multi" => (Some(M::MultilingualE5Small), "query: ", "passage: "),
        "jina-code" => (Some(M::JinaEmbeddingsV2BaseCode), "", ""),
        "nomic-q" => (
            Some(M::NomicEmbedTextV15Q),
            "search_query: ",
            "search_document: ",
        ),
        other => return Err(format!("unknown model variant {other}")),
    };
    Ok(Variant {
        label: name.to_string(),
        model,
        query_prefix: q,
        passage_prefix: p,
    })
}

fn load_model(m: fastembed::EmbeddingModel) -> Result<fastembed::TextEmbedding, String> {
    let default_model = m == fastembed::EmbeddingModel::BGESmallENV15;
    let cpu = ort::execution_providers::CPUExecutionProvider::default().with_arena_allocator();
    fastembed::TextEmbedding::try_new(
        fastembed::InitOptions::new(m)
            .with_cache_dir(match std::env::var_os("VOID_STACK_BENCH_MODELS") {
                // Candidate models go to a throwaway folder, not the user's cache.
                Some(dir) if !default_model => dir.into(),
                _ => super::stats::model_cache_dir(),
            })
            .with_show_download_progress(false)
            .with_execution_providers(vec![cpu.build()]),
    )
    .map_err(|e| e.to_string())
}

/// Embed in length-sorted batches of 8, like the indexer.
fn embed_all(
    model: &mut fastembed::TextEmbedding,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, String> {
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by_key(|i| texts[*i].len());
    let mut out = vec![Vec::new(); texts.len()];
    for batch in order.chunks(8) {
        let b: Vec<String> = batch.iter().map(|i| texts[*i].clone()).collect();
        let embs = model.embed(b, Some(8)).map_err(|e| e.to_string())?;
        for (i, e) in batch.iter().zip(embs) {
            out[*i] = normalized(e);
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Scores {
    hit1: f64,
    hit5: f64,
    mrr: f64,
}

impl Scores {
    fn add(&mut self, ranked_files: &[String], relevant: &HashSet<String>) {
        if let Some(pos) = ranked_files
            .iter()
            .take(TOP_K)
            .position(|f| relevant.contains(f))
        {
            if pos == 0 {
                self.hit1 += 1.0;
            }
            if pos < 5 {
                self.hit5 += 1.0;
            }
            self.mrr += 1.0 / (pos as f64 + 1.0);
        }
    }
    fn row(&self, n: usize) -> String {
        let n = n.max(1) as f64;
        format!(
            "{:.2} | {:.2} | {:.3}",
            self.hit1 / n,
            self.hit5 / n,
            self.mrr / n
        )
    }
}

fn files_in_order(positions: impl IntoIterator<Item = usize>, corpus: &Corpus) -> Vec<String> {
    let mut seen = HashSet::new();
    positions
        .into_iter()
        .map(|i| corpus.files[i].clone())
        .filter(|f| seen.insert(f.clone()))
        .collect()
}

fn quality(
    conn: &rusqlite::Connection,
    corpus: &Corpus,
    evals: &[EvalQuery],
    models: &[String],
) -> Result<(), String> {
    if evals.is_empty() {
        println!("(no eval queries — no usable git history)");
        return Ok(());
    }
    let pos_of: HashMap<i64, usize> = corpus
        .ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    let fetch = 30;
    let lexical: Vec<Vec<i64>> = evals
        .iter()
        .map(|e| lexical_rank(conn, &e.text, fetch).unwrap_or_default())
        .collect();
    let mut lex = Scores::default();
    for (e, l) in evals.iter().zip(&lexical) {
        lex.add(
            &files_in_order(l.iter().filter_map(|id| pos_of.get(id).copied()), corpus),
            &e.relevant,
        );
    }

    println!(
        "| variant | index time | dim | vector hit@1 | hit@5 | MRR | hybrid hit@1 | hit@5 | MRR |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    println!(
        "| lexical only (FTS5) | — | — | {} | — | — | — |",
        lex.row(evals.len())
    );
    for name in models {
        let v = variant(name)?;
        let t = Instant::now();
        let (passages, queries) = match v.model.clone() {
            None => {
                let q = embed_texts(&evals.iter().map(|e| e.text.clone()).collect::<Vec<_>>())
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(normalized)
                    .collect();
                (corpus.embs.clone(), q)
            }
            Some(m) => {
                let mut model = match load_model(m) {
                    Ok(model) => model,
                    Err(e) => {
                        println!("| {} | failed to load: {} | | | | | | | |", v.label, e);
                        continue;
                    }
                };
                let texts: Vec<String> = corpus
                    .texts
                    .iter()
                    .map(|t| format!("{}{}", v.passage_prefix, t))
                    .collect();
                let p = embed_all(&mut model, &texts)?;
                let qt: Vec<String> = evals
                    .iter()
                    .map(|e| format!("{}{}", v.query_prefix, e.text))
                    .collect();
                let q = embed_all(&mut model, &qt)?;
                (p, q)
            }
        };
        let secs = t.elapsed().as_secs_f64();
        let dim = passages.first().map_or(0, Vec::len);
        let mut vec_s = Scores::default();
        let mut hyb_s = Scores::default();
        for ((e, q), lex_ids) in evals.iter().zip(&queries).zip(&lexical) {
            let top = exact_top(&passages, q, fetch);
            vec_s.add(&files_in_order(top.iter().copied(), corpus), &e.relevant);
            let vec_ids: Vec<i64> = top.iter().map(|i| corpus.ids[*i]).collect();
            let weight = if super::search::is_identifier_query(&e.text) {
                IDENTIFIER_LEXICAL_BOOST
            } else {
                1.0
            };
            let fused = rrf_fuse(&vec_ids, lex_ids, RRF_K, weight);
            hyb_s.add(
                &files_in_order(
                    fused.iter().filter_map(|id| pos_of.get(id).copied()),
                    corpus,
                ),
                &e.relevant,
            );
        }
        println!(
            "| {} | {} | {dim} | {} | {} |",
            v.label,
            if v.model.is_none() {
                "(stored)".to_string()
            } else {
                format!("{secs:.0} s")
            },
            vec_s.row(evals.len()),
            hyb_s.row(evals.len())
        );
    }
    Ok(())
}
