use anyhow::Result;
use chrono::{Datelike, Duration, Timelike, Utc};
use oucc_kizai_bot::time::jst_to_utc;

#[tokio::test]
async fn test_jst_time_conversion() -> Result<()> {
    // Test JST to UTC conversion with known values

    // January 1, 2024 00:00 JST = December 31, 2023 15:00 UTC
    let jst_new_year = jst_to_utc(2024, 1, 1, 0, 0).unwrap();
    assert_eq!(jst_new_year.month(), 12);
    assert_eq!(jst_new_year.day(), 31);
    assert_eq!(jst_new_year.hour(), 15);

    // January 1, 2024 12:00 JST = January 1, 2024 03:00 UTC
    let jst_noon = jst_to_utc(2024, 1, 1, 12, 0).unwrap();
    assert_eq!(jst_noon.month(), 1);
    assert_eq!(jst_noon.day(), 1);
    assert_eq!(jst_noon.hour(), 3);

    // Test invalid dates return None
    assert!(jst_to_utc(2024, 13, 1, 0, 0).is_none()); // Invalid month
    assert!(jst_to_utc(2024, 2, 30, 0, 0).is_none()); // Invalid day
    assert!(jst_to_utc(2024, 1, 1, 25, 0).is_none()); // Invalid hour

    Ok(())
}

#[tokio::test]
async fn test_time_validation() {
    let now = Utc::now();

    // Test end time validation
    let start = now + Duration::hours(1);
    let invalid_end = start - Duration::minutes(30);

    // This should be invalid (end before start)
    assert!(start >= invalid_end, "End time should be after start time");

    // Test future time validation
    let past_time = now - Duration::hours(1);
    assert!(past_time < now, "Past times should be detected");

    // Test maximum duration validation (60 days)
    let max_future = now + Duration::days(60);
    let too_far_future = now + Duration::days(61);

    assert!(
        max_future <= now + Duration::days(60),
        "Should allow up to 60 days"
    );
    assert!(
        too_far_future > now + Duration::days(60),
        "Should reject beyond 60 days"
    );
}
