//! The destination is chosen by the native dialog, never supplied by the WebView.
use crate::{BackendState, social_commands::feature};
use serde::Serialize;
use social::{ActorId, ConversationId, MessageId, Revision};
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
        message: format!("保存失败：{error}"),
        retryable: true,
    }
}

#[tauri::command]
pub async fn social_export_conversation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    format: ExportFormat,
    include_images: bool,
) -> Result<ExportResult, wire::ErrorBody> {
    let conversation = feature(state.clone())
        .await?
        .conversation(id)
        .await
        .map_err(|e| e.wire())?;
    let mut name = std::path::PathBuf::from(export_file_name(&conversation.title, id, format));
    if include_images {
        name.set_extension("zip");
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("导出聊天记录")
        .add_filter(
            if include_images {
                "ZIP（聊天记录与图片）"
            } else {
                match format {
                    ExportFormat::Markdown => "Markdown",
                    ExportFormat::Json => "JSON",
                }
            },
            &[if include_images {
                "zip"
            } else {
                format.extension()
            }],
        )
        .set_file_name(name.to_string_lossy())
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(destination) = rx.await.map_err(save_error)? else {
        return Ok(ExportResult::Cancelled);
    };
    let path = destination.into_path().map_err(save_error)?;
    // Recheck the current operator's access after the dialog has closed.
    let social = feature(state).await?;
    let (content, message_count) = if include_images {
        let archive = social
            .export_conversation_archive(id, format)
            .await
            .map_err(|e| e.wire())?;
        (archive.file.data, archive.message_count)
    } else {
        let file = social
            .export_conversation(id, format)
            .await
            .map_err(|e| e.wire())?;
        (file.content.into_bytes(), file.message_count)
    };
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    tokio::task::spawn_blocking(move || write_export(&path, &content))
        .await
        .map_err(save_error)?
        .map_err(save_error)?;
    Ok(ExportResult::Saved {
        file_name,
        message_count,
    })
}

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ImageSaveResult {
    Cancelled,
    Saved {
        #[serde(rename = "fileName")]
        file_name: String,
    },
}

#[tauri::command]
pub async fn social_save_image(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, BackendState>,
    account: Option<ActorId>,
    id: MessageId,
    index: usize,
    revision: Revision,
) -> Result<ImageSaveResult, wire::ErrorBody> {
    let file = feature(state.clone())
        .await?
        .image_file(account, id, index, revision)
        .await
        .map_err(|e| e.wire())?;
    let extension = std::path::Path::new(&file.file_name)
        .extension()
        .expect("image extension")
        .to_string_lossy()
        .into_owned();
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("保存原图")
        .add_filter("原始图片", &[&extension])
        .set_file_name(&file.file_name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    drop(file);
    let Some(destination) = rx.await.map_err(save_error)? else {
        return Ok(ImageSaveResult::Cancelled);
    };
    let path = destination.into_path().map_err(save_error)?;
    // Never save the preview or the pre-dialog bytes: permissions and revision may have changed.
    let file = feature(state)
        .await?
        .image_file(account, id, index, revision)
        .await
        .map_err(|e| e.wire())?;
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    tokio::task::spawn_blocking(move || write_export(&path, &file.data))
        .await
        .map_err(save_error)?
        .map_err(save_error)?;
    Ok(ImageSaveResult::Saved { file_name })
}

fn write_export(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "保存位置没有父目录")
    })?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(content)?;
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
        write_export(&path, "旧内容".as_bytes()).unwrap();
        write_export(&path, "中文 🐈\n第二行\n".as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "中文 🐈\n第二行\n");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn binary_save_preserves_all_bytes_and_reports_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("原图.png");
        let bytes: Vec<u8> = (0..=255).collect();
        write_export(&path, &bytes).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(write_export(&dir.path().join("missing/file.zip"), &bytes).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn failed_replacement_preserves_destination_and_cleans_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep.txt"), "保留").unwrap();
        assert!(write_export(&path, b"export").is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("keep.txt")).unwrap(),
            "保留"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
