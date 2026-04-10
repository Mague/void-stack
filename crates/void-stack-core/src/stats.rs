//! Token savings statistics — tracks how much noise Void Stack filters.
//!
//! Stores records in `~/.void-stack/stats.db` (SQLite) and provides
//! aggregated reports by project and operation type.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;

/// A single savings event.
#[derive(Debug, Clone)]
pub struct TokenSavingsRecord {
    pub timestamp: DateTime<Utc>,
    pub project: String,
    /// Operation type: "log_filter", "claudeignore", etc.
    pub operation: String,
    pub lines_original: usize,
    pub lines_filtered: usize,
    pub savings_pct: f32,
}

/// Aggregated stats for a project.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectStats {
    pub project: String,
    pub avg_savings_pct: f32,
    pub operations: usize,
    pub lines_saved: usize,
}

/// Aggregated stats for an operation type.
#[derive(Debug, Clone, Serialize)]
pub struct OperationStats {
    pub operation: String,
    pub avg_savings_pct: f32,
    pub operations: usize,
    pub lines_saved: usize,
}

/// Full stats report.
#[derive(Debug, Clone, Serialize)]
pub struct StatsReport {
    pub total_operations: usize,
    pub avg_savings_pct: f32,
    pub total_lines_saved: usize,
    pub by_project: Vec<ProjectStats>,
    pub by_operation: Vec<OperationStats>,
    pub period_days: u32,
}

/// Get the path to the stats database.
fn db_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("void-stack").join("stats.db"))
}

/// Open (or create) the stats database connection.
fn open_db() -> Result<Connection, rusqlite::Error> {
    let path = db_path().unwrap_or_else(|| PathBuf::from("stats.db"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(&path)?;
    init_db_with(&conn)?;
    Ok(conn)
}

/// Initialize the stats table if it doesn't exist.
pub fn init_db() -> Result<(), String> {
    open_db().map(|_| ()).map_err(|e| e.to_string())
}

/// Create table on a given connection (also used for testing with :memory:).
pub fn init_db_with(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS token_savings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            project TEXT NOT NULL,
            operation TEXT NOT NULL,
            lines_original INTEGER NOT NULL,
            lines_filtered INTEGER NOT NULL,
            savings_pct REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ts ON token_savings(timestamp);
        CREATE INDEX IF NOT EXISTS idx_project ON token_savings(project);",
    )?;
    Ok(())
}

/// Record a savings event. Silently ignores errors (best-effort tracking).
pub fn record_saving(record: TokenSavingsRecord) {
    if let Ok(conn) = open_db() {
        let _ = record_saving_with(&conn, &record);
    }
}

/// Record on a specific connection (for testing).
pub fn record_saving_with(
    conn: &Connection,
    record: &TokenSavingsRecord,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO token_savings (timestamp, project, operation, lines_original, lines_filtered, savings_pct)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            record.timestamp.to_rfc3339(),
            record.project,
            record.operation,
            record.lines_original as i64,
            record.lines_filtered as i64,
            record.savings_pct as f64,
        ],
    )?;
    Ok(())
}

/// Get aggregated stats, optionally filtered by project and time window.
pub fn get_stats(project: Option<&str>, days: u32) -> Result<StatsReport, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    get_stats_with(&conn, project, days)
}

/// Get stats on a specific connection (for testing).
pub fn get_stats_with(
    conn: &Connection,
    project: Option<&str>,
    days: u32,
) -> Result<StatsReport, String> {
    let cutoff = Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    // Total aggregates
    // - Exclude .tmp* projects (test artifacts from tempfile::tempdir)
    // - Exclude vector_index from avg_savings_pct (infrastructure op with 0% savings distorts the average)
    let (total_operations, avg_savings_pct, total_lines_saved) = if let Some(proj) = project {
        let mut stmt = conn
            .prepare(
                "SELECT COUNT(*), COALESCE(AVG(savings_pct), 0), COALESCE(SUM(lines_original - lines_filtered), 0)
                 FROM token_savings WHERE timestamp >= ?1 AND project = ?2
                 AND operation != 'vector_index'",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_row(rusqlite::params![cutoff_str, proj], |row| {
            Ok((
                row.get::<_, i64>(0)? as usize,
                row.get::<_, f64>(1)? as f32,
                row.get::<_, i64>(2)? as usize,
            ))
        })
        .map_err(|e| e.to_string())?
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT COUNT(*), COALESCE(AVG(savings_pct), 0), COALESCE(SUM(lines_original - lines_filtered), 0)
                 FROM token_savings WHERE timestamp >= ?1
                 AND project NOT LIKE '.tmp%'
                 AND operation != 'vector_index'",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_row(rusqlite::params![cutoff_str], |row| {
            Ok((
                row.get::<_, i64>(0)? as usize,
                row.get::<_, f64>(1)? as f32,
                row.get::<_, i64>(2)? as usize,
            ))
        })
        .map_err(|e| e.to_string())?
    };

    // By project
    let by_project = query_group_stats::<ProjectStats>(conn, &cutoff_str, project, "project")?;

    // By operation
    let by_operation =
        query_group_stats::<OperationStats>(conn, &cutoff_str, project, "operation")?;

    Ok(StatsReport {
        total_operations,
        avg_savings_pct,
        total_lines_saved,
        by_project,
        by_operation,
        period_days: days,
    })
}

/// Trait for types that can be parsed from a grouped stats query row.
trait FromStatsRow: Sized {
    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error>;
}

impl FromStatsRow for ProjectStats {
    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error> {
        Ok(ProjectStats {
            project: row.get(0)?,
            avg_savings_pct: row.get::<_, f64>(1)? as f32,
            operations: row.get::<_, i64>(2)? as usize,
            lines_saved: row.get::<_, i64>(3)? as usize,
        })
    }
}

impl FromStatsRow for OperationStats {
    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error> {
        Ok(OperationStats {
            operation: row.get(0)?,
            avg_savings_pct: row.get::<_, f64>(1)? as f32,
            operations: row.get::<_, i64>(2)? as usize,
            lines_saved: row.get::<_, i64>(3)? as usize,
        })
    }
}

fn query_group_stats<T: FromStatsRow>(
    conn: &Connection,
    cutoff_str: &str,
    project: Option<&str>,
    group_col: &str,
) -> Result<Vec<T>, String> {
    let order = if group_col == "project" {
        "COUNT(*) DESC"
    } else {
        "AVG(savings_pct) DESC"
    };

    let query = if project.is_some() {
        format!(
            "SELECT {col}, AVG(savings_pct), COUNT(*), SUM(lines_original - lines_filtered)
             FROM token_savings WHERE timestamp >= ?1 AND project = ?2
             GROUP BY {col} ORDER BY {order}",
            col = group_col,
            order = order
        )
    } else {
        format!(
            "SELECT {col}, AVG(savings_pct), COUNT(*), SUM(lines_original - lines_filtered)
             FROM token_savings WHERE timestamp >= ?1
             AND project NOT LIKE '.tmp%'
             GROUP BY {col} ORDER BY {order}",
            col = group_col,
            order = order
        )
    };

    let mut stmt = conn.prepare(&query).map_err(|e| e.to_string())?;
    let results: Vec<T> = if let Some(proj) = project {
        let mut rows = stmt
            .query(rusqlite::params![cutoff_str, proj])
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if let Ok(v) = T::from_row(row) {
                out.push(v);
            }
        }
        out
    } else {
        let mut rows = stmt
            .query(rusqlite::params![cutoff_str])
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if let Ok(v) = T::from_row(row) {
                out.push(v);
            }
        }
        out
    };

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db_with(&conn).unwrap();
        conn
    }

    fn sample_record(
        project: &str,
        operation: &str,
        orig: usize,
        filtered: usize,
    ) -> TokenSavingsRecord {
        let savings_pct = if orig > 0 {
            (1.0 - (filtered as f32 / orig as f32)) * 100.0
        } else {
            0.0
        };
        TokenSavingsRecord {
            timestamp: Utc::now(),
            project: project.to_string(),
            operation: operation.to_string(),
            lines_original: orig,
            lines_filtered: filtered,
            savings_pct,
        }
    }

    #[test]
    fn test_init_db() {
        let conn = memory_db();
        // Should not fail on second init
        init_db_with(&conn).unwrap();
    }

    #[test]
    fn test_record_and_get_empty() {
        let conn = memory_db();
        let report = get_stats_with(&conn, None, 30).unwrap();
        assert_eq!(report.total_operations, 0);
        assert_eq!(report.avg_savings_pct, 0.0);
        assert_eq!(report.total_lines_saved, 0);
        assert!(report.by_project.is_empty());
        assert!(report.by_operation.is_empty());
    }

    #[test]
    fn test_record_saving() {
        let conn = memory_db();
        let rec = sample_record("my-app", "log_filter", 100, 30);
        record_saving_with(&conn, &rec).unwrap();

        let report = get_stats_with(&conn, None, 30).unwrap();
        assert_eq!(report.total_operations, 1);
        assert!(report.avg_savings_pct > 60.0);
        assert_eq!(report.total_lines_saved, 70);
    }

    #[test]
    fn test_multiple_records() {
        let conn = memory_db();
        record_saving_with(&conn, &sample_record("app-a", "log_filter", 100, 30)).unwrap();
        record_saving_with(&conn, &sample_record("app-a", "log_filter", 200, 50)).unwrap();
        record_saving_with(&conn, &sample_record("app-b", "claudeignore", 50, 10)).unwrap();

        let report = get_stats_with(&conn, None, 30).unwrap();
        assert_eq!(report.total_operations, 3);
        assert_eq!(report.total_lines_saved, 70 + 150 + 40);
        assert_eq!(report.by_project.len(), 2);
        assert_eq!(report.by_operation.len(), 2);
    }

    #[test]
    fn test_filter_by_project() {
        let conn = memory_db();
        record_saving_with(&conn, &sample_record("app-a", "log_filter", 100, 30)).unwrap();
        record_saving_with(&conn, &sample_record("app-b", "log_filter", 200, 50)).unwrap();

        let report = get_stats_with(&conn, Some("app-a"), 30).unwrap();
        assert_eq!(report.total_operations, 1);
        assert_eq!(report.total_lines_saved, 70);
        assert_eq!(report.by_project.len(), 1);
        assert_eq!(report.by_project[0].project, "app-a");
    }

    #[test]
    fn test_by_operation_breakdown() {
        let conn = memory_db();
        record_saving_with(&conn, &sample_record("app", "log_filter", 100, 20)).unwrap();
        record_saving_with(&conn, &sample_record("app", "log_filter", 100, 30)).unwrap();
        record_saving_with(&conn, &sample_record("app", "claudeignore", 50, 10)).unwrap();

        let report = get_stats_with(&conn, None, 30).unwrap();
        assert_eq!(report.by_operation.len(), 2);

        let log_op = report
            .by_operation
            .iter()
            .find(|o| o.operation == "log_filter")
            .unwrap();
        assert_eq!(log_op.operations, 2);
        assert_eq!(log_op.lines_saved, 80 + 70);

        let ci_op = report
            .by_operation
            .iter()
            .find(|o| o.operation == "claudeignore")
            .unwrap();
        assert_eq!(ci_op.operations, 1);
    }

    #[test]
    fn test_period_days_zero() {
        let conn = memory_db();
        record_saving_with(&conn, &sample_record("app", "log_filter", 100, 50)).unwrap();

        // days=0 means "last 0 days" which is effectively now — should still capture recent
        let report = get_stats_with(&conn, None, 0).unwrap();
        // Record was just inserted, so it should be within "last 0 days" (cutoff = now)
        // This might be 0 or 1 depending on timing, so just verify no crash
        assert!(report.total_operations <= 1);
    }

    #[test]
    fn test_report_serializes() {
        let report = StatsReport {
            total_operations: 5,
            avg_savings_pct: 65.0,
            total_lines_saved: 1000,
            by_project: vec![ProjectStats {
                project: "test".into(),
                avg_savings_pct: 65.0,
                operations: 5,
                lines_saved: 1000,
            }],
            by_operation: vec![],
            period_days: 30,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("total_operations"));
        assert!(json.contains("1000"));
    }

    #[test]
    fn test_avg_savings_calculation() {
        let conn = memory_db();
        // 50% savings
        record_saving_with(&conn, &sample_record("app", "log_filter", 100, 50)).unwrap();
        // 80% savings
        record_saving_with(&conn, &sample_record("app", "log_filter", 100, 20)).unwrap();

        let report = get_stats_with(&conn, None, 30).unwrap();
        // Average should be ~65%
        assert!(report.avg_savings_pct > 60.0 && report.avg_savings_pct < 70.0);
    }

    #[test]
    fn test_get_stats_filters_tmp_projects() {
        let conn = memory_db();
        record_saving_with(&conn, &sample_record("my-app", "semantic_search", 1000, 40)).unwrap();
        record_saving_with(&conn, &sample_record(".tmp1ABC", "claudeignore", 10, 0)).unwrap();

        let report = get_stats_with(&conn, None, 30).unwrap();
        // Only "my-app" should appear, not ".tmp1ABC"
        assert_eq!(report.by_project.len(), 1);
        assert_eq!(report.by_project[0].project, "my-app");
    }

    #[test]
    fn test_get_stats_excludes_vector_index_from_avg() {
        let conn = memory_db();
        // semantic_search: 97.5% savings
        record_saving_with(&conn, &sample_record("app", "semantic_search", 1000, 25)).unwrap();
        // vector_index: 0% savings (infrastructure op)
        let mut vi_record = sample_record("app", "vector_index", 50000, 50000);
        vi_record.savings_pct = 0.0;
        record_saving_with(&conn, &vi_record).unwrap();

        let report = get_stats_with(&conn, None, 30).unwrap();
        // Avg should reflect only semantic_search (~97.5%), not be dragged down by vector_index
        assert!(
            report.avg_savings_pct > 90.0,
            "avg was {} — vector_index should be excluded from avg",
            report.avg_savings_pct
        );
        // But vector_index still appears in by_operation
        assert!(
            report
                .by_operation
                .iter()
                .any(|o| o.operation == "vector_index")
        );
    }
}
