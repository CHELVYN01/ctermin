//! AI settings in `config.toml`. The API key is not here: it lives in Credential Manager.

use serde::{Deserialize, Serialize};

/// The default model for the API key provider.
pub const DEFAULT_API_MODEL: &str = "claude-opus-5-5";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// The Claude Code installed on this computer, with the user's own login.
    #[default]
    ClaudeCode,
    /// The Claude API with an API key from the Console.
    ApiKey,
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct AiSettings {
    #[serde(default)]
    pub provider: ProviderKind,
    /// Empty means the provider's default model.
    #[serde(default)]
    pub model: String,
}

/// One model choice in the settings.
pub struct ModelOption {
    pub id: &'static str,
    pub label: &'static str,
}

/// Claude Code aliases. An empty id means the model Claude Code picks itself.
const CLAUDE_CODE_MODELS: [ModelOption; 5] = [
    ModelOption {
        id: "",
        label: "Claude Code default",
    },
    ModelOption {
        id: "fable",
        label: "Fable",
    },
    ModelOption {
        id: "opus",
        label: "Opus",
    },
    ModelOption {
        id: "sonnet",
        label: "Sonnet",
    },
    ModelOption {
        id: "haiku",
        label: "Haiku",
    },
];

/// Price per million tokens, input / output, as of October 2026.
const API_MODELS: [ModelOption; 4] = [
    ModelOption {
        id: "claude-fable-5-1",
        label: "Fable 5.1 ($10 / $50)",
    },
    ModelOption {
        id: DEFAULT_API_MODEL,
        label: "Opus 5.5 ($4 / $20)",
    },
    ModelOption {
        id: "claude-sonnet-5-5",
        label: "Sonnet 5.5 ($2 / $10)",
    },
    ModelOption {
        id: "claude-haiku-5-5",
        label: "Haiku 5.5 ($0.10 / $0.50)",
    },
];

pub fn model_options(provider: ProviderKind) -> &'static [ModelOption] {
    match provider {
        ProviderKind::ClaudeCode => &CLAUDE_CODE_MODELS,
        ProviderKind::ApiKey => &API_MODELS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::AppConfig;

    #[test]
    fn old_config_without_ai_section_still_loads() {
        let config: AppConfig = toml::from_str("").expect("an empty config is valid");
        assert_eq!(config.ai.provider, ProviderKind::ClaudeCode);
    }

    #[test]
    fn provider_is_saved_with_a_readable_name() {
        let mut config = AppConfig::default();
        config.ai.provider = ProviderKind::ApiKey;
        let text = toml::to_string(&config).expect("serializable");
        assert!(text.contains("provider = \"api-key\""));
        assert_eq!(toml::from_str::<AppConfig>(&text).ok(), Some(config));
    }
}
