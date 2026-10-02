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
    /// The payment is held, not posted: said with a leading `~`.
    pub pending: bool,
    /// The category, as it was written. Resolved against the person's own
    /// categories by the caller - matching is a question about what exists, and
    /// this function does not know. `None` when the line named a counterparty
    /// instead, whose usual category the caller looks up.
    pub category: Option<String>,
    /// Who the money went to or came from, as written after `@`.
    pub counterparty: Option<String>,
    /// The labels written after `#`, each once, in the order they appeared.
    pub tags: Vec<String>,
    /// The account, when the line named one after `from` or `to`. `None` means
    /// the person's default.
    pub account: Option<String>,
    /// The currency, when the line named one. `None` means the account's own.
    pub currency: Option<String>,
    /// The rate the amount was converted at, when the line stated one with
    /// `@`: how many of the account's currency one of `currency` cost. `None`
    /// means the day's rate.
    pub rate: Option<Decimal>,
    /// Whatever was left: the shop, the occasion, the note on the receipt.
    pub note: String,
    /// The same line with the currency word read as the category instead, when
    /// that word is a currency code that is also a word - `45000 pen office`.
    /// Which reading is meant depends on the person's categories, which this
    /// function does not know; the caller does.
    pub if_not_a_currency: Option<Box<Spoken>>,
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
    #[error("`{0}` needs something it was for, as in `{0} food`, or who it was paid to, as in `{0} @shop`")]
    NoCategory(String),
    /// A preposition with nothing after it.
    #[error("`{0}` names no account")]
    DanglingAccount(String),
    /// An `@` followed by a number that is not one.
    #[error("`{0}` is not a rate; write it as `@5965`, how many of the account's currency one unit cost")]
    BadRate(String),
    /// An `@` followed by nothing.
    #[error("`@` names nothing; write `@5965` for the rate charged or `@shop` for who was paid")]
    BareAt,
    /// A rate or a counterparty said twice.
    #[error("`{0}` says again what the line already said; one rate and one counterparty to a line")]
    Twice(String),
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
/// The shape is `[~][+]<amount> [<currency>] [<category>] [note...]`, with any
/// of `from|to <account>`, `@<rate>`, `@<counterparty>` and `#<tag>` anywhere
/// after the amount. The category is the first word that is none of those;
/// every plain word after it is the note, which is why the note needs no
/// quoting and can say anything.
///
/// # Errors
///
/// Returns [`ParseError`] when the line has no amount, says neither a category
/// nor a counterparty, names an account without saying which, or says a rate or
/// a counterparty twice.
pub fn line(input: &str) -> Result<Spoken, ParseError> {
    let mut words = input.split_whitespace();

    let first = words.next().ok_or(ParseError::Empty)?;

    // A leading `+` is the only mark that turns a line around. `-` is not
    // accepted as its opposite: an expense is what a bare line already means,
    // and offering two ways to write the common case is how `-` ends up
    // silently recording income when someone types it out of habit.
    //
    // A leading `~` says the payment is held rather than posted - roughly this
    // much, not final yet, which is what a card's hold is. Either mark may come
    // first: `~+` and `+~` are the same line, and refusing one would be
    // grammar for its own sake.
    let (mut flow, mut pending, mut digits) = (Flow::Expense, false, first);
    loop {
        if flow == Flow::Expense
            && let Some(rest) = digits.strip_prefix('+')
        {
            (flow, digits) = (Flow::Income, rest);
        } else if !pending && let Some(rest) = digits.strip_prefix('~') {
            (pending, digits) = (true, rest);
        } else {
            break;
        }
    }

    let amount = parse_amount(digits).ok_or_else(|| ParseError::NoAmount(first.to_owned()))?;
    if amount.is_zero() {
        return Err(ParseError::ZeroAmount);
    }
    let head = Head { amount, flow, pending, first };

    // A currency may follow the amount: `10 usd food`. Only an ISO 4217 code
    // counts, and only when something follows it to be the category. Since an
    // amount in another currency is converted rather than refused, reading
    // `fun` or `gas` as a currency would move money at some rate nobody meant;
    // the list keeps that to the few codes that are also words (`pen`, `cup`),
    // and for those the reading as a category is kept alongside, for the
    // caller to prefer when the person has a category by that name.
    let rest: Vec<&str> = words.collect();
    let plain = || read(&head, None, &rest);
    match rest.split_first() {
        Some((word, tail)) if is_currency_code(word) && !tail.is_empty() => match read(&head, Some(word.to_uppercase()), tail) {
            Ok(mut spoken) => {
                spoken.if_not_a_currency = plain().ok().map(Box::new);
                Ok(spoken)
            }
            // `45000 cup #home` says nothing it was for once `cup` is taken
            // as the currency, so it was the category all along.
            Err(_) => plain(),
        },
        _ => plain(),
    }
}

/// What the first word of a line said.
struct Head<'a> {
    amount: Decimal,
    flow: Flow,
    pending: bool,
    first: &'a str,
}

/// Reads what follows the amount and its currency: the category, the account,
/// a rate, a counterparty, tags and the note.
fn read(head: &Head<'_>, currency: Option<String>, rest: &[&str]) -> Result<Spoken, ParseError> {
    // Everything but the category and the note may be said anywhere: `45000
    // food from cash lunch` and `45000 food lunch from cash` say the same
    // thing, and a person typing quickly does not think about which.
    let mut category: Option<&str> = None;
    let mut account = None;
    let mut rate = None;
    let mut counterparty: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();
    let mut note_words: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        let word = rest[index];
        index += 1;

        // `@` starts a token of its own, so a note can still hold an `@` inside
        // a word - an email address is neither a rate nor a counterparty.
        if let Some(after) = word.strip_prefix('@') {
            match at_sign(after)? {
                At::Rate(value) if rate.is_none() => rate = Some(value),
                At::Counterparty(name) if counterparty.is_none() => counterparty = Some(name),
                // Said twice, which of the two was meant is a guess.
                _ => return Err(ParseError::Twice(word.to_owned())),
            }
            continue;
        }
        if let Some(tag) = word.strip_prefix('#').and_then(|after| tag_name(trailing_punctuation(after))) {
            if !tags.iter().any(|seen| seen.to_lowercase() == tag.to_lowercase()) {
                tags.push(tag);
            }
            continue;
        }
        if account.is_none() && ACCOUNT_WORDS.iter().any(|w| w.eq_ignore_ascii_case(word)) {
            let Some(named) = rest.get(index) else {
                return Err(ParseError::DanglingAccount(word.to_owned()));
            };
            account = Some((*named).to_owned());
            index += 1;
            continue;
        }
        match category {
            None => category = Some(word),
            Some(_) => note_words.push(word),
        }
    }

    if category.is_none() && counterparty.is_none() {
        return Err(ParseError::NoCategory(head.first.to_owned()));
    }

    Ok(Spoken {
        amount: head.amount,
        flow: head.flow,
        pending: head.pending,
        category: category.map(ToOwned::to_owned),
        counterparty,
        tags,
        account,
        currency,
        rate,
        note: note_words.join(" "),
        if_not_a_currency: None,
    })
}

/// What a word after `@` says.
enum At {
    /// The rate the amount was charged at.
    Rate(Decimal),
    /// Who was paid, or who paid.
    Counterparty(String),
}

/// Reads what follows an `@`: a rate when it starts with a digit, a
/// counterparty otherwise.
///
/// Decided by the first character alone, so `@59oo` is a mistyped rate and
/// refused - kept as a counterparty called `59oo`, the amount would convert at
/// the day's rate while the person believes they stated one.
fn at_sign(after: &str) -> Result<At, ParseError> {
    let said = trailing_punctuation(after);
    match said.chars().next() {
        None => Err(ParseError::BareAt),
        Some(first) if first.is_ascii_digit() => parse_amount(said)
            .filter(|value| !value.is_zero())
            .map(At::Rate)
            .ok_or_else(|| ParseError::BadRate(format!("@{after}"))),
        Some(_) => Ok(At::Counterparty(said.to_owned())),
    }
}

/// A word without the punctuation a sentence leaves on it: `#trip,` is `trip`.
fn trailing_punctuation(word: &str) -> &str {
    word.trim_end_matches(['.', ',', ';', ':', '!', '?'])
}

/// A tag's name, when the text is one: a letter, then letters, digits, `-` and
/// `_`.
///
/// A letter first so `order #1234` stays a note about an order. The same rule
/// holds for a tag created any other way, so every tag can be typed after a
/// `#` and found again.
#[must_use]
pub fn tag_name(text: &str) -> Option<String> {
    let text = text.trim();
    let mut chars = text.chars();
    let starts_with_a_letter = chars.next().is_some_and(char::is_alphabetic);
    (starts_with_a_letter && chars.all(|c| c.is_alphanumeric() || c == '-' || c == '_')).then(|| text.to_owned())
}

/// An amount as a person writes it, read the way a typed line reads one.
///
/// For the surfaces that take an amount on its own - `austeris exchange 600.000
/// cash 100 dollars` - so `600.000` means there what it means in a line.
#[must_use]
pub fn amount(text: &str) -> Option<Decimal> {
    parse_amount(text.trim()).filter(|value| !value.is_zero())
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

/// Whether a word is a currency code.
fn is_currency_code(word: &str) -> bool {
    crate::currency::minor_units(word).is_some()
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
        assert_eq!(spoken.category.as_deref(), Some("food"));
        assert_eq!(spoken.note, "");
        assert_eq!(spoken.account, None);
        assert_eq!(spoken.currency, None);
    }

    #[test]
    fn everything_after_the_category_is_the_note() {
        // No quoting: a person typing a receipt should not have to think about
        // where the note starts.
        let spoken = line("45000 food lunch with the neighbours").expect("parsing");
        assert_eq!(spoken.category.as_deref(), Some("food"));
        assert_eq!(spoken.note, "lunch with the neighbours");
    }

    #[test]
    fn a_leading_plus_turns_the_line_around() {
        let spoken = line("+2500000 salary september").expect("parsing");
        assert_eq!(spoken.flow, Flow::Income);
        assert_eq!(spoken.amount, decimal("2500000"));
        assert_eq!(spoken.category.as_deref(), Some("salary"));
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
        assert_eq!(spoken.category.as_deref(), Some("coffee"));
        assert_eq!(spoken.note, "airport");
    }

    #[test]
    fn a_rate_is_said_with_an_at_sign_anywhere_after_the_category() {
        let spoken = line("10 usd subscriptions @5965 streaming from card").expect("parsing");
        assert_eq!(spoken.rate, Some(decimal("5965")));
        assert_eq!(spoken.account.as_deref(), Some("card"));
        assert_eq!(spoken.note, "streaming");

        // Punctuated the way the amount may be.
        assert_eq!(line("10 usd food @5.965,50").expect("parsing").rate, Some(decimal("5965.50")));
        // Without one, the day's rate is meant.
        assert_eq!(line("10 usd food").expect("parsing").rate, None);
    }

    #[test]
    fn an_at_sign_inside_a_word_is_part_of_the_note() {
        let spoken = line("45000 food paid for bob@example.com").expect("parsing");
        assert_eq!(spoken.rate, None);
        assert_eq!(spoken.note, "paid for bob@example.com");
    }

    #[test]
    fn an_at_sign_with_no_rate_after_it_is_refused_rather_than_kept_as_a_note() {
        // Kept as a note, `@59oo` would convert at the day's rate while the
        // person believes they stated one.
        assert!(matches!(line("10 usd food @59oo"), Err(ParseError::BadRate(_))));
        assert!(matches!(line("10 usd food @0"), Err(ParseError::BadRate(_))));
        assert_eq!(line("10 usd food @"), Err(ParseError::BareAt));
    }

    #[test]
    fn an_at_sign_before_a_name_is_who_was_paid() {
        let spoken = line("45000 food @superseis lunch").expect("parsing");
        assert_eq!(spoken.counterparty.as_deref(), Some("superseis"));
        assert_eq!(spoken.category.as_deref(), Some("food"));
        assert_eq!(spoken.note, "lunch");
        assert_eq!(spoken.rate, None);

        // A rate and a counterparty in one line, in either order.
        let both = line("10 usd subscriptions @5965 @netflix").expect("parsing");
        assert_eq!((both.rate, both.counterparty.as_deref()), (Some(decimal("5965")), Some("netflix")));
        // The comma a sentence leaves is not part of the name.
        assert_eq!(
            line("45000 food @superseis, lunch").expect("parsing").counterparty.as_deref(),
            Some("superseis")
        );
    }

    #[test]
    fn a_counterparty_can_stand_for_the_category() {
        // "This shop is groceries": the caller looks the category up.
        let spoken = line("45000 @superseis").expect("parsing");
        assert_eq!(spoken.category, None);
        assert_eq!(spoken.counterparty.as_deref(), Some("superseis"));

        // Any plain word after it is the category, not a note: which of the
        // two a word was meant as cannot be read off the word.
        let spoken = line("45000 @superseis lunch").expect("parsing");
        assert_eq!(spoken.category.as_deref(), Some("lunch"));
        assert_eq!(spoken.note, "");
    }

    #[test]
    fn a_rate_or_a_counterparty_said_twice_is_refused_rather_than_one_picked() {
        assert!(matches!(line("10 usd food @5965 @5970"), Err(ParseError::Twice(_))));
        assert!(matches!(line("45000 food @superseis @stock"), Err(ParseError::Twice(_))));
    }

    #[test]
    fn tags_are_said_with_a_hash_anywhere_after_the_amount() {
        let spoken = line("45000 #holiday-2027 food dinner #Beach #holiday-2027").expect("parsing");
        assert_eq!(spoken.category.as_deref(), Some("food"));
        assert_eq!(spoken.note, "dinner");
        // Each once, however often written and in whatever case.
        assert_eq!(spoken.tags, ["holiday-2027", "Beach"]);
        // A tag may be in any alphabet: it is the person's own word.
        assert_eq!(line("45000 food #отпуск-2027").expect("parsing").tags, ["отпуск-2027"]);
    }

    #[test]
    fn a_hash_before_a_number_is_part_of_the_note() {
        // An order number is not a label.
        let spoken = line("45000 food order #1234").expect("parsing");
        assert_eq!(spoken.tags, Vec::<String>::new());
        assert_eq!(spoken.note, "order #1234");
        assert_eq!(line("45000 food #").expect("parsing").note, "#");
    }

    #[test]
    fn a_leading_tilde_says_the_payment_is_held() {
        let spoken = line("~120000 fuel from card").expect("parsing");
        assert!(spoken.pending);
        assert_eq!(spoken.amount, decimal("120000"));
        assert_eq!(spoken.flow, Flow::Expense);
        assert!(!line("120000 fuel").expect("parsing").pending);

        // With a plus, in either order.
        for spelling in ["~+5000 refund", "+~5000 refund"] {
            let spoken = line(spelling).expect("parsing");
            assert!(spoken.pending, "{spelling}");
            assert_eq!(spoken.flow, Flow::Income, "{spelling}");
        }
        // Each mark once.
        assert!(matches!(line("~~5000 fuel"), Err(ParseError::NoAmount(_))));
        assert!(matches!(line("++5000 salary"), Err(ParseError::NoAmount(_))));
    }

    #[test]
    fn the_category_is_the_first_word_that_says_nothing_else() {
        let spoken = line("45000 from cash #trip food lunch").expect("parsing");
        assert_eq!(spoken.category.as_deref(), Some("food"));
        assert_eq!(spoken.account.as_deref(), Some("cash"));
        assert_eq!(spoken.note, "lunch");
    }

    #[test]
    fn a_currency_word_with_only_marks_after_it_is_the_category() {
        // `cup` taken as the currency would leave the line saying nothing it
        // was for.
        let spoken = line("45000 cup #kitchen").expect("parsing");
        assert_eq!((spoken.currency, spoken.category.as_deref()), (None, Some("cup")));
        // With a counterparty after it, the currency reading stands.
        let spoken = line("10 usd @netflix").expect("parsing");
        assert_eq!((spoken.currency.as_deref(), spoken.category), (Some("USD"), None));
    }

    #[test]
    fn a_tag_name_starts_with_a_letter_and_is_one_word() {
        assert_eq!(super::tag_name("holiday-2027").as_deref(), Some("holiday-2027"));
        assert_eq!(super::tag_name("  work_trip ").as_deref(), Some("work_trip"));
        for bad in ["", "2027", "-trip", "two words", "trip!", "a#b"] {
            assert_eq!(super::tag_name(bad), None, "`{bad}` was taken for a tag");
        }
    }

    #[test]
    fn a_three_letter_word_that_is_no_currency_is_the_category() {
        // `fun` is not a currency; read as one, the line would have converted
        // money at a rate for a currency that does not exist.
        let spoken = line("45000 fun cinema").expect("parsing");
        assert_eq!(spoken.currency, None);
        assert_eq!(spoken.category.as_deref(), Some("fun"));
        assert_eq!(spoken.note, "cinema");
        assert_eq!(spoken.if_not_a_currency, None);
    }

    #[test]
    fn a_currency_code_that_is_also_a_word_keeps_both_readings() {
        let spoken = line("45000 pen office supplies").expect("parsing");
        assert_eq!(spoken.currency.as_deref(), Some("PEN"));
        assert_eq!(spoken.category.as_deref(), Some("office"));
        let other = spoken.if_not_a_currency.expect("the other reading");
        assert_eq!(
            (other.currency, other.category.as_deref(), other.note.as_str()),
            (None, Some("pen"), "office supplies")
        );
    }

    #[test]
    fn a_three_letter_category_is_not_swallowed_as_a_currency() {
        // `45000 gas` is the whole line: there is nothing after `gas` for it to
        // be the currency of, so it is the category.
        let spoken = line("45000 gas").expect("parsing");
        assert_eq!(spoken.currency, None);
        assert_eq!(spoken.category.as_deref(), Some("gas"));
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
        assert_eq!(spaced.category.as_deref(), Some("food"));
        assert_eq!(spaced.note, "lunch");
    }
}
