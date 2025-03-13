use anyhow::{anyhow, Result};
use sqlx::types::time::PrimitiveDateTime;
use time::format_description::well_known::Iso8601;

pub mod datetime {
    use super::*;

    // Formats a PrimitiveDateTime to an ISO 8601 string (e.g., "2025-03-13T12:00:00Z").
    pub fn format_iso8601(dt: PrimitiveDateTime) -> String {
        dt.assume_utc()
            .format(&Iso8601::DEFAULT)
            .unwrap_or_else(|_| dt.to_string())
    }

    // Parses an ISO 8601 string into a PrimitiveDateTime.
    pub fn parse_iso8601(s: &str) -> Result<PrimitiveDateTime> {
        PrimitiveDateTime::parse(s, &Iso8601::DEFAULT)
            .map_err(|e| anyhow!("Invalid ISO 8601 date format: {}", e))
    }
}
