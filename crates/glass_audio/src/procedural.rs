/// Generates a valid RIFF WAVE PCM16 stereo 48 kHz buffer containing a sine tone.
pub fn generate_tone(freq_hz: f32, duration_secs: f32, amplitude: f32) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 48_000;
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;

    let frames = (SAMPLE_RATE as f32 * duration_secs).round() as u32;
    let sample_count = frames * CHANNELS as u32;
    let data_bytes = sample_count * (BITS as u32 / 8);
    let file_size = 36 + data_bytes;

    let mut buf = Vec::with_capacity(44 + data_bytes as usize);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());          // Subchunk1Size (16 for PCM)
    buf.extend_from_slice(&1u16.to_le_bytes());           // AudioFormat (1 for PCM)
    buf.extend_from_slice(&CHANNELS.to_le_bytes());
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    let byte_rate = SAMPLE_RATE * CHANNELS as u32 * (BITS as u32 / 8);
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = CHANNELS * (BITS / 8);
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&BITS.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    let amp = amplitude.clamp(0.0, 1.0);
    let two_pi_f = std::f32::consts::TAU * freq_hz;
    let inv_sr = 1.0 / SAMPLE_RATE as f32;

    for n in 0..frames {
        let t = n as f32 * inv_sr;
        let s = (two_pi_f * t).sin() * amp;
        let sample = (s * i16::MAX as f32) as i16;
        let bytes = sample.to_le_bytes();
        // Stereo: identical sample in Left and Right
        buf.extend_from_slice(&bytes);
        buf.extend_from_slice(&bytes);
    }

    buf
}

pub fn beep_440() -> Vec<u8> {
    generate_tone(440.0, 0.25, 0.3)
}

pub fn beep_880() -> Vec<u8> {
    generate_tone(880.0, 0.25, 0.3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_wav_header() {
        let wav = beep_440();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert!(wav.len() > 44);
    }
}
