//! Recording, changing and deleting a trade - and the money it moves.
//!
//! A buy or a sell moves money on the account it happened on, and the ledger
//! keeps that money as an entry this module posts and keeps (ADR 0015): the
//! account's line, and the holding the money became or came back from. The
//! trade is the truth and the entry is derived from it, so they change
//! together: the trade is written in this service's transaction, the ledger is
//! asked inside it, and the transaction commits only once the ledger has
//! answered. Should the commit itself fail after that, the ledger is asked to
//! undo what it did - the one moment the two can part, said in the log when
//! even that fails.
//!
//! A fee is money spent - what the broker or the exchange took - and the entry
//! files it under a category the portfolio keeps in the books for it, by its
//! own key: the holding is the price alone (ADR 0017). What a coin cost, for
//! its lot, is still the price and the fee together.

use std::str::FromStr;

use austeris_common::Actor;
use austeris_ledger::currency;
use austeris_proto::ledger::v1::{Holding, Line, ModuleCategory, PostEntryRequest, RestateEntryRequest, line::Side};
use chrono::{Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tonic::{Code, Status};
use uuid::Uuid;

use crate::model::{Trade, TradeBody, TradeKind};
use crate::repository::{self, NewTrade};
use crate::services::{Account, Books, Instrument, Market};
use crate::value;

/// Why a trade was not recorded, changed or deleted.
#[derive(Debug, thiserror::Error)]
pub enum Refused {
    /// The person has no such trade.
    #[error("no such trade")]
    Missing,
    /// The trade is not one: a buy with no price, a transfer to where it came
    /// from. What a form already prevents; said in English for whoever called
    /// the API by hand.
    #[error("{0}")]
    Malformed(&'static str),
    /// An account named is not the person's.
    #[error("no such account")]
    UnknownAccount,
    /// The instrument is not in the market's catalogue.
    #[error("no such instrument")]
    UnknownInstrument,
    /// Positions are held on a broker's or a wallet's account, not this one.
    #[error("positions are held on a broker's or a wallet's account; {name} is not one")]
    NotAHoldingAccount {
        /// The account.
        name: String,
        /// What kind it is.
        kind: String,
    },
    /// The account is closed.
    #[error("{name} is closed; nothing new happens on it")]
    ClosedAccount {
        /// The account.
        name: String,
    },
    /// The instrument is money: a currency, or a stablecoin an account is
    /// kept in.
    #[error("{symbol} is money here, kept on an account rather than held as a position")]
    MoneyNotHolding {
        /// Its ticker.
        symbol: String,
    },
    /// The fee is all of it: a transfer where nothing arrives, a sale that
    /// brings in nothing.
    #[error("the fee is as much as the trade itself")]
    FeeTooLarge,
    /// The money it moves rounds to nothing in the account's currency.
    #[error("the trade moves less money than a unit of {currency}")]
    NothingMoved {
        /// The account's currency.
        currency: String,
    },
    /// Units received with no worth said, and none the market knows for the
    /// moment they arrived.
    #[error("say what a unit of {symbol} was worth when it arrived - 0 for nothing; the market has no price for that moment")]
    ReceiveUnpriced {
        /// The instrument's ticker.
        symbol: String,
    },
    /// It leaves a position below nothing at some moment.
    #[error("that leaves {short} {symbol} fewer than were held at {at}")]
    Oversold {
        /// The instrument's ticker.
        symbol: String,
        /// How many units short.
        short: String,
        /// The moment the position would be below nothing.
        at: String,
    },
    /// The ledger did not answer.
    #[error("the ledger is not answering; nothing was recorded")]
    LedgerUnavailable,
    /// The market did not answer.
    #[error("the market is not answering; nothing was recorded")]
    MarketUnavailable,
    /// The ledger refused the money.
    #[error("the ledger refused the trade's money: {0}")]
    RefusedByLedger(String),
    /// Anything else: a fault, not the caller's.
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

/// The key a trade's money is posted under, and kept by.
#[must_use]
pub fn key(trade: Uuid) -> String {
    format!("{}:trade:{trade}", crate::SOURCE)
}

/// What a trade's money moves the account by, in its currency: negative for a
/// buy - what it cost, with its fee - and positive for a sale - what it
/// brought in, less its fee. Rounded the way the account's currency is; a
/// transfer moves none.
#[must_use]
pub fn money(kind: TradeKind, quantity: Decimal, price: Option<Decimal>, fee: Decimal, currency: Option<&str>) -> Option<Decimal> {
    let (price, currency) = (price?, currency?);
    let gross = quantity * price;
    let signed = match kind {
        TradeKind::Buy => -(gross + fee),
        TradeKind::Sell => gross - fee,
        TradeKind::Transfer | TradeKind::Receive => return None,
    };
    Some(currency::round(signed, currency).normalize())
}

/// A trade as it is answered: what its money moved filled in, and its
/// numbers without the zeros their columns pad them with.
#[must_use]
pub fn with_money(mut trade: Trade) -> Trade {
    trade.money = money(trade.kind, trade.quantity, trade.price, trade.fee, trade.currency.as_deref());
    trade.quantity = trade.quantity.normalize();
    trade.price = trade.price.map(|price| price.normalize());
    trade.fee = trade.fee.normalize();
    trade
}

/// The key of the category a trade's fee is filed under, among the
/// portfolio's own in the books.
pub const FEES_KEY: &str = "fees";

/// What that category is called when it is made, before anyone renames it.
pub const FEES_NAME: &str = "Trading fees";

/// What a trade's money is in the ledger's terms.
struct Leg {
    account_id: Uuid,
    amount: Decimal,
    /// What the broker or the exchange took, rounded as the account's money
    /// is; nothing when it rounds to nothing.
    fee: Decimal,
    currency: String,
    occurred_on: NaiveDate,
    description: String,
}

impl Leg {
    /// The trade's money, when it moves any.
    fn of(trade: &Trade, symbol: &str) -> Option<Self> {
        let amount = money(trade.kind, trade.quantity, trade.price, trade.fee, trade.currency.as_deref())?;
        let currency = trade.currency.clone()?;
        let verb = if trade.kind == TradeKind::Buy { "Buy" } else { "Sell" };
        Some(Self {
            account_id: trade.account_id,
            amount,
            fee: currency::round(trade.fee, &currency).normalize(),
            currency,
            occurred_on: trade.occurred_on,
            description: format!("{verb} {} {symbol}", trade.quantity.normalize()),
        })
    }

    /// The account's side, the fee's, and the holding's - which is what is
    /// left, so the entry balances however the account's money rounds.
    fn lines(&self) -> Vec<Line> {
        let mut lines = vec![Line {
            side: Some(Side::AccountId(self.account_id.to_string())),
            amount: self.amount.to_string(),
            currency: self.currency.clone(),
            note: String::new(),
        }];
        if !self.fee.is_zero() {
            lines.push(Line {
                side: Some(Side::ModuleCategory(ModuleCategory {
                    key: FEES_KEY.to_owned(),
                    name: FEES_NAME.to_owned(),
                    flow: "expense".to_owned(),
                })),
                amount: self.fee.to_string(),
                currency: self.currency.clone(),
                note: String::new(),
            });
        }
        let holding = -self.amount - self.fee;
        if !holding.is_zero() {
            lines.push(Line {
                side: Some(Side::Holding(Holding {})),
                amount: holding.normalize().to_string(),
                currency: self.currency.clone(),
                note: String::new(),
            });
        }
        lines
    }
}

/// A trade checked against the books and the market, ready to be written.
struct Checked {
    trade: NewTrade,
    instrument: Instrument,
}

/// Checks what a person says a trade is.
async fn check<B: Books, M: Market>(books: &B, market: &M, owner: Uuid, body: &TradeBody) -> Result<Checked, Refused> {
    if body.quantity <= Decimal::ZERO {
        return Err(Refused::Malformed("a trade is of more than nothing"));
    }
    let fee = body.fee.unwrap_or_default();
    if fee < Decimal::ZERO {
        return Err(Refused::Malformed("a fee is not negative"));
    }
    match body.kind {
        TradeKind::Buy | TradeKind::Sell => {
            if body.price.is_none_or(|price| price <= Decimal::ZERO) {
                return Err(Refused::Malformed("a buy or a sale has a price, of more than nothing"));
            }
            if body.to_account_id.is_some() {
                return Err(Refused::Malformed("a buy or a sale happens on one account; only a transfer goes to another"));
            }
        }
        TradeKind::Receive => {
            if body.price.is_some_and(|price| price < Decimal::ZERO) {
                return Err(Refused::Malformed("what a unit was worth is not below nothing"));
            }
            if !fee.is_zero() {
                return Err(Refused::Malformed("units received cost nothing to receive; a fee is a buy's or a transfer's"));
            }
            if body.to_account_id.is_some() {
                return Err(Refused::Malformed("units are received on one account; only a transfer goes to another"));
            }
        }
        TradeKind::Transfer => {
            if body.price.is_some() {
                return Err(Refused::Malformed("a transfer has no price; nothing was paid"));
            }
            match body.to_account_id {
                None => return Err(Refused::Malformed("a transfer goes to another account")),
                Some(to) if to == body.account_id => return Err(Refused::Malformed("a transfer goes to another account than it left")),
                Some(_) => {}
            }
            if fee >= body.quantity {
                return Err(Refused::FeeTooLarge);
            }
        }
    }
    let occurred_at = body.occurred_at.with_timezone(&Utc);
    // A minute of grace for a clock on the person's side running ahead.
    if occurred_at > Utc::now() + Duration::minutes(1) {
        return Err(Refused::Malformed("a trade is of a moment that has happened"));
    }

    let accounts = books.accounts(owner).await.map_err(|status| unavailable_ledger(&status))?;
    let account = holding_account(&accounts, body.account_id)?;
    if let Some(to) = body.to_account_id {
        holding_account(&accounts, to)?;
    }

    let instrument = market
        .instruments(&[body.instrument_id])
        .await
        .map_err(|status| {
            tracing::error!(%status, "the market did not say what an instrument is");
            Refused::MarketUnavailable
        })?
        .into_iter()
        .next()
        .ok_or(Refused::UnknownInstrument)?;
    // A currency is money; so is a stablecoin the person keeps an account in.
    if instrument.kind == "fx" || accounts.iter().any(|held| held.currency.eq_ignore_ascii_case(&instrument.symbol)) {
        return Err(Refused::MoneyNotHolding { symbol: instrument.symbol });
    }

    let currency = (body.kind != TradeKind::Transfer).then(|| account.currency.clone());
    if body.kind == TradeKind::Sell && body.price.is_some_and(|price| body.quantity * price <= fee) {
        return Err(Refused::FeeTooLarge);
    }
    // A reward is worth what the market said at the moment it arrived, unless
    // the person says otherwise - and a price the market only knew long
    // before is not that.
    let price = match (body.kind, body.price) {
        (TradeKind::Receive, None) => Some(worth_on_arrival(books, market, &instrument, &account.currency, body).await?),
        (_, said) => said,
    };
    if let Some(moved) = money(body.kind, body.quantity, body.price, fee, currency.as_deref())
        && moved.is_zero()
    {
        return Err(Refused::NothingMoved {
            currency: account.currency.clone(),
        });
    }

    Ok(Checked {
        trade: NewTrade {
            kind: body.kind,
            account_id: body.account_id,
            to_account_id: body.to_account_id,
            instrument_id: body.instrument_id,
            quantity: body.quantity,
            price,
            currency,
            fee,
            occurred_at,
            occurred_on: body.occurred_at.date_naive(),
            note: body.note.trim().to_owned(),
        },
        instrument,
    })
}

/// What a unit received was worth when it arrived, in the account's currency,
/// as the market priced it then.
async fn worth_on_arrival<B: Books, M: Market>(books: &B, market: &M, instrument: &Instrument, currency: &str, body: &TradeBody) -> Result<Decimal, Refused> {
    let at = body.occurred_at.with_timezone(&Utc);
    let prices = value::prices_in(books, market, &[instrument.id], currency, Some(at), body.occurred_at.date_naive())
        .await
        .map_err(|why| {
            tracing::error!(%why, "the worth of units received could not be looked up");
            Refused::MarketUnavailable
        })?;
    prices
        .get(&instrument.id)
        .filter(|price| !price.stale)
        .map(|price| price.price)
        .ok_or_else(|| Refused::ReceiveUnpriced {
            symbol: instrument.symbol.clone(),
        })
}

/// The account a trade names, when positions can be held on it.
fn holding_account(accounts: &[Account], id: Uuid) -> Result<&Account, Refused> {
    let account = accounts.iter().find(|account| account.id == id).ok_or(Refused::UnknownAccount)?;
    if !account.holds_positions() {
        return Err(Refused::NotAHoldingAccount {
            name: account.name.clone(),
            kind: account.kind.clone(),
        });
    }
    if account.closed {
        return Err(Refused::ClosedAccount { name: account.name.clone() });
    }
    Ok(account)
}

/// What recording a trade came to.
#[derive(Debug, Clone)]
pub enum Recorded {
    /// It is recorded now, and its money booked.
    New(Trade),
    /// It was recorded before, under the key it was named by now; it is
    /// answered as it is.
    Already(Trade),
}

/// The longest key a client may name a record by.
const KEY_LENGTH: usize = 200;

/// The key a client names a record by, trimmed; `None` when it named none.
///
/// # Errors
///
/// Returns [`Refused::Malformed`] when the key is empty or too long.
pub fn idempotency_key(said: Option<&str>) -> Result<Option<&str>, Refused> {
    match said.map(str::trim) {
        None => Ok(None),
        Some("") => Err(Refused::Malformed("an idempotency key is not empty")),
        Some(key) if key.chars().count() > KEY_LENGTH => Err(Refused::Malformed("an idempotency key is 200 characters at most")),
        Some(key) => Ok(Some(key)),
    }
}

/// Records a trade, and books its money - or, named by a key a trade was
/// recorded under before, answers that one.
///
/// # Errors
///
/// Returns [`Refused`] when the trade is not one this person can make, or the
/// ledger or the market does not answer.
pub async fn record<B: Books, M: Market>(pool: &PgPool, books: &B, market: &M, owner: Uuid, body: &TradeBody, key: Option<&str>) -> Result<Recorded, Refused> {
    let key = idempotency_key(key)?;
    if let Some(key) = key
        && let Some(found) = repository::trade_by_key(pool, owner, key).await?
    {
        return Ok(Recorded::Already(with_money(found)));
    }

    let Checked { trade: new, instrument } = check(books, market, owner, body).await?;
    let mut transaction = Actor::Person(owner).begin(pool).await?;
    let inserted = repository::insert_trade(&mut transaction, owner, &new, key).await;
    let mut trade = match (inserted, key) {
        (Ok(trade), _) => trade,
        // Recorded under the same key a moment ago, by a request racing this
        // one: the unique index waited for it, and it is the answer.
        (Err(error), Some(key)) if repository::key_taken(&error, repository::TRADE_KEY_TAKEN) => {
            drop(transaction);
            let found = repository::trade_by_key(pool, owner, key)
                .await?
                .ok_or_else(|| anyhow::anyhow!("a trade's key was taken and then not found"))?;
            return Ok(Recorded::Already(with_money(found)));
        }
        (Err(error), _) => return Err(oversold_or(error, &instrument.symbol)),
    };

    let leg = Leg::of(&trade, &instrument.symbol);
    if let Some(leg) = &leg {
        let entry = books.post(posting(owner, trade.id, leg)).await.map_err(|status| refused_by_ledger(&status))?;
        repository::set_entry(&mut transaction, trade.id, Some(entry)).await?;
        trade.entry_id = Some(entry);
    }

    if let Err(error) = transaction.commit().await {
        if leg.is_some() {
            undo(books.withdraw(owner, &self::key(trade.id)).await.map(|_| ()), trade.id);
        }
        return Err(anyhow::Error::new(error).context("recording a trade").into());
    }
    Ok(Recorded::New(with_money(trade)))
}

/// Says what a trade is now, and restates its money.
///
/// # Errors
///
/// Returns [`Refused`] as [`record`] does, and [`Refused::Missing`] when the
/// person has no such trade.
pub async fn replace<B: Books, M: Market>(pool: &PgPool, books: &B, market: &M, owner: Uuid, id: Uuid, body: &TradeBody) -> Result<Trade, Refused> {
    let Checked { trade: new, instrument } = check(books, market, owner, body).await?;
    let mut transaction = Actor::Person(owner).begin(pool).await?;
    let old = repository::trade_for_update(&mut transaction, owner, id).await?.ok_or(Refused::Missing)?;
    let mut trade = repository::replace_trade(&mut transaction, owner, id, &new)
        .await
        .map_err(|error| oversold_or(error, &instrument.symbol))?;

    // What the old trade's money was described by: the same instrument, or
    // another one when the trade is now said to be of another.
    let old_symbol = if old.instrument_id == trade.instrument_id {
        instrument.symbol.clone()
    } else {
        symbol_of(market, old.instrument_id).await
    };
    let before = Leg::of(&old, &old_symbol).map(|leg| (old.entry_id, leg));
    let after = Leg::of(&trade, &instrument.symbol);
    let entry = settle(books, owner, id, before.as_ref().map(|(_, leg)| leg), after.as_ref())
        .await
        .map_err(|status| refused_by_ledger(&status))?;
    if entry != old.entry_id {
        repository::set_entry(&mut transaction, id, entry).await?;
    }
    trade.entry_id = entry;

    if let Err(error) = transaction.commit().await {
        undo(
            settle(books, owner, id, after.as_ref(), before.as_ref().map(|(_, leg)| leg)).await.map(|_| ()),
            id,
        );
        return Err(anyhow::Error::new(error).context("changing a trade").into());
    }
    Ok(with_money(trade))
}

/// Deletes a trade, and takes its money back out of the books.
///
/// # Errors
///
/// Returns [`Refused::Missing`] when the person has no such trade,
/// [`Refused::Oversold`] when a later sale would be left with nothing to sell,
/// and [`Refused::LedgerUnavailable`] when the ledger does not answer.
pub async fn delete<B: Books, M: Market>(pool: &PgPool, books: &B, market: &M, owner: Uuid, id: Uuid) -> Result<(), Refused> {
    let mut transaction = Actor::Person(owner).begin(pool).await?;
    let old = repository::trade_for_update(&mut transaction, owner, id).await?.ok_or(Refused::Missing)?;
    if let Err(error) = repository::delete_trade(&mut transaction, owner, id).await {
        let symbol = symbol_of(market, old.instrument_id).await;
        return Err(oversold_or(error, &symbol));
    }
    let moved = old.kind != TradeKind::Transfer;
    if moved {
        books.withdraw(owner, &key(id)).await.map_err(|status| refused_by_ledger(&status))?;
    }

    if let Err(error) = transaction.commit().await {
        if moved {
            let symbol = symbol_of(market, old.instrument_id).await;
            if let Some(leg) = Leg::of(&old, &symbol) {
                undo(books.post(posting(owner, id, &leg)).await.map(|_| ()), id);
            }
        }
        return Err(anyhow::Error::new(error).context("deleting a trade").into());
    }
    Ok(())
}

/// How many trades are booked anew in one go.
const REBOOK_BATCH: i64 = 100;

/// Books the money of the trades recorded before fees were spending the way
/// it is booked now: each buy's and sale's entry restated with its fee filed
/// under the portfolio's category for fees, and the holding its price alone.
/// Answers how many entries it restated.
///
/// Each trade is locked while it is done, and done once: a trade a person
/// changed meanwhile was booked anew by the change. Asked again after a
/// failure, it goes on from where it stopped.
///
/// # Errors
///
/// Returns [`Refused`] when the ledger or the market does not answer, or the
/// ledger refuses an entry.
pub async fn book_fees_apart<B: Books, M: Market>(pool: &PgPool, books: &B, market: &M) -> Result<usize, Refused> {
    let mut restated = 0;
    loop {
        let waiting = repository::booked_before(pool, REBOOK_BATCH).await?;
        if waiting.is_empty() {
            return Ok(restated);
        }
        let mut ids: Vec<Uuid> = waiting.iter().map(|waiting| waiting.trade.instrument_id).collect();
        ids.sort_unstable();
        ids.dedup();
        let symbols: std::collections::HashMap<Uuid, String> = market
            .instruments(&ids)
            .await
            .map_err(|status| {
                tracing::error!(%status, "the market did not say what instruments are");
                Refused::MarketUnavailable
            })?
            .into_iter()
            .map(|instrument| (instrument.id, instrument.symbol))
            .collect();

        for repository::BookedBefore { owner_id, trade } in waiting {
            let mut transaction = Actor::module(Some(crate::SOURCE)).begin(pool).await?;
            let Some(trade) = repository::lock_booked_before(&mut transaction, owner_id, trade.id).await? else {
                continue;
            };
            let symbol = symbols.get(&trade.instrument_id).cloned().unwrap_or_else(|| trade.instrument_id.to_string());
            // A trade with no fee was booked the same either way.
            if let Some(leg) = Leg::of(&trade, &symbol).filter(|leg| !leg.fee.is_zero()) {
                settle(books, owner_id, trade.id, Some(&leg), Some(&leg))
                    .await
                    .map_err(|status| refused_by_ledger(&status))?;
                restated += 1;
            }
            repository::mark_booked(&mut transaction, trade.id).await?;
            // Should this fail after the ledger restated, the trade is still
            // waiting and is restated again - to the same lines.
            transaction
                .commit()
                .await
                .map_err(|error| anyhow::Error::new(error).context("booking a trade anew"))?;
        }
    }
}

/// Books the old trades anew in the background, trying again a while later
/// when the ledger or the market is not answering yet - in one process they
/// come up side by side - and saying in the log what it did.
pub async fn book_fees_apart_in_time<B: Books, M: Market>(pool: PgPool, books: B, market: M) {
    let mut wait = std::time::Duration::from_secs(5);
    loop {
        match book_fees_apart(&pool, &books, &market).await {
            Ok(0) => return,
            Ok(restated) => {
                tracing::info!(restated, "trades recorded before fees were spending have their fees filed apart now");
                return;
            }
            Err(why) => {
                tracing::warn!(%why, retry_in = ?wait, "trades recorded before fees were spending could not be booked anew yet");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(std::time::Duration::from_secs(600));
            }
        }
    }
}

/// Brings the ledger from what a trade's money was to what it is: posted,
/// restated, taken back, or left alone. Answers the entry it is under now.
async fn settle<B: Books>(books: &B, owner: Uuid, id: Uuid, before: Option<&Leg>, after: Option<&Leg>) -> Result<Option<Uuid>, Status> {
    match (before, after) {
        (Some(_), Some(leg)) => books
            .restate(RestateEntryRequest {
                owner_id: owner.to_string(),
                idempotency_key: key(id),
                source: crate::SOURCE.to_owned(),
                occurred_on: leg.occurred_on.to_string(),
                description: leg.description.clone(),
                lines: leg.lines(),
            })
            .await
            .map(Some),
        (Some(_), None) => books.withdraw(owner, &key(id)).await.map(|_| None),
        (None, Some(leg)) => books.post(posting(owner, id, leg)).await.map(Some),
        (None, None) => Ok(None),
    }
}

/// The entry that books a trade's money.
fn posting(owner: Uuid, id: Uuid, leg: &Leg) -> PostEntryRequest {
    PostEntryRequest {
        owner_id: owner.to_string(),
        occurred_on: leg.occurred_on.to_string(),
        description: leg.description.clone(),
        idempotency_key: key(id),
        source: crate::SOURCE.to_owned(),
        lines: leg.lines(),
        kept: true,
        ..PostEntryRequest::default()
    }
}

/// Says in the log when undoing in the ledger what a failed commit left there
/// did not work either: the trade and its money then disagree, and someone
/// has to look.
fn undo(result: Result<(), Status>, trade: Uuid) {
    if let Err(status) = result {
        tracing::error!(%trade, %status, "a trade was not written, and its money could not be taken back out of the ledger; they disagree until the trade is recorded again");
    }
}

/// The ticker of an instrument, or its id when the market does not say.
async fn symbol_of<M: Market>(market: &M, id: Uuid) -> String {
    match market.instruments(&[id]).await {
        Ok(found) => found.into_iter().next().map_or_else(|| id.to_string(), |instrument| instrument.symbol),
        Err(_) => id.to_string(),
    }
}

/// The ledger not answering.
fn unavailable_ledger(status: &Status) -> Refused {
    tracing::error!(%status, "the ledger did not answer");
    Refused::LedgerUnavailable
}

/// The ledger's answer to booking a trade's money, as this service refuses it.
fn refused_by_ledger(status: &Status) -> Refused {
    match status.code() {
        Code::InvalidArgument | Code::FailedPrecondition | Code::NotFound => Refused::RefusedByLedger(status.message().to_owned()),
        _ => unavailable_ledger(status),
    }
}

/// The schema's refusal of a position below nothing, said with the
/// instrument's ticker; anything else is a fault.
fn oversold_or(error: anyhow::Error, symbol: &str) -> Refused {
    let detail = error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .filter_map(|error| error.as_database_error())
        .filter(|db| db.code().as_deref() == Some(OVERSOLD))
        .find_map(|db| {
            db.try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
                .and_then(|pg| pg.detail().map(ToOwned::to_owned))
        });
    let Some(detail) = detail else {
        return Refused::Failed(error);
    };
    let said: serde_json::Value = serde_json::from_str(&detail).unwrap_or_default();
    let short = said["short"].as_str().and_then(|short| Decimal::from_str(short).ok()).unwrap_or_default();
    Refused::Oversold {
        symbol: symbol.to_owned(),
        short: short.normalize().to_string(),
        at: said["at"].as_str().unwrap_or_default().to_owned(),
    }
}

/// The code this service's schema refuses a position below nothing with.
const OVERSOLD: &str = "AUP01";

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    use super::money;
    use crate::model::TradeKind;

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    #[test]
    fn a_buy_costs_its_price_and_its_fee_and_a_sale_brings_its_price_less_its_fee() {
        let buy = money(TradeKind::Buy, decimal("0.01"), Some(decimal("60000")), decimal("1"), Some("USDT"));
        assert_eq!(buy, Some(decimal("-601")));
        let sale = money(TradeKind::Sell, decimal("0.01"), Some(decimal("65000")), decimal("0.65"), Some("USDT"));
        assert_eq!(sale, Some(decimal("649.35")));
        assert_eq!(money(TradeKind::Transfer, decimal("0.01"), None, decimal("0.0001"), None), None);
    }

    #[test]
    fn a_fee_is_filed_apart_and_the_holding_is_the_price_alone() {
        use austeris_proto::ledger::v1::line::Side;

        let trade = crate::model::Trade {
            id: uuid::Uuid::new_v4(),
            kind: TradeKind::Sell,
            account_id: uuid::Uuid::new_v4(),
            to_account_id: None,
            instrument_id: uuid::Uuid::new_v4(),
            quantity: decimal("0.01"),
            price: Some(decimal("65000")),
            currency: Some("USDT".to_owned()),
            fee: decimal("0.65"),
            occurred_at: chrono::Utc::now(),
            occurred_on: chrono::Utc::now().date_naive(),
            note: String::new(),
            entry_id: None,
            money: None,
            idempotency_key: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let lines = super::Leg::of(&trade, "BTC").expect("a sale moves money").lines();
        let amount = |wanted: fn(&Side) -> bool| {
            lines
                .iter()
                .find(|line| line.side.as_ref().is_some_and(wanted))
                .map(|line| decimal(&line.amount))
                .expect("the line")
        };
        assert_eq!(amount(|side| matches!(side, Side::AccountId(_))), decimal("649.35"));
        assert_eq!(
            amount(|side| matches!(side, Side::ModuleCategory(category) if category.key == "fees")),
            decimal("0.65")
        );
        assert_eq!(amount(|side| matches!(side, Side::Holding(_))), decimal("-650"));

        let free = crate::model::Trade { fee: Decimal::ZERO, ..trade };
        assert_eq!(super::Leg::of(&free, "BTC").expect("a sale").lines().len(), 2, "no fee, no line for it");
    }

    #[test]
    fn money_is_rounded_the_way_the_accounts_currency_is() {
        // Dollars have cents; a stablecoin nobody standardised keeps every
        // digit, as the ledger does.
        let in_dollars = money(TradeKind::Buy, decimal("3"), Some(decimal("101.333")), Decimal::ZERO, Some("USD"));
        assert_eq!(in_dollars, Some(decimal("-304")));
        let in_usdt = money(TradeKind::Buy, decimal("3"), Some(decimal("101.333")), Decimal::ZERO, Some("USDT"));
        assert_eq!(in_usdt, Some(decimal("-303.999")));
    }
}
