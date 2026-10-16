use alvr_common::SlidingWindowAverage;
use alvr_packets::ClientStatistics;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

struct HistoryFrame {
    submitted: bool,
    reported: bool,
    input_acquired: Instant,
    video_packet_received: Instant,
    client_stats: ClientStatistics,
}

pub struct StatisticsManager {
    history_buffer: VecDeque<HistoryFrame>,
    max_history_size: usize,
    prev_vsync: Instant,
    total_pipeline_latency_average: SlidingWindowAverage<Duration>,
}

impl StatisticsManager {
    pub fn new(max_history_size: usize) -> Self {
        Self {
            max_history_size,
            history_buffer: VecDeque::new(),
            prev_vsync: Instant::now(),
            total_pipeline_latency_average: SlidingWindowAverage::new(
                Duration::ZERO,
                max_history_size,
            ),
        }
    }

    pub fn report_input_acquired(&mut self, target_timestamp: Duration) {
        if !self
            .history_buffer
            .iter()
            .any(|frame| frame.client_stats.target_timestamp == target_timestamp)
        {
            self.history_buffer.push_front(HistoryFrame {
                submitted: false,
                reported: false,
                input_acquired: Instant::now(),
                // this is just a placeholder because Instant does not have a default value
                video_packet_received: Instant::now(),
                client_stats: ClientStatistics {
                    target_timestamp,
                    ..Default::default()
                },
            });
        }

        if self.history_buffer.len() > self.max_history_size {
            self.history_buffer.pop_back();
        }
    }

    pub fn report_video_packet_received(&mut self, target_timestamp: Duration) {
        if let Some(frame) = self
            .history_buffer
            .iter_mut()
            .find(|frame| frame.client_stats.target_timestamp == target_timestamp)
        {
            frame.video_packet_received = Instant::now();
        }
    }

    pub fn report_frame_decoded(&mut self, target_timestamp: Duration) {
        if let Some(frame) = self
            .history_buffer
            .iter_mut()
            .find(|frame| frame.client_stats.target_timestamp == target_timestamp)
        {
            frame.client_stats.video_decode =
                Instant::now().saturating_duration_since(frame.video_packet_received);
        }
    }

    pub fn report_compositor_start(&mut self, target_timestamp: Duration) {
        if let Some(frame) = self
            .history_buffer
            .iter_mut()
            .find(|frame| frame.client_stats.target_timestamp == target_timestamp)
        {
            frame.client_stats.video_decoder_queue = Instant::now().saturating_duration_since(
                frame.video_packet_received + frame.client_stats.video_decode,
            );
        }
    }

    // vsync_queue is the latency between this call and the vsync. it cannot be measured by ALVR and
    // should be reported by the VR runtime
    pub fn report_submit(&mut self, target_timestamp: Duration, vsync_queue: Duration) {
        let now = Instant::now();

        if let Some(frame) = self.history_buffer.iter_mut().find(|frame| {
            frame.client_stats.target_timestamp == target_timestamp && !frame.submitted
        }) {
            frame.submitted = true;
            frame.client_stats.rendering = now.saturating_duration_since(
                frame.video_packet_received
                    + frame.client_stats.video_decode
                    + frame.client_stats.video_decoder_queue,
            );
            frame.client_stats.vsync_queue = vsync_queue;
            frame.client_stats.total_pipeline_latency =
                now.saturating_duration_since(frame.input_acquired) + vsync_queue;
            self.total_pipeline_latency_average
                .submit_sample(frame.client_stats.total_pipeline_latency);

            let vsync = now + vsync_queue;
            frame.client_stats.frame_interval = vsync.saturating_duration_since(self.prev_vsync);
            self.prev_vsync = vsync;
        }
    }

    pub fn summary(&mut self, target_timestamp: Duration) -> Option<ClientStatistics> {
        self.history_buffer
            .iter_mut()
            .find(|frame| {
                frame.client_stats.target_timestamp == target_timestamp
                    && frame.submitted
                    && !frame.reported
            })
            .map(|frame| {
                frame.reported = true;
                frame.client_stats.clone()
            })
    }

    // latency used for head prediction
    pub fn average_total_pipeline_latency(&self) -> Duration {
        self.total_pipeline_latency_average.get_average()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reprojection_does_not_count_as_a_new_video_frame() {
        let mut stats = StatisticsManager::new(8);
        let timestamp = Duration::from_secs(1);
        stats.report_input_acquired(timestamp);
        assert!(stats.summary(timestamp).is_none());
        stats.report_submit(timestamp, Duration::from_millis(5));
        let vsync = stats.prev_vsync;
        let original = stats.summary(timestamp).unwrap();
        stats.report_submit(timestamp, Duration::from_millis(100));
        assert_eq!(stats.prev_vsync, vsync);
        assert!(stats.summary(timestamp).is_none());
        assert_eq!(original.vsync_queue, Duration::from_millis(5));
        let next = timestamp + Duration::from_millis(10);
        stats.report_input_acquired(next);
        stats.report_submit(next, Duration::from_millis(10));
        assert!(stats.summary(next).is_some());
    }

    #[test]
    fn expired_and_unknown_frames_do_not_change_cadence() {
        let mut stats = StatisticsManager::new(1);
        stats.report_input_acquired(Duration::from_secs(1));
        stats.report_input_acquired(Duration::from_secs(2));
        let vsync = stats.prev_vsync;
        stats.report_submit(Duration::from_secs(1), Duration::from_secs(1));
        assert_eq!(stats.prev_vsync, vsync);
        assert!(stats.summary(Duration::from_secs(1)).is_none());
    }
}
