// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared playback restoration policy; platform decoders and storage remain adapters.

/// Restores an unfinished position, while completed or malformed positions restart.
#[uniffi::export]
pub fn media_resume_position(position: f64, duration: f64) -> f64 {
    if !position.is_finite() || !duration.is_finite() || duration <= 0.0 || position < 0.0 {
        return 0.0;
    }
    let end_margin = 10.0_f64.min(duration * 0.05);
    if position >= duration - end_margin {
        0.0
    } else {
        position.min(duration)
    }
}

/// Normalizes arbitrary signed quarter-turn input without overflow.
#[uniffi::export]
pub fn media_normalize_rotation(degrees: i32) -> u16 {
    let normalized = degrees.rem_euclid(360);
    u16::try_from(((normalized + 45) / 90 * 90) % 360).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_rejects_invalid_values_and_completed_tracks() {
        for (position, duration) in [(f64::NAN, 10.0), (4.0, f64::INFINITY), (-1.0, 10.0), (8.0, 0.0)] {
            assert_eq!(media_resume_position(position, duration), 0.0);
        }
        assert_eq!(media_resume_position(99.0, 100.0), 0.0);
        assert_eq!(media_resume_position(900.0, 1_000.0), 900.0);
        assert_eq!(media_resume_position(1.0, 4.0), 1.0);
    }

    #[test]
    fn rotation_handles_negative_and_extreme_angles() {
        assert_eq!(media_normalize_rotation(-90), 270);
        assert_eq!(media_normalize_rotation(450), 90);
        assert!(media_normalize_rotation(i32::MIN) < 360);
        assert!(media_normalize_rotation(i32::MAX) < 360);
    }
}
