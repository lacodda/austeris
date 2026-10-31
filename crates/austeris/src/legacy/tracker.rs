//! What the 2025 tracker kept, read out of its tables.
//!
//! Its schema: `assets` (a coin, by ticker and `CoinMarketCap` id), `wallets`
//! (a place coins are kept), `transactions` (a buy or a sell of an asset on a
//! wallet, at a price, with a fee), `portfolio_snapshots` (a list of what was
//! held, as JSON) and `asset_prices` (prices in dollars it fetched). Amounts
//! were binary floats and moments had no zone; the server ran in UTC.
//!
//! A row that cannot be read - a number that is not one, a moment that is not
//! a date - is set aside with why, rather than failing the file: one bad row
//! of a year's records should cost that row.

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;

use super::dump::{Dump, Table};

/// A coin the tracker knew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Its row.
    pub id: i64,
    /// Its ticker.
    pub symbol: String,
    /// Its name.
    pub name: String,
    /// `CoinMarketCap`'s id for it.
    pub cmc_id: Option<i64>,
    /// How many decimals it has.
    pub decimals: Option<i32>,
}

/// A place coins were kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wallet {
    /// Its row.
    pub id: i64,
    /// What the person called it.
    pub name: String,
    /// What kind it was: `exchange`, `hardware`.
    pub kind: String,
}

/// Which way a transaction went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// Units arrived.
    Buy,
    /// Units left.
    Sell,
}

impl Side {
    /// As the tracker wrote it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }
}

/// A buy or a sell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    /// Its row.
    pub id: i64,
    /// What was bought or sold; nothing when the row lost it.
    pub asset_id: Option<i64>,
    /// Where; nothing when the row lost it.
    pub wallet_id: Option<i64>,
    /// How many units.
    pub amount: Decimal,
    /// What one cost, in dollars.
    pub price: Decimal,
    /// What it cost to make, in dollars.
    pub fee: Option<Decimal>,
    /// Which way.
    pub side: Side,
    /// What the person wrote.
    pub notes: Option<String>,
    /// When it was recorded.
    pub at: DateTime<Utc>,
}

/// One line of a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotLine {
    /// The ticker.
    pub symbol: String,
    /// How many were held.
    pub amount: Decimal,
    /// `CoinMarketCap`'s id.
    pub cmc_id: Option<i64>,
}

/// What was held at a moment, as the tracker kept it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Its row.
    pub id: i64,
    /// When it was taken.
    pub at: DateTime<Utc>,
    /// What it held.
    pub lines: Vec<SnapshotLine>,
}

/// A price the tracker fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPrice {
    /// Of which asset.
    pub asset_id: i64,
    /// In dollars.
    pub price: Decimal,
    /// When.
    pub at: DateTime<Utc>,
}

/// A row set aside, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// The table.
    pub table: &'static str,
    /// Its id, when that much could be read.
    pub id: Option<i64>,
    /// Why.
    pub why: String,
}

/// Everything the tracker kept.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    /// Its coins.
    pub assets: Vec<Asset>,
    /// Its wallets.
    pub wallets: Vec<Wallet>,
    /// Its buys and sells, in the order they were recorded.
    pub transactions: Vec<Transaction>,
    /// Its snapshots, oldest first.
    pub snapshots: Vec<Snapshot>,
    /// Its prices.
    pub prices: Vec<AssetPrice>,
    /// Rows that could not be read.
    pub unreadable: Vec<Unreadable>,
    /// Tables the tracker had by its last version that the dump has none of.
    pub missing: Vec<&'static str>,
    /// Schemas other than the tracker's own that the dump carries.
    pub other_schemas: Vec<String>,
}

/// The tables without which there is nothing to bring.
const REQUIRED: [&str; 3] = ["assets", "wallets", "transactions"];

impl Tracker {
    /// Reads what the tracker kept out of its dump.
    ///
    /// # Errors
    ///
    /// Returns an error when a table there is nothing without is missing, or
    /// a table lacks a column every row needs.
    pub fn read(dump: &Dump) -> Result<Self> {
        for name in REQUIRED {
            if dump.table(name).is_none() {
                if dump.inserts(name) {
                    bail!(
                        "this dump inserts its rows one statement at a time (pg_dump --inserts); make it again without --inserts, as plain `pg_dump --format=plain`"
                    );
                }
                bail!("this dump has no rows of a table called {name}; it is not a dump of the 2025 tracker's database");
            }
        }
        let mut tracker = Self {
            other_schemas: dump.other_schemas(),
            ..Self::default()
        };
        tracker.read_assets(dump.table("assets").unwrap_or(&Table::default()))?;
        tracker.read_wallets(dump.table("wallets").unwrap_or(&Table::default()))?;
        tracker.read_transactions(dump.table("transactions").unwrap_or(&Table::default()))?;
        match dump.table("portfolio_snapshots") {
            Some(table) => tracker.read_snapshots(table)?,
            None => tracker.missing.push("portfolio_snapshots"),
        }
        match dump.table("asset_prices") {
            Some(table) => tracker.read_prices(table)?,
            None => tracker.missing.push("asset_prices"),
        }
        tracker.transactions.sort_by_key(|transaction| (transaction.at, transaction.id));
        tracker.snapshots.sort_by_key(|snapshot| (snapshot.at, snapshot.id));
        Ok(tracker)
    }

    /// The asset of a row, by its id.
    #[must_use]
    pub fn asset(&self, id: i64) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.id == id)
    }

    /// The wallet of a row, by its id.
    #[must_use]
    pub fn wallet(&self, id: i64) -> Option<&Wallet> {
        self.wallets.iter().find(|wallet| wallet.id == id)
    }

    fn read_assets(&mut self, table: &Table) -> Result<()> {
        let columns = Columns::of(table, "assets", &["id", "symbol", "name"])?;
        for row in &table.rows {
            let read = (|| -> Result<Asset, String> {
                Ok(Asset {
                    id: columns.integer(row, "id")?.ok_or("no id")?,
                    symbol: columns.text(row, "symbol").ok_or("no ticker")?.trim().to_owned(),
                    name: columns.text(row, "name").unwrap_or_default().trim().to_owned(),
                    cmc_id: columns.integer(row, "cmc_id")?,
                    decimals: columns.integer(row, "decimals")?.and_then(|value| i32::try_from(value).ok()),
                })
            })();
            match read {
                Ok(asset) => self.assets.push(asset),
                Err(why) => self.set_aside("assets", &columns, row, why),
            }
        }
        Ok(())
    }

    fn read_wallets(&mut self, table: &Table) -> Result<()> {
        let columns = Columns::of(table, "wallets", &["id", "name"])?;
        for row in &table.rows {
            let read = (|| -> Result<Wallet, String> {
                Ok(Wallet {
                    id: columns.integer(row, "id")?.ok_or("no id")?,
                    name: columns.text(row, "name").ok_or("no name")?.trim().to_owned(),
                    kind: columns.text(row, "type").unwrap_or_default().trim().to_owned(),
                })
            })();
            match read {
                Ok(wallet) => self.wallets.push(wallet),
                Err(why) => self.set_aside("wallets", &columns, row, why),
            }
        }
        Ok(())
    }

    fn read_transactions(&mut self, table: &Table) -> Result<()> {
        let columns = Columns::of(table, "transactions", &["id", "asset_id", "wallet_id", "amount", "price", "type", "created_at"])?;
        for row in &table.rows {
            let read = (|| -> Result<Transaction, String> {
                Ok(Transaction {
                    id: columns.integer(row, "id")?.ok_or("no id")?,
                    asset_id: columns.integer(row, "asset_id")?,
                    wallet_id: columns.integer(row, "wallet_id")?,
                    amount: columns.number(row, "amount")?.ok_or("no amount")?,
                    price: columns.number(row, "price")?.ok_or("no price")?,
                    fee: columns.number(row, "fee")?,
                    side: match columns.text(row, "type").map(|kind| kind.trim().to_ascii_uppercase()).as_deref() {
                        Some("BUY") => Side::Buy,
                        Some("SELL") => Side::Sell,
                        other => return Err(format!("`{}` is neither a buy nor a sell", other.unwrap_or_default())),
                    },
                    notes: columns
                        .text(row, "notes")
                        .map(|notes| notes.trim().to_owned())
                        .filter(|notes| !notes.is_empty()),
                    at: columns.moment(row, "created_at")?.ok_or("no moment")?,
                })
            })();
            match read {
                Ok(transaction) => self.transactions.push(transaction),
                Err(why) => self.set_aside("transactions", &columns, row, why),
            }
        }
        Ok(())
    }

    fn read_snapshots(&mut self, table: &Table) -> Result<()> {
        let columns = Columns::of(table, "portfolio_snapshots", &["id", "created_at", "assets"])?;
        for row in &table.rows {
            let read = (|| -> Result<Snapshot, String> {
                let id = columns.integer(row, "id")?.ok_or("no id")?;
                let at = columns.moment(row, "created_at")?.ok_or("no moment")?;
                let json: serde_json::Value =
                    serde_json::from_str(columns.text(row, "assets").ok_or("no list")?).map_err(|error| format!("its list is not JSON: {error}"))?;
                let lines = json
                    .as_array()
                    .ok_or("its list is not a list")?
                    .iter()
                    .map(|line| {
                        let amount = match &line["amount"] {
                            serde_json::Value::Number(number) => decimal(&number.to_string())?,
                            serde_json::Value::String(text) => decimal(text)?,
                            _ => return Err("a line with no amount".to_owned()),
                        };
                        Ok(SnapshotLine {
                            symbol: line["symbol"].as_str().ok_or("a line with no ticker")?.trim().to_owned(),
                            amount,
                            cmc_id: line["cmc_id"].as_i64(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Snapshot { id, at, lines })
            })();
            match read {
                Ok(snapshot) => self.snapshots.push(snapshot),
                Err(why) => self.set_aside("portfolio_snapshots", &columns, row, why),
            }
        }
        Ok(())
    }

    fn read_prices(&mut self, table: &Table) -> Result<()> {
        let columns = Columns::of(table, "asset_prices", &["asset_id", "price_usd", "timestamp"])?;
        for row in &table.rows {
            let read = (|| -> Result<AssetPrice, String> {
                Ok(AssetPrice {
                    asset_id: columns.integer(row, "asset_id")?.ok_or("no asset")?,
                    price: columns.number(row, "price_usd")?.ok_or("no price")?,
                    at: columns.moment(row, "timestamp")?.ok_or("no moment")?,
                })
            })();
            match read {
                Ok(price) => self.prices.push(price),
                Err(why) => self.set_aside("asset_prices", &columns, row, why),
            }
        }
        Ok(())
    }

    fn set_aside(&mut self, table: &'static str, columns: &Columns<'_>, row: &[Option<String>], why: String) {
        self.unreadable.push(Unreadable {
            table,
            id: columns.integer(row, "id").ok().flatten(),
            why,
        });
    }
}

/// A table's columns by name.
struct Columns<'a> {
    table: &'a Table,
}

impl<'a> Columns<'a> {
    /// The columns of a table, which must have those named.
    fn of(table: &'a Table, name: &str, needed: &[&str]) -> Result<Self> {
        let missing: Vec<&&str> = needed.iter().filter(|column| table.column(column).is_none()).collect();
        if !missing.is_empty() {
            bail!(
                "the {name} table of this dump has no {} column; it is not the 2025 tracker's",
                missing.iter().map(|column| (**column).to_owned()).collect::<Vec<_>>().join(", ")
            );
        }
        Ok(Self { table })
    }

    fn text<'r>(&self, row: &'r [Option<String>], name: &str) -> Option<&'r str> {
        self.table.column(name).and_then(|at| row.get(at)).and_then(Option::as_deref)
    }

    fn integer(&self, row: &[Option<String>], name: &str) -> Result<Option<i64>, String> {
        self.text(row, name)
            .map(|text| text.trim().parse::<i64>().map_err(|_| format!("{name} `{text}` is not a whole number")))
            .transpose()
    }

    fn number(&self, row: &[Option<String>], name: &str) -> Result<Option<Decimal>, String> {
        self.text(row, name)
            .map(|text| decimal(text).map_err(|why| format!("{name} {why}")))
            .transpose()
    }

    fn moment(&self, row: &[Option<String>], name: &str) -> Result<Option<DateTime<Utc>>, String> {
        self.text(row, name).map(moment).transpose()
    }
}

/// The significant digits a binary float keeps of a decimal: any number of
/// fifteen digits or fewer goes into one and comes back out the same.
const FLOAT_DIGITS: u32 = 15;

/// A float as PostgreSQL prints one - `0.1`, `1e-05`, `298.29999999999995` -
/// as the decimal it stands for: its first fifteen significant digits. What
/// follows them is how binary spells a sum like 2000 - 1701.7, not anything
/// anyone typed, and carried into the books it would be a coin's quantity
/// that never existed. `NaN` and the infinities are no amount of anything.
///
/// # Errors
///
/// Returns why when it is not a finite number.
pub fn decimal(text: &str) -> Result<Decimal, String> {
    let text = text.trim();
    Decimal::from_str(text)
        .or_else(|_| Decimal::from_scientific(text))
        .map(|number| number.round_sf(FLOAT_DIGITS).unwrap_or(number).normalize())
        .map_err(|_| format!("`{text}` is not a number"))
}

/// A moment as PostgreSQL prints a `timestamp` - in UTC, the tracker's
/// server's zone - or a `timestamptz`, with its offset.
fn moment(text: &str) -> Result<DateTime<Utc>, String> {
    let text = text.trim();
    if let Ok(naive) = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(naive.and_utc());
    }
    // `+00`, `-03`, `+05:30`: chrono wants minutes, PostgreSQL leaves them out.
    let padded = match text.rfind(['+', '-']) {
        Some(at) if text.len() - at == 3 => format!("{text}:00"),
        _ => text.to_owned(),
    };
    DateTime::parse_from_str(&padded, "%Y-%m-%d %H:%M:%S%.f%:z")
        .map(|moment| moment.with_timezone(&Utc))
        .map_err(|_| format!("`{text}` is not a moment"))
}

/// How many of each asset, by its ticker.
pub type Holdings = BTreeMap<String, Decimal>;

#[cfg(test)]
mod tests {
    use super::{Side, Tracker, decimal, moment};
    use crate::legacy::dump::Dump;

    #[test]
    fn a_float_is_the_decimal_it_stands_for() {
        assert_eq!(decimal("0.1").expect("a number").to_string(), "0.1");
        // A sum of floats, and the decimal it was.
        assert_eq!(decimal("298.29999999999995").expect("a number").to_string(), "298.3");
        assert_eq!(decimal("0.30000000000000004").expect("a number").to_string(), "0.3");
        // Fifteen digits are a float's own, and kept.
        assert_eq!(decimal("0.123456789012345").expect("a number").to_string(), "0.123456789012345");
        assert_eq!(decimal("84123.4567890123").expect("a number").to_string(), "84123.4567890123");
        assert_eq!(decimal("1e-05").expect("a number").to_string(), "0.00001");
        assert_eq!(decimal("85000").expect("a number").to_string(), "85000");
        assert!(decimal("NaN").is_err());
        assert!(decimal("Infinity").is_err());
    }

    #[test]
    fn a_moment_without_a_zone_is_utc_and_one_with_a_zone_is_converted() {
        assert_eq!(moment("2025-03-05 14:10:00").expect("a moment").to_rfc3339(), "2025-03-05T14:10:00+00:00");
        assert_eq!(moment("2025-03-05 14:10:00.5").expect("a moment").timestamp_millis() % 1000, 500);
        assert_eq!(moment("2025-03-05 11:10:00-03").expect("a moment").to_rfc3339(), "2025-03-05T14:10:00+00:00");
        assert!(moment("yesterday").is_err());
    }

    #[test]
    fn a_row_that_cannot_be_read_is_set_aside_and_the_rest_are_read() {
        let dump = Dump::parse(
            "COPY public.assets (id, symbol, name, cmc_id, decimals, rank, created_at) FROM stdin;\n\
             1\tBTC\tBitcoin\t1\t8\t1\t2025-03-01 10:00:00\n\\.\n\
             COPY public.wallets (id, name, type, address, created_at) FROM stdin;\n\
             1\tBinance\texchange\t\\N\t2025-03-01 10:00:00\n\\.\n\
             COPY public.transactions (id, asset_id, wallet_id, amount, price, fee, type, notes, created_at) FROM stdin;\n\
             1\t1\t1\t0.5\t60000\t\\N\tBUY\t\\N\t2025-03-02 10:00:00\n\
             2\t1\t1\tNaN\t60000\t\\N\tBUY\t\\N\t2025-03-02 10:00:00\n\
             3\t1\t1\t0.1\t61000\t1\tSWAP\t\\N\t2025-03-02 10:00:00\n\\.\n",
        )
        .expect("a dump");
        let tracker = Tracker::read(&dump).expect("the tracker's records");
        assert_eq!(tracker.transactions.len(), 1);
        assert_eq!(tracker.transactions[0].side, Side::Buy);
        let set_aside: Vec<Option<i64>> = tracker.unreadable.iter().map(|row| row.id).collect();
        assert_eq!(set_aside, [Some(2), Some(3)]);
        assert_eq!(tracker.missing, ["portfolio_snapshots", "asset_prices"]);
    }

    #[test]
    fn a_dump_of_something_else_is_said_to_be() {
        let error = Tracker::read(&Dump::parse("COPY public.notes (id) FROM stdin;\n\\.\n").expect("a dump")).expect_err("not the tracker");
        assert!(error.to_string().contains("not a dump of the 2025 tracker"), "{error}");
    }
}
