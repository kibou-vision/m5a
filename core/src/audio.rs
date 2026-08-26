//! 音声データの変換。
//!
//! Realtime API とは G.711 μ-law でやりとりする。PCM16 のまま送ると
//! base64 化して片方向 500kbps 近くになり、ESP32-S3 の無線と TLS では
//! 途切れやすいため、既定では 8kHz の μ-law に落として約 85kbps に収める。

/// μ-law の符号化で足す下駄。ITU-T G.711 の規定値。
const BIAS: i32 = 0x84;
/// μ-law で表せる最大の振幅（14bit 換算）。
const CLIP: i32 = 8_159;
/// 各セグメントの上限値。振幅から指数部を引くために使う。
const SEGMENT_ENDS: [i32; 8] = [0x3F, 0x7F, 0xFF, 0x1FF, 0x3FF, 0x7FF, 0xFFF, 0x1FFF];

/// 口の開きを最大にする実効値。話し声はフルスケールまで振れないため、
/// 会話時の実効値で頭打ちになるようにして口の動きを見えやすくする。
const MOUTH_FULL_SCALE_RMS: i64 = 6_000;

/// PCM16 の1標本を μ-law 1バイトに変換する。
pub fn encode_ulaw(sample: i16) -> u8 {
    let mut magnitude = i32::from(sample) >> 2;

    let mask = if magnitude < 0 {
        magnitude = -magnitude;
        0x7F
    } else {
        0xFF
    };

    if magnitude > CLIP {
        magnitude = CLIP;
    }
    magnitude += BIAS >> 2;

    let segment = find_segment(magnitude);
    if segment >= SEGMENT_ENDS.len() {
        return 0x7F ^ mask;
    }

    let mantissa = (magnitude >> (segment + 1)) & 0x0F;
    ((segment as i32) << 4 | mantissa) as u8 ^ mask as u8
}

/// μ-law 1バイトを PCM16 の1標本に戻す。
pub fn decode_ulaw(encoded: u8) -> i16 {
    let value = !encoded;
    let mantissa = i32::from(value & 0x0F);
    let exponent = u32::from((value & 0x70) >> 4);

    let magnitude = ((mantissa << 3) + BIAS) << exponent;

    let sample = if value & 0x80 != 0 {
        BIAS - magnitude
    } else {
        magnitude - BIAS
    };

    sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn find_segment(magnitude: i32) -> usize {
    SEGMENT_ENDS
        .iter()
        .position(|&end| magnitude <= end)
        .unwrap_or(SEGMENT_ENDS.len())
}

/// PCM16 の並びをまとめて μ-law に変換する。
pub fn encode_ulaw_block(samples: &[i16]) -> Vec<u8> {
    samples.iter().copied().map(encode_ulaw).collect()
}

/// μ-law の並びをまとめて PCM16 に戻す。
pub fn decode_ulaw_block(encoded: &[u8]) -> Vec<i16> {
    encoded.iter().copied().map(decode_ulaw).collect()
}

/// 標本化周波数を変換する。線形補間で足りる用途のため簡素に保つ。
pub fn resample_linear(samples: &[i16], from_hz: u32, to_hz: u32) -> Vec<i16> {
    if samples.is_empty() || from_hz == 0 || to_hz == 0 {
        return Vec::new();
    }
    if from_hz == to_hz {
        return samples.to_vec();
    }

    let output_len = (samples.len() as u64 * u64::from(to_hz) / u64::from(from_hz)) as usize;
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let source_position = index as f32 * from_hz as f32 / to_hz as f32;
        let left = source_position as usize;
        let right = (left + 1).min(samples.len() - 1);
        let fraction = source_position - left as f32;

        let interpolated = f32::from(samples[left]) * (1.0 - fraction)
            + f32::from(samples[right]) * fraction;
        output.push(interpolated as i16);
    }

    output
}

/// 音量を 0〜100 で返す。口の開き具合に使う。
pub fn measure_level(samples: &[i16]) -> u8 {
    if samples.is_empty() {
        return 0;
    }

    let sum_of_squares: i64 = samples
        .iter()
        .map(|&sample| i64::from(sample) * i64::from(sample))
        .sum();
    let rms = (sum_of_squares / samples.len() as i64).isqrt();

    ((rms * 100 / MOUTH_FULL_SCALE_RMS).min(100)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_maps_to_the_standard_ulaw_byte() {
        assert_eq!(encode_ulaw(0), 0xFF);
        assert_eq!(decode_ulaw(0xFF), 0);
    }

    #[test]
    fn every_ulaw_byte_survives_a_round_trip() {
        // G.711 は 0 に正負ふたつの符号を持つ。負のゼロ 0x7F を復号すると 0 になり、
        // 再び符号化すると正のゼロ 0xFF になるため、そこだけは別扱いにする。
        const NEGATIVE_ZERO: u8 = 0x7F;
        const POSITIVE_ZERO: u8 = 0xFF;

        for encoded in 0..=u8::MAX {
            let decoded = decode_ulaw(encoded);
            let expected = if encoded == NEGATIVE_ZERO {
                POSITIVE_ZERO
            } else {
                encoded
            };

            assert_eq!(
                encode_ulaw(decoded),
                expected,
                "μ-law {encoded:#04x} が往復で変わった"
            );
        }
    }

    #[test]
    fn both_signed_zeros_decode_to_silence() {
        assert_eq!(decode_ulaw(0x7F), 0);
        assert_eq!(decode_ulaw(0xFF), 0);
    }

    #[test]
    fn keeps_quantisation_error_within_the_ulaw_budget() {
        // μ-law の誤差は振幅に比例する。振幅の 12% 以内なら聴感上問題ない。
        for sample in (-32_000..32_000).step_by(97) {
            let restored = decode_ulaw(encode_ulaw(sample));
            let error = (i32::from(restored) - i32::from(sample)).abs();
            let budget = (i32::from(sample).abs() / 8).max(180);

            assert!(
                error <= budget,
                "{sample} → {restored} の誤差 {error} が許容 {budget} を超えた"
            );
        }
    }

    #[test]
    fn preserves_the_sign_of_the_sample() {
        assert!(decode_ulaw(encode_ulaw(5_000)) > 0);
        assert!(decode_ulaw(encode_ulaw(-5_000)) < 0);
    }

    #[test]
    fn converts_blocks_in_both_directions() {
        let samples = [0, 1_000, -1_000, 20_000, -20_000];

        let restored = decode_ulaw_block(&encode_ulaw_block(&samples));

        assert_eq!(restored.len(), samples.len());
        assert_eq!(restored[0], 0);
    }

    #[test]
    fn resampling_to_the_same_rate_changes_nothing() {
        let samples = [1, 2, 3, 4];

        assert_eq!(resample_linear(&samples, 16_000, 16_000), samples.to_vec());
    }

    #[test]
    fn halving_the_rate_halves_the_sample_count() {
        let samples: Vec<i16> = (0..160).collect();

        let downsampled = resample_linear(&samples, 16_000, 8_000);

        assert_eq!(downsampled.len(), 80);
    }

    #[test]
    fn upsampling_keeps_the_waveform_monotonic() {
        let samples: Vec<i16> = (0..10).map(|n| n * 100).collect();

        let upsampled = resample_linear(&samples, 8_000, 24_000);

        assert_eq!(upsampled.len(), 30);
        assert!(
            upsampled.windows(2).all(|pair| pair[0] <= pair[1]),
            "単調増加の波形は補間しても増加のままのはず: {upsampled:?}"
        );
    }

    #[test]
    fn resampling_an_empty_block_yields_nothing() {
        assert!(resample_linear(&[], 16_000, 8_000).is_empty());
    }

    #[test]
    fn level_rises_with_amplitude() {
        let silence = measure_level(&[0; 100]);
        let quiet = measure_level(&[500; 100]);
        let loud = measure_level(&[20_000; 100]);

        assert_eq!(silence, 0);
        assert!(quiet < loud);
        assert_eq!(loud, 100);
    }

    #[test]
    fn level_of_an_empty_block_is_zero() {
        assert_eq!(measure_level(&[]), 0);
    }
}
