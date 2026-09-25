//! How old a published rate may be before it is no longer today's.
//!
//! One number for every service that shows a rate, so the ledger converting a
//! balance and the market listing a price call the same rate stale on the same
//! day.

/// Days a daily rate stays current after the day it was set for.
///
/// Central banks publish on business days, so a Monday morning runs on
/// Friday's rate and a long weekend on Thursday's: four days covers a weekend
/// with a holiday on either side. Older than that, rates have stopped arriving,
/// and a number that looks current is what this exists to prevent.
pub const DAILY_RATE_DAYS: i64 = 4;
