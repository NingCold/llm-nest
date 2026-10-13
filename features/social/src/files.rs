/// Original bytes prepared by a permission-checked social use case.
pub struct DownloadFile {
    pub file_name: String,
    pub media_type: &'static str,
    pub data: Vec<u8>,
}
