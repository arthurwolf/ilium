//! PCM16 little-endian conversion, sample-boundary alignment and linear resampling of the voice audio path.
//! Pure sample arithmetic moved from `ilium-voice`; the device callbacks that feed it are host work.

/// The sample rate of the provider's realtime audio.
pub const REALTIME_SAMPLE_RATE: u32 = 24_000;

pub fn f32_to_pcm16_le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let integer = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        bytes.extend_from_slice(&integer.to_le_bytes());
    }
    bytes
}

pub fn pcm16_le_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / f32::from(i16::MAX))
        .collect()
}

/// Re-aligns a streamed PCM16-LE byte sequence to sample boundaries across
/// arbitrary chunk splits. Network audio deltas are not guaranteed to end on
/// a 16-bit sample boundary; without carrying the dangling byte to the next
/// chunk, `chunks_exact(2)` would silently drop it and decode the entire
/// following chunk one byte out of phase, which plays as loud noise.
#[derive(Debug, Default)]
pub struct Pcm16SampleAligner {
    pending_byte: Option<u8>,
}

impl Pcm16SampleAligner {
    /// Decodes every complete sample available once `bytes` is appended to
    /// the carried remainder, keeping any new dangling byte for the next call.
    pub fn process(&mut self, bytes: &[u8]) -> Vec<f32> {
        // Only allocate a joined buffer when a byte was actually carried.
        let joined_storage;
        let aligned_bytes = match self.pending_byte.take() {
            Some(carried_byte) => {
                let mut joined = Vec::with_capacity(bytes.len() + 1);
                joined.push(carried_byte);
                joined.extend_from_slice(bytes);
                joined_storage = joined;
                joined_storage.as_slice()
            }
            None => bytes,
        };

        if aligned_bytes.len() % 2 == 1 {
            self.pending_byte = aligned_bytes.last().copied();
        }

        // `pcm16_le_to_f32` uses `chunks_exact(2)`, which ignores exactly the
        // dangling byte stored above.
        pcm16_le_to_f32(aligned_bytes)
    }

    /// Drops any carried byte; the stream it belonged to has ended.
    pub fn reset(&mut self) {
        self.pending_byte = None;
    }
}

/// Chunk-boundary-independent linear resampler. It deliberately keeps a
/// source sample between calls, preventing clicks and drift between CPAL
/// callbacks without coupling the API to a particular DSP library.
#[derive(Debug)]
pub struct StreamingLinearResampler {
    step: f64,
    source_position: f64,
    buffered_samples: Vec<f32>,
}

impl StreamingLinearResampler {
    pub fn new(input_sample_rate: u32, output_sample_rate: u32) -> Self {
        Self {
            step: f64::from(input_sample_rate) / f64::from(output_sample_rate),
            source_position: 0.0,
            buffered_samples: Vec::new(),
        }
    }

    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        self.buffered_samples.extend_from_slice(samples);
        let mut output = Vec::new();

        while self.source_position + 1.0 < self.buffered_samples.len() as f64 {
            let lower_index = self.source_position.floor() as usize;
            let upper_index = lower_index + 1;
            let fraction = (self.source_position - lower_index as f64) as f32;
            let sample = self.buffered_samples[lower_index]
                + (self.buffered_samples[upper_index] - self.buffered_samples[lower_index])
                    * fraction;
            output.push(sample);
            self.source_position += self.step;
        }

        // The loop's final iteration can advance `source_position` by up to
        // `step` past the last index it actually consumed (it adds `step`
        // after passing the `+ 1.0 < len` check), so for step > 2 -- i.e.
        // input sample rates above 48 kHz being downsampled to
        // REALTIME_SAMPLE_RATE -- the floor of `source_position` can exceed
        // `buffered_samples.len()`. Clamp the drain to what actually exists;
        // `source_position` keeps the (now-negative-after-subtraction)
        // leftover offset so the next call's `extend_from_slice` lines the
        // fractional position back up correctly.
        let consumed = (self.source_position.floor() as usize).min(self.buffered_samples.len());
        if consumed > 0 {
            self.buffered_samples.drain(..consumed);
            self.source_position -= consumed as f64;
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: PORT:ilium-voice/src/audio.rs::pcm16_conversion_clamps_and_round_trips
    #[test]
    fn pcm16_conversion_clamps_and_round_trips() {
        let encoded = f32_to_pcm16_le(&[-2.0, -0.5, 0.0, 0.5, 2.0]);
        let decoded = pcm16_le_to_f32(&encoded);

        assert_eq!(decoded.len(), 5);
        assert!((decoded[0] + 1.0).abs() < 0.0001);
        assert!((decoded[1] + 0.5).abs() < 0.0001);
        assert_eq!(decoded[2], 0.0);
        assert!((decoded[3] - 0.5).abs() < 0.0001);
        assert!((decoded[4] - 1.0).abs() < 0.0001);
    }

    /// REQ: PORT:ilium-voice/src/audio.rs::pcm16_sample_aligner_is_independent_of_chunk_boundaries
    #[test]
    fn pcm16_sample_aligner_is_independent_of_chunk_boundaries() {
        let samples = (0..500)
            .map(|index| ((index as f32) / 15.0).sin())
            .collect::<Vec<_>>();
        let encoded = f32_to_pcm16_le(&samples);
        let expected = pcm16_le_to_f32(&encoded);

        // Odd chunk sizes force a carried byte on every call; the prime 7
        // additionally walks the split point through every byte offset.
        for chunk_size in [1_usize, 3, 7, 64, 129] {
            let mut aligner = Pcm16SampleAligner::default();
            let actual = encoded
                .chunks(chunk_size)
                .flat_map(|chunk| aligner.process(chunk))
                .collect::<Vec<_>>();

            assert_eq!(actual, expected, "mismatch for chunk_size={chunk_size}");
        }
    }

    /// REQ: PORT:ilium-voice/src/audio.rs::pcm16_sample_aligner_reset_drops_the_carried_byte
    #[test]
    fn pcm16_sample_aligner_reset_drops_the_carried_byte() {
        let mut aligner = Pcm16SampleAligner::default();

        // One dangling byte: no complete sample yet, byte is carried.
        assert!(aligner.process(&[0x12]).is_empty());
        aligner.reset();

        // After reset the next two bytes must decode as one whole sample
        // instead of pairing with the stale carried byte.
        let decoded = aligner.process(&[0x00, 0x40]);
        assert_eq!(decoded.len(), 1);
        assert!((decoded[0] - f32::from(0x4000_i16) / f32::from(i16::MAX)).abs() < 0.0001);
    }

    /// REQ: PORT:ilium-voice/src/audio.rs::streaming_resampler_is_independent_of_callback_boundaries
    #[test]
    fn streaming_resampler_is_independent_of_callback_boundaries() {
        let input = (0..4_800)
            .map(|index| ((index as f32) / 20.0).sin())
            .collect::<Vec<_>>();
        let mut whole = StreamingLinearResampler::new(48_000, 24_000);
        let expected = whole.process(&input);
        let mut chunked = StreamingLinearResampler::new(48_000, 24_000);
        let actual = input
            .chunks(137)
            .flat_map(|chunk| chunked.process(chunk))
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
        assert!(actual.len().abs_diff(2_400) <= 1);
    }

    /// REQ: PORT:ilium-voice/src/audio.rs::streaming_resampler_does_not_panic_downsampling_high_rate_inputs_in_small_chunks
    #[test]
    fn streaming_resampler_does_not_panic_downsampling_high_rate_inputs_in_small_chunks() {
        // step > 2 (96 kHz and 192 kHz down to 24 kHz) is the regime where the
        // final loop iteration can push `source_position` past
        // `buffered_samples.len()`; varied small chunk sizes exercise every
        // possible remainder against `step` so a fixed chunk size can't hide
        // the out-of-bounds drain.
        for input_sample_rate in [96_000_u32, 192_000_u32] {
            let step = f64::from(input_sample_rate) / f64::from(24_000_u32);
            let input = (0..20_000)
                .map(|index| ((index as f32) / 40.0).sin())
                .collect::<Vec<_>>();

            let mut whole = StreamingLinearResampler::new(input_sample_rate, 24_000);
            let expected = whole.process(&input);

            for chunk_size in [5_usize, 6, 7, 137] {
                let mut chunked = StreamingLinearResampler::new(input_sample_rate, 24_000);
                let actual = input
                    .chunks(chunk_size)
                    .flat_map(|chunk| chunked.process(chunk))
                    .collect::<Vec<_>>();

                assert_eq!(
                    actual, expected,
                    "mismatch for input_sample_rate={input_sample_rate}, chunk_size={chunk_size}, step={step}"
                );
            }
        }
    }
}
