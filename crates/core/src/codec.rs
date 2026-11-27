use crate::{
    models::{Model, invalid},
    residual,
};
use std::io::{self, Read, Write};

pub(crate) const HEADER_BYTES: usize = 49;
const BLOCK_HEADER_BYTES: usize = 12;
const MAGIC: &[u8; 5] = b"MZIP\x01";

pub(crate) struct EncodedBlock {
    pub model: Model,
    pub compressed: bool,
    pub params: Vec<u8>,
    pub payload: Vec<u8>,
}

impl EncodedBlock {
    pub fn size(&self) -> usize {
        self.metadata_size() + self.payload.len()
    }

    pub fn metadata_size(&self) -> usize {
        BLOCK_HEADER_BYTES + self.params.len()
    }

    pub fn codec_name(&self) -> &'static str {
        if self.compressed { "Zstd" } else { "Doğrudan" }
    }

    pub fn write(&self, writer: &mut impl Write, len: usize) -> io::Result<()> {
        writer.write_all(&[self.model as u8, u8::from(self.compressed)])?;
        writer.write_all(&(len as u32).to_le_bytes())?;
        writer.write_all(&(self.params.len() as u16).to_le_bytes())?;
        writer.write_all(&(self.payload.len() as u32).to_le_bytes())?;
        writer.write_all(&self.params)?;
        writer.write_all(&self.payload)
    }
}

pub(crate) fn write_header(
    writer: &mut impl Write,
    len: u64,
    block_size: usize,
    digest: &[u8; 32],
) -> io::Result<()> {
    writer.write_all(MAGIC)?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&(block_size as u32).to_le_bytes())?;
    writer.write_all(digest)
}

pub(crate) fn read_header(reader: &mut impl Read) -> io::Result<(u64, usize, [u8; 32])> {
    let mut header = [0; HEADER_BYTES];
    reader.read_exact(&mut header)?;
    let len = u64::from_le_bytes(header[5..13].try_into().unwrap());
    let block_size = u32::from_le_bytes(header[13..17].try_into().unwrap()) as usize;
    if &header[..5] != MAGIC || !(1..=crate::MAX_BLOCK_SIZE).contains(&block_size) {
        return Err(invalid("Geçersiz MonaZip başlığı"));
    }
    Ok((len, block_size, header[17..].try_into().unwrap()))
}

pub(crate) fn decode_block(reader: &mut impl Read, expected: usize) -> io::Result<Vec<u8>> {
    let mut header = [0; BLOCK_HEADER_BYTES];
    reader.read_exact(&mut header)?;
    let model = Model::from_id(header[0])?;
    let codec = header[1];
    let len = u32::from_le_bytes(header[2..6].try_into().unwrap()) as usize;
    let param_len = u16::from_le_bytes(header[6..8].try_into().unwrap()) as usize;
    let payload_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if len == 0 || len != expected || len > crate::MAX_BLOCK_SIZE || param_len > 1024 {
        return Err(invalid("Geçersiz blok uzunluğu"));
    }
    let valid_codec = if model.is_pattern() {
        codec == 0 && payload_len == 0
    } else {
        match codec {
            0 => payload_len == len,
            1 => payload_len > 0 && payload_len <= zstd::zstd_safe::compress_bound(len),
            _ => false,
        }
    };
    if !valid_codec {
        return Err(invalid("Geçersiz blok codec veya payload uzunluğu"));
    }
    let mut params = vec![0; param_len];
    reader.read_exact(&mut params)?;
    let width = model.validate(&params, len)?;
    if model.is_pattern() {
        return Ok(params.iter().copied().cycle().take(len).collect());
    }
    let mut payload = vec![0; payload_len];
    reader.read_exact(&mut payload)?;
    if codec == 1 {
        payload = zstd::bulk::decompress(&payload, len)?;
    }
    if payload.len() != len {
        return Err(invalid("Çözülen blok uzunluğu farklı"));
    }
    Ok(residual::decode(model, payload, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_headers_and_lengths_are_rejected() {
        assert!(read_header(&mut &[0; HEADER_BYTES][..]).is_err());
        let block = EncodedBlock {
            model: Model::StrideDelta,
            compressed: false,
            params: vec![3, 0],
            payload: vec![1, 2, 3],
        };
        let mut bytes = Vec::new();
        block.write(&mut bytes, 3).unwrap();
        assert!(decode_block(&mut bytes.as_slice(), 3).is_err());
        bytes[0] = Model::Raw as u8;
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_block(&mut bytes.as_slice(), 3).is_err());
    }

    #[test]
    fn pattern_decoding_accepts_partial_repetitions() {
        for (model, params, expected) in [
            (Model::Constant, vec![7], vec![7; 5]),
            (Model::Periodic, vec![7, 9], vec![7, 9, 7, 9, 7]),
        ] {
            let block = EncodedBlock {
                model,
                compressed: false,
                params,
                payload: vec![],
            };
            let mut bytes = Vec::new();
            block.write(&mut bytes, expected.len()).unwrap();
            assert_eq!(
                decode_block(&mut bytes.as_slice(), expected.len()).unwrap(),
                expected
            );
        }
    }
}
