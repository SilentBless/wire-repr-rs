use serde::Serialize;
use thiserror::Error;

use super::Report;

#[derive(Debug, Error)]
pub enum JsonError {
    #[error("failed to serialize measurement report: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Serialize)]
struct Envelope<'report> {
    schema: u32,
    #[serde(flatten)]
    report: &'report Report,
}

pub fn render_json(report: &Report) -> Result<String, JsonError> {
    serde_json::to_string_pretty(&Envelope { schema: 1, report }).map_err(Into::into)
}
