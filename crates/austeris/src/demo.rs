//! The `demo` subcommand: a fictional household, so an installation is not
//! empty on the day it is first opened.
//!
//! Everything here is made up - the people, the accounts, the amounts and the
//! prices - which is the point: screenshots for the documentation, and a stand
//! somebody can look at, without a real number anywhere near either.
//!
//! It writes through each service's own repository, one schema at a time, so
//! the demo obeys the same rules a person typing would: the ledger refuses an
//! entry that does not balance whether a person or this file posted it. This is
//! the composition root, the one place allowed to hold every service's pool.
//!
//! Seeding is idempotent. Accounts and categories are found by name before
//! they are created, entries carry an idempotency key made from their day, and
//! prices are upserted - so running it again fills in the days since the last
//! run and duplicates nothing, and a run that died halfway is finished by the
//! next one.

use anyhow::{Context, Result, bail};
use austeris_common::{Config, db};
use austeris_ledger::exchange::{self, Money};
use austeris_ledger::model::{AccountKind, Flow, Purpose};
use austeris_ledger::repository::{self as ledger, NewEntry, NewLine, Side};
use austeris_market::model::Kind;
use austeris_market::repository as market;
use chrono::{Datelike, Days, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::service::Service;

/// The address the demo household signs in with.
pub const EMAIL: &str = "demo@austeris.local";

/// Its password. Printed, documented, and meant to be: nothing behind it is
/// real, and the demo refuses to share an installation with anything that is.
pub const PASSWORD: &str = "austeris-demo";

/// How far back the books go.
const DAYS: u64 = 120;

/// Where each entry and price says it came from.
const SOURCE: &str = "demo";

/// Whether `AUSTERIS_DEMO` asks `serve` to seed before it starts.
#[must_use]
pub fn requested() -> bool {
    std::env::var("AUSTERIS_DEMO").is_ok_and(|value| value == "true")
}

/// The pools the demo writes to, one per schema it touches.
pub struct Books {
    /// People and sessions.
    pub identity: PgPool,
    /// Accounts, categories and entries.
    pub ledger: PgPool,
    /// Instruments and prices.
    pub market: PgPool,
}

/// What a seeding run did.
#[derive(Debug)]
pub struct Seeded {
    /// Whether the household was created by this run rather than found.
    pub created: bool,
    /// Entries posted by this run; earlier days are already there.
    pub entries: usize,
}

impl Seeded {
    /// Says what happened and how to sign in, where an operator will see it.
    ///
    /// Printed rather than logged, like the first account's password: this is
    /// something a person reads off a terminal and types.
    pub fn report(&self) {
        println!();
        if self.created {
            println!("  A demo household was created. Everything in it is made up.");
        } else {
            println!("  The demo household is here; {} new entries bring it up to today.", self.entries);
        }
        println!();
        println!("      email:    {EMAIL}");
        println!("      password: {PASSWORD}");
        println!();
    }
}

/// Runs the subcommand: brings every schema up and seeds it.
///
/// # Errors
///
/// Returns an error when the database cannot be reached, when a real person
/// already has an account here, or when a write fails.
pub async fn run() -> Result<()> {
    let config = Config::from_env()?;
    let open = |service: Service| {
        let config = config.clone();
        async move {
            let (Some(schema), Some(migrator)) = (service.schema(), service.migrator()) else {
                bail!("{service} owns no schema");
            };
            let pool = db::connect(&config, schema).await?;
            austeris_common::migrate::run(&pool, migrator)
                .await
                .with_context(|| format!("migrating the {schema} schema"))?;
            Ok(pool)
        }
    };

    let books = Books {
        identity: open(Service::Identity).await?,
        ledger: open(Service::Ledger).await?,
        market: open(Service::Market).await?,
    };
    seed(&books).await?.report();
    Ok(())
}

/// Fills the installation with the demo household, or tops it up to today.
///
/// # Errors
///
/// Returns an error when anyone but the demo household can sign in here - the
/// demo writes shared prices a real installation relies on, so it never shares
/// one - or when a write fails.
pub async fn seed(books: &Books) -> Result<Seeded> {
    let people = austeris_identity::routes::people(&books.identity).await?;
    let others: Vec<&String> = people.iter().filter(|email| !email.eq_ignore_ascii_case(EMAIL)).collect();
    if !others.is_empty() {
        bail!(
            "this installation has {} account(s) that are not the demo's; the demo only fills an empty one (start from an empty database to see it)",
            others.len()
        );
    }

    let (owner, created) = match austeris_identity::routes::person(&books.identity, EMAIL).await? {
        Some(id) => (id, false),
        None => (austeris_identity::routes::create_person(&books.identity, EMAIL, PASSWORD).await?, true),
    };

    let today = Utc::now().date_naive();
    let first = today - Days::new(DAYS);

    let household = Household::open(&books.ledger, owner).await?;
    let mut entries = 0;
    for day in first.iter_days().take_while(|day| *day <= today) {
        // What a dollar cost in euros, for the one account kept in dollars -
        // recorded before the day's entries, because the exchange before a trip
        // is measured against it.
        ledger::record_rate(&books.ledger, "USD", "EUR", day, usd_in_eur(day), SOURCE).await?;
        let mut day_entries = household.day(day);
        if let Some(exchange) = household.exchange(&books.ledger, owner, day).await? {
            day_entries.push(exchange);
        }
        for entry in day_entries {
            if ledger::post_entry(&books.ledger, owner, &entry).await?.created {
                entries += 1;
            }
        }
    }

    prices(&books.market, first, today).await?;
    Ok(Seeded { created, entries })
}

/// The household's accounts and categories, found or created.
struct Household {
    everyday: Uuid,
    cash: Uuid,
    savings: Uuid,
    travel: Uuid,
    salary: Uuid,
    side_work: Uuid,
    rent: Uuid,
    utilities: Uuid,
    groceries: Uuid,
    eating_out: Uuid,
    transport: Uuid,
    health: Uuid,
    fun: Uuid,
    trips: Uuid,
}

impl Household {
    async fn open(pool: &PgPool, owner: Uuid) -> Result<Self> {
        let account = |kind, name: &'static str, currency: &'static str, opening: i64| async move {
            match ledger::account_by_name(pool, owner, name).await? {
                Some(found) => Ok::<_, anyhow::Error>(found.id),
                None => Ok(ledger::create_account(pool, owner, kind, name, currency, Decimal::from(opening)).await?.id),
            }
        };

        let everyday = account(AccountKind::Bank, "Everyday", "EUR", 2_400).await?;
        let cash = account(AccountKind::Cash, "Cash", "EUR", 180).await?;
        let savings = account(AccountKind::Deposit, "Savings", "EUR", 8_000).await?;
        let travel = account(AccountKind::Card, "Travel card", "USD", 1_500).await?;

        let home = category(pool, owner, None, Flow::Expense, "Home").await?;
        let food = category(pool, owner, None, Flow::Expense, "Food").await?;
        Ok(Self {
            everyday,
            cash,
            savings,
            travel,
            salary: category(pool, owner, None, Flow::Income, "Salary").await?,
            side_work: category(pool, owner, None, Flow::Income, "Side work").await?,
            rent: category(pool, owner, Some(home), Flow::Expense, "Rent").await?,
            utilities: category(pool, owner, Some(home), Flow::Expense, "Utilities").await?,
            groceries: category(pool, owner, Some(food), Flow::Expense, "Groceries").await?,
            eating_out: category(pool, owner, Some(food), Flow::Expense, "Eating out").await?,
            transport: category(pool, owner, None, Flow::Expense, "Transport").await?,
            health: category(pool, owner, None, Flow::Expense, "Health").await?,
            fun: category(pool, owner, None, Flow::Expense, "Fun").await?,
            trips: category(pool, owner, None, Flow::Expense, "Trips").await?,
        })
    }

    /// What happened on one day.
    ///
    /// Decided by the day alone: the same day always yields the same entries,
    /// which is what lets a second run recognise the first one's by key.
    fn day(&self, day: NaiveDate) -> Vec<NewEntry> {
        let mut dice = Dice::for_day(day);
        let mut out = Vec::new();
        let mut spend = |key: &str, account: Uuid, category: Uuid, currency: &str, amount: Decimal, what: &str| {
            out.push(movement(day, key, (account, currency), Side::Category(category), -amount, what));
        };

        match day.day() {
            3 => spend("rent", self.everyday, self.rent, "EUR", Decimal::from(1_150), "Rent"),
            11 => spend("power", self.everyday, self.utilities, "EUR", dice.cents(70, 140), "Power and water"),
            _ => {}
        }
        if dice.chance(38) {
            let account = if dice.chance(25) { self.cash } else { self.everyday };
            spend("groceries", account, self.groceries, "EUR", dice.cents(18, 115), "Groceries");
        }
        if day.weekday().number_from_monday() >= 5 && dice.chance(55) {
            spend("out", self.cash, self.eating_out, "EUR", dice.cents(14, 68), "Dinner out");
        }
        if dice.chance(20) {
            spend("transport", self.everyday, self.transport, "EUR", dice.cents(9, 48), "Fuel and tickets");
        }
        if dice.chance(5) {
            spend("health", self.everyday, self.health, "EUR", dice.cents(12, 90), "Pharmacy");
        }
        if dice.chance(9) {
            spend("fun", self.everyday, self.fun, "EUR", dice.cents(10, 75), "Cinema and books");
        }
        // A trip abroad every other month, paid in dollars from the card kept
        // in dollars - so the balances page has a second currency to convert.
        if day.month().is_multiple_of(2) && (12..=16).contains(&day.day()) {
            spend("trip", self.travel, self.trips, "USD", dice.cents(40, 160), "Travelling");
        }

        let mut earn = |key: &str, category: Uuid, amount: Decimal, what: &str| {
            out.push(movement(day, key, (self.everyday, "EUR"), Side::Category(category), amount, what));
        };
        if day.day() == 1 {
            earn("salary", self.salary, Decimal::from(3_400), "Salary");
        }
        if day.day() == 18 && !day.month().is_multiple_of(3) {
            earn("side", self.side_work, dice.cents(250, 620), "Invoice paid");
        }

        // Money moved between the household's own accounts: the case double
        // entry makes the same as any other movement.
        if day.weekday().number_from_monday() == 1 {
            out.push(movement(
                day,
                "atm",
                (self.everyday, "EUR"),
                Side::Account(self.cash),
                Decimal::from(-200),
                "Cash machine",
            ));
        }
        if day.day() == 5 {
            out.push(movement(
                day,
                "save",
                (self.everyday, "EUR"),
                Side::Account(self.savings),
                Decimal::from(-500),
                "Into savings",
            ));
        }
        out
    }
}

impl Household {
    /// Euros changed into dollars two days before each trip, at an exchange
    /// office that keeps a percent and a half - so the fee category has
    /// something in it, and the balances page a conversion to show.
    async fn exchange(&self, pool: &PgPool, owner: Uuid, day: NaiveDate) -> Result<Option<NewEntry>> {
        if !(day.month().is_multiple_of(2) && day.day() == 10) {
            return Ok(None);
        }

        let given = Decimal::from(400);
        let rate = usd_in_eur(day) * Decimal::new(1015, 3);
        let got = (given / rate).round_dp(2);
        let reference = ledger::rate_in_force(pool, "USD", "EUR", day).await?;
        let plan = exchange::plan(
            Money {
                amount: given,
                currency: "EUR".to_owned(),
            },
            Money {
                amount: got,
                currency: "USD".to_owned(),
            },
            reference,
        )?;
        let fees = if plan.has_fee() {
            Some(ledger::purpose_category(pool, owner, Purpose::ExchangeFees).await?.id)
        } else {
            None
        };

        Ok(Some(NewEntry {
            occurred_on: day,
            description: "Dollars for the trip".to_owned(),
            idempotency_key: Some(format!("{SOURCE}:{day}:exchange")),
            source: Some(SOURCE.to_owned()),
            lines: plan.lines(Side::Account(self.everyday), Side::Account(self.travel), fees)?,
        }))
    }
}

/// What a dollar cost in euros on a day, in the demo's invented market.
fn usd_in_eur(day: NaiveDate) -> Decimal {
    wave(day, 0.92, 0.03, 45.0)
}

/// A category under `parent`, found by name or created.
async fn category(pool: &PgPool, owner: Uuid, parent: Option<Uuid>, flow: Flow, name: &str) -> Result<Uuid> {
    let existing = ledger::categories(pool, owner)
        .await?
        .into_iter()
        .find(|c| c.parent_id == parent && c.name == name);
    match existing {
        Some(found) => Ok(found.id),
        None => Ok(ledger::create_category(pool, owner, parent, flow, name).await?.id),
    }
}

/// A two-line entry: `amount` on the account, its opposite on the other side.
fn movement(day: NaiveDate, key: &str, (account, currency): (Uuid, &str), other: Side, amount: Decimal, what: &str) -> NewEntry {
    NewEntry {
        occurred_on: day,
        description: what.to_owned(),
        idempotency_key: Some(format!("{SOURCE}:{day}:{key}")),
        source: Some(SOURCE.to_owned()),
        lines: vec![
            NewLine {
                side: Side::Account(account),
                amount,
                currency: currency.to_owned(),
                note: String::new(),
            },
            NewLine {
                side: other,
                amount: -amount,
                currency: currency.to_owned(),
                note: String::new(),
            },
        ],
    }
}

/// Invented instruments, with a price a day that looks like a market.
async fn prices(pool: &PgPool, first: NaiveDate, last: NaiveDate) -> Result<()> {
    // Names nobody trades, so a demo price can never be taken for a real one.
    let instruments = [
        (Kind::Crypto, "DMC", "Demo Coin", 54_000.0, 0.12, 30.0),
        (Kind::Fund, "DWLD", "Demo World Index", 112.0, 0.05, 60.0),
    ];
    for (kind, symbol, name, level, swing, period) in instruments {
        let instrument = market::upsert_instrument(pool, kind, symbol, name, None).await?;
        market::bind_source(pool, instrument.id, SOURCE, symbol, 100).await?;
        for day in first.iter_days().take_while(|day| *day <= last) {
            let at = day.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap_or_default()).and_utc();
            market::record_price(pool, instrument.id, "EUR", at, wave(day, level, swing, period), SOURCE).await?;
        }
    }
    Ok(())
}

/// A value drifting around `level` by up to `swing` of itself, over `period`
/// days, with a little noise - deterministic in the day.
fn wave(day: NaiveDate, level: f64, swing: f64, period: f64) -> Decimal {
    let t = f64::from(day.num_days_from_ce());
    let noise = f64::from(Dice::for_day(day).roll(1_000)) / 1_000.0 - 0.5;
    let value = level * (1.0 + swing * (t * std::f64::consts::TAU / period).sin() + swing * 0.2 * noise);
    // Through a string at four places: the demo's only float, and it stops
    // here - what is stored is a decimal like every other amount (ADR 0004).
    format!("{value:.4}").parse().unwrap_or(Decimal::ONE)
}

/// A small deterministic generator, seeded by the day.
///
/// Not `rand`: the same day must give the same entries on every run and every
/// machine, and a seeded generator from a crate is one version bump away from
/// a different sequence - which here would mean a duplicate day of spending.
struct Dice(u64);

impl Dice {
    fn for_day(day: NaiveDate) -> Self {
        // SplitMix64's constant, so neighbouring days do not start alike.
        Self(u64::from(day.num_days_from_ce().unsigned_abs()).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// xorshift64: enough for choosing groceries.
    fn roll(&mut self, below: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        u32::try_from(self.0 % u64::from(below)).unwrap_or(0)
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.roll(100) < percent
    }

    /// An amount between `low` and `high`, in cents.
    fn cents(&mut self, low: u32, high: u32) -> Decimal {
        let cents = low * 100 + self.roll((high - low) * 100);
        Decimal::new(i64::from(cents), 2)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use austeris_common::{Config, db};
    use chrono::{NaiveDate, Utc};
    use rust_decimal::Decimal;
    use sqlx::PgPool;
    use sqlx::migrate::Migrator;

    use super::{Books, Dice, EMAIL, seed, wave};

    const SKIP: &str = "skipped: AUSTERIS_DATABASE_URL is not set";

    /// A schema of this test's own, migrated from empty.
    async fn pool(schema: &str, migrator: &Migrator) -> Option<PgPool> {
        let Ok(database_url) = std::env::var("AUSTERIS_DATABASE_URL") else {
            eprintln!("{SKIP}");
            return None;
        };
        let config = Config {
            database_url: Some(database_url),
            bind: String::new(),
            max_connections: 4,
            acquire_timeout: Duration::from_secs(10),
        };
        let pool = db::connect(&config, schema).await.expect("connecting to the test database");
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA \"{schema}\" CASCADE")))
            .execute(&pool)
            .await
            .expect("dropping the test schema");
        pool.close().await;

        let pool = db::connect(&config, schema).await.expect("recreating the test schema");
        austeris_common::migrate::run(&pool, migrator).await.expect("migrating");
        Some(pool)
    }

    async fn books(name: &str) -> Option<Books> {
        Some(Books {
            identity: pool(&format!("test_{name}_identity"), &austeris_identity::MIGRATOR).await?,
            ledger: pool(&format!("test_{name}_ledger"), &austeris_ledger::MIGRATOR).await?,
            market: pool(&format!("test_{name}_market"), &austeris_market::MIGRATOR).await?,
        })
    }

    #[tokio::test]
    async fn a_second_run_finds_the_household_and_posts_nothing_twice() {
        let Some(books) = books("demo_twice").await else { return };

        let first = seed(&books).await.expect("seeding an empty installation");
        assert!(first.created, "the household was not created on an empty installation");
        assert!(first.entries > 100, "four months of a household is more than {} entries", first.entries);

        let second = seed(&books).await.expect("seeding again");
        assert!(!second.created, "a second run created a second household");
        assert_eq!(second.entries, 0, "a second run on the same day posted entries again");

        // Every account the demo made has a balance someone would believe: the
        // cash machine keeps cash above zero, the salary keeps the bank there.
        let owner = austeris_identity::routes::person(&books.identity, EMAIL)
            .await
            .expect("reading the household")
            .expect("the household exists");
        let balances = austeris_ledger::repository::balances(&books.ledger, owner, Utc::now().date_naive())
            .await
            .expect("reading balances");
        assert_eq!(balances.len(), 4, "the demo has four accounts");
        for balance in balances {
            assert!(balance.amount > Decimal::ZERO, "{} went below zero: {}", balance.name, balance.amount);
        }
    }

    #[tokio::test]
    async fn a_real_person_on_the_installation_stops_the_demo() {
        // The demo writes shared prices; on an installation with real books
        // they would be taken for real ones.
        let Some(books) = books("demo_refuses").await else { return };
        austeris_identity::routes::create_person(&books.identity, "someone@example.org", "a real password")
            .await
            .expect("creating a person");

        let refused = seed(&books).await.expect_err("the demo ran beside a real person");
        assert!(refused.to_string().contains("not the demo's"), "{refused}");

        let instruments = austeris_market::repository::instruments(&books.market).await.expect("reading instruments");
        assert!(instruments.is_empty(), "the refusal came after prices were written");
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, d).unwrap()
    }

    #[test]
    fn the_same_day_rolls_the_same_dice() {
        // What makes a second run idempotent: the entries of a day are keyed by
        // the day, so they must also be decided by it.
        let mut a = Dice::for_day(day(7));
        let mut b = Dice::for_day(day(7));
        let rolls = |dice: &mut Dice| (0..20).map(|_| dice.roll(1000)).collect::<Vec<_>>();
        assert_eq!(rolls(&mut a), rolls(&mut b));

        let mut c = Dice::for_day(day(8));
        assert_ne!(rolls(&mut Dice::for_day(day(7))), rolls(&mut c), "neighbouring days roll alike");
    }

    #[test]
    fn amounts_stay_in_their_range_and_in_cents() {
        let mut dice = Dice::for_day(day(1));
        for _ in 0..1000 {
            let amount = dice.cents(18, 115);
            assert!(amount >= Decimal::from(18) && amount < Decimal::from(115), "{amount}");
            assert!(amount.scale() <= 2);
        }
    }

    #[test]
    fn a_price_is_positive_and_near_its_level() {
        for d in 1..=30 {
            let price = wave(day(d), 100.0, 0.1, 30.0);
            assert!(price > Decimal::from(85) && price < Decimal::from(115), "{price}");
        }
    }
}
