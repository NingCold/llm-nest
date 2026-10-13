use crate::{Result, SocialError, SocialFeature};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use social::{ActorId, MessageBody, MessageId, MessageStatus, Revision, SocialContent};
/// Original bytes prepared by a permission-checked social use case.
pub struct DownloadFile {
    pub file_name: String,
    pub media_type: &'static str,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct FileInput {
    pub name: String,
    pub mime: String,
    pub data: String,
}
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_TOTAL_FILE_BYTES: usize = 20 * 1024 * 1024;
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 240
        && !name.ends_with([' ', '.'])
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        && !matches!(name, "." | "..")
        && !matches!(
            name.split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}
pub(crate) fn file_name(name: Option<&str>) -> String {
    name.filter(|n| valid_name(n))
        .unwrap_or("attachment.bin")
        .into()
}
fn valid_mime(mime: &str) -> bool {
    mime.len() <= 127
        && mime.split_once('/').is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && !b.contains('/')
                && mime
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/!#$&^_.+-".contains(&c))
        })
}
pub(crate) fn body(
    text: String,
    images: Vec<crate::ImageInput>,
    files: Vec<FileInput>,
) -> Result<MessageBody> {
    let mut body = crate::images::body(text, images)?;
    if files.len() > 4 {
        return Err(SocialError::Invalid("每条消息最多 4 个文件".into()));
    }
    let mut total = 0;
    for file in files {
        if !valid_name(&file.name) || !valid_mime(&file.mime) {
            return Err(SocialError::Invalid("文件名或文件类型无效".into()));
        }
        if file.data.len() > MAX_FILE_BYTES.div_ceil(3) * 4 {
            return Err(SocialError::Invalid("单个文件不超过 10 MiB".into()));
        }
        let data = STANDARD
            .decode(file.data)
            .map_err(|_| SocialError::Invalid("文件编码无效".into()))?;
        total += data.len();
        if data.len() > MAX_FILE_BYTES || total > MAX_TOTAL_FILE_BYTES {
            return Err(SocialError::Invalid(
                "单个文件不超过 10 MiB，合计不超过 20 MiB".into(),
            ));
        }
        body.0.push(SocialContent::File {
            name: Some(file.name),
            mime: file.mime,
            data,
        });
    }
    Ok(body)
}
impl SocialFeature {
    pub async fn message_file(
        &self,
        account: Option<ActorId>,
        id: MessageId,
        index: usize,
        revision: Revision,
    ) -> Result<DownloadFile> {
        let s = self.services()?;
        let m = match account {
            Some(account) => {
                s.resources
                    .store
                    .account_view_message(s.human, account, id)
                    .await?
            }
            None => s.resources.store.message(s.human, id).await?,
        };
        if m.revision != revision || m.status == MessageStatus::Withdrawn {
            return Err(SocialError::Invalid("消息已修改或撤回，请重新查看".into()));
        }
        let Some(SocialContent::File { name, data, .. }) = m.body.0.into_iter().nth(index) else {
            return Err(SocialError::Invalid("这条消息中没有该文件".into()));
        };
        Ok(DownloadFile {
            file_name: file_name(name.as_deref()),
            media_type: "application/octet-stream",
            data,
        })
    }
}

pub(crate) fn placeholder(name: Option<&str>, mime: &str, size: usize) -> String {
    format!(
        "[文件 · {} · {} · {} 字节；文件内容尚未读取]",
        file_name(name),
        mime,
        size
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input(name: &str, data: &[u8]) -> FileInput {
        FileInput {
            name: name.into(),
            mime: "application/octet-stream".into(),
            data: STANDARD.encode(data),
        }
    }
    #[test]
    fn preserves_binary_and_empty_files_and_legacy_names() {
        for bytes in [vec![], vec![0, 255, 128, 42]] {
            let body = body(String::new(), vec![], vec![input("中文 🐈.bin", &bytes)]).unwrap();
            assert!(!body.is_empty());
            let encoded = serde_json::to_vec(&body).unwrap();
            assert_eq!(
                serde_json::from_slice::<MessageBody>(&encoded).unwrap(),
                body
            );
            assert!(
                matches!(&body.0[0], SocialContent::File {data, name: Some(name), ..} if data == &bytes && name == "中文 🐈.bin")
            );
        }
        let legacy: SocialContent =
            serde_json::from_str(r#"{"kind":"file","mime":"text/plain","data":[0,255]}"#).unwrap();
        assert!(!serde_json::to_string(&legacy).unwrap().contains("name"));
        assert_eq!(file_name(None), "attachment.bin");
    }
    #[test]
    fn rejects_external_metadata_encoding_and_limits() {
        for name in [
            "",
            "../a",
            "C:\\a",
            "https://a",
            "bad\nname",
            "CON.txt",
            "a.",
            "a ",
        ] {
            assert!(
                body("".into(), vec![], vec![input(name, b"a")]).is_err(),
                "{name:?}"
            );
        }
        let mut file = input("ok.bin", b"a");
        file.mime = "text/html\r\nX-Evil: 1".into();
        assert!(body("".into(), vec![], vec![file]).is_err());
        let mut file = input("ok.bin", b"a");
        file.data = "%%%".into();
        assert!(body("".into(), vec![], vec![file]).is_err());
        assert!(body("".into(), vec![], vec![input("ok.bin", b""); 5]).is_err());
        assert!(
            body(
                "".into(),
                vec![],
                vec![input("ok.bin", &vec![0; MAX_FILE_BYTES + 1])]
            )
            .is_err()
        );
        assert!(
            body(
                "".into(),
                vec![],
                vec![input("ok.bin", &vec![0; 6 * 1024 * 1024]); 4]
            )
            .is_err()
        );
        assert!(
            body(
                "".into(),
                vec![],
                vec![input("ok.bin", &vec![0; MAX_FILE_BYTES]); 2]
            )
            .is_ok()
        );
    }
}
