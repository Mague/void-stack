//! Truncation of long output (first 20 + last 30, middle omitted).

const MAX_LINES: usize = 150;
const HEAD_LINES: usize = 20;
const TAIL_LINES: usize = 30;

pub(super) fn truncate_lines(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= MAX_LINES {
        return lines;
    }

    let omitted = lines.len() - HEAD_LINES - TAIL_LINES;
    let mut result = Vec::with_capacity(HEAD_LINES + TAIL_LINES + 1);
    result.extend_from_slice(&lines[..HEAD_LINES]);
    result.push(format!("... [{} lines omitted] ...", omitted));
    result.extend_from_slice(&lines[lines.len() - TAIL_LINES..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        let lines: Vec<String> = (0..50).map(|i| format!("line {}", i)).collect();
        let result = truncate_lines(lines.clone());
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn test_truncate_long() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {}", i)).collect();
        let result = truncate_lines(lines);
        // 20 head + 1 omitted marker + 30 tail = 51
        assert_eq!(result.len(), 51);
        assert!(result[0].contains("line 0"));
        assert!(result[19].contains("line 19"));
        assert!(result[20].contains("lines omitted"));
        assert!(result[21].contains("line 170"));
        assert!(result[50].contains("line 199"));
    }
}
