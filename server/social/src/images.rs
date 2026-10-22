//! Images enter through the same send transaction as text. Reads are versioned and scoped.
use crate::{DownloadFile, Result, SocialError, SocialFeature};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use social::{MessageBody, MessageId, MessageStatus, Revision, SocialContent};
use std::io::Cursor;

pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGES: usize = 4;

#[derive(Debug, Clone, Deserialize)]
pub struct ImageInput {
    pub mime: String,
    pub data: String,
}
#[derive(Debug, Serialize)]
pub struct ImagePayload {
    pub mime: String,
    pub data: String,
}

fn invalid() -> SocialError {
    SocialError::Invalid(
        "图片无法读取；请选择有效的 PNG、JPEG 或 WebP，单张不超过 4 MiB，边长不超过 4096 像素"
            .into(),
    )
}

fn reader(data: &[u8]) -> Result<image::ImageReader<Cursor<&[u8]>>> {
    let mut reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|_| invalid())?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
    ) {
        return Err(invalid());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

pub(crate) fn body(text: String, images: Vec<ImageInput>) -> Result<MessageBody> {
    if images.len() > MAX_IMAGES || text.len() > 80_000 {
        return Err(SocialError::Invalid(
            "每条消息最多 4 张图片，文字不超过 20000 个汉字".into(),
        ));
    }
    let mut parts = vec![];
    if !text.is_empty() {
        parts.push(SocialContent::Text { text });
    }
    let mut total = 0;
    for input in images {
        if input.data.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err(invalid());
        }
        let data = STANDARD.decode(&input.data).map_err(|_| invalid())?;
        if data.is_empty() || data.len() > MAX_IMAGE_BYTES {
            return Err(invalid());
        }
        total += data.len();
        if total > MAX_TOTAL_BYTES {
            return Err(SocialError::Invalid(
                "每条消息的图片合计不超过 8 MiB".into(),
            ));
        }
        let reader = reader(&data)?;
        let mime = reader.format().expect("checked format").to_mime_type();
        if input.mime != mime {
            return Err(SocialError::Invalid("图片类型与文件内容不一致".into()));
        }
        reader.decode().map_err(|_| invalid())?;
        parts.push(SocialContent::Image {
            mime: Some(input.mime),
            data,
        });
    }
    Ok(MessageBody(parts))
}

impl SocialFeature {
    pub async fn image_file(
        &self,
        account: Option<social::ActorId>,
        id: MessageId,
        index: usize,
        revision: Revision,
    ) -> Result<DownloadFile> {
        let s = self.services()?;
        let message = match account {
            Some(account) => {
                s.resources
                    .store
                    .account_view_message(s.human, account, id)
                    .await?
            }
            None => s.resources.store.message(s.human, id).await?,
        };
        original(message, index, revision)
    }
    pub async fn account_view_image(
        &self,
        account: social::ActorId,
        id: MessageId,
        index: usize,
        revision: Revision,
    ) -> Result<ImagePayload> {
        let s = self.services()?;
        payload(
            s.resources
                .store
                .account_view_message(s.human, account, id)
                .await?,
            index,
            revision,
        )
    }
    pub async fn message_image(
        &self,
        id: MessageId,
        index: usize,
        revision: Revision,
    ) -> Result<ImagePayload> {
        let s = self.services()?;
        payload(
            s.resources.store.message(s.human, id).await?,
            index,
            revision,
        )
    }
}
fn payload(m: social::SocialMessage, index: usize, revision: Revision) -> Result<ImagePayload> {
    let file = original(m, index, revision)?;
    Ok(ImagePayload {
        mime: file.media_type.into(),
        data: STANDARD.encode(file.data),
    })
}

fn original(m: social::SocialMessage, index: usize, revision: Revision) -> Result<DownloadFile> {
    if m.revision != revision || m.status == MessageStatus::Withdrawn {
        return Err(SocialError::Invalid("消息已修改或撤回，请重新查看".into()));
    }
    let Some(SocialContent::Image { data, .. }) = m.body.0.into_iter().nth(index) else {
        return Err(SocialError::Invalid("这条消息中没有该图片".into()));
    };
    image_file(m.id, index, data)
}

pub(crate) fn image_file(id: MessageId, index: usize, data: Vec<u8>) -> Result<DownloadFile> {
    let format = reader(&data)?.format().expect("checked format");
    Ok(DownloadFile {
        file_name: format!("{id}-{index}.{}", format.extensions_str()[0]),
        media_type: format.to_mime_type(),
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(format: image::ImageFormat, width: u32) -> ImageInput {
        let mut data = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(width, 2)
            .write_to(&mut data, format)
            .unwrap();
        ImageInput {
            mime: format.to_mime_type().into(),
            data: STANDARD.encode(data.into_inner()),
        }
    }
    #[test]
    fn validated_images_keep_original_bytes_and_allow_image_only() {
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::WebP,
        ] {
            let image = input(format, 2);
            let result = body(String::new(), vec![image.clone()]).unwrap();
            assert!(!result.is_empty());
            let SocialContent::Image { data, mime } = &result.0[0] else {
                panic!("image")
            };
            assert_eq!(STANDARD.encode(data), image.data);
            assert_eq!(mime.as_ref(), Some(&image.mime));
            assert_eq!(builtin_agent::attachments::image_cost(data), 1025);
            let file = image_file(MessageId::new(), 0, data.clone()).unwrap();
            assert_eq!(&file.data, data);
            assert_eq!(file.media_type, image.mime);
            assert!(file.file_name.ends_with(format.extensions_str()[0]));
        }
    }
    #[test]
    fn rejects_mismatched_corrupt_oversized_and_excessive_images() {
        let png = input(image::ImageFormat::Png, 2);
        assert!(body("".into(), vec![png.clone(); 5]).is_err());
        assert!(
            body(
                "".into(),
                vec![ImageInput {
                    mime: "image/jpeg".into(),
                    ..png.clone()
                }]
            )
            .is_err()
        );
        assert!(
            body(
                "".into(),
                vec![ImageInput {
                    mime: "image/png".into(),
                    data: "%%%".into()
                }]
            )
            .is_err()
        );
        assert!(
            body(
                "".into(),
                vec![ImageInput {
                    mime: "image/png".into(),
                    data: STANDARD.encode(b"<svg></svg>")
                }]
            )
            .is_err()
        );
        assert!(
            body(
                "".into(),
                vec![ImageInput {
                    mime: "image/png".into(),
                    data: "A".repeat(MAX_IMAGE_BYTES * 2)
                }]
            )
            .is_err()
        );
        assert!(body("".into(), vec![input(image::ImageFormat::Png, 4097)]).is_err());
        let mut truncated = STANDARD.decode(&png.data).unwrap();
        truncated.truncate(truncated.len() / 2);
        assert!(
            body(
                "".into(),
                vec![ImageInput {
                    data: STANDARD.encode(truncated),
                    ..png
                }]
            )
            .is_err()
        );
    }
}
