//! FrameConnect negotiation and measurement policy. Driver declarations are not benchmarks.
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const CAPABILITY_VERSION: u32 = 2;
pub const QUALITY_EYE_SIZE: u32 = 2176; // ALVR requires multiples of 32; never round 2160 down.

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DecodeMode {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HardwareDecoder {
    pub name: String,
    pub mime: String,
    pub ten_bit: bool,
    pub modes: Vec<DecodeMode>,
    /// "mediacodec-size-and-rate" | "v4l2-size-only"
    #[serde(default)]
    pub rate_source: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FrameConnectCapabilities {
    pub version: u32,
    pub decoders: Vec<HardwareDecoder>,
    pub can_request_refresh_rate: bool,
    pub eye_gaze: bool,
    pub probe_error: Option<String>,
}

impl FrameConnectCapabilities {
    pub fn decoder(
        &self,
        mime: &str,
        width: u32,
        height: u32,
        fps: f32,
        ten_bit: bool,
    ) -> Option<&HardwareDecoder> {
        self.decoders.iter().find(|d| {
            d.mime == mime
                && (!ten_bit || d.ten_bit)
                && d.modes
                    .iter()
                    .any(|m| m.width == width && m.height == height && (m.fps - fps).abs() < 0.5)
        })
    }
}

/// Select only modes explicitly reported at the requested resolution. No codec/resolution fallback.
pub fn select_rate(rates: &[f32], requested: f32, ceiling: Option<f32>) -> Option<f32> {
    if !requested.is_finite() || requested <= 0.0 {
        return None;
    }
    let ceiling = ceiling
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(requested)
        .min(requested);
    let mut valid_rates = rates
        .iter()
        .copied()
        .filter(|r| r.is_finite() && *r > 0.0)
        .collect::<Vec<_>>();
    valid_rates.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    valid_rates
        .into_iter()
        .find(|candidate| *candidate <= ceiling + 0.1)
}

/// A bounded observation window; receives one sample per newly decoded/presented video frame.
/// This measures delivered cadence, not the headset's display rate or motion-to-photon latency.
pub struct RateMonitor {
    rate: f32,
    warmup: f64,
    elapsed: f64,
    frames: u64,
    decode_overruns: u64,
    queue_start: Option<f64>,
    queue_end: f64,
}

impl RateMonitor {
    pub fn new(rate: f32) -> Self {
        Self {
            rate,
            warmup: 5.0,
            elapsed: 0.0,
            frames: 0,
            decode_overruns: 0,
            queue_start: None,
            queue_end: 0.0,
        }
    }

    /// Returns a reason after a complete ten-second window. Sleep/gaps reset the observation.
    pub fn observe(
        &mut self,
        interval: Duration,
        decode: Duration,
        queue: Duration,
    ) -> Option<String> {
        let dt = interval.as_secs_f64();
        if !self.rate.is_finite() || self.rate <= 0.0 || dt <= 0.0 || dt > 0.25 {
            *self = Self::new(self.rate);
            return None;
        }
        if self.warmup > 0.0 {
            self.warmup -= dt;
            return None;
        }
        self.elapsed += dt;
        self.frames += 1;
        self.decode_overruns += u64::from(decode.as_secs_f64() > 1.0 / self.rate as f64);
        self.queue_start.get_or_insert(queue.as_secs_f64());
        self.queue_end = queue.as_secs_f64();
        if self.elapsed < 10.0 {
            return None;
        }
        let delivered = self.frames as f64 / self.elapsed;
        let reason = if delivered < self.rate as f64 * 0.99 {
            Some(format!(
                "実配信 {:.1} fps が目標 {:.0} Hz に届かないため、画質を維持して表示周期を下げます",
                delivered, self.rate
            ))
        } else if self.decode_overruns as f64 / self.frames as f64 > 0.05 {
            Some("復号処理がフレーム時間を超えるため、画質を維持して表示周期を下げます".into())
        } else if self.queue_end - self.queue_start.unwrap_or(0.0) > 2.0 / self.rate as f64 {
            Some("復号待ちが増えているため、画質を維持して表示周期を下げます".into())
        } else {
            None
        };
        self.elapsed = 0.0;
        self.frames = 0;
        self.decode_overruns = 0;
        self.queue_start = None;
        reason
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_reported_rates_and_never_above_request() {
        assert_eq!(select_rate(&[90.0, 120.0], 144.0, None), Some(120.0));
        assert_eq!(select_rate(&[144.0], 120.0, None), None);
        assert_eq!(select_rate(&[90.0, 144.0], 144.0, Some(120.0)), Some(90.0));
        assert_eq!(select_rate(&[f32::NAN, 0.0], 144.0, None), None);
        assert_eq!(select_rate(&[144.0], f32::NAN, None), None);
    }
    #[test]
    fn slow_stream_triggers_but_healthy_stream_and_sleep_do_not() {
        let mut monitor = RateMonitor::new(144.0);
        for _ in 0..2400 {
            assert!(
                monitor
                    .observe(
                        Duration::from_secs_f64(1.0 / 144.0),
                        Duration::from_millis(2),
                        Duration::ZERO
                    )
                    .is_none()
            );
        }
        assert!(
            monitor
                .observe(Duration::from_secs(3), Duration::ZERO, Duration::ZERO)
                .is_none()
        );
        let mut reason = None;
        for _ in 0..2000 {
            reason = reason.or(monitor.observe(
                Duration::from_secs_f64(1.0 / 120.0),
                Duration::from_millis(2),
                Duration::ZERO,
            ));
        }
        assert!(reason.is_some());
    }
    #[test]
    fn resolution_and_bit_depth_must_match_the_same_decoder() {
        let caps = FrameConnectCapabilities {
            decoders: vec![HardwareDecoder {
                name: "hardware.av1".into(),
                mime: "video/av01".into(),
                ten_bit: false,
                modes: vec![DecodeMode {
                    width: 4352,
                    height: 2176,
                    fps: 90.0,
                }],
                rate_source: "mediacodec-size-and-rate".into(),
            }],
            ..Default::default()
        };
        assert!(
            caps.decoder("video/av01", 4352, 2176, 90.0, false)
                .is_some()
        );
        assert!(caps.decoder("video/av01", 4352, 2176, 90.0, true).is_none());
        assert!(
            caps.decoder("video/av01", 5760, 2880, 90.0, false)
                .is_none()
        );
        assert!(
            caps.decoder("video/hevc", 4352, 2176, 90.0, false)
                .is_none()
        );
    }
}
