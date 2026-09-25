//! Time helpers. All persisted timestamps are Unix epoch milliseconds (UTC).

/// Current time in epoch milliseconds.
pub fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}
