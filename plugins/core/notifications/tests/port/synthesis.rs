//! Twins of the kernel tests of `ilium-sound/src/synthesis.rs`, run against `ilium_plugin_core_notifications::synthesis`.

use ilium_plugin_core_notifications::synthesis::*;

/// REQ: PORT:ilium-sound/src/synthesis.rs::wav_header_and_pcm_payload_are_consistent_and_repeatable
#[test]
fn wav_header_and_pcm_payload_are_consistent_and_repeatable() {
    let design = SoundDesign::default();
    let first = render_wav(&design);
    let second = render_wav(&design);
    let pcm = render_pcm(&design);
    assert_eq!(first, second);
    assert_eq!(&first[0..4], b"RIFF");
    assert_eq!(&first[8..12], b"WAVE");
    assert_eq!(&first[12..16], b"fmt ");
    assert_eq!(
        u32::from_le_bytes(first[24..28].try_into().unwrap()),
        SAMPLE_RATE
    );
    assert_eq!(u16::from_le_bytes(first[34..36].try_into().unwrap()), 16);
    assert_eq!(&first[36..40], b"data");
    assert_eq!(
        u32::from_le_bytes(first[4..8].try_into().unwrap()) as usize + 8,
        first.len()
    );
    assert_eq!(
        u32::from_le_bytes(first[40..44].try_into().unwrap()) as usize,
        pcm.len() * 2
    );
    for (sample, encoded) in pcm.iter().zip(first[44..].chunks_exact(2)) {
        assert_eq!(sample.to_le_bytes(), encoded);
    }
}

/// REQ: PORT:ilium-sound/src/synthesis.rs::hostile_controls_are_capped_and_audio_has_headroom_and_quiet_endpoints
#[test]
fn hostile_controls_are_capped_and_audio_has_headroom_and_quiet_endpoints() {
    let design = SoundDesign {
        pitch_hz: u16::MAX,
        pitch_slide_cents: i16::MAX,
        attack_ms: u16::MAX,
        decay_ms: u16::MAX,
        sustain_percent: u8::MAX,
        release_ms: u16::MAX,
        pulse_rate_tenths_hz: u16::MAX,
        pulse_depth_percent: u8::MAX,
        harmony_semitones: i8::MAX,
        harmony_mix_percent: u8::MAX,
        brightness_percent: u8::MAX,
        noise_percent: u8::MAX,
        duration_ms: u16::MAX,
        volume_percent: u8::MAX,
        ..SoundDesign::default()
    };
    let normalized = design.normalized();
    assert_eq!(normalized.duration_ms, MAX_DURATION_MS);
    assert!(normalized.pitch_hz <= 1_800);
    assert!(normalized.harmony_mix_percent <= 60);
    let pcm = render_pcm(&design);
    assert!(pcm.len() <= MAX_PCM_SAMPLES);
    assert_eq!(pcm.first(), Some(&0));
    assert_eq!(pcm.last(), Some(&0));
    assert!(pcm.iter().any(|sample| *sample != 0));
    assert!(pcm.iter().all(|sample| i32::from(sample.abs()) <= 26_215));
}

/// REQ: PORT:ilium-sound/src/synthesis.rs::each_active_control_changes_the_audible_pcm
#[test]
fn each_active_control_changes_the_audible_pcm() {
    let base = SoundDesign::default();
    let baseline = render_pcm(&base);
    let variants = [
        SoundDesign {
            waveform: Waveform::Saw,
            ..base.clone()
        },
        SoundDesign {
            pitch_hz: 900,
            ..base.clone()
        },
        SoundDesign {
            pitch_slide_cents: 300,
            ..base.clone()
        },
        SoundDesign {
            attack_ms: 100,
            ..base.clone()
        },
        SoundDesign {
            decay_ms: 300,
            ..base.clone()
        },
        SoundDesign {
            sustain_percent: 70,
            ..base.clone()
        },
        SoundDesign {
            release_ms: 400,
            ..base.clone()
        },
        SoundDesign {
            pulse_rate_tenths_hz: 120,
            ..base.clone()
        },
        SoundDesign {
            pulse_depth_percent: 60,
            ..base.clone()
        },
        SoundDesign {
            harmony_semitones: 12,
            ..base.clone()
        },
        SoundDesign {
            harmony_mix_percent: 40,
            ..base.clone()
        },
        SoundDesign {
            brightness_percent: 80,
            ..base.clone()
        },
        SoundDesign {
            noise_percent: 20,
            ..base.clone()
        },
        SoundDesign {
            volume_percent: 80,
            ..base.clone()
        },
    ];
    for (index, variant) in variants.into_iter().enumerate() {
        assert_ne!(
            render_pcm(&variant),
            baseline,
            "control {index} had no effect"
        );
    }
    assert_ne!(
        render_pcm(&SoundDesign {
            duration_ms: 700,
            ..base
        }),
        baseline
    );
}

/// REQ: PORT:ilium-sound/src/synthesis.rs::preview_columns_are_exact_extrema_of_the_rendered_pcm
#[test]
fn preview_columns_are_exact_extrema_of_the_rendered_pcm() {
    let design = SoundDesign::default();
    let pcm = render_pcm(&design);
    let preview = waveform_preview(&design, 17);
    assert_eq!(preview.len(), 17);
    for (index, column) in preview.iter().enumerate() {
        let start = index * pcm.len() / preview.len();
        let end = (index + 1) * pcm.len() / preview.len();
        assert_eq!(column.min, *pcm[start..end].iter().min().unwrap());
        assert_eq!(column.max, *pcm[start..end].iter().max().unwrap());
    }
    assert!(waveform_preview(&design, 0).is_empty());
    assert_eq!(
        waveform_preview(&design, usize::MAX).len(),
        MAX_PREVIEW_COLUMNS
    );
}

/// REQ: PORT:ilium-sound/src/synthesis.rs::preview_render_stops_when_cancellation_arrives_during_synthesis
#[test]
fn preview_render_stops_when_cancellation_arrives_during_synthesis() {
    let mut checks = 0;
    let result = try_waveform_preview(&SoundDesign::default(), 17, || {
        checks += 1;
        checks == 3
    });

    assert!(
        result.is_none(),
        "cancelled synthesis must not publish columns"
    );
    assert_eq!(checks, 3, "cancellation should stop further sample work");
}

/// REQ: PORT:ilium-sound/src/synthesis.rs::cancellable_preview_matches_the_existing_waveform_renderer
#[test]
fn cancellable_preview_matches_the_existing_waveform_renderer() {
    let design = SoundDesign::default();
    let pcm = render_pcm(&design);
    let expected = (0..17)
        .map(|index| {
            let start = index * pcm.len() / 17;
            let end = (index + 1) * pcm.len() / 17;
            let samples = &pcm[start..end];
            WaveformColumn {
                min: *samples.iter().min().expect("nonempty preview interval"),
                max: *samples.iter().max().expect("nonempty preview interval"),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(try_waveform_preview(&design, 17, || false), Some(expected));
}

/// REQ: PORT:ilium-sound/src/synthesis.rs::design_serialization_round_trips_without_floating_point_values
#[test]
fn design_serialization_round_trips_without_floating_point_values() {
    let design = SoundDesign::default();
    let encoded = serde_json::to_string(&design).unwrap();
    let decoded: SoundDesign = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, design);
}
