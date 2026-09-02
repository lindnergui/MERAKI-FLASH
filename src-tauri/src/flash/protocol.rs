use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFlashRequest {
    pub iso_path: String,
    pub device_id: String,
    pub image_kind: ImageKind,
    pub unattend_xml_content: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    Linux,
    Windows,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFlashResponse {
    pub operation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevatedFlashRequest {
    pub operation_id: String,
    pub token: String,
    pub callback_port: u16,
    pub iso_path: String,
    pub iso_size: u64,
    pub device_id: String,
    pub device_path: String,
    pub device_size: u64,
    pub device_serial: Option<String>,
    pub image_kind: ImageKind,
    pub unattend_xml_content: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlashPhase {
    Preparing,
    Analyzing,
    Splitting,
    Formatting,
    Extracting,
    Writing,
    Syncing,
    Done,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashProgress {
    pub operation_id: String,
    pub phase: FlashPhase,
    pub percentage: f64,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<u64>,
    pub message: Option<String>,
}

impl FlashProgress {
    pub fn new(
        operation_id: impl Into<String>,
        phase: FlashPhase,
        percentage: f64,
        bytes_per_second: f64,
        eta_seconds: Option<u64>,
        message: impl Into<Option<String>>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            phase,
            percentage: percentage.clamp(0.0, 100.0),
            bytes_per_second,
            eta_seconds,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperEnvelope {
    pub token: String,
    pub progress: FlashProgress,
}
