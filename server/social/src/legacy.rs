//! Explicit historical import, using the current operator and guarded data directory.
use crate::{Result, SocialError, SocialFeature};
use storage::social::{LegacyImportResult, LegacyPreview};

impl SocialFeature {
    pub async fn preview_legacy(&self) -> Result<LegacyPreview> {
        self.services()?
            .resources
            .store
            .preview_legacy()
            .await
            .map_err(import_error)
    }

    pub async fn import_legacy(&self, preview: LegacyPreview) -> Result<LegacyImportResult> {
        let services = self.services()?;
        services
            .resources
            .store
            .import_legacy(services.human, preview)
            .await
            .map_err(import_error)
    }
}

fn import_error(error: storage::social::SocialStorageError) -> SocialError {
    match error {
        storage::social::SocialStorageError::Legacy { .. } => {
            SocialError::Invalid(error.to_string())
        }
        other => other.into(),
    }
}
