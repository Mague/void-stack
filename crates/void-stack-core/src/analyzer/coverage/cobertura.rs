//! Cobertura XML parser (pytest-cov, cargo tarpaulin, many CI tools).
//!
//! Extracts line coverage from `<class>` or `<package>` elements.

use super::{CoverageData, FileCoverage};

pub fn parse(content: &str) -> Option<CoverageData> {
    // Quick check that it's Cobertura format
    if !content.contains("<coverage") {
        return None;
    }

    let mut files = Vec::new();

    // Extract per-file coverage from <class> elements
    // Format: <class name="..." filename="..." line-rate="0.85" ...>
    let mut pos = 0;
    while let Some(start) = content[pos..].find("<class ") {
        let abs_start = pos + start;
        let end = match content[abs_start..].find('>') {
            Some(e) => abs_start + e,
            None => break,
        };
        let tag = &content[abs_start..=end];

        let filename = extract_attr(tag, "filename");
        let line_rate = extract_attr(tag, "line-rate");

        if let (Some(fname), Some(rate_str)) = (filename, line_rate)
            && let Ok(rate) = rate_str.parse::<f64>()
        {
            // Count lines from <line> elements within this class
            let (total, covered) = count_lines_in_class(content, abs_start);
            let (total, covered) = if total > 0 {
                (total, covered)
            } else {
                // Fallback: estimate from line-rate
                (100, (rate * 100.0) as usize)
            };

            files.push(FileCoverage {
                path: fname,
                total_lines: total,
                covered_lines: covered,
                coverage_percent: rate as f32 * 100.0,
            });
        }

        pos = end + 1;
    }

    // If no <class> elements found, try top-level <coverage> attributes
    if files.is_empty()
        && let Some(cov_start) = content.find("<coverage ")
    {
        let cov_end = content[cov_start..].find('>').unwrap_or(200) + cov_start;
        let tag = &content[cov_start..=cov_end.min(content.len() - 1)];

        let line_rate = extract_attr(tag, "line-rate");
        let lines_valid = extract_attr(tag, "lines-valid");
        let lines_covered = extract_attr(tag, "lines-covered");

        if let (Some(valid_str), Some(covered_str)) = (lines_valid, lines_covered) {
            let total = valid_str.parse::<usize>().unwrap_or(0);
            let covered = covered_str.parse::<usize>().unwrap_or(0);
            let pct = line_rate.and_then(|r| r.parse::<f32>().ok()).unwrap_or(0.0) * 100.0;

            files.push(FileCoverage {
                path: "(project total)".to_string(),
                total_lines: total,
                covered_lines: covered,
                coverage_percent: pct,
            });
        }
    }

    if files.is_empty() {
        return None;
    }

    let tool = if content.contains("tarpaulin") || content.contains("Tarpaulin") {
        "tarpaulin"
    } else if content.contains("coverage.py") || content.contains("python") {
        "pytest-cov"
    } else {
        "cobertura"
    };

    let mut data = CoverageData {
        tool: tool.to_string(),
        total_lines: 0,
        covered_lines: 0,
        coverage_percent: 0.0,
        files,
    };
    data.recalculate();
    Some(data)
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = tag.find(&pattern)? + pattern.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn count_lines_in_class(content: &str, class_start: usize) -> (usize, usize) {
    // Find the end of this class element
    let rest = &content[class_start..];
    let end = rest
        .find("</class>")
        .or_else(|| rest.find("/>").map(|p| p + 2))
        .unwrap_or(rest.len().min(10000));

    let class_content = &rest[..end];
    let mut total = 0usize;
    let mut covered = 0usize;

    // Parse <line number="N" hits="N"/> elements
    let mut pos = 0;
    while let Some(start) = class_content[pos..].find("<line ") {
        let abs = pos + start;
        let line_end = match class_content[abs..].find("/>") {
            Some(e) => abs + e + 2,
            None => break,
        };
        let line_tag = &class_content[abs..line_end];

        if let Some(hits_str) = extract_attr(line_tag, "hits") {
            total += 1;
            if let Ok(hits) = hits_str.parse::<u32>()
                && hits > 0
            {
                covered += 1;
            }
        }
        pos = line_end;
    }

    (total, covered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_attr_fn() {
        let tag = r#"<class name="app.py" filename="src/app.py" line-rate="0.85">"#;
        assert_eq!(
            extract_attr(tag, "filename"),
            Some("src/app.py".to_string())
        );
        assert_eq!(extract_attr(tag, "line-rate"), Some("0.85".to_string()));
        assert_eq!(extract_attr(tag, "missing"), None);
    }

    #[test]
    fn test_parse_cobertura_top_level() {
        let xml = r#"<?xml version="1.0" ?>
<coverage line-rate="0.70" lines-valid="200" lines-covered="140">
</coverage>"#;
        let data = parse(xml).unwrap();
        assert_eq!(data.total_lines, 200);
        assert_eq!(data.covered_lines, 140);
    }

    #[test]
    fn test_parse_cobertura_tarpaulin() {
        let xml = r#"<?xml version="1.0" ?>
<coverage line-rate="0.50" lines-valid="100" lines-covered="50">
<!-- Generated by tarpaulin -->
</coverage>"#;
        let data = parse(xml).unwrap();
        assert_eq!(data.tool, "tarpaulin");
    }

    #[test]
    fn test_parse_cobertura_pytest() {
        let xml = r#"<?xml version="1.0" ?>
<coverage version="6.0" line-rate="0.80" lines-valid="50" lines-covered="40">
<!-- Generated by coverage.py -->
</coverage>"#;
        let data = parse(xml).unwrap();
        assert_eq!(data.tool, "pytest-cov");
    }

    #[test]
    fn test_parse_non_cobertura() {
        assert!(parse("<html>not xml coverage</html>").is_none());
        assert!(parse("plain text").is_none());
    }

    #[test]
    fn test_parse_cobertura() {
        let content = r#"<?xml version="1.0" ?>
<coverage version="6.0" timestamp="1234" lines-valid="100" lines-covered="75" line-rate="0.75" branches-valid="0" branches-covered="0" branch-rate="0" complexity="0">
    <packages>
        <package name="myapp">
            <classes>
                <class name="app.py" filename="app.py" line-rate="0.80" branch-rate="0" complexity="0">
                    <lines>
                        <line number="1" hits="1"/>
                        <line number="2" hits="1"/>
                        <line number="3" hits="1"/>
                        <line number="4" hits="1"/>
                        <line number="5" hits="0"/>
                    </lines>
                </class>
                <class name="utils.py" filename="utils.py" line-rate="0.60" branch-rate="0" complexity="0">
                    <lines>
                        <line number="1" hits="1"/>
                        <line number="2" hits="0"/>
                        <line number="3" hits="1"/>
                        <line number="4" hits="0"/>
                        <line number="5" hits="1"/>
                    </lines>
                </class>
            </classes>
        </package>
    </packages>
</coverage>"#;
        let data = parse(content).unwrap();
        assert_eq!(data.files.len(), 2);
        assert_eq!(data.files[0].path, "app.py");
        assert_eq!(data.files[0].total_lines, 5);
        assert_eq!(data.files[0].covered_lines, 4);
        assert_eq!(data.files[1].path, "utils.py");
        assert_eq!(data.files[1].covered_lines, 3);
    }
}
