//! What the 2025 tracker's records become in the books - worked out before
//! anything is written, so a dry run and an import say the same thing.
//!
//! The tracker kept coins and nothing else: a buy of Bitcoin did not say what
//! paid for it, and USDT was one more coin. The books keep money: a buy takes
//! its cost from the account it is made on, as an entry the portfolio keeps
//! (ADR 0015), and USDT on an exchange is that account's money. So:
//!
//! - every wallet is an account in USDT, and every buy or sale of a coin is a
//!   trade on it, whose money the books work out;
//! - a USDT row that is the money of such a trade - the other way, within the
//!   hour, for nearly what it cost - is dropped: the books book it themselves;
//! - every other USDT row is money that came into the wallet, or left it, from
//!   outside what the tracker saw. The books cannot date money from outside
//!   them, so it is the account's opening balance - with whatever more the
//!   trades needed to have been paid for, which the tracker never saw arrive;
//! - a coin "bought" at a price of nothing - a reward, an airdrop - is units
//!   received, at the worth of nothing the tracker gave them;
//! - what the books cannot say at all - a sale for nothing, a sale of more
//!   than was bought - is left out, and named.
//!
//! The snapshots are worked out again from the trades, one a day, and set
//! beside what the tracker said: where they part is what the report is for.

use std::collections::{BTreeMap, HashMap};

use austeris_portfolio::model::TradeKind;
use chrono::{DateTime, Days, FixedOffset, NaiveDate, NaiveTime, Offset as _, TimeZone as _, Utc};
use rust_decimal::Decimal;

use super::tracker::{Asset, Holdings, Side, Tracker, Transaction};

/// The coin the tracker held that is money in the books: an exchange's cash.
pub const MONEY: &str = "USDT";

/// The name the tracker's prices and records are brought under.
pub const SOURCE: &str = "austeris-2025";

/// How far apart a USDT row and the trade it paid for can have been recorded.
const PAIR_WINDOW: chrono::Duration = chrono::Duration::hours(1);

/// How far a USDT row's amount can be from the trade's money and still be it:
/// a fee typed on one and not the other, a price rounded.
const PAIR_TOLERANCE: Decimal = Decimal::from_parts(2, 0, 0, false, 2);

/// Where the person was, which decides the day each moment falls on.
#[derive(Debug, Clone, Copy)]
pub enum Zone {
    /// This machine's time zone, at each moment.
    Local,
    /// One offset for every moment.
    Fixed(FixedOffset),
}

impl Zone {
    /// The offset at a moment.
    #[must_use]
    pub fn offset(self, at: DateTime<Utc>) -> FixedOffset {
        match self {
            Self::Fixed(offset) => offset,
            Self::Local => chrono::Local.offset_from_utc_datetime(&at.naive_utc()).fix(),
        }
    }

    /// The day a moment falls on where the person was.
    #[must_use]
    pub fn day(self, at: DateTime<Utc>) -> NaiveDate {
        at.with_timezone(&self.offset(at)).date_naive()
    }
}

/// An account the import needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountPlan {
    /// The wallet it was.
    pub wallet_id: i64,
    /// What the person called it.
    pub name: String,
    /// USDT that came into the wallet, less what left it, that no trade
    /// explains.
    pub arrived: Decimal,
    /// What more the trades needed to have been paid for, which the tracker
    /// never saw arrive.
    pub shortfall: Decimal,
    /// The day the money would have been lowest without it.
    pub short_on: Option<NaiveDate>,
    /// The two together: what the account opens with.
    pub opening: Decimal,
    /// The USDT the tracker said the wallet held at the end.
    pub cash_then: Decimal,
    /// The money the books will say the account holds at the end.
    pub cash_now: Decimal,
}

/// An instrument the import needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentPlan {
    /// Its ticker.
    pub symbol: String,
    /// Its name.
    pub name: String,
    /// Its decimals.
    pub decimals: Option<i32>,
    /// `CoinMarketCap`'s id for it.
    pub cmc_id: Option<i64>,
    /// The tracker's assets it was.
    pub asset_ids: Vec<i64>,
}

/// A trade the import records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradePlan {
    /// The transaction it was.
    pub legacy_id: i64,
    /// The wallet it was on.
    pub wallet_id: i64,
    /// What it was of.
    pub symbol: String,
    /// A buy or a sell.
    pub kind: TradeKind,
    /// How many units.
    pub quantity: Decimal,
    /// At what, in the account's money.
    pub price: Decimal,
    /// For what, in the account's money.
    pub fee: Decimal,
    /// When.
    pub at: DateTime<Utc>,
    /// Where the person was then.
    pub offset: FixedOffset,
    /// What the person wrote.
    pub note: String,
    /// The name it is recorded under, so bringing it again is the same trade.
    pub key: String,
    /// What its money moves the account by.
    pub money: Decimal,
}

impl TradePlan {
    /// The day it is booked under.
    #[must_use]
    pub fn day(&self) -> NaiveDate {
        self.at.with_timezone(&self.offset).date_naive()
    }
}

/// A transaction the books cannot say, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    /// The transaction.
    pub legacy_id: i64,
    /// The wallet, as named then.
    pub wallet: String,
    /// The coin.
    pub symbol: String,
    /// Which way.
    pub side: Side,
    /// How many.
    pub amount: Decimal,
    /// When.
    pub at: DateTime<Utc>,
    /// Why.
    pub why: String,
}

/// A USDT row that was the money of a trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    /// The USDT row.
    pub money_id: i64,
    /// Which way it went.
    pub side: Side,
    /// How much.
    pub amount: Decimal,
    /// The trade it paid for, or was paid by.
    pub trade_id: i64,
    /// That trade, in a few words.
    pub trade: String,
}

/// A price the import records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricePlan {
    /// The instrument's ticker.
    pub symbol: String,
    /// When.
    pub at: DateTime<Utc>,
    /// In dollars.
    pub price: Decimal,
}

/// What a dollar's worth of the money was on a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatePlan {
    /// The day.
    pub day: NaiveDate,
    /// Dollars a USDT bought.
    pub rate: Decimal,
}

/// A snapshot the import takes, and what the tracker said on its day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPlan {
    /// The tracker's snapshot it stands for: the day's last.
    pub legacy_id: i64,
    /// When the tracker took it.
    pub at: DateTime<Utc>,
    /// The day it shows.
    pub day: NaiveDate,
    /// The name it is taken under.
    pub key: String,
    /// What is written on it.
    pub note: String,
    /// What the tracker said was held.
    pub then: Holdings,
    /// What the books will say was held, by the trades.
    pub now: Holdings,
    /// What the tracker's holdings were worth at the tracker's prices of the
    /// day's end, and what had no price.
    pub value_then: Valued,
}

/// A sum in dollars, and what was left out of it for want of a price.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Valued {
    /// The sum.
    pub total: Decimal,
    /// Tickers without a price.
    pub unpriced: Vec<String>,
}

/// What the tracker held at its end, by wallet, beside what the books will.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct End {
    /// The last day of the tracker's records.
    pub day: Option<NaiveDate>,
    /// By wallet and ticker: what the tracker said, and what the books will.
    pub lines: BTreeMap<(String, String), (Decimal, Decimal)>,
    /// What the tracker's holdings were worth at the day's end.
    pub value_then: Valued,
}

/// Everything the import will do, and everything it will say.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// The accounts, one a wallet that brings anything.
    pub accounts: Vec<AccountPlan>,
    /// The instruments, one a ticker that is traded.
    pub instruments: Vec<InstrumentPlan>,
    /// The trades, in the order they happened.
    pub trades: Vec<TradePlan>,
    /// What is left out.
    pub left_out: Vec<LeftOut>,
    /// The USDT rows that were a trade's money.
    pub paired: Vec<Paired>,
    /// The prices.
    pub prices: Vec<PricePlan>,
    /// Prices of no use: of nothing traded, or not a price.
    pub prices_unused: usize,
    /// What USDT was worth in dollars, a day at a time.
    pub rates: Vec<RatePlan>,
    /// The snapshots, one a day.
    pub snapshots: Vec<SnapshotPlan>,
    /// Snapshots folded into the day's last: (folded, kept).
    pub folded: Vec<(i64, i64)>,
    /// Wallets nothing happened on.
    pub unused_wallets: Vec<String>,
    /// Assets nothing names.
    pub unused_assets: usize,
    /// The end of the records.
    pub end: End,
    /// What else is worth saying.
    pub notes: Vec<String>,
}

/// A USDT row: money in or out of a wallet.
#[derive(Debug, Clone)]
struct MoneyRow {
    id: i64,
    wallet_id: i64,
    side: Side,
    amount: Decimal,
    at: DateTime<Utc>,
}

/// Works out what the tracker's records become.
#[must_use]
pub fn plan(tracker: &Tracker, zone: Zone) -> Plan {
    let mut plan = Plan::default();
    let (money_rows, candidates) = classify(tracker, &mut plan);
    replay(tracker, candidates, zone, &mut plan);
    pair(&mut plan, &money_rows);
    plan.accounts = accounts(tracker, &plan, &money_rows, zone);
    plan.instruments = instruments(tracker, &plan.trades, &mut plan.notes);
    prices(tracker, &mut plan, zone);
    snapshots(tracker, &mut plan, zone);
    end(tracker, &mut plan, zone);
    remarks(tracker, &mut plan);
    plan
}

/// The name of a wallet a row names, or `?` when it names none.
fn wallet_name(tracker: &Tracker, id: Option<i64>) -> String {
    id.and_then(|id| tracker.wallet(id))
        .map_or_else(|| "?".to_owned(), |wallet| wallet.name.clone())
}

/// A coin's row the books can say: the row, its asset and its wallet.
type Candidate<'a> = (&'a Transaction, &'a Asset, i64);

/// Sorts the rows: money, a trade the books can say, or neither - which is
/// left out with why.
fn classify<'a>(tracker: &'a Tracker, plan: &mut Plan) -> (Vec<MoneyRow>, Vec<Candidate<'a>>) {
    let mut money_rows = Vec::new();
    let mut candidates = Vec::new();
    for transaction in &tracker.transactions {
        let asset = transaction.asset_id.and_then(|id| tracker.asset(id));
        let wallet = transaction.wallet_id.and_then(|id| tracker.wallet(id));
        let leave = |plan: &mut Plan, why: &str| {
            plan.left_out.push(LeftOut {
                legacy_id: transaction.id,
                wallet: wallet_name(tracker, transaction.wallet_id),
                symbol: asset.map_or_else(|| "?".to_owned(), |asset| asset.symbol.to_uppercase()),
                side: transaction.side,
                amount: transaction.amount,
                at: transaction.at,
                why: why.to_owned(),
            });
        };
        let (Some(asset), Some(wallet)) = (asset, wallet) else {
            leave(plan, "its coin or its wallet is no longer in the tracker's records");
            continue;
        };
        if transaction.amount <= Decimal::ZERO {
            leave(plan, "it moved nothing");
            continue;
        }
        if is_money(asset) {
            money_rows.push(MoneyRow {
                id: transaction.id,
                wallet_id: wallet.id,
                side: transaction.side,
                amount: transaction.amount,
                at: transaction.at,
            });
            continue;
        }
        let fee = transaction.fee.unwrap_or_default();
        let why = if transaction.price < Decimal::ZERO {
            Some("its price is below nothing")
        } else if transaction.price.is_zero() && transaction.side == Side::Sell {
            Some("it was sold for nothing, and a sale in the books brings money")
        } else if transaction.price.is_zero() && !fee.is_zero() {
            Some("it arrived for nothing with a fee, and units received cost nothing to receive")
        } else if fee < Decimal::ZERO {
            Some("its fee is below nothing")
        } else if transaction.side == Side::Sell && transaction.amount * transaction.price <= fee {
            Some("its fee is as much as the sale brought")
        } else {
            None
        };
        match why {
            Some(why) => leave(plan, why),
            None => candidates.push((transaction, asset, wallet.id)),
        }
    }
    (money_rows, candidates)
}

/// Replays the coins' rows into trades. Nobody sells what they did not hold:
/// each position is added up in the order things happened, what arrives at an
/// instant before what leaves, as the books add it up - and a sale of more is
/// left out.
fn replay(tracker: &Tracker, mut candidates: Vec<Candidate<'_>>, zone: Zone, plan: &mut Plan) {
    candidates.sort_by_key(|(transaction, _, _)| (transaction.at, transaction.side, transaction.id));
    let mut held: HashMap<(i64, String), Decimal> = HashMap::new();
    for (transaction, asset, wallet_id) in candidates {
        let symbol = asset.symbol.to_uppercase();
        let position = held.entry((wallet_id, symbol.clone())).or_default();
        let kind = match transaction.side {
            // Bought for nothing: a reward, an airdrop - units received.
            Side::Buy if transaction.price.is_zero() => TradeKind::Receive,
            Side::Buy => TradeKind::Buy,
            Side::Sell => TradeKind::Sell,
        };
        if kind == TradeKind::Sell && transaction.amount > *position {
            plan.left_out.push(LeftOut {
                legacy_id: transaction.id,
                wallet: wallet_name(tracker, Some(wallet_id)),
                symbol: symbol.clone(),
                side: transaction.side,
                amount: transaction.amount,
                at: transaction.at,
                why: format!(
                    "it sells {} {symbol} more than the trades before it bought",
                    (transaction.amount - *position).normalize()
                ),
            });
            continue;
        }
        *position += if kind == TradeKind::Sell { -transaction.amount } else { transaction.amount };
        let fee = transaction.fee.unwrap_or_default();
        let money = austeris_portfolio::trade::money(kind, transaction.amount, Some(transaction.price), fee, Some(MONEY)).unwrap_or_default();
        plan.trades.push(TradePlan {
            legacy_id: transaction.id,
            wallet_id,
            symbol,
            kind,
            quantity: transaction.amount,
            price: transaction.price,
            fee,
            at: transaction.at,
            offset: zone.offset(transaction.at),
            note: transaction.notes.clone().unwrap_or_default(),
            key: format!("{SOURCE}:transaction:{}", transaction.id),
            money,
        });
    }
    plan.left_out.sort_by_key(|left| (left.at, left.legacy_id));
}

/// What nothing names, and what else the dump says that is worth saying.
fn remarks(tracker: &Tracker, plan: &mut Plan) {
    plan.unused_wallets = tracker
        .wallets
        .iter()
        .filter(|wallet| !tracker.transactions.iter().any(|transaction| transaction.wallet_id == Some(wallet.id)))
        .map(|wallet| wallet.name.clone())
        .collect();
    plan.unused_assets = tracker
        .assets
        .iter()
        .filter(|asset| {
            !tracker.transactions.iter().any(|transaction| transaction.asset_id == Some(asset.id))
                && !tracker
                    .snapshots
                    .iter()
                    .any(|snapshot| snapshot.lines.iter().any(|line| line.symbol.eq_ignore_ascii_case(&asset.symbol)))
        })
        .count();

    for table in &tracker.missing {
        plan.notes
            .push(format!("the dump has no {table} table: an older tracker, or a dump of part of it"));
    }
    for schema in &tracker.other_schemas {
        plan.notes.push(format!(
            "the dump also carries the schema `{schema}`, which is not the tracker's own records; it is not read"
        ));
    }
    for unreadable in &tracker.unreadable {
        plan.notes.push(format!(
            "a row of {} {}could not be read: {}",
            unreadable.table,
            unreadable.id.map(|id| format!("(#{id}) ")).unwrap_or_default(),
            unreadable.why
        ));
    }
}

/// Whether an asset is the money of an exchange account.
fn is_money(asset: &Asset) -> bool {
    asset.symbol.trim().eq_ignore_ascii_case(MONEY)
}

/// Finds the USDT rows that were trades' money.
fn pair(plan: &mut Plan, money_rows: &[MoneyRow]) {
    let mut taken = vec![false; money_rows.len()];
    // Units received moved no money, and have none to find.
    for trade in plan.trades.iter().filter(|trade| trade.kind != TradeKind::Receive) {
        // A buy's money left the wallet; a sale's arrived.
        let wanted = if trade.kind == TradeKind::Buy { Side::Sell } else { Side::Buy };
        let cost = trade.money.abs();
        let best = money_rows
            .iter()
            .enumerate()
            .filter(|(at, row)| {
                !taken[*at]
                    && row.wallet_id == trade.wallet_id
                    && row.side == wanted
                    && (row.at - trade.at).abs() <= PAIR_WINDOW
                    && (row.amount - cost).abs() <= cost * PAIR_TOLERANCE
            })
            .min_by_key(|(_, row)| ((row.at - trade.at).abs(), (row.amount - cost).abs(), row.id));
        if let Some((at, row)) = best {
            taken[at] = true;
            plan.paired.push(Paired {
                money_id: row.id,
                side: row.side,
                amount: row.amount,
                trade_id: trade.legacy_id,
                trade: format!(
                    "{} {} {}",
                    if trade.kind == TradeKind::Buy { "buy" } else { "sell" },
                    trade.quantity.normalize(),
                    trade.symbol
                ),
            });
        }
    }
}

/// The accounts: what each opens with, and what it ends with.
fn accounts(tracker: &Tracker, plan: &Plan, money_rows: &[MoneyRow], zone: Zone) -> Vec<AccountPlan> {
    let paired: Vec<i64> = plan.paired.iter().map(|paired| paired.money_id).collect();
    let mut wallets: Vec<i64> = plan.trades.iter().map(|trade| trade.wallet_id).collect();
    wallets.extend(money_rows.iter().map(|row| row.wallet_id));
    wallets.sort_unstable();
    wallets.dedup();

    let signed = |row: &MoneyRow| if row.side == Side::Buy { row.amount } else { -row.amount };
    let mut out = Vec::new();
    for wallet_id in wallets {
        let Some(wallet) = tracker.wallet(wallet_id) else { continue };
        // The wallet's money as it moved: what came in or left from outside,
        // and each trade's money - what arrives at an instant before what
        // leaves.
        let mut moves: Vec<(DateTime<Utc>, Decimal)> = money_rows
            .iter()
            .filter(|row| row.wallet_id == wallet_id && !paired.contains(&row.id))
            .map(|row| (row.at, signed(row)))
            .collect();
        moves.extend(
            plan.trades
                .iter()
                .filter(|trade| trade.wallet_id == wallet_id)
                .map(|trade| (trade.at, trade.money)),
        );
        moves.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let (mut running, mut lowest, mut lowest_at) = (Decimal::ZERO, Decimal::ZERO, None);
        for (at, amount) in &moves {
            running += amount;
            if running < lowest {
                lowest = running;
                lowest_at = Some(*at);
            }
        }
        let arrived: Decimal = money_rows
            .iter()
            .filter(|row| row.wallet_id == wallet_id && !paired.contains(&row.id))
            .map(signed)
            .sum();
        let shortfall = -lowest;
        let opening = arrived + shortfall;
        let traded: Decimal = plan.trades.iter().filter(|trade| trade.wallet_id == wallet_id).map(|trade| trade.money).sum();
        out.push(AccountPlan {
            wallet_id,
            name: wallet.name.clone(),
            arrived: arrived.normalize(),
            shortfall: shortfall.normalize(),
            short_on: lowest_at.map(|at| zone.day(at)),
            opening: opening.normalize(),
            cash_then: money_rows
                .iter()
                .filter(|row| row.wallet_id == wallet_id)
                .map(signed)
                .sum::<Decimal>()
                .normalize(),
            cash_now: (opening + traded).normalize(),
        });
    }
    out
}

/// The instruments: one a ticker traded, whichever of the tracker's assets
/// carried it.
fn instruments(tracker: &Tracker, trades: &[TradePlan], notes: &mut Vec<String>) -> Vec<InstrumentPlan> {
    let mut by_symbol: BTreeMap<String, InstrumentPlan> = BTreeMap::new();
    for trade in trades {
        if by_symbol.contains_key(&trade.symbol) {
            continue;
        }
        let assets: Vec<&Asset> = tracker
            .assets
            .iter()
            .filter(|asset| asset.symbol.trim().to_uppercase() == trade.symbol)
            .collect();
        let Some(first) = assets.first() else { continue };
        if assets.len() > 1 {
            notes.push(format!(
                "{} of the tracker's assets share the ticker {} ({}); they are one instrument here",
                assets.len(),
                trade.symbol,
                assets
                    .iter()
                    .map(|asset| format!("#{} {}", asset.id, asset.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        by_symbol.insert(
            trade.symbol.clone(),
            InstrumentPlan {
                symbol: trade.symbol.clone(),
                name: if first.name.is_empty() { trade.symbol.clone() } else { first.name.clone() },
                decimals: first.decimals,
                cmc_id: first.cmc_id,
                asset_ids: assets.iter().map(|asset| asset.id).collect(),
            },
        );
    }
    by_symbol.into_values().collect()
}

/// The prices of what is traded, and what USDT was worth in dollars.
fn prices(tracker: &Tracker, plan: &mut Plan, zone: Zone) {
    let mut rates: BTreeMap<NaiveDate, (DateTime<Utc>, Decimal)> = BTreeMap::new();
    for price in &tracker.prices {
        let asset = tracker.asset(price.asset_id);
        let instrument = plan.instruments.iter().find(|instrument| instrument.asset_ids.contains(&price.asset_id));
        if price.price <= Decimal::ZERO {
            plan.prices_unused += 1;
        } else if let Some(instrument) = instrument {
            plan.prices.push(PricePlan {
                symbol: instrument.symbol.clone(),
                at: price.at,
                price: price.price,
            });
        } else if asset.is_some_and(is_money) {
            // A day's rate is the last word of that day.
            let day = zone.day(price.at);
            let entry = rates.entry(day).or_insert((price.at, price.price));
            if price.at >= entry.0 {
                *entry = (price.at, price.price);
            }
        } else {
            plan.prices_unused += 1;
        }
    }
    plan.prices.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.at.cmp(&b.at)));
    plan.rates = rates.into_iter().map(|(day, (_, rate))| RatePlan { day, rate: rate.normalize() }).collect();
}

/// The end of a day in UTC, which is what a day's prices are as of.
fn end_of(day: NaiveDate) -> DateTime<Utc> {
    (day + Days::new(1)).and_time(NaiveTime::MIN).and_utc() - chrono::Duration::seconds(1)
}

/// What holdings were worth at the tracker's own prices, as of a moment.
fn valued(tracker: &Tracker, holdings: &Holdings, at: DateTime<Utc>) -> Valued {
    let mut out = Valued::default();
    for (symbol, amount) in holdings {
        if amount.is_zero() {
            continue;
        }
        let price = tracker
            .prices
            .iter()
            .filter(|price| price.at <= at && price.price > Decimal::ZERO)
            .filter(|price| {
                tracker
                    .asset(price.asset_id)
                    .is_some_and(|asset| asset.symbol.trim().eq_ignore_ascii_case(symbol))
            })
            .max_by_key(|price| price.at);
        match price {
            Some(price) => out.total += *amount * price.price,
            None => out.unpriced.push(symbol.clone()),
        }
    }
    out.total = out.total.normalize();
    out
}

/// What the books will hold on a day: the positions of the trades of that
/// day and before, and the money of every account.
fn books_on(plan: &Plan, day: NaiveDate) -> BTreeMap<(i64, String), Decimal> {
    let mut held: BTreeMap<(i64, String), Decimal> = BTreeMap::new();
    for account in &plan.accounts {
        let money: Decimal = plan
            .trades
            .iter()
            .filter(|trade| trade.wallet_id == account.wallet_id && trade.day() <= day)
            .map(|trade| trade.money)
            .sum();
        held.insert((account.wallet_id, MONEY.to_owned()), account.opening + money);
    }
    for trade in plan.trades.iter().filter(|trade| trade.day() <= day) {
        let delta = if trade.kind == TradeKind::Sell { -trade.quantity } else { trade.quantity };
        *held.entry((trade.wallet_id, trade.symbol.clone())).or_default() += delta;
    }
    held
}

/// Adds lines up by ticker, leaving out what is nothing.
fn by_symbol(lines: impl IntoIterator<Item = (String, Decimal)>) -> Holdings {
    let mut out = Holdings::new();
    for (symbol, amount) in lines {
        *out.entry(symbol).or_default() += amount;
    }
    out.retain(|_, amount| !amount.is_zero());
    for amount in out.values_mut() {
        *amount = amount.normalize();
    }
    out
}

/// One snapshot a day: the day's last of the tracker's, worked out again.
fn snapshots(tracker: &Tracker, plan: &mut Plan, zone: Zone) {
    let mut by_day: BTreeMap<NaiveDate, Vec<&super::tracker::Snapshot>> = BTreeMap::new();
    for snapshot in &tracker.snapshots {
        by_day.entry(zone.day(snapshot.at)).or_default().push(snapshot);
    }
    for (day, taken) in by_day {
        let Some(kept) = taken.last() else { continue };
        for folded in &taken[..taken.len() - 1] {
            plan.folded.push((folded.id, kept.id));
        }
        let then = by_symbol(kept.lines.iter().map(|line| (line.symbol.to_uppercase(), line.amount)));
        let now = by_symbol(books_on(plan, day).into_iter().map(|((_, symbol), amount)| (symbol, amount)));
        plan.snapshots.push(SnapshotPlan {
            legacy_id: kept.id,
            at: kept.at,
            day,
            key: format!("{SOURCE}:snapshot:{}", kept.id),
            note: format!("The 2025 tracker's snapshot #{} of {} UTC", kept.id, kept.at.format("%Y-%m-%d %H:%M")),
            value_then: valued(tracker, &then, end_of(day)),
            then,
            now,
        });
    }
}

/// What the tracker held at its end, wallet by wallet, beside the books.
fn end(tracker: &Tracker, plan: &mut Plan, zone: Zone) {
    let last = tracker
        .transactions
        .iter()
        .map(|transaction| zone.day(transaction.at))
        .chain(tracker.snapshots.iter().map(|snapshot| zone.day(snapshot.at)))
        .max();
    let Some(day) = last else { return };

    let mut lines: BTreeMap<(String, String), (Decimal, Decimal)> = BTreeMap::new();
    let mut then_by_symbol: Vec<(String, Decimal)> = Vec::new();
    for transaction in &tracker.transactions {
        let (Some(asset), Some(wallet)) = (
            transaction.asset_id.and_then(|id| tracker.asset(id)),
            transaction.wallet_id.and_then(|id| tracker.wallet(id)),
        ) else {
            continue;
        };
        let symbol = asset.symbol.trim().to_uppercase();
        let delta = if transaction.side == Side::Buy {
            transaction.amount
        } else {
            -transaction.amount
        };
        lines.entry((wallet.name.clone(), symbol.clone())).or_default().0 += delta;
        then_by_symbol.push((symbol, delta));
    }
    for ((wallet_id, symbol), amount) in books_on(plan, day) {
        let name = tracker.wallet(wallet_id).map_or_else(|| "?".to_owned(), |wallet| wallet.name.clone());
        lines.entry((name, symbol)).or_default().1 += amount;
    }
    lines.retain(|_, (then, now)| !(then.is_zero() && now.is_zero()));
    for (then, now) in lines.values_mut() {
        *then = then.normalize();
        *now = now.normalize();
    }
    plan.end = End {
        day: Some(day),
        value_then: valued(tracker, &by_symbol(then_by_symbol), end_of(day)),
        lines,
    };
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{FixedOffset, NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;

    use austeris_portfolio::model::TradeKind;

    use super::{Plan, Zone, plan};
    use crate::legacy::tracker::{Asset, AssetPrice, Side, Snapshot, SnapshotLine, Tracker, Transaction, Wallet};

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    fn asset(id: i64, symbol: &str) -> Asset {
        Asset {
            id,
            symbol: symbol.to_owned(),
            name: format!("{symbol} coin"),
            cmc_id: Some(id * 10),
            decimals: Some(8),
        }
    }

    fn wallet(id: i64, name: &str) -> Wallet {
        Wallet {
            id,
            name: name.to_owned(),
            kind: "exchange".to_owned(),
        }
    }

    /// A transaction of an asset on a wallet - an amount at a price, with a
    /// fee - on a day of March 2025, at an hour, UTC.
    fn row(id: i64, (asset, wallet): (i64, i64), side: Side, (amount, price, fee): (&str, &str, Option<&str>), (day, hour): (u32, u32)) -> Transaction {
        Transaction {
            id,
            asset_id: Some(asset),
            wallet_id: Some(wallet),
            amount: decimal(amount),
            price: decimal(price),
            fee: fee.map(decimal),
            side,
            notes: None,
            at: Utc.with_ymd_and_hms(2025, 3, day, hour, 0, 0).single().expect("a moment"),
        }
    }

    const ASUNCION: Zone = Zone::Fixed(FixedOffset::west_opt(3 * 3600).expect("an offset"));

    /// An exchange where USDT came in, bought Bitcoin with its money said
    /// both ways, bought Ether with its money said only by the trade, was
    /// given Solana for nothing and sold some, and a hardware wallet where
    /// Bitcoin was "bought" with no money at all.
    fn tracker() -> Tracker {
        Tracker {
            assets: vec![asset(1, "BTC"), asset(2, "ETH"), asset(3, "USDT"), asset(4, "SOL")],
            wallets: vec![wallet(1, "Binance"), wallet(2, "Ledger Nano"), wallet(3, "Never used")],
            transactions: vec![
                row(1, (3, 1), Side::Buy, ("2000", "1", None), (3, 13)),
                row(2, (1, 1), Side::Buy, ("0.02", "85000", Some("1.7")), (5, 14)),
                // Its money, said again, within minutes.
                row(3, (3, 1), Side::Sell, ("1701.7", "1", None), (5, 14)),
                row(4, (2, 1), Side::Buy, ("0.5", "2100", Some("1.05")), (10, 9)),
                row(5, (1, 1), Side::Sell, ("0.005", "84000", Some("0.42")), (20, 16)),
                row(6, (3, 1), Side::Buy, ("419.58", "1", None), (20, 16)),
                row(7, (4, 1), Side::Buy, ("10", "0", None), (25, 12)),
                row(8, (4, 1), Side::Sell, ("4", "120", None), (26, 12)),
                row(9, (1, 2), Side::Buy, ("0.01", "86000", None), (6, 8)),
            ],
            snapshots: vec![
                Snapshot {
                    id: 1,
                    at: Utc.with_ymd_and_hms(2025, 3, 21, 1, 0, 0).single().expect("a moment"),
                    lines: vec![],
                },
                Snapshot {
                    id: 2,
                    at: Utc.with_ymd_and_hms(2025, 3, 21, 2, 30, 0).single().expect("a moment"),
                    lines: vec![
                        SnapshotLine {
                            symbol: "BTC".to_owned(),
                            amount: decimal("0.025"),
                            cmc_id: Some(10),
                        },
                        SnapshotLine {
                            symbol: "USDT".to_owned(),
                            amount: decimal("717.8799999999999"),
                            cmc_id: Some(30),
                        },
                    ],
                },
            ],
            prices: vec![
                AssetPrice {
                    asset_id: 1,
                    price: decimal("84000"),
                    at: Utc.with_ymd_and_hms(2025, 3, 20, 15, 0, 0).single().expect("a moment"),
                },
                AssetPrice {
                    asset_id: 3,
                    price: decimal("1.0001"),
                    at: Utc.with_ymd_and_hms(2025, 3, 20, 15, 0, 0).single().expect("a moment"),
                },
                AssetPrice {
                    asset_id: 3,
                    price: decimal("0.9999"),
                    at: Utc.with_ymd_and_hms(2025, 3, 20, 18, 0, 0).single().expect("a moment"),
                },
            ],
            ..Tracker::default()
        }
    }

    fn planned() -> Plan {
        plan(&tracker(), ASUNCION)
    }

    #[test]
    fn a_coin_is_a_trade_and_what_the_books_cannot_say_is_left_out_with_why() {
        let planned = planned();
        let brought: Vec<i64> = planned.trades.iter().map(|trade| trade.legacy_id).collect();
        assert_eq!(brought, [2, 9, 4, 5, 7, 8], "in the order they happened");
        assert!(planned.left_out.is_empty(), "{:?}", planned.left_out);
        assert_eq!(planned.trades[0].key, "austeris-2025:transaction:2");
        assert_eq!(planned.trades[0].money, decimal("-1701.7"));
        assert_eq!(planned.trades[0].day(), NaiveDate::from_ymd_opt(2025, 3, 5).expect("a day"));

        // A sale of more than was bought before it is left out, by how much.
        let mut oversold = tracker();
        oversold.transactions.retain(|row| row.id != 7);
        let refused = plan(&oversold, ASUNCION);
        assert_eq!(refused.left_out.len(), 1, "{:?}", refused.left_out);
        assert!(refused.left_out[0].why.contains("4 SOL more"), "{}", refused.left_out[0].why);
    }

    #[test]
    fn a_coin_bought_for_nothing_is_received_and_a_sale_for_nothing_is_left_out() {
        let planned = planned();
        let reward = planned.trades.iter().find(|trade| trade.legacy_id == 7).expect("the reward");
        assert_eq!(reward.kind, TradeKind::Receive);
        assert_eq!(
            (reward.price, reward.money),
            (Decimal::ZERO, Decimal::ZERO),
            "worth what the tracker said: nothing"
        );

        let mut tracker = tracker();
        tracker.transactions.push(row(10, (4, 1), Side::Sell, ("1", "0", None), (26, 13)));
        tracker.transactions.push(row(11, (4, 1), Side::Buy, ("1", "0", Some("0.1")), (26, 14)));
        let more = plan(&tracker, ASUNCION);
        let left: Vec<(i64, bool)> = more.left_out.iter().map(|left| (left.legacy_id, left.why.contains("for nothing"))).collect();
        assert_eq!(left, [(10, true), (11, true)], "{:?}", more.left_out);
    }

    #[test]
    fn a_usdt_row_that_was_a_trades_money_is_dropped_and_the_rest_opens_the_account() {
        let plan = planned();
        let paired: Vec<(i64, i64)> = plan.paired.iter().map(|paired| (paired.money_id, paired.trade_id)).collect();
        assert_eq!(paired, [(3, 2), (6, 5)]);

        let binance = plan.accounts.iter().find(|account| account.name == "Binance").expect("Binance");
        // 2000 arrived; the Ether bought on the 10th took the money below
        // nothing by 752.75 - money the tracker never saw arrive.
        assert_eq!(binance.arrived, decimal("2000"));
        assert_eq!(binance.shortfall, decimal("752.75"));
        assert_eq!(binance.short_on, NaiveDate::from_ymd_opt(2025, 3, 10));
        assert_eq!(binance.opening, decimal("2752.75"));
        assert_eq!(binance.cash_then, decimal("717.88"));
        // With what the Solana given for nothing was sold for.
        assert_eq!(binance.cash_now, decimal("899.58"));

        // A wallet with a buy and no money at all opens with what it cost.
        let nano = plan.accounts.iter().find(|account| account.name == "Ledger Nano").expect("the Nano");
        assert_eq!(
            (nano.arrived, nano.shortfall, nano.opening, nano.cash_now),
            (Decimal::ZERO, decimal("860"), decimal("860"), Decimal::ZERO)
        );
        assert_eq!(plan.unused_wallets, ["Never used"]);
    }

    #[test]
    fn a_snapshot_is_one_a_day_and_is_set_beside_what_the_trades_say() {
        let plan = planned();
        assert_eq!(plan.snapshots.len(), 1, "two of one day are one");
        assert_eq!(plan.folded, [(1, 2)]);
        let snapshot = &plan.snapshots[0];
        assert_eq!(
            snapshot.day,
            NaiveDate::from_ymd_opt(2025, 3, 20).expect("a day"),
            "01:00 UTC is the evening before in Asunción"
        );
        assert_eq!(snapshot.key, "austeris-2025:snapshot:2");
        assert_eq!(snapshot.then.get("USDT"), Some(&decimal("717.8799999999999")));
        assert_eq!(snapshot.now.get("BTC"), Some(&decimal("0.025")));
        assert_eq!(snapshot.now.get("ETH"), Some(&decimal("0.5")));
        assert_eq!(snapshot.now.get("USDT"), Some(&decimal("419.58")));
        // At the tracker's prices of the day's end: Bitcoin at 84000 and USDT
        // at its last word of the day.
        // 0.025 x 84000 + 717.8799999999999 x 0.9999, every digit kept.
        assert_eq!(snapshot.value_then.total, decimal("2817.80821199999990001"));
        assert_eq!(snapshot.value_then.unpriced, Vec::<String>::new());

        assert_eq!(plan.rates.len(), 1);
        assert_eq!(plan.rates[0].rate, decimal("0.9999"), "a day's rate is its last word");
    }

    #[test]
    fn the_end_sets_each_wallet_beside_its_account() {
        let plan = planned();
        assert_eq!(plan.end.day, NaiveDate::from_ymd_opt(2025, 3, 26));
        let line = |wallet: &str, symbol: &str| plan.end.lines.get(&(wallet.to_owned(), symbol.to_owned())).copied();
        assert_eq!(line("Binance", "SOL"), Some((decimal("6"), decimal("6"))), "received, and some sold");
        assert_eq!(line("Binance", "USDT"), Some((decimal("717.88"), decimal("899.58"))));
        assert_eq!(line("Ledger Nano", "BTC"), Some((decimal("0.01"), decimal("0.01"))));
        assert_eq!(line("Ledger Nano", "USDT"), None, "nothing on either side is no line");
        assert_eq!(plan.end.value_then.unpriced, ["ETH", "SOL"]);
    }
}
