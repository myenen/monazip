use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub block_size: usize,
    pub auto_block_size: bool,
    pub auto_stride: bool,
    pub zstd_level: i32,
    pub detail_limit: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            block_size: crate::block_size::DEFAULT,
            auto_block_size: false,
            auto_stride: true,
            zstd_level: 3,
            detail_limit: 128,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    SelectingBlockSize,
    Analyzing,
    Verifying,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Progress {
    pub phase: Phase,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct StrideSummary {
    pub stride: usize,
    pub selected_for_trials: u64,
}

#[derive(Debug, Serialize)]
pub struct BlockSizeTrial {
    pub block_size: usize,
    pub sampled_bytes: u64,
    pub encoded_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct BlockDetail {
    pub index: u64,
    pub offset: u64,
    pub original_bytes: usize,
    pub experimental_bytes: usize,
    pub zstd_bytes: usize,
    pub model: &'static str,
    pub codec: &'static str,
    pub parameter_bytes: usize,
    pub stride: Option<usize>,
    pub entropy_bits_per_byte: f64,
    pub zero_percent: f64,
    pub adjacent_repeat_percent: f64,
    pub top_neighbors: Vec<crate::geometry::Neighbor>,
}

#[derive(Debug, Serialize)]
pub struct AnalysisReport {
    pub original_bytes: u64,
    pub experimental_bytes: u64,
    pub metadata_bytes: u64,
    pub zstd_stream_bytes: u64,
    pub zstd_block_bytes: u64,
    pub savings_percent: Option<f64>,
    pub compression_ratio: Option<f64>,
    pub zstd_stream_gain_percent: Option<f64>,
    pub zstd_block_gain_percent: Option<f64>,
    pub verified: bool,
    pub elapsed_ms: u64,
    pub throughput_mib_per_sec: f64,
    pub block_count: u64,
    pub model_counts: BTreeMap<&'static str, u64>,
    pub method_bytes: BTreeMap<String, u64>,
    pub dominant_method: Option<String>,
    pub top_strides: Vec<StrideSummary>,
    pub entropy_bits_per_byte: Option<f64>,
    pub zero_percent: Option<f64>,
    pub adjacent_repeat_percent: Option<f64>,
    pub options: Options,
    pub block_size_trials: Vec<BlockSizeTrial>,
    pub warnings: Vec<&'static str>,
    pub blocks: Vec<BlockDetail>,
}
