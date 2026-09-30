//! Quién usó la GPU, cuánto, y cuánto esperó. En SQLite, para que sobreviva.
//!
//! Sigue el patrón de `void-stack-core/src/stats.rs`: un fichero en el
//! directorio de datos de void-stack, tablas con `IF NOT EXISTS`, y escribir es
//! de mejor esfuerzo — un disco lleno no puede tumbar al árbitro de la GPU.

use std::path::Path;

use rusqlite::{Connection, params};

use crate::model::{HistoryEntry, Outcome, Priority};

pub const FILE_NAME: &str = "gpu-history.db";

pub struct History {
    conn: Connection,
}

fn priority_str(p: Priority) -> &'static str {
    match p {
        Priority::Critical => "critical",
        Priority::Normal => "normal",
        Priority::Low => "low",
        Priority::Opportunistic => "opportunistic",
    }
}

fn priority_from(s: &str) -> Priority {
    match s {
        "critical" => Priority::Critical,
        "normal" => Priority::Normal,
        "opportunistic" => Priority::Opportunistic,
        _ => Priority::Low,
    }
}

fn outcome_str(o: Outcome) -> &'static str {
    match o {
        Outcome::Released => "released",
        Outcome::Yielded => "yielded",
        Outcome::Expired => "expired",
        Outcome::Cancelled => "cancelled",
    }
}

fn outcome_from(s: &str) -> Outcome {
    match s {
        "yielded" => Outcome::Yielded,
        "expired" => Outcome::Expired,
        "cancelled" => Outcome::Cancelled,
        _ => Outcome::Released,
    }
}

impl History {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS leases (
                lease_id        TEXT NOT NULL,
                owner           TEXT NOT NULL,
                task            TEXT NOT NULL,
                vram_mb         INTEGER NOT NULL,
                priority        TEXT NOT NULL,
                requested_at_ms INTEGER NOT NULL,
                granted_at_ms   INTEGER,
                ended_at_ms     INTEGER NOT NULL,
                outcome         TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_leases_ended ON leases(ended_at_ms);",
        )?;
        Ok(Self { conn })
    }

    pub fn record(&self, e: &HistoryEntry) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO leases (lease_id, owner, task, vram_mb, priority,
                                 requested_at_ms, granted_at_ms, ended_at_ms, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                e.lease_id,
                e.owner,
                e.task,
                e.vram_mb,
                priority_str(e.priority),
                e.requested_at_ms as i64,
                e.granted_at_ms.map(|v| v as i64),
                e.ended_at_ms as i64,
                outcome_str(e.outcome),
            ],
        )?;
        Ok(())
    }

    /// Lo que terminó desde `since_ms`, del más viejo al más nuevo: la línea
    /// de tiempo de las últimas 24 h que pinta La Oficina.
    pub fn since(&self, since_ms: u64) -> rusqlite::Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT lease_id, owner, task, vram_mb, priority, requested_at_ms,
                    granted_at_ms, ended_at_ms, outcome
             FROM leases WHERE ended_at_ms >= ?1 ORDER BY ended_at_ms ASC",
        )?;
        let rows = stmt.query_map(params![since_ms as i64], |r| {
            Ok(HistoryEntry {
                lease_id: r.get(0)?,
                owner: r.get(1)?,
                task: r.get(2)?,
                vram_mb: r.get::<_, i64>(3)? as u32,
                priority: priority_from(&r.get::<_, String>(4)?),
                requested_at_ms: r.get::<_, i64>(5)? as u64,
                granted_at_ms: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                ended_at_ms: r.get::<_, i64>(7)? as u64,
                outcome: outcome_from(&r.get::<_, String>(8)?),
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(owner: &str, ended: u64, outcome: Outcome) -> HistoryEntry {
        HistoryEntry {
            lease_id: format!("x-{owner}"),
            owner: owner.into(),
            task: "t".into(),
            vram_mb: 9_216,
            priority: Priority::Normal,
            requested_at_ms: ended - 5_000,
            granted_at_ms: Some(ended - 4_000),
            ended_at_ms: ended,
            outcome,
        }
    }

    #[test]
    fn lo_escrito_se_lee_igual() {
        let h = History::in_memory().unwrap();
        let e = entry("osac", 10_000, Outcome::Yielded);
        h.record(&e).unwrap();
        assert_eq!(h.since(0).unwrap(), vec![e]);
    }

    #[test]
    fn la_ventana_deja_fuera_lo_viejo_y_ordena_por_final() {
        let h = History::in_memory().unwrap();
        h.record(&entry("viejo", 1_000_000, Outcome::Released))
            .unwrap();
        h.record(&entry("b", 3_000_000, Outcome::Expired)).unwrap();
        h.record(&entry("a", 2_000_000, Outcome::Cancelled))
            .unwrap();
        let owners: Vec<_> = h
            .since(1_500_000)
            .unwrap()
            .into_iter()
            .map(|e| e.owner)
            .collect();
        assert_eq!(owners, ["a", "b"]);
    }

    #[test]
    fn quien_nunca_entro_se_guarda_sin_hora_de_entrada() {
        let h = History::in_memory().unwrap();
        let mut e = entry("cansado", 10_000, Outcome::Expired);
        e.granted_at_ms = None;
        h.record(&e).unwrap();
        assert_eq!(h.since(0).unwrap()[0].granted_at_ms, None);
    }

    #[test]
    fn sobrevive_a_cerrar_y_abrir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        History::open(&path)
            .unwrap()
            .record(&entry("osac", 5_000, Outcome::Released))
            .unwrap();
        assert_eq!(History::open(&path).unwrap().since(0).unwrap().len(), 1);
    }
}
