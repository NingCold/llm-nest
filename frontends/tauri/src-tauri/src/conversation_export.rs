//! The destination is chosen by the native dialog, never supplied by the WebView.
use crate::{BackendState, social_commands::feature};
use serde::Serialize;
use social::ConversationId;
use social_feature::{ExportFormat, export_file_name, wire};
use std::{io::Write, path::Path};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ExportResult {
    Cancelled,
    Saved {
        #[serde(rename = "fileName")]
        file_name: String,
        #[serde(rename = "messageCount")]
        message_count: usize,
    },
}

fn save_error(error: impl std::fmt::Display) -> wire::ErrorBody {
    wire::ErrorBody {
        code: "export_failed",
        message: format!("导出失败：{error}"),
        retryable: true,
    }
}

#[tauri::command]
pub async fn social_export_conversation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    format: ExportFormat,
) -> Result<ExportResult, wire::ErrorBody> {
    let conversation = feature(state.clone())
        .await?
        .conversation(id)
        .await
        .map_err(|e| e.wire())?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("导出聊天记录")
        .add_filter(
            match format {
                ExportFormat::Markdown => "Markdown",
                ExportFormat::Json => "JSON",
            },
            &[format.extension()],
        )
        .set_file_name(export_file_name(&conversation.title, id, format))
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(destination) = rx.await.map_err(save_error)? else {
        return Ok(ExportResult::Cancelled);
    };
    let path = destination.into_path().map_err(save_error)?;
    // Recheck the current operator's access after the dialog has closed.
    let file = feature(state)
        .await?
        .export_conversation(id, format)
        .await
        .map_err(|e| e.wire())?;
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let message_count = file.message_count;
    tokio::task::spawn_blocking(move || write_export(&path, &file.content))
        .await
        .map_err(save_error)?
        .map_err(save_error)?;
    Ok(ExportResult::Saved {
        file_name,
        message_count,
    })
}

fn write_export(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "保存位置没有父目录")
    })?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(content.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_utf8_and_replaces_only_after_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("中文聊天.md");
        write_export(&path, "旧内容").unwrap();
        write_export(&path, "中文 🐈\n第二行\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "中文 🐈\n第二行\n");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn failed_replacement_preserves_destination_and_cleans_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep.txt"), "保留").unwrap();
        assert!(write_export(&path, "export").is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("keep.txt")).unwrap(),
            "保留"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
