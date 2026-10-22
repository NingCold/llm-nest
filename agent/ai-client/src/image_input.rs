//! Declared image-input support. Unknown is distinct from an explicit denial.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageSupport {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilitySource {
    Config,
    Catalog,
    Builtin,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInputCapability {
    pub support: ImageSupport,
    pub source: CapabilitySource,
}

impl ImageInputCapability {
    pub fn resolve(config: Option<bool>, catalog: Option<bool>, builtin: Option<bool>) -> Self {
        for (value, source) in [
            (config, CapabilitySource::Config),
            (catalog, CapabilitySource::Catalog),
            (builtin, CapabilitySource::Builtin),
        ] {
            if let Some(value) = value {
                return Self {
                    support: if value {
                        ImageSupport::Supported
                    } else {
                        ImageSupport::Unsupported
                    },
                    source,
                };
            }
        }
        Self::default()
    }
}

/// Small offline baseline; exact route/model matches only. Other entries are unknown.
/// Source: https://pi.dev/api/models/providers/anthropic (checked 2026-10-13).
pub fn builtin_image_input(provider: &str, model: &str) -> Option<bool> {
    match (provider, model) {
        (
            "anthropic",
            "claude-haiku-4-5" | "claude-opus-4-5" | "claude-opus-4-6" | "claude-sonnet-4-5"
            | "claude-sonnet-5",
        ) => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_unknown_do_not_guess_from_names() {
        assert_eq!(
            ImageInputCapability::resolve(Some(false), Some(true), Some(true)),
            ImageInputCapability {
                support: ImageSupport::Unsupported,
                source: CapabilitySource::Config
            }
        );
        assert_eq!(
            ImageInputCapability::resolve(None, Some(false), Some(true)).source,
            CapabilitySource::Catalog
        );
        assert_eq!(
            ImageInputCapability::resolve(None, None, Some(true)).source,
            CapabilitySource::Builtin
        );
        assert_eq!(
            ImageInputCapability::resolve(None, None, None).support,
            ImageSupport::Unknown
        );
        assert_eq!(builtin_image_input("custom", "claude-haiku-4-5"), None);
        assert_eq!(builtin_image_input("custom", "vision-vl"), None);
    }
}
