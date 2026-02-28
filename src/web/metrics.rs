use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use salvo::prelude::*;

static MATRIX_MESSAGES_RECEIVED: AtomicU64 = AtomicU64::new(0);
static MATRIX_MESSAGES_SUCCESS: AtomicU64 = AtomicU64::new(0);
static MATRIX_MESSAGES_FAILED: AtomicU64 = AtomicU64::new(0);
static SLACK_MESSAGES_RECEIVED: AtomicU64 = AtomicU64::new(0);
static SLACK_MESSAGES_SUCCESS: AtomicU64 = AtomicU64::new(0);
static SLACK_MESSAGES_FAILED: AtomicU64 = AtomicU64::new(0);
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static PRESENCE_QUEUE_SIZE: AtomicU64 = AtomicU64::new(0);
static MESSAGES_LATENCY_MS: AtomicU64 = AtomicU64::new(0);
static MESSAGES_LATENCY_COUNT: AtomicU64 = AtomicU64::new(0);
static ACTIVE_USERS: AtomicU64 = AtomicU64::new(0);
static BRIDGED_ROOMS: AtomicU64 = AtomicU64::new(0);
static ERROR_COUNT: AtomicU64 = AtomicU64::new(0);
static EDITS_PROCESSED: AtomicU64 = AtomicU64::new(0);
static DELETES_PROCESSED: AtomicU64 = AtomicU64::new(0);
static ATTACHMENTS_UPLOADED: AtomicU64 = AtomicU64::new(0);
static EMOJI_CONVERTED: AtomicU64 = AtomicU64::new(0);

pub struct Metrics {
    started_at: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }

    pub fn matrix_message_received() {
        MATRIX_MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);
    }

    pub fn matrix_message_success() {
        MATRIX_MESSAGES_SUCCESS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn matrix_message_failed() {
        MATRIX_MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
    }

    pub fn slack_message_received() {
        SLACK_MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);
    }

    pub fn slack_message_success() {
        SLACK_MESSAGES_SUCCESS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn slack_message_failed() {
        SLACK_MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
    }

    pub fn cache_hit() {
        CACHE_HITS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn cache_miss() {
        CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_presence_queue_size(size: u64) {
        PRESENCE_QUEUE_SIZE.store(size, Ordering::Relaxed);
    }

    pub fn record_latency(latency_ms: u64) {
        MESSAGES_LATENCY_MS.fetch_add(latency_ms, Ordering::Relaxed);
        MESSAGES_LATENCY_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_active_users(count: u64) {
        ACTIVE_USERS.store(count, Ordering::Relaxed);
    }

    pub fn set_bridged_rooms(count: u64) {
        BRIDGED_ROOMS.store(count, Ordering::Relaxed);
    }

    pub fn error_occurred() {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    pub fn edit_processed() {
        EDITS_PROCESSED.fetch_add(1, Ordering::Relaxed);
    }

    pub fn delete_processed() {
        DELETES_PROCESSED.fetch_add(1, Ordering::Relaxed);
    }

    pub fn attachment_uploaded() {
        ATTACHMENTS_UPLOADED.fetch_add(1, Ordering::Relaxed);
    }

    pub fn emoji_converted() {
        EMOJI_CONVERTED.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn format_prometheus() -> String {
    let uptime = Instant::now().elapsed().as_secs();
    let matrix_received = MATRIX_MESSAGES_RECEIVED.load(Ordering::Relaxed);
    let matrix_success = MATRIX_MESSAGES_SUCCESS.load(Ordering::Relaxed);
    let matrix_failed = MATRIX_MESSAGES_FAILED.load(Ordering::Relaxed);
    let slack_received = SLACK_MESSAGES_RECEIVED.load(Ordering::Relaxed);
    let slack_success = SLACK_MESSAGES_SUCCESS.load(Ordering::Relaxed);
    let slack_failed = SLACK_MESSAGES_FAILED.load(Ordering::Relaxed);
    let cache_hits = CACHE_HITS.load(Ordering::Relaxed);
    let cache_misses = CACHE_MISSES.load(Ordering::Relaxed);
    let presence_queue = PRESENCE_QUEUE_SIZE.load(Ordering::Relaxed);
    let latency_total = MESSAGES_LATENCY_MS.load(Ordering::Relaxed);
    let latency_count = MESSAGES_LATENCY_COUNT.load(Ordering::Relaxed);
    let active_users = ACTIVE_USERS.load(Ordering::Relaxed);
    let bridged_rooms = BRIDGED_ROOMS.load(Ordering::Relaxed);
    let error_count = ERROR_COUNT.load(Ordering::Relaxed);
    let edits = EDITS_PROCESSED.load(Ordering::Relaxed);
    let deletes = DELETES_PROCESSED.load(Ordering::Relaxed);
    let attachments = ATTACHMENTS_UPLOADED.load(Ordering::Relaxed);
    let emoji = EMOJI_CONVERTED.load(Ordering::Relaxed);

    let total_cache = cache_hits + cache_misses;
    let cache_hit_rate = if total_cache > 0 {
        (cache_hits as f64 / total_cache as f64) * 100.0
    } else {
        0.0
    };

    let avg_latency = if latency_count > 0 {
        latency_total as f64 / latency_count as f64
    } else {
        0.0
    };

    format!(
        r#"# HELP bridge_uptime_seconds Number of seconds the bridge has been running
# TYPE bridge_uptime_seconds gauge
bridge_uptime_seconds {}

# HELP matrix_messages_received Total number of Matrix messages received
# TYPE matrix_messages_received counter
matrix_messages_received {}

# HELP matrix_messages_success Number of Matrix messages successfully processed
# TYPE matrix_messages_success counter
matrix_messages_success {}

# HELP matrix_messages_failed Number of Matrix messages that failed to process
# TYPE matrix_messages_failed counter
matrix_messages_failed {}

# HELP slack_messages_received Total number of Slack messages received
# TYPE slack_messages_received counter
slack_messages_received {}

# HELP slack_messages_success Number of Slack messages successfully processed
# TYPE slack_messages_success counter
slack_messages_success {}

# HELP slack_messages_failed Number of Slack messages that failed to process
# TYPE slack_messages_failed counter
slack_messages_failed {}

# HELP cache_hits_total Number of cache hits
# TYPE cache_hits_total counter
cache_hits_total {}

# HELP cache_misses_total Number of cache misses
# TYPE cache_misses_total counter
cache_misses_total {}

# HELP cache_hit_rate_percent Cache hit rate as percentage
# TYPE cache_hit_rate_percent gauge
cache_hit_rate_percent {}

# HELP presence_queue_size Current size of presence queue
# TYPE presence_queue_size gauge
presence_queue_size {}

# HELP message_latency_avg_ms Average message processing latency in milliseconds
# TYPE message_latency_avg_ms gauge
message_latency_avg_ms {}

# HELP active_users_total Number of active bridged users
# TYPE active_users_total gauge
active_users_total {}

# HELP bridged_rooms_total Number of bridged rooms
# TYPE bridged_rooms_total gauge
bridged_rooms_total {}

# HELP errors_total Total number of errors encountered
# TYPE errors_total counter
errors_total {}

# HELP edits_processed_total Total number of message edits processed
# TYPE edits_processed_total counter
edits_processed_total {}

# HELP deletes_processed_total Total number of message deletes processed
# TYPE deletes_processed_total counter
deletes_processed_total {}

# HELP attachments_uploaded_total Total number of attachments uploaded
# TYPE attachments_uploaded_total counter
attachments_uploaded_total {}

# HELP emoji_converted_total Total number of emojis converted
# TYPE emoji_converted_total counter
emoji_converted_total {}
"#,
        uptime,
        matrix_received,
        matrix_success,
        matrix_failed,
        slack_received,
        slack_success,
        slack_failed,
        cache_hits,
        cache_misses,
        cache_hit_rate,
        presence_queue,
        avg_latency,
        active_users,
        bridged_rooms,
        error_count,
        edits,
        deletes,
        attachments,
        emoji,
    )
}

#[handler]
pub async fn metrics_endpoint(res: &mut Response) {
    res.headers_mut()
        .insert("Content-Type", "text/plain; charset=utf-8".parse().unwrap());
    res.body(format_prometheus());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_increments_counters() {
        Metrics::matrix_message_received();
        Metrics::matrix_message_success();
        Metrics::slack_message_received();
        Metrics::slack_message_failed();
        Metrics::cache_hit();
        Metrics::cache_miss();
        Metrics::edit_processed();
        Metrics::delete_processed();
        Metrics::attachment_uploaded();
        Metrics::emoji_converted();

        assert_eq!(MATRIX_MESSAGES_RECEIVED.load(Ordering::Relaxed), 1);
        assert_eq!(MATRIX_MESSAGES_SUCCESS.load(Ordering::Relaxed), 1);
        assert_eq!(SLACK_MESSAGES_RECEIVED.load(Ordering::Relaxed), 1);
        assert_eq!(SLACK_MESSAGES_FAILED.load(Ordering::Relaxed), 1);
        assert_eq!(CACHE_HITS.load(Ordering::Relaxed), 1);
        assert_eq!(CACHE_MISSES.load(Ordering::Relaxed), 1);
        assert_eq!(EDITS_PROCESSED.load(Ordering::Relaxed), 1);
        assert_eq!(DELETES_PROCESSED.load(Ordering::Relaxed), 1);
        assert_eq!(ATTACHMENTS_UPLOADED.load(Ordering::Relaxed), 1);
        assert_eq!(EMOJI_CONVERTED.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn format_prometheus_includes_all_metrics() {
        let output = format_prometheus();
        assert!(output.contains("bridge_uptime_seconds"));
        assert!(output.contains("matrix_messages_received"));
        assert!(output.contains("slack_messages_received"));
        assert!(output.contains("cache_hits_total"));
        assert!(output.contains("presence_queue_size"));
        assert!(output.contains("message_latency_avg_ms"));
        assert!(output.contains("active_users_total"));
        assert!(output.contains("bridged_rooms_total"));
        assert!(output.contains("errors_total"));
        assert!(output.contains("edits_processed_total"));
        assert!(output.contains("deletes_processed_total"));
        assert!(output.contains("attachments_uploaded_total"));
        assert!(output.contains("emoji_converted_total"));
    }
}
