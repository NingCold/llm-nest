use std::io::Cursor;

/// Estimate image context cost; imported history can predate image validation.
pub fn image_cost(data: &[u8]) -> u32 {
    let dimensions = (|| {
        let mut reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .ok()?;
        if !matches!(
            reader.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
        ) {
            return None;
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        reader.into_dimensions().ok()
    })();
    match dimensions {
        Some((w, h)) => 1024 + w.div_ceil(28).saturating_mul(h.div_ceil(28)),
        None => 4096,
    }
}
