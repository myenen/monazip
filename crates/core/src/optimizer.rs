use crate::{
    AnalysisError, check_cancel,
    codec::EncodedBlock,
    geometry::Geometry,
    models::{Model, period},
    residual,
};
use std::{collections::BTreeSet, sync::atomic::AtomicBool};

pub(crate) fn choose(
    bytes: &[u8],
    baseline: Vec<u8>,
    geometry: &Geometry,
    zstd: &mut zstd::bulk::Compressor<'_>,
    cancel: &AtomicBool,
) -> Result<EncodedBlock, AnalysisError> {
    let mut best = EncodedBlock {
        model: Model::Raw,
        compressed: false,
        params: vec![],
        payload: bytes.to_vec(),
    };
    if baseline.len() < bytes.len() {
        best = EncodedBlock {
            model: Model::Raw,
            compressed: true,
            params: vec![],
            payload: baseline,
        };
    }

    let pattern_len = period(bytes);
    if pattern_len == 1 || (pattern_len < bytes.len() && pattern_len <= 1024) {
        let pattern = EncodedBlock {
            model: if pattern_len == 1 {
                Model::Constant
            } else {
                Model::Periodic
            },
            compressed: false,
            params: bytes[..pattern_len].to_vec(),
            payload: vec![],
        };
        if pattern.size() < best.size() {
            best = pattern;
        }
        if pattern_len == 1 {
            return Ok(best);
        }
    }

    let mut trials = vec![
        (Model::Delta, 0),
        (Model::Linear2, 0),
        (Model::BitShuffle, 0),
    ];
    for neighbor in geometry.strides.iter().chain(&geometry.discovered) {
        if neighbor.width > 1 {
            trials.push((Model::StrideDelta, neighbor.width));
        }
        trials.push((Model::StrideXor, neighbor.width));
    }
    let mut seen = BTreeSet::new();
    for width in geometry
        .spatial
        .iter()
        .chain(&geometry.strides)
        .map(|neighbor| neighbor.width)
        .filter(|&width| width > 1)
    {
        if seen.insert(width) {
            trials.push((Model::Spatial, width));
            trials.push((Model::Transpose, width));
        }
        if seen.len() >= 4 {
            break;
        }
    }
    for (model, width) in trials {
        check_cancel(cancel)?;
        let transformed = residual::encode(model, bytes, width);
        let candidate = EncodedBlock {
            model,
            compressed: true,
            params: if model.uses_width() {
                (width as u16).to_le_bytes().to_vec()
            } else {
                vec![]
            },
            payload: zstd.compress(&transformed)?,
        };
        if candidate.size() < best.size() {
            best = candidate;
        }
    }
    Ok(best)
}
