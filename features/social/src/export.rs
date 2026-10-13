use crate::{DownloadFile, Result, SocialError, SocialFeature};
use serde::{Deserialize, Serialize};
use social::{ConversationId, MessageStatus};
use std::io::{Cursor, Write};
use storage::social::{ConversationTranscript, TranscriptPart};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Markdown,
    Json,
}
impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationExport {
    pub file_name: String,
    pub media_type: &'static str,
    pub message_count: usize,
    pub content: String,
}

pub struct ConversationArchive {
    pub file: DownloadFile,
    pub message_count: usize,
}

pub fn export_file_name(title: &str, id: ConversationId, format: ExportFormat) -> String {
    let mut bytes = 0;
    let title: String = title
        .chars()
        .filter(|c| !c.is_control() && !"/\\:*?\"<>|".contains(*c))
        .take(60)
        .take_while(|c| {
            bytes += c.len_utf8();
            bytes <= 160
        })
        .collect();
    let title = title.trim().trim_matches('.');
    format!("LLM-Nest-{title}-{id}.{}", format.extension())
}

impl SocialFeature {
    pub async fn export_conversation(
        &self,
        id: ConversationId,
        format: ExportFormat,
    ) -> Result<ConversationExport> {
        let s = self.services()?;
        let snapshot = s
            .resources
            .store
            .conversation_transcript(s.human, id, false)
            .await?;
        render_export(snapshot, format)
    }

    pub async fn export_conversation_archive(
        &self,
        id: ConversationId,
        format: ExportFormat,
    ) -> Result<ConversationArchive> {
        let s = self.services()?;
        let snapshot = s
            .resources
            .store
            .conversation_transcript(s.human, id, true)
            .await?;
        tokio::task::spawn_blocking(move || render_archive(snapshot, format))
            .await
            .map_err(|e| SocialError::FileExport(e.to_string()))?
    }
}

fn render_archive(
    mut snapshot: ConversationTranscript,
    format: ExportFormat,
) -> Result<ConversationArchive> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let error = |e: zip::result::ZipError| SocialError::FileExport(e.to_string());
    let io_error = |e: std::io::Error| SocialError::FileExport(e.to_string());
    for message in &mut snapshot.messages {
        for (index, part) in message.parts.iter_mut().enumerate() {
            if let TranscriptPart::Image {
                data, file_path, ..
            } = part
            {
                let file = crate::images::image_file(
                    message.id,
                    index,
                    data.take().expect("image snapshot includes bytes"),
                )?;
                let path = format!("images/{}", file.file_name);
                archive.start_file(&path, options).map_err(error)?;
                archive.write_all(&file.data).map_err(io_error)?;
                *file_path = Some(path);
            }
        }
    }
    let document = render_document(&snapshot, format, true)?;
    archive
        .start_file(format!("chat.{}", format.extension()), options)
        .map_err(error)?;
    archive
        .write_all(document.content.as_bytes())
        .map_err(io_error)?;
    let data = archive.finish().map_err(error)?.into_inner();
    let file_name = std::path::Path::new(&document.file_name)
        .with_extension("zip")
        .to_string_lossy()
        .into_owned();
    Ok(ConversationArchive {
        file: DownloadFile {
            file_name,
            media_type: "application/zip",
            data,
        },
        message_count: document.message_count,
    })
}

fn render_export(
    snapshot: ConversationTranscript,
    format: ExportFormat,
) -> Result<ConversationExport> {
    render_document(&snapshot, format, false)
}

fn render_document(
    snapshot: &ConversationTranscript,
    format: ExportFormat,
    include_images: bool,
) -> Result<ConversationExport> {
    let file_name = export_file_name(&snapshot.title, snapshot.conversation_id, format);
    let message_count = snapshot.messages.len();
    let (media_type, content) = match format {
        ExportFormat::Json => {
            #[derive(Serialize)]
            struct Document<'a> {
                format: &'static str,
                version: u32,
                #[serde(flatten)]
                transcript: &'a ConversationTranscript,
            }
            let content = serde_json::to_string_pretty(&Document {
                format: "llm-nest.conversation",
                version: if include_images { 2 } else { 1 },
                transcript: snapshot,
            })
            .map_err(storage::social::SocialStorageError::from)?;
            ("application/json;charset=utf-8", format!("{content}\n"))
        }
        ExportFormat::Markdown => (
            "text/markdown;charset=utf-8",
            markdown(snapshot, include_images),
        ),
    };
    Ok(ConversationExport {
        file_name,
        media_type,
        message_count,
        content,
    })
}

fn heading(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_control() {
            out.push(' ');
        } else if c == '&' {
            out.push_str("&amp;");
        } else if c == '<' {
            out.push_str("&lt;");
        } else {
            if "\\`*_{}[]()#+-.!|>".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}

fn markdown(s: &ConversationTranscript, include_images: bool) -> String {
    let mut out = format!(
        "# {}\n\n导出时间：{}\n\n聊天 ID：{}\n\n消息数：{}\n\n正文按原文文本保存；{}生成中的消息只包含此时已保存的片段。\n",
        heading(&s.title),
        s.captured_at.to_rfc3339(),
        s.conversation_id,
        s.messages.len(),
        if include_images {
            "图片位于 images 文件夹，其他附件仅含占位说明。"
        } else {
            "附件仅含占位说明。"
        }
    );
    if s.messages.is_empty() {
        out.push_str("\n暂无消息。\n");
    }
    for m in &s.messages {
        let status = match m.status {
            MessageStatus::Committed => "已发送",
            MessageStatus::Generating => "生成中",
            MessageStatus::Interrupted => "已中断",
            MessageStatus::Withdrawn => "已撤回",
        };
        out.push_str(&format!("\n---\n\n## {} · {}\n\n状态：{} · 序号：{} · 修订：{}\n\n消息 ID：{}\n\n发言账号：{}\n", heading(&m.sender_name), m.created_at.to_rfc3339(), status, m.sequence, m.revision, m.id, m.sender_id));
        if let Some(id) = m.reply_to {
            out.push_str(&format!("\n回复消息：{id}\n"));
        }
        if let Some(reference) = &m.context_reference {
            out.push_str(&format!(
                "\n群话题引用：{}（{}，不附带群聊正文）\n",
                reference.selection.conversation_id,
                if reference.stale {
                    "已失效"
                } else {
                    "来源未展开"
                }
            ));
        }
        if !m.mentions.is_empty() {
            out.push_str(&format!(
                "\n提及账号：{}\n",
                m.mentions
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("、")
            ));
        }
        if m.status == MessageStatus::Withdrawn {
            out.push_str("\n消息已撤回。\n");
        }
        for part in &m.parts {
            match part {
                TranscriptPart::Text { text } => {
                    // A message containing Markdown/HTML cannot forge another speaker's heading.
                    let fence = "`".repeat(
                        text.split(|c| c != '`')
                            .map(str::len)
                            .max()
                            .unwrap_or(0)
                            .max(2)
                            + 1,
                    );
                    out.push_str(&format!("\n{fence}text\n{text}\n{fence}\n"));
                }
                TranscriptPart::Image {
                    file_path: Some(path),
                    ..
                } => out.push_str(&format!("\n![图片]({path})\n")),
                TranscriptPart::Image {
                    mime, byte_length, ..
                } => out.push_str(&format!(
                    "\n[图片未附带 · {} · {} 字节]\n",
                    heading(mime.as_deref().unwrap_or("未知类型")),
                    byte_length
                )),
                TranscriptPart::File { mime, byte_length } => out.push_str(&format!(
                    "\n[文件未附带 · {} · {} 字节]\n",
                    heading(mime),
                    byte_length
                )),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use social::*;
    use storage::social::TranscriptMessage;
    fn snapshot() -> ConversationTranscript {
        ConversationTranscript {
            captured_at: chrono::Utc::now(),
            conversation_id: ConversationId::new(),
            title: "中文 <script> & # 标题\n下一行".into(),
            kind: ConversationKind::Group,
            legacy: false,
            messages: vec![TranscriptMessage {
                id: MessageId::new(),
                sequence: Sequence::new(9_007_199_254_740_993).unwrap(),
                sender_id: ActorId::new(),
                sender_name: "[冒名](https://example.invalid)\n## 标题".into(),
                created_at: chrono::Utc::now(),
                revision: Revision::ONE,
                status: MessageStatus::Committed,
                reply_to: None,
                mentions: vec![],
                context_reference: None,
                parts: vec![
                    TranscriptPart::Text {
                        text: "中文🙂\n````\n## 另一个发言人\n<script>alert(1)</script>\n".into(),
                    },
                    TranscriptPart::File {
                        mime: "application/pdf".into(),
                        byte_length: 5,
                    },
                ],
            }],
        }
    }
    #[test]
    fn json_keeps_unicode_metadata_and_lossless_counters() {
        let s = snapshot();
        let f = render_export(s, ExportFormat::Json).unwrap();
        let json: serde_json::Value = serde_json::from_str(&f.content).unwrap();
        assert_eq!(json["format"], "llm-nest.conversation");
        assert_eq!(json["version"], 1);
        assert_eq!(json["messages"][0]["sequence"], "9007199254740993");
        assert_eq!(
            json["messages"][0]["parts"][0]["text"],
            snapshot().messages[0]
                .parts
                .iter()
                .find_map(|p| if let TranscriptPart::Text { text } = p {
                    Some(text.clone())
                } else {
                    None
                })
                .unwrap()
        );
        assert_eq!(json["messages"][0]["parts"][1]["byteLength"], 5);
        assert_eq!(f.message_count, 1);
        assert!(f.file_name.ends_with(".json"));
    }
    #[test]
    fn markdown_separates_speaker_headings_from_literal_message_content() {
        let f = render_export(snapshot(), ExportFormat::Markdown).unwrap();
        assert!(
            f.content
                .starts_with("# 中文 &lt;script\\> &amp; \\# 标题 下一行\n")
        );
        assert!(
            f.content
                .contains("## \\[冒名\\]\\(https://example\\.invalid\\) \\#\\# 标题")
        );
        assert!(
            f.content
                .contains("`````text\n中文🙂\n````\n## 另一个发言人")
        );
        assert!(f.content.contains("</script>\n\n`````\n"));
        assert!(f.content.contains("文件未附带"));
        assert!(f.content.contains("状态：已发送"));
    }
    #[test]
    fn empty_conversation_and_all_message_states_are_readable() {
        let mut s = snapshot();
        for (state, label) in [
            (MessageStatus::Generating, "生成中"),
            (MessageStatus::Interrupted, "已中断"),
            (MessageStatus::Withdrawn, "已撤回"),
        ] {
            s.messages[0].status = state;
            if state == MessageStatus::Withdrawn {
                s.messages[0].parts.clear();
            }
            assert!(markdown(&s, false).contains(&format!("状态：{label}")));
        }
        s.messages.clear();
        assert!(markdown(&s, false).contains("暂无消息。"));
        assert!(serde_json::from_str::<ExportFormat>("\"pdf\"").is_err());
    }
    #[test]
    fn suggested_file_names_are_portable_single_components() {
        for title in ["../CON\\foo:bar?\"<>|\r\n", "...", &"🙂".repeat(100)] {
            let filename = export_file_name(title, ConversationId::new(), ExportFormat::Markdown);
            assert!(filename.len() < 255);
            assert!(
                filename
                    .chars()
                    .all(|c| !c.is_control() && !"/\\:*?\"<>|".contains(c))
            );
            assert!(filename.starts_with("LLM-Nest-"));
            assert!(filename.ends_with(".md"));
        }
    }

    #[test]
    fn archives_link_original_images_with_safe_paths_and_never_embed_binary_in_json() {
        use std::io::Read;
        for format in [ExportFormat::Markdown, ExportFormat::Json] {
            let mut s = snapshot();
            let id = s.messages[0].id;
            let mut encoded = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(2, 3)
                .write_to(&mut encoded, image::ImageFormat::Png)
                .unwrap();
            let data = encoded.into_inner();
            for _ in 0..2 {
                s.messages[0].parts.push(TranscriptPart::Image {
                    mime: Some("image/png".into()),
                    byte_length: data.len(),
                    data: Some(data.clone()),
                    file_path: None,
                });
            }
            let result = render_archive(s, format).unwrap();
            assert!(result.file.file_name.ends_with(".zip"));
            assert_eq!(result.message_count, 1);
            let mut zip = zip::ZipArchive::new(Cursor::new(result.file.data)).unwrap();
            assert_eq!(zip.len(), 3);
            for index in [2, 3] {
                let mut raw = Vec::new();
                let mut file = zip.by_name(&format!("images/{id}-{index}.png")).unwrap();
                assert!(file.enclosed_name().is_some());
                file.read_to_end(&mut raw).unwrap();
                assert_eq!(raw, data);
            }
            let mut text = String::new();
            zip.by_name(&format!("chat.{}", format.extension()))
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            match format {
                ExportFormat::Json => {
                    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(document["version"], 2);
                    assert_eq!(
                        document["messages"][0]["parts"][2]["filePath"],
                        format!("images/{id}-2.png")
                    );
                    assert!(document["messages"][0]["parts"][2].get("data").is_none());
                }
                ExportFormat::Markdown => {
                    assert!(text.contains(&format!("![图片](images/{id}-2.png)")))
                }
            }
        }
    }

    #[test]
    fn invalid_imported_images_fail_the_archive_instead_of_silently_dropping_bytes() {
        let mut s = snapshot();
        s.messages[0].parts.push(TranscriptPart::Image {
            mime: Some("image/png".into()),
            byte_length: 4,
            data: Some(b"oops".to_vec()),
            file_path: None,
        });
        assert!(render_archive(s, ExportFormat::Json).is_err());
    }
}
