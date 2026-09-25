//! What the ledger knows about currencies themselves.
//!
//! One fact: how many decimal places an amount of a currency has. A card in
//! guaranies is charged in whole guaranies, so 10 USD at 5900.28 leaves it as
//! 59003, not 59002.8 - and an account holding fractions of a unit that does not
//! have any is a balance nobody's bank statement will ever match.

use rust_decimal::{Decimal, RoundingStrategy};

/// ISO 4217 currencies whose amounts are not in hundredths.
///
/// Everything else in [`ISO_4217`] has two places. Kept as the exceptions
/// rather than one table of every code with its number, because the exceptions
/// are what a reader needs to check.
const EXCEPTIONS: &[(&str, u32)] = &[
    ("BHD", 3),
    ("BIF", 0),
    ("CLF", 4),
    ("CLP", 0),
    ("DJF", 0),
    ("GNF", 0),
    ("IQD", 3),
    ("ISK", 0),
    ("JOD", 3),
    ("JPY", 0),
    ("KMF", 0),
    ("KRW", 0),
    ("KWD", 3),
    ("LYD", 3),
    ("OMR", 3),
    ("PYG", 0),
    ("RWF", 0),
    ("TND", 3),
    ("UGX", 0),
    ("UYI", 0),
    ("UYW", 4),
    ("VND", 0),
    ("VUV", 0),
    ("XAF", 0),
    ("XOF", 0),
    ("XPF", 0),
];

/// Every ISO 4217 code with minor units, current and recently withdrawn.
///
/// Withdrawn ones stay: an account opened in levs before Bulgaria took the euro
/// still has an amount with two places. Metals and testing codes (`XAU`, `XTS`)
/// are absent because they have no minor unit, and so is anything not in ISO
/// 4217 at all - `BTC` is a three-letter code with eight places or eighteen
/// depending on who is asked, and the ledger does not pretend to know.
const ISO_4217: &str = "AED AFN ALL AMD ANG AOA ARS AUD AWG AZN BAM BBD BDT BGN BHD BIF BMD BND BOB BOV BRL \
     BSD BTN BWP BYN BZD CAD CDF CHE CHF CHW CLF CLP CNY COP COU CRC CUC CUP CVE CZK DJF DKK DOP DZD EGP ERN \
     ETB EUR FJD FKP GBP GEL GHS GIP GMD GNF GTQ GYD HKD HNL HRK HTG HUF IDR ILS INR IQD IRR ISK JMD JOD JPY \
     KES KGS KHR KMF KPW KRW KWD KYD KZT LAK LBP LKR LRD LSL LYD MAD MDL MGA MKD MMK MNT MOP MRU MUR MVR MWK \
     MXN MXV MYR MZN NAD NGN NIO NOK NPR NZD OMR PAB PEN PGK PHP PKR PLN PYG QAR RON RSD RUB RWF SAR SBD SCR \
     SDG SEK SGD SHP SLE SLL SOS SRD SSP STN SVC SYP SZL THB TJS TMT TND TOP TRY TTD TWD TZS UAH UGX USD USN \
     UYI UYU UYW UZS VED VES VND VUV WST XAF XCD XCG XOF XPF YER ZAR ZMW ZWG ZWL";

/// How many decimal places an amount of `code` has, when that is known.
///
/// `None` for a code outside ISO 4217: an amount in it is kept to every digit
/// it was given, which is never wrong, only untidy.
#[must_use]
pub fn minor_units(code: &str) -> Option<u32> {
    if let Some((_, units)) = EXCEPTIONS.iter().find(|(known, _)| known.eq_ignore_ascii_case(code)) {
        return Some(*units);
    }
    ISO_4217.split_whitespace().any(|known| known.eq_ignore_ascii_case(code)).then_some(2)
}

/// An amount as it can actually be held in `code`.
///
/// Half away from zero, the way a bank rounds a charge - not the banker's
/// rounding `Decimal` does by default, under which 0.5 guarani would become
/// nothing and 1.5 would become 2.
#[must_use]
pub fn round(amount: Decimal, code: &str) -> Decimal {
    match minor_units(code) {
        Some(units) => amount.round_dp_with_strategy(units, RoundingStrategy::MidpointAwayFromZero),
        None => amount,
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    use super::{EXCEPTIONS, ISO_4217, minor_units, round};

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    #[test]
    fn the_three_currencies_of_this_household_have_the_places_their_banks_use() {
        assert_eq!(minor_units("PYG"), Some(0));
        assert_eq!(minor_units("USD"), Some(2));
        assert_eq!(minor_units("rub"), Some(2));
    }

    #[test]
    fn a_code_nobody_standardised_keeps_every_digit() {
        assert_eq!(minor_units("BTC"), None);
        assert_eq!(round(decimal("0.123456789012345678"), "BTC"), decimal("0.123456789012345678"));
    }

    #[test]
    fn a_charge_is_rounded_the_way_a_bank_rounds_it() {
        assert_eq!(round(decimal("59002.8"), "PYG"), decimal("59003"));
        // Banker's rounding would make both of these even.
        assert_eq!(round(decimal("0.5"), "PYG"), decimal("1"));
        assert_eq!(round(decimal("10.125"), "USD"), decimal("10.13"));
        assert_eq!(round(decimal("-10.125"), "USD"), decimal("-10.13"));
    }

    #[test]
    fn every_exception_is_an_iso_code() {
        // An exception spelt wrongly would silently give that currency two
        // places, and nothing else would notice.
        for (code, _) in EXCEPTIONS {
            assert!(ISO_4217.split_whitespace().any(|known| known == *code), "{code} is not in the ISO list");
        }
        assert!(
            ISO_4217
                .split_whitespace()
                .all(|code| code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase()))
        );
    }
}
