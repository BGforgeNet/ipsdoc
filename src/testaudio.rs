//! Speech-like test sounds. A plain tone reads as a hum (an m or an n) to the lip-sync model, which judges sounds by
//! their spectrum; these carry a vowel's or a hiss's.

/// `len` samples at `rate` of a held "ah": a 120 Hz pulse train through the vowel's first three formants (730, 1090
/// and 2440 Hz), at peak 1.
pub(crate) fn vowel(len: usize, rate: u32) -> Vec<f32> {
    let period = (rate as f32 / 120.0).round() as usize;
    let mut out: Vec<f32> = (0..len).map(|i| if i % period == 0 { 1.0 } else { 0.0 }).collect();
    for (hz, width) in [(730.0, 90.0), (1090.0, 110.0), (2440.0, 170.0)] {
        let t = 1.0 / rate as f32;
        let c = -(-std::f32::consts::TAU * width * t).exp();
        let b = 2.0 * (-std::f32::consts::PI * width * t).exp() * (std::f32::consts::TAU * hz * t).cos();
        let (mut y1, mut y2) = (0.0, 0.0);
        for x in &mut out {
            let y = (1.0 - b - c) * *x + b * y1 + c * y2;
            (y2, y1) = (y1, y);
            *x = y;
        }
    }
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(f32::MIN_POSITIVE);
    out.iter().map(|x| x / peak).collect()
}

/// `len` samples of a hiss (an s): white noise at peak 1, the same every call.
pub(crate) fn hiss(len: usize) -> Vec<f32> {
    let mut seed: u32 = 7;
    (0..len)
        .map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 16) as f32 / 32768.0 - 1.0
        })
        .collect()
}
