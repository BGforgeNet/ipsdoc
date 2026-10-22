//! Shared by the comparison tests.

use oracles::Oracle;

/// Our output (wrapping overflows, as the engines do) and each of `oracles`' beside `expected`; see
/// [`oracle_disagreements`].
pub fn disagreements(acm: &[u8], oracles: &[Oracle], expected: &[i16]) -> Vec<String> {
    let ours = ipsdoc::acm::decode_with(acm, ipsdoc::acm::Overflow::Wrap)
        .map(|d| d.samples)
        .map_err(|e| format!("ipsdoc: {e}"));
    let mut out: Vec<String> = compare("ipsdoc", expected.len(), ours, expected).into_iter().collect();
    out.extend(oracle_disagreements(acm, oracles, expected));
    out
}

/// One line per decoder that fails, differs in length, or differs in a sample (the first one). libacm and
/// `infinitier` emit whole frames only, so on a stereo header with an odd sample count they end one sample early;
/// the engines and ours do not.
pub fn oracle_disagreements(acm: &[u8], oracles: &[Oracle], expected: &[i16]) -> Vec<String> {
    let channels = usize::from(u16::from_le_bytes([acm[8], acm[9]])).max(1);
    let whole_frames = expected.len() - expected.len() % channels;
    oracles
        .iter()
        .filter_map(|&o| {
            let len = if matches!(o, Oracle::Libacm | Oracle::Infinitier) {
                whole_frames
            } else {
                expected.len()
            };
            compare(
                &format!("{o:?}"),
                len,
                o.decode(acm).map_err(|e| e.to_string()),
                expected,
            )
        })
        .collect()
}

fn compare(name: &str, len: usize, result: Result<Vec<i16>, String>, expected: &[i16]) -> Option<String> {
    match result {
        Err(e) => Some(e),
        Ok(d) if d.len() != len => Some(format!("{name}: {} of {len} samples", d.len())),
        Ok(d) => d
            .iter()
            .zip(expected)
            .position(|(a, b)| a != b)
            .map(|i| format!("{name}: differs at sample {i}: {} != {}", d[i], expected[i])),
    }
}
