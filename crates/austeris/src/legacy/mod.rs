//! `austeris import legacy`: the 2025 tracker's records, brought into the
//! books.
//!
//! The tracker austeris grew out of kept coins - buys and sells on wallets,
//! snapshots of what was held, the prices it fetched - in a database of its
//! own. This reads a plain `pg_dump` of that database, works out what each
//! record becomes in the books (see `plan`), writes it through the
//! installation's API under keys that make a second run change nothing, and
//! says where the books and the tracker part (ADR 0016).

pub mod dump;
pub mod plan;
pub mod report;
pub mod tracker;
pub mod write;

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::FixedOffset;

use self::dump::Dump;
use self::plan::Zone;
use self::tracker::Tracker;

/// Arguments to `austeris import legacy`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// What `pg_dump` wrote of the 2025 tracker's database: the plain SQL
    /// script, gzipped or not. An archive is turned into one by
    /// `pg_restore --file legacy.sql <archive>`.
    #[arg(value_name = "DUMP")]
    pub dump: PathBuf,

    /// Where you were in 2025, as an offset from UTC: `-03:00`. The day each
    /// trade is booked under is that moment's date there. This machine's time
    /// zone when omitted.
    #[arg(long, value_name = "OFFSET", allow_hyphen_values = true)]
    pub offset: Option<String>,

    /// Read the dump and say what it would become, without writing anything
    /// or signing in.
    #[arg(long)]
    pub dry_run: bool,
}

/// Brings the tracker's records into the books, or says what it would.
///
/// # Errors
///
/// Returns an error when the dump cannot be read as the tracker's, the offset
/// is not one, nobody is signed in, or the installation cannot be reached or
/// refuses what the import cannot go on without.
pub async fn run(args: &Args) -> Result<()> {
    let zone = match &args.offset {
        Some(text) => Zone::Fixed(offset(text)?),
        None => Zone::Local,
    };
    let dump = Dump::read(&args.dump)?;
    let tracker = Tracker::read(&dump)?;
    let plan = plan::plan(&tracker, zone);
    let written = if args.dry_run {
        None
    } else {
        let api = write::Api::signed_in()?;
        Some(write::write(&plan, &api).await?)
    };
    print!("{}", report::report(&args.dump, &tracker, &plan, zone, written.as_ref()));
    Ok(())
}

/// An offset as a person types it: `-03:00`, `+3`, `UTC`.
fn offset(text: &str) -> Result<FixedOffset> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("utc") || text.eq_ignore_ascii_case("z") {
        return Ok(FixedOffset::east_opt(0).unwrap_or_else(|| unreachable!("zero is an offset")));
    }
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'+') => (1, &text[1..]),
        Some(b'-') => (-1, &text[1..]),
        _ => (1, text),
    };
    let (hours, minutes) = rest.split_once(':').unwrap_or((rest, "0"));
    let parsed = hours
        .parse::<i32>()
        .ok()
        .zip(minutes.parse::<i32>().ok())
        .filter(|(hours, minutes)| *hours <= 14 && *minutes < 60)
        .and_then(|(hours, minutes)| FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60)));
    parsed.with_context(|| format!("`{text}` is not an offset from UTC; write it as -03:00"))
}

#[cfg(test)]
mod tests {
    use super::offset;

    #[test]
    fn an_offset_is_read_as_it_is_typed() {
        assert_eq!(offset("-03:00").expect("an offset").local_minus_utc(), -3 * 3600);
        assert_eq!(offset("+5:30").expect("an offset").local_minus_utc(), 5 * 3600 + 30 * 60);
        assert_eq!(offset("-3").expect("an offset").local_minus_utc(), -3 * 3600);
        assert_eq!(offset("UTC").expect("an offset").local_minus_utc(), 0);
        assert!(offset("Asuncion").is_err());
        assert!(offset("+25:00").is_err());
    }
}
