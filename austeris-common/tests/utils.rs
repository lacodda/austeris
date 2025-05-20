// Tests for utility functions
use austeris_common::utils::datetime::{format_iso8601, parse_iso8601};

#[tokio::test]
async fn test_datetime_format_parse() {
    let input = "2025-05-20T12:00:00.000000000Z";
    let parsed = parse_iso8601(input).unwrap();
    let formatted = format_iso8601(parsed);
    assert_eq!(formatted, input);

    let invalid_input = "2025-13-01T00:00:00.000000000Z";
    assert!(parse_iso8601(invalid_input).is_err());
}