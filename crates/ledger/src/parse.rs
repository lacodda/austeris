//! Reading an entry out of one line someone typed.
//!
//! `45000 food lunch` is a whole expense: an amount, what it was for, and a
//! note. This is what makes recording a coffee cost a sentence rather than a
//! form, and it is the difference between a ledger someone keeps and one they
//! abandon in March.
//!
//! A pure function over a string, deliberately. It resolves nothing - not the
//! category, not the account, not today's date - so the CLI, the REST endpoint
//! and the phone screen all call this same code and cannot drift into parsing
//! the same sentence differently. What it returns is what was *said*; turning
//! that into an entry is the caller's half, and needs the database.

use rust_decimal::Decimal;

/// What one typed line said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spoken {
    /// How much. Always positive: which way the money went is `flow`'s job, and
    /// a negative amount typed into an expense would mean it twice.
    pub amount: Decimal,
    /// Which way the money went.
    pub flow: Flow,
    /// The category, as it was written. Resolved against the person's own
    /// categories by the caller - matching is a question about what exists, and
    /// this function does not know.
    pub category: String,
    /// The account, when the line named one after `from` or `to`. `None` means
    /// the person's default.
    pub account: Option<String>,
    /// The currency, when the line named one. `None` means the account's own.
    pub currency: Option<String>,
    /// Whatever was left: the shop, the occasion, the note on the receipt.
    pub note: String,
}

/// Which way the money went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Money spent. What a bare line means.
    Expense,
    /// Money earned. Said with a leading `+`.
    Income,
}

/// Why a line could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Nothing but spaces.
    #[error("there is nothing here to record")]
    Empty,
    /// The line did not start with an amount.
    #[error("`{0}` is not an amount; a line starts with one, as in `45000 food lunch`")]
    NoAmount(String),
    /// An amount of nothing.
    #[error("an entry of zero records nothing")]
    ZeroAmount,
    /// An amount but nothing to attribute it to.
    #[error("`{0}` needs something it was for, as in `{0} food`")]
    NoCategory(String),
    /// A preposition with nothing after it.
    #[error("`{0}` names no account")]
    DanglingAccount(String),
}

/// The words that introduce an account rather than a note.
///
/// `from` for money leaving one, `to` for money arriving. Both are accepted for
/// either flow: a person writing `+200000 salary to bank` and one writing
/// `45000 food from cash` are both naming the account, and refusing one of them
/// would be grammar for its own sake.
const ACCOUNT_WORDS: [&str; 2] = ["from", "to"];

/// Reads a line.
///
/// The shape is `[+]<amount> [<currency>] <category> [from|to <account>] [note...]`.
/// Everything after the category that is not an account is the note, which is
/// why the note needs no quoting and can say anything.
///
/// # Errors
///
/// Returns [`ParseError`] when the line has no amount, no category, or names an
/// account without saying which.
pub fn line(input: &str) -> Result<Spoken, ParseError> {
    let mut words = input.split_whitespace();

    let first = words.next().ok_or(ParseError::Empty)?;

    // A leading `+` is the only mark that turns a line around. `-` is not
    // accepted as its opposite: an expense is what a bare line already means,
    // and offering two ways to write the common case is how `-` ends up
    // silently recording income when someone types it out of habit.
    let (flow, digits) = match first.strip_prefix('+') {
        Some(rest) => (Flow::Income, rest),
        None => (Flow::Expense, first),
    };

    let amount = parse_amount(digits).ok_or_else(|| ParseError::NoAmount(first.to_owned()))?;
    if amount.is_zero() {
        return Err(ParseError::ZeroAmount);
    }

    // A currency may follow the amount: `10 usd food`. Recognised by shape -
    // three letters - rather than by a list, because an installation's
    // currencies are its own and a list here would be one more place to add
    // one. A three-letter category (`gas`, `tax`) would be ambiguous, so the
    // currency only counts when something follows it to be the category.
    let rest: Vec<&str> = words.collect();
    let (currency, rest) = match rest.split_first() {
        Some((first, tail)) if is_currency_code(first) && !tail.is_empty() => (Some(first.to_uppercase()), tail),
        _ => (None, rest.as_slice()),
    };

    let (category, rest) = rest.split_first().ok_or_else(|| ParseError::NoCategory(first.to_owned()))?;

    // The account may be named anywhere in the tail: `45000 food from cash for
    // lunch` and `45000 food lunch from cash` say the same thing, and a person
    // typing quickly does not think about which.
    let mut account = None;
    let mut note_words: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        let word = rest[index];
        if account.is_none() && ACCOUNT_WORDS.iter().any(|w| w.eq_ignore_ascii_case(word)) {
            let Some(named) = rest.get(index + 1) else {
                return Err(ParseError::DanglingAccount(word.to_owned()));
            };
            account = Some((*named).to_owned());
            index += 2;
            continue;
        }
        note_words.push(word);
        index += 1;
    }

    Ok(Spoken {
        amount,
        flow,
        category: (*category).to_owned(),
        account,
        currency,
        note: note_words.join(" "),
    })
}

/// An amount as a person writes one.
///
/// Thousands separators are dropped and a comma is read as a decimal point:
/// `1,500.50`, `1 500,50` and `1500.5` are the same money, and a ledger that
/// refuses two of those spellings is a ledger that gets argued with. The space
/// form arrives already split by [`line`], so what reaches here is one token.
fn parse_amount(text: &str) -> Option<Decimal> {
    // Which of `.` and `,` is the decimal point is decided by which comes last:
    // in `1.500,50` the comma is, in `1,500.50` the dot is. With only one
    // present and three digits after it, it is a thousands separator - `1,500`
    // is fifteen hundred, not one and a half.
    let (last_dot, last_comma) = (text.rfind('.'), text.rfind(','));
    let decimal_at = match (last_dot, last_comma) {
        (Some(dot), Some(comma)) => Some(dot.max(comma)),
        (Some(at), None) | (None, Some(at)) => {
            let after = text.len() - at - 1;
            (after != 3).then_some(at)
        }
        (None, None) => None,
    };

    let mut cleaned = String::with_capacity(text.len());
    for (at, character) in text.char_indices() {
        match character {
            '.' | ',' if Some(at) == decimal_at => cleaned.push('.'),
            '.' | ',' | '_' | '\'' => {}
            digit if digit.is_ascii_digit() => cleaned.push(digit),
            // Anything else means this was never an amount: `food` must not
            // parse as an empty number.
            _ => return None,
        }
    }

    (!cleaned.is_empty() && cleaned != ".").then(|| cleaned.parse().ok()).flatten()
}

/// Whether a word looks like a currency code.
fn is_currency_code(word: &str) -> bool {
    word.len() == 3 && word.chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Flow, ParseError, line, parse_amount};
    use rust_decimal::Decimal;

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    #[test]
    fn the_shortest_useful_line_is_an_amount_and_a_category() {
        let spoken = line("45000 food").expect("parsing");
        assert_eq!(spoken.amount, decimal("45000"));
        assert_eq!(spoken.flow, Flow::Expense);
        assert_eq!(spoken.category, "food");
        assert_eq!(spoken.note, "");
        assert_eq!(spoken.account, None);
        assert_eq!(spoken.currency, None);
    }

    #[test]
    fn everything_after_the_category_is_the_note() {
        // No quoting: a person typing a receipt should not have to think about
        // where the note starts.
        let spoken = line("45000 food lunch with the neighbours").expect("parsing");
        assert_eq!(spoken.category, "food");
        assert_eq!(spoken.note, "lunch with the neighbours");
    }

    #[test]
    fn a_leading_plus_turns_the_line_around() {
        let spoken = line("+2500000 salary september").expect("parsing");
        assert_eq!(spoken.flow, Flow::Income);
        assert_eq!(spoken.amount, decimal("2500000"));
        assert_eq!(spoken.category, "salary");
    }

    #[test]
    fn a_minus_is_not_the_opposite_of_a_plus() {
        // An expense is what a bare line already means. Accepting `-45000`
        // would be a second spelling of the common case, and the day someone
        // types `-` on an income line it would silently record the opposite.
        let error = line("-45000 food").expect_err("a minus should not be an amount");
        assert!(matches!(error, ParseError::NoAmount(_)), "{error:?}");
    }

    #[test]
    fn the_account_can_be_named_before_or_after_the_note() {
        // A person typing quickly does not think about the order.
        let before = line("45000 food from cash lunch").expect("parsing");
        let after = line("45000 food lunch from cash").expect("parsing");
        assert_eq!(before.account.as_deref(), Some("cash"));
        assert_eq!(after.account.as_deref(), Some("cash"));
        assert_eq!(before.note, "lunch");
        assert_eq!(after.note, "lunch");
    }

    #[test]
    fn to_and_from_both_name_the_account() {
        assert_eq!(line("+200000 salary to bank").expect("parsing").account.as_deref(), Some("bank"));
        assert_eq!(line("45000 food from wallet").expect("parsing").account.as_deref(), Some("wallet"));
    }

    #[test]
    fn only_the_first_account_word_counts() {
        // "to" appears in notes: "45000 gift to the neighbours". The first one
        // names the account, the rest is what the person wrote.
        let spoken = line("45000 gifts from cash to the neighbours").expect("parsing");
        assert_eq!(spoken.account.as_deref(), Some("cash"));
        assert_eq!(spoken.note, "to the neighbours");
    }

    #[test]
    fn a_currency_after_the_amount_is_read_as_one() {
        let spoken = line("10 usd coffee airport").expect("parsing");
        assert_eq!(spoken.amount, decimal("10"));
        assert_eq!(spoken.currency.as_deref(), Some("USD"));
        assert_eq!(spoken.category, "coffee");
        assert_eq!(spoken.note, "airport");
    }

    #[test]
    fn a_three_letter_category_is_not_swallowed_as_a_currency() {
        // `45000 gas` is the whole line: there is nothing after `gas` for it to
        // be the currency of, so it is the category.
        let spoken = line("45000 gas").expect("parsing");
        assert_eq!(spoken.currency, None);
        assert_eq!(spoken.category, "gas");
    }

    #[test]
    fn an_amount_is_read_however_it_is_punctuated() {
        // Same money, four spellings. A ledger that refuses three of them is
        // one that gets argued with.
        for spelling in ["1500.50", "1,500.50", "1.500,50", "1_500.50"] {
            assert_eq!(parse_amount(spelling), Some(decimal("1500.50")), "{spelling} did not parse");
        }
    }

    #[test]
    fn a_lone_separator_before_three_digits_is_a_thousands_separator() {
        // `1,500` is fifteen hundred. Read as a decimal point it would be one
        // and a half - a hundredfold error in the direction nobody checks.
        assert_eq!(parse_amount("1,500"), Some(decimal("1500")));
        assert_eq!(parse_amount("1.500"), Some(decimal("1500")));
        // Two digits after it cannot be thousands, so it is the decimal point.
        assert_eq!(parse_amount("1,50"), Some(decimal("1.50")));
    }

    #[test]
    fn a_line_that_is_not_an_entry_says_which_half_is_missing() {
        assert_eq!(line(""), Err(ParseError::Empty));
        assert_eq!(line("   "), Err(ParseError::Empty));
        assert!(matches!(line("food lunch"), Err(ParseError::NoAmount(_))));
        assert!(matches!(line("45000"), Err(ParseError::NoCategory(_))));
        assert_eq!(line("0 food"), Err(ParseError::ZeroAmount));
        assert!(matches!(line("45000 food from"), Err(ParseError::DanglingAccount(_))));
    }

    #[test]
    fn a_word_that_is_not_a_number_is_not_an_empty_amount() {
        // The cleaner strips punctuation; without the rejection below, `food`
        // would strip to nothing and parse as zero.
        assert_eq!(parse_amount("food"), None);
        assert_eq!(parse_amount("."), None);
        assert_eq!(parse_amount("1a2"), None);
    }

    #[test]
    fn spacing_does_not_change_what_was_said() {
        let spaced = line("  45000   food    lunch  ").expect("parsing");
        assert_eq!(spaced.amount, decimal("45000"));
        assert_eq!(spaced.category, "food");
        assert_eq!(spaced.note, "lunch");
    }
}
