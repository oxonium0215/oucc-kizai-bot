use chrono::{DateTime, Timelike, Utc};
use oucc_kizai_bot::time::*;

#[test]
fn test_jst_conversion() {
    let utc_time = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let jst_string = utc_to_jst_string(utc_time);
    assert_eq!(jst_string, "2024/01/01 09:00");
}

#[test]
fn test_jst_to_utc_conversion() {
    let utc_result = jst_to_utc(2024, 1, 1, 9, 0);
    assert!(utc_result.is_some());

    let utc_time = utc_result.unwrap();
    assert_eq!(utc_time.hour(), 0); // Should be midnight UTC
    assert_eq!(utc_time.minute(), 0);
}

#[test]
fn test_is_past_jst() {
    let past_time = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    assert!(is_past_jst(past_time));
}

#[test]
fn test_format_duration() {
    use oucc_kizai_bot::utils::format_duration_minutes;

    assert_eq!(format_duration_minutes(30), "30分");
    assert_eq!(format_duration_minutes(60), "1時間");
    assert_eq!(format_duration_minutes(90), "1時間30分");
    assert_eq!(format_duration_minutes(120), "2時間");
}
