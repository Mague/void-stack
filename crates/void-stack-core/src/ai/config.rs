//! AI provider configuration: provider selection, model, base URL and
//! persistence to `ai.toml` in the global config directory.

use serde::{Deserialize, Serialize};

use crate::error::{Result, VoidStackError};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
