//! Deduplication of consecutive repeated log lines → "message (×N)".

pub(super) fn deduplicate(lines: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut prev: Option<&str> = None;
    let mut count: usize = 0;

    for line in lines {
        if Some(line.as_str()) == prev {
            count += 1;
        } else {
            // Flush previous
            if count > 0
                && let Some(last) = result.last_mut()
            {
                *last = format!("{} (×{})", last, count + 1);
            }
            result.push(line.clone());
            prev = Some(line.as_str());
            count = 0;
        }
    }

    // Flush final
    if count > 0
        && let Some(last) = result.last_mut()
    {
        *last = format!("{} (×{})", last, count + 1);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deduplicate_consecutive() {
        let lines: Vec<String> = vec![
            "hello".into(),
            "hello".into(),
            "hello".into(),
            "world".into(),
        ];
        let result = deduplicate(&lines);
        assert_eq!(result, vec!["hello (×3)", "world"]);
    }

    #[test]
    fn test_deduplicate_no_repeats() {
        let lines: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let result = deduplicate(&lines);
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_deduplicate_at_end() {
        let lines: Vec<String> = vec!["a".into(), "b".into(), "b".into()];
        let result = deduplicate(&lines);
        assert_eq!(result, vec!["a", "b (×2)"]);
    }
}
