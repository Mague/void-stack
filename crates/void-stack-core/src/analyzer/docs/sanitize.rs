//! Mermaid ID sanitization helpers.

/// Sanitize a string for use as a Mermaid diagram node ID.
///
/// Replaces any non-alphanumeric character (except underscore) with `_`.
pub(crate) fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("hello"), "hello");
        assert_eq!(sanitize_id("src/main.py"), "src_main_py");
        assert_eq!(sanitize_id("my-module"), "my_module");
        assert_eq!(sanitize_id("path/to/file.rs"), "path_to_file_rs");
    }
}
