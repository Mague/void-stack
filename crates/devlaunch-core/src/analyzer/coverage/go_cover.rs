//! Go coverage profile parser (go test -coverprofile).
//!
//! Format:
//! ```text
//! mode: set
//! github.com/user/pkg/file.go:10.2,12.3 1 1
//! github.com/user/pkg/file.go:14.2,16.3 1 0
//! ```

use std::collections::HashMap;

use super::{CoverageData, FileCoverage};

pub fn parse(content: &str) -> Option<CoverageData> {
    let mut lines = content.lines();

    // First line should be "mode: ..."
    let first = lines.next()?.trim();
    if !first.starts_with("mode:") {
        return None;
    }

    // Aggregate per file: (total_stmts, covered_stmts)
    let mut file_stats: HashMap<String, (usize, usize)> = HashMap::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Format: file:startline.startcol,endline.endcol num_stmts count
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        // Extract file path (before first ':')
        let file_and_range = parts[0];
        let colon = match file_and_range.find(':') {
            Some(p) => p,
            None => continue,
        };
        let file_path = &file_and_range[..colon];

        let num_stmts: usize = parts[1].parse().unwrap_or(0);
        let count: usize = parts[2].parse().unwrap_or(0);

        let entry = file_stats.entry(file_path.to_string()).or_insert((0, 0));
        entry.0 += num_stmts;
        if count > 0 {
            entry.1 += num_stmts;
        }
    }

    if file_stats.is_empty() {
        return None;
    }

    let files: Vec<FileCoverage> = file_stats.into_iter()
        .map(|(path, (total, covered))| {
            let pct = if total > 0 { covered as f32 / total as f32 * 100.0 } else { 0.0 };
            FileCoverage {
                path,
                total_lines: total,
                covered_lines: covered,
                coverage_percent: pct,
            }
        })
        .collect();

    let mut data = CoverageData {
        tool: "go-cover".to_string(),
        total_lines: 0,
        covered_lines: 0,
        coverage_percent: 0.0,
        files,
    };
    data.recalculate();
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_go_cover() {
        let content = r#"mode: set
github.com/user/pkg/handler.go:10.2,12.3 2 1
github.com/user/pkg/handler.go:14.2,18.3 4 0
github.com/user/pkg/service.go:5.2,8.3 3 1
github.com/user/pkg/service.go:10.2,12.3 2 1
"#;
        let data = parse(content).unwrap();
        assert_eq!(data.files.len(), 2);
        assert_eq!(data.total_lines, 11);
        assert_eq!(data.covered_lines, 7); // 2+3+2 = 7 covered stmts
    }
}
