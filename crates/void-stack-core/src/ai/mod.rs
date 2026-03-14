//! AI-powered contextual suggestions via configurable providers (Ollama, etc.).
//!
//! Analyzes code analysis results and generates actionable refactoring suggestions
//! using local LLMs.

pub mod ollama;
pub mod prompt;

use serde::{Deserialize, Serialize};

use crate::analyzer::AnalysisResult;
use crate::error::{Result, VoidStackError};

// ── Configuration ────────────────────────────────────────────

/// AI provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Provider name: "ollama" (more planned)
    #[serde(default = "default_provider")]
    pub provider: AiProvider,
    /// Model identifier (e.g., "qwen2.5:7b", "llama3.2")
    #[serde(default = "default_model")]
    pub model: String,
    /// Base URL for the AI provider API
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: AiProvider::Ollama,
            model: default_model(),
            base_url: default_base_url(),
        }
    }
}

impl AiConfig {
    /// Human-readable provider name.
    pub fn provider_name(&self) -> &str {
        match self.provider {
            AiProvider::Ollama => "Ollama",
        }
    }
}

fn default_provider() -> AiProvider {
    AiProvider::Ollama
}

fn default_model() -> String {
    "qwen2.5:7b".to_string()
}

fn default_base_url() -> String {
    "http://localhost:11434".to_string()
}

/// Supported AI providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Ollama,
    // Future: OpenAI, Anthropic
}

// ── Result types ─────────────────────────────────────────────

/// Result from an AI suggestion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionResult {
    pub suggestions: Vec<Suggestion>,
    pub model_used: String,
    pub raw_response: String,
}

/// A single actionable suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// Category: "refactoring", "architecture", "performance", "security"
    pub category: String,
    pub title: String,
    pub description: String,
    pub affected_files: Vec<String>,
    pub priority: SuggestionPriority,
}

/// Priority levels for suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SuggestionPriority {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for SuggestionPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SuggestionPriority::Critical => write!(f, "Critical"),
            SuggestionPriority::High => write!(f, "High"),
            SuggestionPriority::Medium => write!(f, "Medium"),
            SuggestionPriority::Low => write!(f, "Low"),
        }
    }
}

// ── Public API ───────────────────────────────────────────────

/// Generate AI-powered suggestions from analysis results.
///
/// If the AI provider is not available, returns an error with the analysis context
/// that can be used directly by an AI assistant.
pub async fn suggest(
    config: &AiConfig,
    analysis: &AnalysisResult,
    project_name: &str,
) -> Result<SuggestionResult> {
    let context = prompt::build_prompt(analysis, project_name);

    match config.provider {
        AiProvider::Ollama => ollama::generate(&config.base_url, &config.model, &context).await,
    }
}

/// Build the analysis context string without calling the AI provider.
/// Useful as a fallback when the provider is unavailable.
pub fn build_context(analysis: &AnalysisResult, project_name: &str) -> String {
    prompt::build_prompt(analysis, project_name)
}

/// Load AI config from the global config directory.
/// Returns default config if file doesn't exist.
pub fn load_ai_config() -> Result<AiConfig> {
    let dir = crate::global_config::global_config_dir()?;
    let path = dir.join("ai.toml");
    if !path.exists() {
        return Ok(AiConfig::default());
    }
    let content = std::fs::read_to_string(&path)?;
    let config: AiConfig = toml::from_str(&content)
        .map_err(|e| VoidStackError::InvalidConfig(format!("ai.toml: {}", e)))?;
    Ok(config)
}

/// Save AI config to the global config directory.
pub fn save_ai_config(config: &AiConfig) -> Result<()> {
    let dir = crate::global_config::global_config_dir()?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    let path = dir.join("ai.toml");
    let content =
        toml::to_string_pretty(config).map_err(|e| VoidStackError::InvalidConfig(e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Parse suggestions from raw LLM response text.
pub(crate) fn parse_suggestions(raw: &str) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();
    let mut current: Option<PartialSuggestion> = None;

    for line in raw.lines() {
        let trimmed = line.trim();

        // Detect suggestion headers: "### 1." or "**1." or "1." patterns
        if is_suggestion_header(trimmed) {
            // Save previous if exists
            if let Some(partial) = current.take()
                && let Some(s) = partial.finalize()
            {
                suggestions.push(s);
            }
            let title = extract_title(trimmed);
            let category = detect_category(trimmed, &title);
            let priority = detect_priority(trimmed, &title);
            current = Some(PartialSuggestion {
                category,
                title,
                description: String::new(),
                affected_files: Vec::new(),
                priority,
            });
        } else if let Some(ref mut partial) = current {
            // Collect file paths mentioned
            let files = extract_file_paths(trimmed);
            partial.affected_files.extend(files);

            // Append to description
            if !trimmed.is_empty() {
                if !partial.description.is_empty() {
                    partial.description.push(' ');
                }
                partial.description.push_str(trimmed);
            }
        }
    }

    // Don't forget the last one
    if let Some(partial) = current.take()
        && let Some(s) = partial.finalize()
    {
        suggestions.push(s);
    }

    // If no structured suggestions found, create a single one from the whole text
    if suggestions.is_empty() && !raw.trim().is_empty() {
        suggestions.push(Suggestion {
            category: "general".to_string(),
            title: "Sugerencias del modelo".to_string(),
            description: raw.trim().to_string(),
            affected_files: extract_file_paths(raw),
            priority: SuggestionPriority::Medium,
        });
    }

    suggestions
}

struct PartialSuggestion {
    category: String,
    title: String,
    description: String,
    affected_files: Vec<String>,
    priority: SuggestionPriority,
}

impl PartialSuggestion {
    fn finalize(self) -> Option<Suggestion> {
        if self.title.is_empty() && self.description.is_empty() {
            return None;
        }
        // Deduplicate files
        let mut files = self.affected_files;
        files.sort();
        files.dedup();

        Some(Suggestion {
            category: self.category,
            title: self.title,
            description: self.description.trim().to_string(),
            affected_files: files,
            priority: self.priority,
        })
    }
}

fn is_suggestion_header(line: &str) -> bool {
    // Match: "### 1.", "**1.", "1.", "- **", "## "
    let stripped = line
        .trim_start_matches('#')
        .trim_start_matches('*')
        .trim_start_matches('-')
        .trim();
    if stripped.is_empty() {
        return false;
    }
    // Numbered items
    if stripped.starts_with(|c: char| c.is_ascii_digit())
        && let Some(rest) = stripped.strip_prefix(|c: char| c.is_ascii_digit())
    {
        return rest.starts_with('.') || rest.starts_with(')');
    }
    // Markdown headers with content
    line.starts_with("### ") || line.starts_with("## ")
}

fn extract_title(line: &str) -> String {
    let cleaned = line
        .trim_start_matches('#')
        .trim_start_matches('*')
        .trim_start_matches('-')
        .trim();
    // Remove leading number: "1. Title" -> "Title"
    if let Some(pos) = cleaned.find(['.', ')']) {
        let after = cleaned[pos + 1..].trim_start_matches('*').trim();
        if !after.is_empty() {
            return after.trim_end_matches('*').trim().to_string();
        }
    }
    cleaned.trim_end_matches('*').trim().to_string()
}

fn detect_category(line: &str, title: &str) -> String {
    let lower = format!("{} {}", line, title).to_lowercase();
    if lower.contains("seguridad") || lower.contains("security") || lower.contains("vulnerab") {
        "security".to_string()
    } else if lower.contains("rendimiento")
        || lower.contains("performance")
        || lower.contains("optimiz")
    {
        "performance".to_string()
    } else if lower.contains("arquitectura")
        || lower.contains("architecture")
        || lower.contains("patron")
        || lower.contains("pattern")
    {
        "architecture".to_string()
    } else {
        "refactoring".to_string()
    }
}

fn detect_priority(line: &str, title: &str) -> SuggestionPriority {
    let lower = format!("{} {}", line, title).to_lowercase();
    if lower.contains("critical") || lower.contains("critico") || lower.contains("urgente") {
        SuggestionPriority::Critical
    } else if lower.contains("alto") || lower.contains("high") || lower.contains("importante") {
        SuggestionPriority::High
    } else if lower.contains("bajo") || lower.contains("low") || lower.contains("menor") {
        SuggestionPriority::Low
    } else {
        SuggestionPriority::Medium
    }
}

fn extract_file_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    // Match patterns like: `path/to/file.ext`, path/to/file.ext:123
    let re = regex::Regex::new(r"(?:`([a-zA-Z0-9_./-]+\.[a-zA-Z0-9]+(?::\d+)?)`|(?:^|\s)((?:[a-zA-Z0-9_.-]+/)+[a-zA-Z0-9_.-]+\.[a-zA-Z0-9]+))")
        .unwrap();
    for cap in re.captures_iter(text) {
        let path = cap.get(1).or(cap.get(2)).map(|m| m.as_str().to_string());
        if let Some(p) = path {
            // Filter out obvious non-paths
            let p_clean = p.split(':').next().unwrap_or(&p).to_string();
            if !p_clean.contains("http") && !p_clean.contains("//") {
                paths.push(p_clean);
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_suggestions_numbered() {
        let raw = r#"
### 1. Dividir God Class en módulos
El archivo `src/server.rs` tiene demasiadas responsabilidades. Se debería separar en módulos.

### 2. Eliminar dependencia circular
`src/a.rs` y `src/b.rs` tienen una dependencia circular que se puede resolver.
"#;
        let suggestions = parse_suggestions(raw);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].title, "Dividir God Class en módulos");
        assert!(
            suggestions[0]
                .affected_files
                .contains(&"src/server.rs".to_string())
        );
        assert_eq!(suggestions[1].title, "Eliminar dependencia circular");
    }

    #[test]
    fn test_parse_suggestions_empty() {
        let suggestions = parse_suggestions("");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_parse_suggestions_unstructured() {
        let raw = "Deberías refactorizar el código para mejorar la legibilidad.";
        let suggestions = parse_suggestions(raw);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].category, "general");
    }

    #[test]
    fn test_detect_category() {
        assert_eq!(
            detect_category("security fix", "fix vulnerability"),
            "security"
        );
        assert_eq!(
            detect_category("refactor", "improve performance"),
            "performance"
        );
        assert_eq!(detect_category("", "cambiar arquitectura"), "architecture");
        assert_eq!(detect_category("", "dividir clase"), "refactoring");
    }

    #[test]
    fn test_extract_file_paths() {
        let text = "El archivo `src/main.rs` y `lib/utils.py:42` tienen problemas.";
        let paths = extract_file_paths(text);
        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(paths.contains(&"lib/utils.py".to_string()));
    }

    #[test]
    fn test_ai_config_default() {
        let config = AiConfig::default();
        assert_eq!(config.provider, AiProvider::Ollama);
        assert_eq!(config.model, "qwen2.5:7b");
        assert_eq!(config.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_ai_config_serialization() {
        let config = AiConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: AiConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.provider, config.provider);
        assert_eq!(loaded.model, config.model);
    }

    #[test]
    fn test_provider_name() {
        let config = AiConfig::default();
        assert_eq!(config.provider_name(), "Ollama");
    }

    #[test]
    fn test_suggestion_priority_display() {
        assert_eq!(format!("{}", SuggestionPriority::Critical), "Critical");
        assert_eq!(format!("{}", SuggestionPriority::High), "High");
        assert_eq!(format!("{}", SuggestionPriority::Medium), "Medium");
        assert_eq!(format!("{}", SuggestionPriority::Low), "Low");
    }

    #[test]
    fn test_is_suggestion_header() {
        assert!(is_suggestion_header("### 1. Title"));
        assert!(is_suggestion_header("## Section"));
        assert!(is_suggestion_header("1. Item"));
        assert!(is_suggestion_header("**1. Bold item"));
        assert!(!is_suggestion_header(""));
        assert!(!is_suggestion_header("Just text"));
        assert!(!is_suggestion_header("###"));
    }

    #[test]
    fn test_extract_title() {
        assert_eq!(extract_title("### 1. Dividir clase"), "Dividir clase");
        assert_eq!(extract_title("1. Simple"), "Simple");
        assert_eq!(extract_title("## Architecture"), "Architecture");
        assert_eq!(extract_title("**2. Bold title**"), "Bold title");
    }

    #[test]
    fn test_detect_priority() {
        assert_eq!(
            detect_priority("critical fix", "urgente"),
            SuggestionPriority::Critical
        );
        assert_eq!(
            detect_priority("", "high priority"),
            SuggestionPriority::High
        );
        assert_eq!(detect_priority("low priority", ""), SuggestionPriority::Low);
        assert_eq!(
            detect_priority("something", "else"),
            SuggestionPriority::Medium
        );
    }

    #[test]
    fn test_parse_suggestions_preserves_files() {
        let raw =
            "### 1. Fix security issue\nThe file `src/auth.rs` and `src/crypto.rs` need updates.";
        let suggestions = parse_suggestions(raw);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].category, "security");
        assert!(
            suggestions[0]
                .affected_files
                .contains(&"src/auth.rs".to_string())
        );
        assert!(
            suggestions[0]
                .affected_files
                .contains(&"src/crypto.rs".to_string())
        );
    }

    #[test]
    fn test_suggestion_result_serde() {
        let result = SuggestionResult {
            suggestions: vec![Suggestion {
                category: "refactoring".into(),
                title: "Test".into(),
                description: "A suggestion".into(),
                affected_files: vec!["main.rs".into()],
                priority: SuggestionPriority::Medium,
            }],
            model_used: "qwen2.5:7b".into(),
            raw_response: "raw".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: SuggestionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.suggestions.len(), 1);
        assert_eq!(back.model_used, "qwen2.5:7b");
    }

    #[test]
    fn test_extract_file_paths_no_urls() {
        let text = "Visit http://example.com/path/file.html for details.";
        let paths = extract_file_paths(text);
        // Should not extract URLs
        assert!(paths.is_empty() || !paths.iter().any(|p| p.contains("http")));
    }

    #[test]
    fn test_partial_suggestion_finalize_empty() {
        let partial = PartialSuggestion {
            category: "test".into(),
            title: String::new(),
            description: String::new(),
            affected_files: vec![],
            priority: SuggestionPriority::Medium,
        };
        assert!(partial.finalize().is_none());
    }

    #[test]
    fn test_partial_suggestion_finalize_deduplicates() {
        let partial = PartialSuggestion {
            category: "test".into(),
            title: "Test".into(),
            description: "desc".into(),
            affected_files: vec!["a.rs".into(), "b.rs".into(), "a.rs".into()],
            priority: SuggestionPriority::Medium,
        };
        let result = partial.finalize().unwrap();
        assert_eq!(result.affected_files.len(), 2);
    }
}
