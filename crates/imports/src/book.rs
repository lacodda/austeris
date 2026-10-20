//! What a receipt becomes in the books.
//!
//! One entry: the money that left the account, and one line per item on what
//! it was for, each with the item as printed - so a receipt of twelve items
//! reads as twelve lines rather than one sum nobody can check. The lines add up
//! to what the items came to; what the till rounded off or took off the whole
//! receipt is a line of its own, so the account moves by what was actually
//! paid and the entry still balances to the unit.
//!
//! Which account and which category are decided here or asked for, never
//! guessed: the account the person named (or the folder it arrived in did),
//! else the only cash account in the receipt's currency when it was paid in
//! cash; the category the person named, else the one the shop is usually
//! filed under. Whatever is still unknown is what the document waits for.

use austeris_common::Code;
use austeris_proto::ledger::v1::{CounterpartyName, File, Line, Place, PostEntryRequest, line::Side};
use rust_decimal::Decimal;
use serde_json::json;
use tonic::Status;
use uuid::Uuid;

use crate::ledger::{Account, Books};
use crate::model::{Needed, Reason};
use crate::sifen::{Kind, Receipt};

/// What the person said about a document, or the folder it arrived in did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Said {
    /// The account it was paid from.
    pub account: Option<Uuid>,
    /// The account it was paid from, by name: the folder it was dropped in.
    pub account_name: Option<String>,
    /// What it was for.
    pub category: Option<Uuid>,
}

/// What booking a receipt came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// It is in the books.
    Booked {
        /// The entry.
        entry_id: Uuid,
    },
    /// It is not, yet.
    Waiting {
        /// What the person has to say.
        needs: Vec<Needed>,
        /// Why, when there is more to it than what is needed.
        reason: Option<Reason>,
    },
}

/// The receipt's file, kept with its entry.
#[derive(Debug, Clone, Copy)]
pub struct Proof<'a> {
    /// Its name.
    pub name: &'a str,
    /// Its bytes.
    pub content: &'a [u8],
    /// Its SHA-256, in hex: the entry's key when the receipt carries no code
    /// of its own.
    pub sha256: &'a str,
}

/// Books a receipt, or says what it waits for.
///
/// # Errors
///
/// Returns the ledger's status when it could not be asked or refused for a
/// reason of its own: unreachable, or failing. A refusal of the entry itself,
/// such as a category that is not the person's any more, is an outcome rather
/// than an error: the document waits with the ledger's sentence.
pub async fn book(books: &impl Books, owner: Uuid, receipt: &Receipt, proof: Proof<'_>, said: &Said) -> Result<Outcome, Status> {
    let accounts = books.accounts(owner).await?;
    let shop = shop_name(receipt);
    let known = books.counterparty(owner, receipt.issuer.tax_id.as_deref(), &shop).await?;

    let mut needs = Vec::new();
    let (account, reason) = match choose_account(&accounts, receipt, said) {
        Ok(account) => (Some(account), None),
        Err(reason) => {
            needs.push(Needed::Account);
            (None, reason)
        }
    };
    let category = said.category.or_else(|| known.as_ref().and_then(|known| known.usual_category));
    if category.is_none() {
        needs.push(Needed::Category);
    }
    let (Some(account), Some(category), true) = (account, category, needs.is_empty()) else {
        return Ok(Outcome::Waiting { needs, reason });
    };

    let rate = if account.currency == receipt.currency {
        None
    } else {
        let day = receipt.issued_on();
        match books.rate(&receipt.currency, &account.currency, day).await? {
            Some(rate) => Some(rate.rate),
            None => {
                return Ok(Outcome::Waiting {
                    needs: Vec::new(),
                    reason: Some(Reason::new(
                        Code::DocumentNoRate,
                        json!({ "from": receipt.currency, "to": account.currency, "day": day }),
                        format!(
                            "{} is in {}, the receipt in {}, and no {}->{} rate is known on or before {day}; record one, or say another account",
                            account.name, account.currency, receipt.currency, receipt.currency, account.currency
                        ),
                    )),
                });
            }
        }
    };

    let Some(entry) = lines(receipt, account, category, rate) else {
        return Ok(Outcome::Waiting {
            needs: Vec::new(),
            reason: Some(refused_by_ledger(&format!(
                "{} {} is worth nothing in {}",
                receipt.total, receipt.currency, account.currency
            ))),
        });
    };

    let request = PostEntryRequest {
        owner_id: owner.to_string(),
        occurred_on: receipt.issued_on().to_string(),
        // The shop names the entry; a description beside it would say it
        // twice.
        description: String::new(),
        idempotency_key: key(receipt, proof.sha256),
        source: crate::SOURCE.to_owned(),
        lines: entry.lines,
        counterparty: Some(CounterpartyName {
            name: shop,
            tax_id: receipt.issuer.tax_id.clone().unwrap_or_default(),
            usual_category_id: category.to_string(),
        }),
        place: Some(Place {
            // SIFEN is Paraguay's: every document it issues was issued there.
            country: "PY".to_owned(),
            city: receipt.issuer.city.as_deref().map(city_name).unwrap_or_default(),
        }),
        tags: Vec::new(),
        pending: entry.pending,
        files: vec![File {
            name: proof.name.to_owned(),
            content: proof.content.to_vec(),
        }],
    };

    match books.post(request).await {
        Ok(posted) => Ok(Outcome::Booked { entry_id: posted.entry_id }),
        // The entry itself was refused: the person can act on the ledger's
        // sentence, so it is said rather than retried.
        Err(status) if status.code() == tonic::Code::InvalidArgument => Ok(Outcome::Waiting {
            needs: Vec::new(),
            reason: Some(refused_by_ledger(status.message())),
        }),
        Err(status) => Err(status),
    }
}

/// The account a receipt was paid from, or why it is not known.
///
/// `Err(None)` is "not said, and nothing on the receipt says"; `Err(Some(_))`
/// carries a reason worth telling the person - the folder's name matches no
/// account, or the one named is closed.
fn choose_account<'a>(accounts: &'a [Account], receipt: &Receipt, said: &Said) -> Result<&'a Account, Option<Reason>> {
    let open = |account: &'a Account| -> Result<&'a Account, Option<Reason>> {
        if account.closed {
            Err(Some(Reason::new(
                Code::DocumentClosedAccount,
                json!({ "name": account.name }),
                format!("{} is closed; say another account", account.name),
            )))
        } else {
            Ok(account)
        }
    };

    if let Some(id) = said.account {
        // An account that is not among the person's is not one they said:
        // it is asked for again rather than trusted.
        return accounts.iter().find(|account| account.id == id).map_or(Err(None), open);
    }
    if let Some(name) = &said.account_name {
        return match accounts.iter().find(|account| account.name.trim().eq_ignore_ascii_case(name.trim())) {
            Some(account) => open(account),
            None => Err(Some(Reason::new(
                Code::DocumentUnknownAccount,
                json!({ "name": name }),
                format!("the folder is named `{name}`, and you have no account called that"),
            ))),
        };
    }
    // Paid in cash, the money left the person's cash in the receipt's
    // currency - when there is exactly one such account. With two, which
    // wallet is a guess.
    if receipt.paid_in_cash() {
        let cash: Vec<&Account> = accounts
            .iter()
            .filter(|account| !account.closed && account.kind == "cash" && account.currency == receipt.currency)
            .collect();
        if let [only] = cash.as_slice() {
            return Ok(only);
        }
    }
    Err(None)
}

/// An entry's lines, and whether it is held.
#[derive(Debug, Clone, PartialEq)]
pub struct Lines {
    /// The sides of the movement.
    pub lines: Vec<Line>,
    /// Held: paid from an account in another currency, so what the bank took
    /// is known only when it posts.
    pub pending: bool,
}

/// What a line says it is when it is what the till rounded off or took off
/// the whole receipt.
pub const ADJUSTMENT_NOTE: &str = "rounding and discounts";

/// The lines a receipt is booked as, paid from `account` and filed under
/// `category`.
///
/// `rate` is the rate from the receipt's currency into the account's, when
/// they differ; the account then moves by the total at that rate, rounded to
/// its currency's places, and the entry is held - the bank's own rate is what
/// it actually took, and it is said when the hold is posted. `None` when the
/// total is worth nothing in the account's currency.
#[must_use]
pub fn lines(receipt: &Receipt, account: &Account, category: Uuid, rate: Option<Decimal>) -> Option<Lines> {
    // An invoice is money out of the account onto what it was for; a credit
    // note is the same money back.
    let towards_category = match receipt.kind {
        Kind::Invoice => Decimal::ONE,
        Kind::CreditNote => Decimal::NEGATIVE_ONE,
    };
    let line = |side: Side, amount: Decimal, currency: &str, note: String| Line {
        side: Some(side),
        amount: amount.normalize().to_string(),
        currency: currency.to_owned(),
        note,
    };
    let on_category = |amount: Decimal, note: String| line(Side::CategoryId(category.to_string()), amount * towards_category, &receipt.currency, note);

    let mut out = Vec::with_capacity(receipt.items.len() + 4);
    let mut items_total = Decimal::ZERO;
    for item in receipt.items.iter().filter(|item| !item.total.is_zero()) {
        items_total += item.total;
        out.push(on_category(item.total, item_note(item)));
    }
    let adjustment = receipt.total - items_total;
    if !adjustment.is_zero() {
        // A receipt with no items at all is one line for the whole of it.
        let note = if items_total.is_zero() { String::new() } else { ADJUSTMENT_NOTE.to_owned() };
        out.push(on_category(adjustment, note));
    }

    let (account_lines, pending) = match rate {
        None => (
            vec![line(
                Side::AccountId(account.id.to_string()),
                -receipt.total * towards_category,
                &account.currency,
                String::new(),
            )],
            false,
        ),
        Some(rate) => {
            let in_account = austeris_ledger::currency::round(receipt.total * rate, &account.currency);
            if in_account.is_zero() {
                return None;
            }
            let conversion = || Side::Conversion(austeris_proto::ledger::v1::Conversion {});
            (
                vec![
                    line(
                        Side::AccountId(account.id.to_string()),
                        -in_account * towards_category,
                        &account.currency,
                        String::new(),
                    ),
                    line(conversion(), in_account * towards_category, &account.currency, String::new()),
                    line(conversion(), -receipt.total * towards_category, &receipt.currency, String::new()),
                ],
                true,
            )
        }
    };
    let mut lines = account_lines;
    lines.extend(out);
    Some(Lines { lines, pending })
}

/// An item as its line says it: as printed, and how many when not one.
fn item_note(item: &crate::sifen::Item) -> String {
    if item.quantity == Decimal::ONE {
        item.description.clone()
    } else {
        format!("{} × {}", item.description, item.quantity.normalize())
    }
}

/// The entry's key: the receipt's own code, so one receipt is one entry
/// however many times and as whatever files it arrives; the file's hash when
/// it carries none.
#[must_use]
pub fn key(receipt: &Receipt, sha256: &str) -> String {
    match &receipt.code {
        Some(code) => format!("sifen:{code}"),
        None => format!("document:{sha256}"),
    }
}

/// The name a shop is filed under: the one it trades under when the receipt
/// gives it, else its legal name without the legal form - `SOL ALIMENTOS S.A.`
/// is "Sol Alimentos" - in the case a person writes it.
#[must_use]
pub fn shop_name(receipt: &Receipt) -> String {
    let printed = receipt.issuer.trade_name.as_deref().unwrap_or(&receipt.issuer.name);
    let mut words: Vec<&str> = printed.split_whitespace().collect();
    while words.len() > 1 && words.last().is_some_and(|word| is_legal_form(word)) {
        words.pop();
    }
    let name = words.join(" ");
    let name = name.trim_end_matches([',', '-', ' ']);
    as_written(name)
}

/// Whether a word is a company's legal form: `S.A.`, `SRL`, `S.A.E.C.A.`.
fn is_legal_form(word: &str) -> bool {
    const FORMS: &[&str] = &["SA", "SAE", "SAECA", "SRL", "EAS", "SACI", "SAIC", "SACIA", "SACIEI", "LTDA", "SC", "CIA"];
    let bare: String = word.chars().filter(char::is_ascii_alphabetic).collect::<String>().to_uppercase();
    FORMS.contains(&bare.as_str())
}

/// The city a receipt was issued in, as a place is named: `ASUNCION
/// (DISTRITO)` is "Asuncion" - the ledger matches it to the person's Asunción
/// however the accents were typed.
#[must_use]
pub fn city_name(printed: &str) -> String {
    let bare = match printed.find('(') {
        Some(at) => &printed[..at],
        None => printed,
    };
    as_written(bare.trim())
}

/// The words a name keeps in lower case after its first: `Ciudad del Este`.
const SMALL: &[&str] = &["de", "del", "la", "las", "los", "el", "y", "e"];

/// A name printed in capitals, in the case a person writes it: `CIUDAD DEL
/// ESTE` is "Ciudad del Este". A name already written in mixed case is left as
/// its owner wrote it.
fn as_written(printed: &str) -> String {
    if printed.chars().any(char::is_lowercase) {
        return printed.to_owned();
    }
    printed
        .split_whitespace()
        .enumerate()
        .map(|(at, word)| {
            let lower = word.to_lowercase();
            if at > 0 && SMALL.contains(&lower.as_str()) {
                lower
            } else if word.chars().any(|c| c.is_ascii_digit()) {
                word.to_owned()
            } else {
                let mut chars = lower.chars();
                chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// The ledger refused the entry, in its own words.
fn refused_by_ledger(message: &str) -> Reason {
    Reason::new(
        Code::DocumentRefusedByLedger,
        json!({ "message": message }),
        format!("the ledger refused it: {message}"),
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use austeris_proto::ledger::v1::line::Side;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::{ADJUSTMENT_NOTE, Said, choose_account, city_name, lines, shop_name};
    use crate::ledger::Account;
    use crate::sifen::{Kind, read};

    fn fixture(name: &str) -> crate::sifen::Receipt {
        let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sifen").join(name)).expect("reading a fixture");
        read(&bytes).expect("a receipt")
    }

    fn account(name: &str, kind: &str, currency: &str) -> Account {
        Account {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            currency: currency.to_owned(),
            closed: false,
        }
    }

    /// Each currency's lines summed, which is what the ledger checks.
    fn sums(lines: &[austeris_proto::ledger::v1::Line]) -> Vec<(String, Decimal)> {
        let mut out: Vec<(String, Decimal)> = Vec::new();
        for line in lines {
            let amount = Decimal::from_str(&line.amount).expect("an amount");
            match out.iter_mut().find(|(currency, _)| *currency == line.currency) {
                Some((_, sum)) => *sum += amount,
                None => out.push((line.currency.clone(), amount)),
            }
        }
        out
    }

    #[test]
    fn a_receipt_is_one_line_per_item_and_one_for_what_the_till_rounded_off() {
        let receipt = fixture("invoice-cash.xml");
        let wallet = account("Wallet", "cash", "PYG");
        let groceries = Uuid::new_v4();
        let booked = lines(&receipt, &wallet, groceries, None).expect("lines");
        assert!(!booked.pending);

        let said: Vec<(String, &str)> = booked.lines.iter().map(|line| (line.amount.clone(), line.note.as_str())).collect();
        assert_eq!(
            said,
            vec![
                // The account moves by what was paid, not by what the items
                // add up to.
                ("-32950".to_owned(), ""),
                ("17000".to_owned(), "LECHE ENTERA LARGA VIDA 1LT × 2"),
                ("6408".to_owned(), "PAN FELIPE X KG × 0.534"),
                ("9550".to_owned(), "GALLETITAS DULCES 400GR"),
                ("-8".to_owned(), ADJUSTMENT_NOTE),
            ]
        );
        assert!(sums(&booked.lines).iter().all(|(_, sum)| sum.is_zero()), "{:?}", sums(&booked.lines));
        assert!(matches!(&booked.lines[0].side, Some(Side::AccountId(id)) if *id == wallet.id.to_string()));
        assert!(
            booked.lines[1..]
                .iter()
                .all(|line| matches!(&line.side, Some(Side::CategoryId(id)) if *id == groceries.to_string()))
        );
    }

    #[test]
    fn a_credit_note_is_the_same_money_coming_back() {
        let receipt = fixture("credit-note.xml");
        assert_eq!(receipt.kind, Kind::CreditNote);
        let booked = lines(&receipt, &account("Wallet", "cash", "PYG"), Uuid::new_v4(), None).expect("lines");
        let amounts: Vec<&str> = booked.lines.iter().map(|line| line.amount.as_str()).collect();
        assert_eq!(amounts, vec!["9550", "-9550"]);
    }

    #[test]
    fn a_receipt_paid_from_another_currency_is_held_at_the_days_rate() {
        // 58 000 guaranies from a dollar card: the dollars are what the bank
        // will say, so the entry is held at the day's rate until it does.
        let receipt = fixture("invoice-card.xml");
        let card = account("Dollar card", "card", "USD");
        let rate = Decimal::ONE / Decimal::from(7350);
        let booked = lines(&receipt, &card, Uuid::new_v4(), Some(rate)).expect("lines");
        assert!(booked.pending);
        assert_eq!(booked.lines[0].amount, "-7.89");
        assert_eq!(booked.lines[0].currency, "USD");
        assert!(sums(&booked.lines).iter().all(|(_, sum)| sum.is_zero()), "{:?}", sums(&booked.lines));
        assert_eq!(booked.lines.iter().filter(|line| matches!(line.side, Some(Side::Conversion(_)))).count(), 2);
    }

    #[test]
    fn a_shop_is_filed_under_the_name_it_trades_under_as_a_person_writes_it() {
        assert_eq!(shop_name(&fixture("invoice-cash.xml")), "Supermercado Sol");
        // No trade name: the legal one, without its legal form.
        assert_eq!(shop_name(&fixture("invoice-card.xml")), "Farmacia Central");

        let mut receipt = fixture("invoice-card.xml");
        receipt.issuer.name = "CASA DE LA MODA S.A.E.C.A.".to_owned();
        assert_eq!(shop_name(&receipt), "Casa de la Moda");
        receipt.issuer.name = "Kiosko 24hs".to_owned();
        assert_eq!(shop_name(&receipt), "Kiosko 24hs", "a name written in mixed case was rewritten");
    }

    #[test]
    fn a_city_is_named_without_what_sifen_adds_to_it() {
        assert_eq!(city_name("ASUNCION (DISTRITO)"), "Asuncion");
        assert_eq!(city_name("CIUDAD DEL ESTE"), "Ciudad del Este");
        assert_eq!(city_name("San Lorenzo"), "San Lorenzo");
    }

    #[test]
    fn cash_is_the_only_cash_account_in_the_receipts_currency_and_otherwise_asked() {
        let receipt = fixture("invoice-cash.xml");
        let wallet = account("Wallet", "cash", "PYG");
        let dollars = account("Dollars", "cash", "USD");
        let bank = account("Bank", "bank", "PYG");

        let all = [wallet.clone(), dollars.clone(), bank.clone()];
        let found = choose_account(&all, &receipt, &Said::default()).expect("the wallet");
        assert_eq!(found.id, wallet.id);

        // Two wallets in guaranies: which one is a guess, and is not made.
        let second = account("Kitchen jar", "cash", "PYG");
        assert!(choose_account(&[wallet.clone(), second, bank.clone()], &receipt, &Said::default()).is_err());

        // A card payment says nothing about which of the person's cards.
        assert!(choose_account(&[wallet.clone(), bank.clone()], &fixture("invoice-card.xml"), &Said::default()).is_err());

        // Said outright, it is taken - when it is one of theirs.
        let said = Said {
            account: Some(bank.id),
            ..Said::default()
        };
        assert_eq!(choose_account(&[wallet.clone(), bank.clone()], &receipt, &said).expect("the bank").id, bank.id);
        let stranger = Said {
            account: Some(Uuid::new_v4()),
            ..Said::default()
        };
        assert!(choose_account(&[wallet.clone(), bank.clone()], &receipt, &stranger).is_err());
    }

    #[test]
    fn a_folder_names_its_account_and_a_closed_or_unknown_one_is_said_back() {
        let receipt = fixture("invoice-card.xml");
        let mut card = account("Visa Debit", "card", "PYG");
        let by_folder = |name: &str| Said {
            account_name: Some(name.to_owned()),
            ..Said::default()
        };
        assert_eq!(
            choose_account(std::slice::from_ref(&card), &receipt, &by_folder("visa debit"))
                .expect("found")
                .id,
            card.id
        );

        let unknown = choose_account(std::slice::from_ref(&card), &receipt, &by_folder("Mastercard")).expect_err("no such account");
        assert_eq!(unknown.expect("a reason").code, "document.unknown_account");

        card.closed = true;
        let closed = choose_account(std::slice::from_ref(&card), &receipt, &by_folder("Visa Debit")).expect_err("closed");
        assert_eq!(closed.expect("a reason").code, "document.closed_account");
    }
}
