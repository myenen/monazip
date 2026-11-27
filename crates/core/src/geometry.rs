use crate::{AnalysisError, check_cancel, statistics::entropy};
use serde::Serialize;
use std::sync::atomic::AtomicBool;

const FIXED_EXTRA: [usize; 4] = [128, 256, 512, 1024];
const MAX_DISCOVERED_WIDTH: usize = 4096;
const DISCOVERED_LIMIT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Horizontal,
    Vertical,
    UpperLeft,
    UpperRight,
}

#[derive(Clone, Debug, Serialize)]
pub struct Neighbor {
    pub width: usize,
    pub direction: Direction,
    pub pairs: usize,
    pub equal_percent: f64,
    pub delta_entropy: f64,
    pub xor_entropy: f64,
    pub estimated_bytes: f64,
}

fn measure(bytes: &[u8], width: usize, direction: Direction) -> Option<Neighbor> {
    let mut delta = [0; 256];
    let mut xor = [0; 256];
    let mut pairs = 0;
    let mut tally = |current: &[u8], neighbors: &[u8]| {
        pairs += current.len().min(neighbors.len());
        for (&byte, &neighbor) in current.iter().zip(neighbors) {
            delta[byte.wrapping_sub(neighbor) as usize] += 1;
            xor[(byte ^ neighbor) as usize] += 1;
        }
    };
    match direction {
        Direction::Vertical => tally(&bytes[width..], bytes),
        Direction::Horizontal => {
            for row in bytes.chunks(width) {
                tally(&row[1..], row);
            }
        }
        Direction::UpperLeft | Direction::UpperRight => {
            let rows = bytes.chunks(width);
            for (above, current) in rows.clone().zip(rows.skip(1)) {
                if direction == Direction::UpperLeft {
                    tally(&current[1..], above);
                } else {
                    tally(current, &above[1..]);
                }
            }
        }
    }
    if pairs == 0 {
        return None;
    }
    let delta_entropy = entropy(&delta, pairs as u64);
    let xor_entropy = entropy(&xor, pairs as u64);
    Some(Neighbor {
        width,
        direction,
        pairs,
        equal_percent: 100.0 * delta[0] as f64 / pairs as f64,
        delta_entropy,
        xor_entropy,
        estimated_bytes: (bytes.len() - pairs) as f64
            + pairs as f64 * delta_entropy.min(xor_entropy) / 8.0,
    })
}

fn discover(bytes: &[u8], cancel: &AtomicBool) -> Result<Vec<usize>, AnalysisError> {
    let limit = MAX_DISCOVERED_WIDTH.min(bytes.len().saturating_sub(128));
    if limit <= 64 {
        return Ok(Vec::new());
    }

    // Use the same target positions for every distance so scores are comparable.
    // An odd step through a stratified sample avoids alignment with a record width.
    let positions: [usize; 128] = std::array::from_fn(|index| {
        let span = bytes.len() - limit;
        let start = index * span / 128;
        let end = (index + 1) * span / 128;
        limit + start + (index * 53 + 17) % (end - start)
    });
    let mut scores = Vec::new();
    for width in 65..=limit {
        if width % 64 == 1 {
            check_cancel(cancel)?;
        }
        if FIXED_EXTRA.contains(&width) {
            continue;
        }
        let mut close = 0;
        let mut exact = 0;
        for &index in &positions {
            let delta = bytes[index].wrapping_sub(bytes[index - width]);
            close += usize::from(delta <= 2 || delta >= 254);
            exact += usize::from(delta == 0);
        }
        if close >= positions.len() / 4 {
            scores.push((width, close, exact));
        }
    }
    scores.sort_unstable_by_key(|&(width, close, exact)| {
        (std::cmp::Reverse(close), std::cmp::Reverse(exact), width)
    });
    Ok(scores
        .into_iter()
        .take(DISCOVERED_LIMIT)
        .map(|(width, _, _)| width)
        .collect())
}

pub(crate) fn explore(
    bytes: &[u8],
    auto_stride: bool,
    cancel: &AtomicBool,
) -> Result<Geometry, AnalysisError> {
    let mut strides = Vec::new();
    let mut spatial = Vec::new();
    for width in (1..=64)
        .chain(FIXED_EXTRA)
        .take_while(|&width| width < bytes.len())
    {
        check_cancel(cancel)?;
        strides.extend(measure(bytes, width, Direction::Vertical));
        if width > 1 {
            for direction in [
                Direction::Horizontal,
                Direction::UpperLeft,
                Direction::UpperRight,
            ] {
                spatial.extend(measure(bytes, width, direction));
            }
        }
    }
    for scores in [&mut strides, &mut spatial] {
        scores.sort_by(|a, b| a.estimated_bytes.total_cmp(&b.estimated_bytes));
    }
    strides.truncate(4);
    spatial.truncate(4);
    let mut discovered = Vec::new();
    if auto_stride {
        for width in discover(bytes, cancel)? {
            check_cancel(cancel)?;
            discovered.extend(measure(bytes, width, Direction::Vertical));
        }
    }
    Ok(Geometry {
        strides,
        spatial,
        discovered,
    })
}

pub(crate) struct Geometry {
    pub strides: Vec<Neighbor>,
    pub spatial: Vec<Neighbor>,
    pub discovered: Vec<Neighbor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighborhoods_exclude_row_crossings_and_handle_ragged_rows() {
        let bytes = [0, 1, 2, 3, 4, 5, 6];
        for (direction, expected) in [
            (Direction::Horizontal, 4),
            (Direction::Vertical, 4),
            (Direction::UpperLeft, 2),
            (Direction::UpperRight, 3),
        ] {
            assert_eq!(measure(&bytes, 3, direction).unwrap().pairs, expected);
        }
    }
}
