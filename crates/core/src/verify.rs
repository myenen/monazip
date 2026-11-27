use crate::{AnalysisError, check_cancel, codec};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::atomic::AtomicBool,
};

pub(crate) fn verify(
    input: &mut (impl Read + Seek),
    container: &mut (impl Read + Seek),
    len: u64,
    block_size: usize,
    source_digest: &[u8; 32],
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<(), AnalysisError> {
    input.seek(SeekFrom::Start(0))?;
    container.seek(SeekFrom::Start(0))?;
    let (encoded_len, encoded_block_size, digest) = codec::read_header(container)?;
    if encoded_len != len || encoded_block_size != block_size || &digest != source_digest {
        return Err(AnalysisError::Verification);
    }
    let mut processed = 0;
    let mut original = vec![0; block_size];
    let mut hash = blake3::Hasher::new();
    while processed < len {
        check_cancel(cancel)?;
        let count = (len - processed).min(block_size as u64) as usize;
        if crate::block::fill(input, &mut original[..count])? != count {
            return Err(AnalysisError::SourceChanged);
        }
        let decoded = codec::decode_block(container, count)?;
        if decoded != original[..count] {
            return Err(AnalysisError::Verification);
        }
        hash.update(&decoded);
        processed += count as u64;
        progress(processed);
    }
    let mut extra = [0];
    if crate::block::fill(input, &mut extra)? != 0 {
        return Err(AnalysisError::SourceChanged);
    }
    if crate::block::fill(container, &mut extra)? != 0 || hash.finalize().as_bytes() != &digest {
        return Err(AnalysisError::Verification);
    }
    check_cancel(cancel)
}
