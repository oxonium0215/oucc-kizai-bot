use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Asia::Tokyo;

/// Convert UTC DateTime to JST formatted string
pub fn utc_to_jst_string(utc_time: DateTime<Utc>) -> String {
    let jst_time = Tokyo.from_utc_datetime(&utc_time.naive_utc());
    jst_time.format("%Y/%m/%d %H:%M").to_string()
}

/// Convert UTC DateTime to JST date string
pub fn utc_to_jst_date_string(utc_time: DateTime<Utc>) -> String {
    let jst_time = Tokyo.from_utc_datetime(&utc_time.naive_utc());
    jst_time.format("%Y/%m/%d").to_string()
}

/// Convert UTC DateTime to JST time string
pub fn utc_to_jst_time_string(utc_time: DateTime<Utc>) -> String {
    let jst_time = Tokyo.from_utc_datetime(&utc_time.naive_utc());
    jst_time.format("%H:%M").to_string()
}

/// Get current time in JST as formatted string
pub fn now_jst_string() -> String {
    let now_utc = Utc::now();
    utc_to_jst_string(now_utc)
}

/// Parse JST date/time and convert to UTC
pub fn jst_to_utc(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
) -> Option<DateTime<Utc>> {
    let jst_naive = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.and_hms_opt(hour, minute, 0))?;

    let jst_time = Tokyo.from_local_datetime(&jst_naive).single()?;

    Some(jst_time.with_timezone(&Utc))
}

/// Check if a time is in the past (JST comparison)
pub fn is_past_jst(utc_time: DateTime<Utc>) -> bool {
    let now_utc = Utc::now();
    utc_time < now_utc
}

/// Get JST timezone offset string for display
pub fn jst_offset_string() -> &'static str {
    "JST (UTC+9)"
}
