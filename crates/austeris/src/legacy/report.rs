//! What the import says: what the tracker's records became, what was left
//! out and why, and where the books and the tracker part.
//!
//! The same report for a dry run and for an import. A dry run sets the
//! tracker beside what the books will hold; an import, beside what they hold
//! and what it is worth.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use rust_decimal::Decimal;

use super::plan::{MONEY, Plan, SOURCE, Zone};
use super::tracker::{Holdings, Tracker};
use super::write::{Books, Written};

/// The report, as text.
#[must_use]
pub fn report(path: &Path, tracker: &Tracker, plan: &Plan, zone: Zone, written: Option<&Written>) -> String {
    let mut out = String::new();
    heading(&mut out, path, tracker, plan, zone, written.is_some());
    accounts(&mut out, plan, written);
    trades(&mut out, plan, written);
    prices(&mut out, plan, written);
    snapshots(&mut out, plan, written);
    end(&mut out, tracker, plan, written);
    out.push('\n');
    out.push_str(if written.is_some() {
        "Running it again changes nothing: what is already there is recognised by its key.\n"
    } else {
        "Nothing was written. Run it again without --dry-run to bring it into the books.\n"
    });
    out
}

/// Adds a line.
fn line(out: &mut String, text: impl AsRef<str>) {
    out.push_str(text.as_ref());
    out.push('\n');
}

fn heading(out: &mut String, path: &Path, tracker: &Tracker, plan: &Plan, zone: Zone, wrote: bool) {
    line(
        out,
        format!(
            "The 2025 tracker's records in {}: {} transactions, {} snapshots and {} prices, of {} wallets and {} coins",
            path.display(),
            tracker.transactions.len(),
            tracker.snapshots.len(),
            tracker.prices.len(),
            tracker.wallets.len(),
            tracker.assets.len()
        ),
    );
    line(
        out,
        format!(
            "  {}; days are {}",
            if wrote {
                "Brought into the books"
            } else {
                "A dry run: nothing was written, and the books' side is what they will hold"
            },
            match zone {
                Zone::Local => "this machine's".to_owned(),
                Zone::Fixed(offset) => format!("those of {offset}"),
            }
        ),
    );
    for note in &plan.notes {
        line(out, format!("  Note: {note}"));
    }
}

fn accounts(out: &mut String, plan: &Plan, written: Option<&Written>) {
    line(out, "");
    line(out, format!("Accounts, in {MONEY} - an exchange's money"));
    for account in &plan.accounts {
        let placed = written.and_then(|written| written.accounts.iter().find(|placed| placed.wallet_id == account.wallet_id));
        let said = match placed {
            Some(placed) if !placed.created && placed.opening != account.opening => format!(
                "  {}: already there; its opening balance is left at {} {MONEY}, where the 2025 records ask for {}",
                placed.name,
                amount(placed.opening),
                amount(account.opening)
            ),
            Some(placed) if !placed.created => format!("  {}: already there, opening with {} {MONEY}", placed.name, amount(placed.opening)),
            Some(placed) => format!("  {}: opened with {} {MONEY}", placed.name, amount(account.opening)),
            None => format!("  {}: opens with {} {MONEY}", account.name, amount(account.opening)),
        };
        line(out, said);
        if !account.arrived.is_zero() {
            line(
                out,
                format!(
                    "      {} {MONEY} {} from outside what the tracker saw: the {MONEY} it recorded that no trade explains",
                    amount(account.arrived.abs()),
                    if account.arrived > Decimal::ZERO { "came in" } else { "left" }
                ),
            );
        }
        if !account.shortfall.is_zero() {
            line(
                out,
                format!(
                    "      {} {MONEY} more the trades needed by {} to have been paid for, which the tracker never saw arrive",
                    amount(account.shortfall),
                    account.short_on.map_or_else(|| "?".to_owned(), |day| day.to_string())
                ),
            );
        }
        if account.opening < Decimal::ZERO {
            line(
                out,
                format!("      below nothing until its trades bring money in: {MONEY} left it after they did, and the books cannot date money leaving them"),
            );
        }
    }
    if !plan.unused_wallets.is_empty() {
        line(out, format!("  Not brought, nothing happened on them: {}", plan.unused_wallets.join(", ")));
    }
}

fn trades(out: &mut String, plan: &Plan, written: Option<&Written>) {
    line(out, "");
    let refused = written.map(|written| written.refused.as_slice()).unwrap_or_default();
    let mut said = format!("Trades: {} brought", plan.trades.len() - refused.len());
    if let Some(written) = written {
        let _ = write!(said, " ({} new, {} already there)", written.new, written.already);
    }
    let _ = write!(said, ", {} left out", plan.left_out.len() + refused.len());
    line(out, said);
    for left in &plan.left_out {
        line(
            out,
            format!(
                "  #{} {} {}: {} {} {} - {}",
                left.legacy_id,
                left.at.format("%Y-%m-%d"),
                left.wallet,
                left.side.word(),
                amount(left.amount),
                left.symbol,
                left.why
            ),
        );
    }
    for refused in refused {
        let trade = plan.trades.iter().find(|trade| trade.legacy_id == refused.legacy_id);
        line(
            out,
            format!(
                "  #{} {} - refused by the installation: {}",
                refused.legacy_id,
                trade
                    .map(|trade| format!("{} {} {}", trade.day(), amount(trade.quantity), trade.symbol))
                    .unwrap_or_default(),
                refused.why
            ),
        );
    }
    if !plan.paired.is_empty() {
        line(
            out,
            format!(
                "  {} {MONEY} rows were a trade's money, which the books now work out from the trade:",
                plan.paired.len()
            ),
        );
        for paired in &plan.paired {
            line(
                out,
                format!(
                    "    #{} {} {} {MONEY} - the money of #{} ({})",
                    paired.money_id,
                    paired.side.word(),
                    amount(paired.amount),
                    paired.trade_id,
                    paired.trade
                ),
            );
        }
    }
}

fn prices(out: &mut String, plan: &Plan, written: Option<&Written>) {
    line(out, "");
    let mut said = format!("Prices: {} under `{SOURCE}`", written.map_or(plan.prices.len(), |written| written.prices));
    if plan.prices_unused > 0 {
        let _ = write!(said, " ({} of coins nothing traded, or not a price, left out)", plan.prices_unused);
    }
    let _ = write!(said, "; {MONEY} in dollars on {} days", plan.rates.len());
    line(out, said);
}

fn snapshots(out: &mut String, plan: &Plan, written: Option<&Written>) {
    line(out, "");
    let mut said = format!("Snapshots: {} taken, one a day", plan.snapshots.len());
    if let Some(written) = written.filter(|written| written.snapshots_already > 0) {
        let _ = write!(said, " ({} already there)", written.snapshots_already);
    }
    line(out, said);
    for (folded, kept) in &plan.folded {
        line(out, format!("  #{folded} is of the same day as #{kept}, and is folded into it"));
    }
    for (at, snapshot) in plan.snapshots.iter().enumerate() {
        line(
            out,
            format!(
                "  {} - the tracker's #{} of {} UTC",
                snapshot.day,
                snapshot.legacy_id,
                snapshot.at.format("%Y-%m-%d %H:%M")
            ),
        );
        let books = written.and_then(|written| written.snapshots.get(at));
        columns(out, written.is_some());
        side_by_side(out, &snapshot.then, books.map_or(&snapshot.now, |books| &books.holdings));
        worth(out, snapshot.value_then.total, &snapshot.value_then.unpriced, books);
    }
}

fn end(out: &mut String, tracker: &Tracker, plan: &Plan, written: Option<&Written>) {
    let Some(day) = plan.end.day else { return };
    line(out, "");
    line(out, format!("At the end of the records, {day}"));
    let books = written.and_then(|written| written.end.as_ref());

    // Every wallet and ticker either side has.
    let mut keys: BTreeSet<(String, String)> = plan.end.lines.keys().cloned().collect();
    if let (Some(books), Some(written)) = (books, written) {
        for (account_id, symbol) in books.lines.keys() {
            if let Some(placed) = written.accounts.iter().find(|placed| &placed.id == account_id)
                && let Some(wallet) = tracker.wallet(placed.wallet_id)
            {
                keys.insert((wallet.name.clone(), symbol.clone()));
            }
        }
    }
    columns(out, written.is_some());
    for (wallet, symbol) in keys {
        let planned = plan.end.lines.get(&(wallet.clone(), symbol.clone())).copied().unwrap_or_default();
        let now = match (books, written) {
            (Some(books), Some(written)) => written
                .accounts
                .iter()
                .filter(|placed| tracker.wallet(placed.wallet_id).is_some_and(|found| found.name == wallet))
                .filter_map(|placed| books.lines.get(&(placed.id.clone(), symbol.clone())))
                .sum(),
            _ => planned.1,
        };
        if !(planned.0.is_zero() && now.is_zero()) {
            row(out, &format!("{wallet} {symbol}"), planned.0, now);
        }
    }
    worth(out, plan.end.value_then.total, &plan.end.value_then.unpriced, books);
}

/// The heading of a table: the tracker's column and the books'.
fn columns(out: &mut String, wrote: bool) {
    line(
        out,
        format!(
            "    {:24}  {:>20}  {:>20}",
            "",
            "the tracker",
            if wrote { "the books" } else { "the books, to be" }
        ),
    );
}

/// One line of a table, saying when the two differ.
fn row(out: &mut String, name: &str, then: Decimal, now: Decimal) {
    line(
        out,
        format!(
            "    {name:24}  {:>20}  {:>20}{}",
            amount(then),
            amount(now),
            if then == now { "" } else { "  - differs" }
        ),
    );
}

/// What the tracker said beside what the books say, ticker by ticker.
fn side_by_side(out: &mut String, then: &Holdings, now: &Holdings) {
    let symbols: BTreeSet<&String> = then.keys().chain(now.keys()).collect();
    for symbol in symbols {
        row(
            out,
            symbol,
            then.get(symbol).copied().unwrap_or_default(),
            now.get(symbol).copied().unwrap_or_default(),
        );
    }
}

/// What the tracker's holdings were worth at its prices, beside what the
/// books say they are worth.
fn worth(out: &mut String, then: Decimal, unpriced: &[String], books: Option<&Books>) {
    let now = books.map_or_else(|| "-".to_owned(), |books| money(books.total));
    let gap = books.map(|books| books.total - then).filter(|gap| gap.round_dp(2) != Decimal::ZERO);
    line(
        out,
        format!(
            "    {:24}  {:>20}  {:>20}{}",
            "worth, USD",
            money(then),
            now,
            gap.map(|gap| format!("  - {} by {}", if gap > Decimal::ZERO { "more" } else { "less" }, money(gap.abs())))
                .unwrap_or_default()
        ),
    );
    if !unpriced.is_empty() {
        line(out, format!("      the tracker had no price for {}", unpriced.join(", ")));
    }
    // Not whether the books' prices are stale: of a day in 2025 they are, by
    // a coin's two hours, and saying so of every line says nothing.
    if let Some(books) = books.filter(|books| !books.unvalued.is_empty()) {
        line(out, format!("      the books have no price or rate for {}", books.unvalued.join(", ")));
    }
}

/// A quantity, with every digit it has and none it does not.
fn amount(value: Decimal) -> String {
    value.normalize().to_string()
}

/// A sum of money, to the cent.
fn money(value: Decimal) -> String {
    value.round_dp(2).normalize().to_string()
}
