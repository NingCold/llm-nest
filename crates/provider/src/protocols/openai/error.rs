use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub message: String,

    #[serde(rename = "type")]
    pub kind: String,

    pub code: Option<String>,
}