//! An exchange office's slip, read from a photo.
//!
//! The slip says what changed hands - `USD 100,00`, `Gs. 735.000` - and at what
//! rate, `COTIZ. 7.350`. Read off a photo, those fill in an exchange so it is
//! not typed: the amounts, and the rate when only one amount was legible. The
//! photo itself is then kept with the exchange's entry, like any receipt.
//!
//! The photo is read here, on this machine, by Tesseract - no picture of the
//! person's money leaves for anyone's cloud. What was read is only ever a
//! suggestion: the form shows it, and the person saves what they see.
//!
//! Reading the text is pure and kept apart from running the OCR, so what a
//! slip's words are taken to mean is tested without a picture.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

/// What a slip was read as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Reading {
    /// The amounts it names, one per currency: the largest of each, which on a
    /// slip is the one that changed hands rather than a line of its arithmetic.
    pub amounts: Vec<Money>,
    /// The rate it states, or the one its two amounts imply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<Rate>,
    /// The text the photo was read as, for the person to check it against.
    pub text: String,
}

/// An amount in a currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Money {
    /// How much.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "100")]
    pub amount: Decimal,
    /// In what.
    pub currency: String,
}

/// A rate as a slip states it: one of `base` is `rate` of `quote`.
///
/// Either currency may be unknown - a slip naming only dollars, or only
/// guaranies - and is then left out rather than guessed: the form placing the
/// rate knows the two accounts' currencies, and the rate is always the dearer
/// currency priced in the cheaper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Rate {
    /// The currency priced - the dearer one, as an exchange office's board
    /// reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The currency it is priced in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// How many of `quote` one `base` is.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "7350")]
    pub rate: Decimal,
    /// Stated on the slip, rather than worked out from its two amounts.
    pub stated: bool,
}

/// The words a slip writes a currency with, after the accents are taken off
/// and the letters are capitals. A sign is a word of its own; a code is too.
const CURRENCIES: &[(&str, &[&str])] = &[
    ("USD", &["USD", "US$", "U$S", "U$", "DOLAR", "DOLARES", "DOLLAR", "DOLLARS"]),
    ("PYG", &["PYG", "GS", "GS.", "G.", "₲", "GUARANI", "GUARANIES"]),
    ("BRL", &["BRL", "R$", "REAL", "REALES", "REAIS"]),
    ("ARS", &["ARS"]),
    ("EUR", &["EUR", "€", "EURO", "EUROS"]),
    ("RUB", &["RUB", "₽", "RUBLO", "RUBLOS"]),
];

/// The words a slip introduces its rate with.
const RATE_WORDS: &[&str] = &["COTIZ", "COTIZ.", "COTIZACION", "CAMBIO", "T/C", "TC", "T.C.", "TASA", "TIPO", "RATE", "CURSO"];

/// Reads a slip's text.
#[must_use]
pub fn read_text(text: &str) -> Reading {
    let mut amounts: Vec<Money> = Vec::new();
    let mut stated: Option<(Option<String>, Option<String>, Decimal)> = None;

    for line in text.lines() {
        let words = words(line);
        if words.is_empty() {
            continue;
        }

        // `1 USD = 7.350 GS`, or `USD 1 = 7.350`: a rate said outright.
        if let Some(equals) = words.iter().position(|word| word == "=") {
            let (left, right) = (&words[..equals], &words[equals + 1..]);
            let base = left.iter().find_map(|word| currency(word));
            let one = left.iter().filter_map(|word| number(word)).any(|value| value == Decimal::ONE);
            if let (Some(base), true, Some(value)) = (base, one, right.iter().find_map(|word| number(word))) {
                let quote = right.iter().find_map(|word| currency(word));
                stated = Some((Some(base), quote, value));
                continue;
            }
        }

        // `COTIZ. 7.350`: a rate introduced by its word - the number after
        // it, or before it when nothing follows - with a currency beside the
        // word being the one priced.
        let mut rate_at = None;
        if let Some(word_at) = words.iter().position(|word| RATE_WORDS.contains(&word.as_str())) {
            let after = (word_at + 1..words.len()).find(|&at| number(&words[at]).is_some());
            let before = (0..word_at).rev().find(|&at| number(&words[at]).is_some());
            if let Some(at) = after.or(before)
                && let Some(value) = number(&words[at]).filter(|value| *value > Decimal::ONE)
            {
                rate_at = Some(at);
                if stated.is_none() {
                    let near = [word_at.checked_sub(1), Some(word_at + 1), at.checked_sub(1), Some(at + 1)];
                    let priced = near.into_iter().flatten().filter_map(|near| words.get(near)).find_map(|near| currency(near));
                    stated = Some((priced, None, value));
                }
            }
        }

        // An amount beside the currency it is in - before it (`USD 100,00`)
        // or after it (`735.000 Gs`).
        for (at, word) in words.iter().enumerate() {
            if Some(at) == rate_at {
                continue;
            }
            let Some(value) = number(word) else { continue };
            let beside = [at.checked_sub(1), Some(at + 1)]
                .into_iter()
                .flatten()
                .filter_map(|near| words.get(near))
                .find_map(|near| currency(near));
            if let Some(currency) = beside {
                match amounts.iter_mut().find(|money| money.currency == currency) {
                    Some(money) if money.amount < value => money.amount = value,
                    Some(_) => {}
                    None => amounts.push(Money { amount: value, currency }),
                }
            }
        }
    }

    let rate = match stated {
        Some((base, quote, value)) => {
            let (base, quote) = placed(base, quote, &amounts);
            Some(Rate {
                base,
                quote,
                rate: value.normalize(),
                stated: true,
            })
        }
        None => implied(&amounts),
    };

    Reading {
        amounts: amounts
            .into_iter()
            .map(|money| Money {
                amount: money.amount.normalize(),
                currency: money.currency,
            })
            .collect(),
        rate,
        text: text.trim().to_owned(),
    }
}

/// The currencies a stated rate leaves unsaid, from the slip's own amounts.
///
/// With two currencies on the slip, the dearer is the one with the smaller
/// number - it is priced, the other prices it. With one, it is the base when it
/// is a currency a rate above one is usually *of*, and the quote when it is one
/// that is usually the price: a slip saying only `Gs. 735.000` and `7.350`
/// priced something in guaranies.
fn placed(base: Option<String>, quote: Option<String>, amounts: &[Money]) -> (Option<String>, Option<String>) {
    let other = |known: &str| amounts.iter().find(|money| money.currency != known).map(|money| money.currency.clone());
    match (base, quote) {
        (Some(base), Some(quote)) => (Some(base), Some(quote)),
        (Some(base), None) => {
            let quote = other(&base);
            (Some(base), quote)
        }
        (None, Some(quote)) => (other(&quote), Some(quote)),
        (None, None) => match amounts {
            [one, two] => {
                let (dear, cheap) = if one.amount <= two.amount { (one, two) } else { (two, one) };
                (Some(dear.currency.clone()), Some(cheap.currency.clone()))
            }
            [only] if CHEAP.contains(&only.currency.as_str()) => (None, Some(only.currency.clone())),
            [only] => (Some(only.currency.clone()), None),
            _ => (None, None),
        },
    }
}

/// Currencies in which a rate is usually the price, not the thing priced: a
/// unit of any of them is worth less than a unit of most others.
const CHEAP: &[&str] = &["PYG", "ARS", "RUB"];

/// The rate two amounts in two currencies imply: the larger number of the
/// cheaper currency for one of the dearer.
fn implied(amounts: &[Money]) -> Option<Rate> {
    let [first, second] = amounts else { return None };
    let (dear, cheap) = if first.amount <= second.amount { (first, second) } else { (second, first) };
    if dear.amount.is_zero() {
        return None;
    }
    Some(Rate {
        base: Some(dear.currency.clone()),
        quote: Some(cheap.currency.clone()),
        rate: (cheap.amount / dear.amount).round_dp(4).normalize(),
        stated: false,
    })
}

/// A line's words, in capitals without accents, with `=` and the signs that
/// stick to numbers (`US$100`, `₲735.000`) split off as words of their own.
fn words(line: &str) -> Vec<String> {
    let folded: String = line
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'Á' | 'À' | 'Â' | 'Ä' => 'A',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'ó' | 'ò' | 'ô' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Ö' => 'O',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            other => other.to_uppercase().next().unwrap_or(other),
        })
        .collect();
    let mut spaced = String::with_capacity(folded.len() + 8);
    for c in folded.chars() {
        match c {
            '=' | ':' | '₲' | '€' | '₽' => {
                spaced.push(' ');
                if c != ':' {
                    spaced.push(c);
                }
                spaced.push(' ');
            }
            '$' => {
                spaced.push('$');
                spaced.push(' ');
            }
            '(' | ')' | '[' | ']' => spaced.push(' '),
            _ => spaced.push(c),
        }
    }
    // `735 000` is one number: a space between groups of three digits is a
    // thousands separator, and joined here before the line is split.
    let raw: Vec<&str> = spaced.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for word in raw {
        let is_group = word.len() == 3 && word.chars().all(|c| c.is_ascii_digit());
        match out.last_mut() {
            Some(last) if is_group && last.chars().next().is_some_and(|c| c.is_ascii_digit()) && last.chars().all(|c| c.is_ascii_digit()) => {
                last.push_str(word);
            }
            _ => out.push(word.to_owned()),
        }
    }
    out
}

/// The currency a word writes, if it writes one.
fn currency(word: &str) -> Option<String> {
    let bare = word.trim_end_matches([',', ';']);
    CURRENCIES
        .iter()
        .find(|(_, spellings)| spellings.contains(&bare))
        .map(|(code, _)| (*code).to_owned())
}

/// The amount a word writes, if it writes one: `735.000`, `100,00`, `7350`.
fn number(word: &str) -> Option<Decimal> {
    let bare = word.trim_end_matches([',', ';', '.', ')']).trim_start_matches('(');
    if !bare.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    austeris_ledger::parse::amount(bare)
}

/// Why a photo could not be read.
#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    /// This installation has no Tesseract.
    #[error("this installation has no OCR; Tesseract is not installed")]
    Unavailable,
    /// Tesseract ran and failed.
    #[error("the photo could not be read: {0}")]
    Failed(String),
}

/// Tesseract, as found on this machine.
#[derive(Debug, Clone)]
pub struct Ocr {
    program: PathBuf,
    languages: String,
}

/// How long a photo may take to read: a Raspberry Pi reading a phone's
/// twelve-megapixel photo takes seconds, and one that takes minutes is stuck.
const READ_TIMEOUT: Duration = Duration::from_secs(90);

impl Ocr {
    /// Tesseract on this machine's `PATH`, with the languages a slip in
    /// Paraguay is written in when it has them - looked for once.
    #[must_use]
    pub fn find() -> Option<Self> {
        static FOUND: OnceLock<Option<Ocr>> = OnceLock::new();
        FOUND.get_or_init(Self::look).clone()
    }

    fn look() -> Option<Self> {
        let program = PathBuf::from("tesseract");
        let listed = std::process::Command::new(&program).arg("--list-langs").stderr(Stdio::null()).output().ok()?;
        if !listed.status.success() {
            return None;
        }
        let listed = String::from_utf8_lossy(&listed.stdout);
        let has = |language: &str| listed.lines().any(|line| line.trim() == language);
        // Spanish for the words, English for what a slip borrows; digits read
        // the same in either.
        let languages = match (has("spa"), has("eng")) {
            (true, true) => "spa+eng",
            (false, true) => "eng",
            (true, false) => "spa",
            (false, false) => return None,
        };
        Some(Self {
            program,
            languages: languages.to_owned(),
        })
    }

    /// The languages it reads with: `spa+eng`.
    #[must_use]
    pub fn languages(&self) -> &str {
        &self.languages
    }

    /// Reads a photo's text.
    ///
    /// # Errors
    ///
    /// Returns [`OcrError::Failed`] when Tesseract cannot read the picture or
    /// takes longer than a picture takes.
    pub async fn read(&self, image: &[u8]) -> Result<String, OcrError> {
        // Page segmentation 4: one column of text of varying size, which is
        // what a slip is.
        let mut child = tokio::process::Command::new(&self.program)
            .args(["stdin", "stdout", "-l", &self.languages, "--psm", "4"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| OcrError::Failed(format!("starting Tesseract: {error}")))?;

        let mut stdin = child.stdin.take().ok_or_else(|| OcrError::Failed("Tesseract took no input".to_owned()))?;
        let image = image.to_vec();
        let writing = tokio::spawn(async move {
            let written = stdin.write_all(&image).await;
            drop(stdin);
            written
        });

        let output = tokio::time::timeout(READ_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| OcrError::Failed(format!("Tesseract took longer than {} seconds", READ_TIMEOUT.as_secs())))?
            .map_err(|error| OcrError::Failed(format!("running Tesseract: {error}")))?;
        writing
            .await
            .map_err(|error| OcrError::Failed(format!("handing the photo over: {error}")))?
            .map_err(|error| OcrError::Failed(format!("handing the photo over: {error}")))?;
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            return Err(OcrError::Failed(said.lines().last().unwrap_or("Tesseract failed").trim().to_owned()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    use super::{Money, read_text};

    fn money(amount: &str, currency: &str) -> Money {
        Money {
            amount: Decimal::from_str(amount).expect("a decimal"),
            currency: currency.to_owned(),
        }
    }

    #[test]
    fn a_slip_says_what_changed_hands_and_at_what_rate() {
        let reading = read_text(
            "CAMBIOS EJEMPLO S.A.\n\
             COMPRA DE DIVISAS\n\
             FECHA: 18/10/2026 10:42\n\
             USD 100,00\n\
             COTIZ. 7.350\n\
             TOTAL Gs. 735.000\n\
             GRACIAS POR SU PREFERENCIA",
        );
        assert_eq!(reading.amounts, vec![money("100", "USD"), money("735000", "PYG")]);
        let rate = reading.rate.expect("a rate");
        assert_eq!(
            (rate.base.as_deref(), rate.quote.as_deref(), rate.rate, rate.stated),
            (Some("USD"), Some("PYG"), Decimal::from(7350), true)
        );
    }

    #[test]
    fn a_rate_written_as_an_equation_names_its_currencies() {
        let reading = read_text("1 USD = 7.350 Gs\nRECIBE: 1.470.000 Gs");
        let rate = reading.rate.expect("a rate");
        assert_eq!(
            (rate.base.as_deref(), rate.quote.as_deref(), rate.rate),
            (Some("USD"), Some("PYG"), Decimal::from(7350))
        );
        // The rate is not taken for an amount of guaranies.
        assert_eq!(reading.amounts, vec![money("1470000", "PYG")]);
    }

    #[test]
    fn a_rate_beside_an_amount_on_one_line_is_told_apart_from_it() {
        let reading = read_text("USD 100\nTOTAL GS 735.000 (T/C 7.350)");
        assert_eq!(reading.amounts, vec![money("100", "USD"), money("735000", "PYG")]);
        assert_eq!(reading.rate.expect("a rate").rate, Decimal::from(7350));
    }

    #[test]
    fn two_amounts_imply_the_rate_when_none_is_stated() {
        let reading = read_text("ENTREGA US$ 250.00\nRECIBE ₲ 1 837 500");
        assert_eq!(reading.amounts, vec![money("250", "USD"), money("1837500", "PYG")]);
        let rate = reading.rate.expect("a rate");
        assert_eq!(
            (rate.base.as_deref(), rate.quote.as_deref(), rate.rate, rate.stated),
            (Some("USD"), Some("PYG"), Decimal::from(7350), false)
        );
    }

    #[test]
    fn of_several_amounts_in_one_currency_the_one_that_changed_hands_is_kept() {
        // The cash handed over and the change are lines of the arithmetic;
        // what was exchanged is the largest.
        let reading = read_text("USD 300\nCOTIZACION 7.340\nTOTAL GS 2.202.000\nVUELTO GS 2.000");
        assert_eq!(reading.amounts, vec![money("300", "USD"), money("2202000", "PYG")]);
    }

    #[test]
    fn a_slip_that_says_nothing_legible_is_read_as_nothing_rather_than_guessed() {
        let reading = read_text("C4MB10S\nG R A C I A S\n12/10/2026");
        assert_eq!(reading.amounts, Vec::new());
        assert_eq!(reading.rate, None);
        // A number with no currency beside it is not money.
        let reading = read_text("TICKET 004512\nCAJA 3");
        assert_eq!(reading.amounts, Vec::new());
    }

    #[test]
    fn a_stated_rate_with_one_currency_says_what_it_does_not_know() {
        let reading = read_text("DOLARES 100\nT/C 7.350");
        let rate = reading.rate.expect("a rate");
        assert_eq!((rate.base.as_deref(), rate.quote.as_deref()), (Some("USD"), None));

        // Guaranies priced nothing; they are what something was priced in.
        let reading = read_text("TOTAL Gs. 735.000\nCOTIZ 7.350");
        let rate = reading.rate.expect("a rate");
        assert_eq!((rate.base.as_deref(), rate.quote.as_deref()), (None, Some("PYG")));
    }
}
