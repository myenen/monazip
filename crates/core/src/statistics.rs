pub(crate) fn histogram(bytes: &[u8]) -> [u64; 256] {
    let mut counts = [0; 256];
    for &byte in bytes {
        counts[byte as usize] += 1;
    }
    counts
}

pub(crate) fn entropy(counts: &[u64; 256], total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    counts
        .iter()
        .copied()
        .filter(|&n| n > 0)
        .map(|n| {
            let probability = n as f64 / total as f64;
            -probability * probability.log2()
        })
        .sum()
}

pub(crate) fn percent(count: u64, total: u64) -> Option<f64> {
    (total != 0).then(|| 100.0 * count as f64 / total as f64)
}

pub(crate) fn savings(encoded: u64, reference: u64) -> Option<f64> {
    (reference != 0).then(|| 100.0 * (1.0 - encoded as f64 / reference as f64))
}
