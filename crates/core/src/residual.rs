use crate::models::Model;

fn prediction(model: Model, bytes: &[u8], i: usize, width: usize) -> Option<u8> {
    match model {
        Model::Delta if i > 0 => Some(bytes[i - 1]),
        Model::StrideDelta | Model::StrideXor if i >= width => Some(bytes[i - width]),
        Model::Linear2 if i >= 2 => Some(bytes[i - 1].wrapping_mul(2).wrapping_sub(bytes[i - 2])),
        Model::Spatial if i >= width && !i.is_multiple_of(width) => Some(
            bytes[i - 1]
                .wrapping_add(bytes[i - width])
                .wrapping_sub(bytes[i - width - 1]),
        ),
        _ => None,
    }
}

fn transpose(bytes: &[u8], width: usize, inverse: bool) -> Vec<u8> {
    let mut output = vec![0; bytes.len()];
    let mut position = 0;
    for column in 0..width {
        for index in (column..bytes.len()).step_by(width) {
            if inverse {
                output[index] = bytes[position];
            } else {
                output[position] = bytes[index];
            }
            position += 1;
        }
    }
    output
}

fn bit_planes(bytes: &[u8], inverse: bool) -> Vec<u8> {
    let groups = bytes.len() / 8;
    let mut output = vec![0; bytes.len()];
    for group in 0..groups {
        for bit in 0..8 {
            for lane in 0..8 {
                let byte_index = group * 8 + lane;
                let plane_index = bit * groups + group;
                if inverse {
                    output[byte_index] |= ((bytes[plane_index] >> lane) & 1) << bit;
                } else {
                    output[plane_index] |= ((bytes[byte_index] >> bit) & 1) << lane;
                }
            }
        }
    }
    output[groups * 8..].copy_from_slice(&bytes[groups * 8..]);
    output
}

pub(crate) fn encode(model: Model, bytes: &[u8], width: usize) -> Vec<u8> {
    match model {
        Model::Transpose => transpose(bytes, width, false),
        Model::BitShuffle => bit_planes(bytes, false),
        _ => bytes
            .iter()
            .enumerate()
            .map(|(i, &byte)| match prediction(model, bytes, i, width) {
                Some(p) if model == Model::StrideXor => byte ^ p,
                Some(p) => byte.wrapping_sub(p),
                None => byte,
            })
            .collect(),
    }
}

pub(crate) fn decode(model: Model, mut bytes: Vec<u8>, width: usize) -> Vec<u8> {
    match model {
        Model::Transpose => transpose(&bytes, width, true),
        Model::BitShuffle => bit_planes(&bytes, true),
        _ => {
            for i in 0..bytes.len() {
                if let Some(p) = prediction(model, &bytes, i, width) {
                    bytes[i] = if model == Model::StrideXor {
                        bytes[i] ^ p
                    } else {
                        bytes[i].wrapping_add(p)
                    };
                }
            }
            bytes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_transforms_round_trip_with_short_and_partial_blocks() {
        for len in [1, 2, 7, 8, 9, 19, 257] {
            let bytes: Vec<_> = (0..len).map(|i| (i * i * 13 + i * 73) as u8).collect();
            for id in 0..=7 {
                let model = Model::from_id(id).unwrap();
                let width = usize::from(model.uses_width()) * 3;
                if width >= len {
                    continue;
                }
                assert_eq!(decode(model, encode(model, &bytes, width), width), bytes);
            }
        }
    }
}
