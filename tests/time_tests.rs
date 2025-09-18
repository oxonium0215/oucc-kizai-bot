use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Asia::Tokyo;
use oucc_kizai_bot::time::*;

#[test]
fn test_utc_to_jst_conversion() {
    // Test basic conversion
    let utc_time = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let jst_string = utc_to_jst_string(utc_time);
    assert_eq!(jst_string, "2024/01/01 09:00");
}

#[test]
fn test_jst_to_utc_conversion() {
    // Test basic conversion
    let utc_result = jst_to_utc(2024, 1, 1, 9, 0);
    assert!(utc_result.is_some());

    let utc_time = utc_result.unwrap();
    assert_eq!(utc_time.hour(), 0); // Should be midnight UTC
    assert_eq!(utc_time.minute(), 0);
}

#[test]
fn test_round_trip_conversion() {
    // Test that UTC -> JST -> UTC is consistent
    let original_utc = DateTime::parse_from_rfc3339("2024-06-15T14:30:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // Convert to JST components
    let jst_time = Tokyo.from_utc_datetime(&original_utc.naive_utc());
    let year = jst_time.year();
    let month = jst_time.month();
    let day = jst_time.day();
    let hour = jst_time.hour();
    let minute = jst_time.minute();

    // Convert back to UTC
    let converted_back = jst_to_utc(year, month, day, hour, minute).unwrap();

    // Should match original (ignoring seconds)
    assert_eq!(original_utc.date_naive(), converted_back.date_naive());
    assert_eq!(original_utc.hour(), converted_back.hour());
    assert_eq!(original_utc.minute(), converted_back.minute());
}

#[test]
fn test_jst_no_dst_transitions() {
    // JST doesn't observe daylight saving time, so conversions should be consistent
    // Test dates around traditional DST transition times

    let spring_transition = jst_to_utc(2024, 3, 10, 12, 0).unwrap(); // March 10, 2024
    let fall_transition = jst_to_utc(2024, 11, 3, 12, 0).unwrap(); // November 3, 2024

    // Both should have the same offset (9 hours)
    let spring_jst = Tokyo.from_utc_datetime(&spring_transition.naive_utc());
    let fall_jst = Tokyo.from_utc_datetime(&fall_transition.naive_utc());

    assert_eq!(spring_jst.hour(), 12);
    assert_eq!(fall_jst.hour(), 12);

    // Verify the offset is always +9
    assert_eq!(spring_transition.hour(), 3); // 12 - 9 = 3 UTC
    assert_eq!(fall_transition.hour(), 3); // 12 - 9 = 3 UTC
}

#[test]
fn test_edge_case_times() {
    // Test midnight boundaries
    let jst_midnight = jst_to_utc(2024, 1, 1, 0, 0).unwrap();
    assert_eq!(jst_midnight.hour(), 15); // Previous day 15:00 UTC
    assert_eq!(jst_midnight.day(), 31); // December 31, 2023

    // Test noon
    let jst_noon = jst_to_utc(2024, 1, 1, 12, 0).unwrap();
    assert_eq!(jst_noon.hour(), 3); // 03:00 UTC same day
    assert_eq!(jst_noon.day(), 1); // January 1, 2024

    // Test end of day
    let jst_late = jst_to_utc(2024, 1, 1, 23, 59).unwrap();
    assert_eq!(jst_late.hour(), 14); // 14:59 UTC same day
    assert_eq!(jst_late.minute(), 59);
}

#[test]
fn test_invalid_jst_dates() {
    // Test invalid dates return None
    assert!(jst_to_utc(2024, 13, 1, 12, 0).is_none()); // Invalid month
    assert!(jst_to_utc(2024, 2, 30, 12, 0).is_none()); // Invalid day
    assert!(jst_to_utc(2024, 1, 1, 25, 0).is_none()); // Invalid hour
    assert!(jst_to_utc(2024, 1, 1, 12, 60).is_none()); // Invalid minute
}

#[test]
fn test_leap_year_handling() {
    // Test February 29 in leap year
    let leap_day = jst_to_utc(2024, 2, 29, 12, 0);
    assert!(leap_day.is_some());

    // Test February 29 in non-leap year
    let non_leap_day = jst_to_utc(2023, 2, 29, 12, 0);
    assert!(non_leap_day.is_none());
}

#[test]
fn test_is_past_jst() {
    let future_time = Utc::now() + chrono::Duration::hours(1);
    let past_time = Utc::now() - chrono::Duration::hours(1);

    assert!(!is_past_jst(future_time));
    assert!(is_past_jst(past_time));
}

#[test]
fn test_jst_formatting() {
    let utc_time = DateTime::parse_from_rfc3339("2024-12-25T05:30:00Z")
        .unwrap()
        .with_timezone(&Utc);

    // 05:30 UTC + 9 hours = 14:30 JST
    assert_eq!(utc_to_jst_string(utc_time), "2024/12/25 14:30");
    assert_eq!(utc_to_jst_date_string(utc_time), "2024/12/25");
    assert_eq!(utc_to_jst_time_string(utc_time), "14:30");
}

#[test]
fn test_jst_offset_string() {
    assert_eq!(jst_offset_string(), "JST (UTC+9)");
}
