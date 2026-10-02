//! What the ledger knows about countries: which two-letter codes are ones.
//!
//! A place is stored by its ISO 3166-1 alpha-2 code, and a code that looks
//! right but is not one - `UK` for the United Kingdom, whose code is `GB` - would
//! file a trip's spending under a country no report will ever group it with.

/// Every assigned ISO 3166-1 alpha-2 code.
///
/// Taken from the time zone database's `iso3166.tab`, which is maintained
/// beside the standard and ships with every operating system's tzdata.
const ISO_3166: &str = "AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ \
     BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM \
     DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS \
     GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN \
     KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ \
     MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM \
     PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV \
     SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI \
     VN VU WF WS YE YT ZA ZM ZW";

/// The code, upper-cased, when it is an assigned one.
#[must_use]
pub fn code(raw: &str) -> Option<String> {
    let upper = raw.trim().to_ascii_uppercase();
    ISO_3166.split_whitespace().any(|known| known == upper).then_some(upper)
}

#[cfg(test)]
mod tests {
    use super::{ISO_3166, code};

    #[test]
    fn the_countries_of_this_household_are_codes() {
        assert_eq!(code("py").as_deref(), Some("PY"));
        assert_eq!(code(" BR ").as_deref(), Some("BR"));
        assert_eq!(code("ru").as_deref(), Some("RU"));
    }

    #[test]
    fn a_code_that_only_looks_like_one_is_refused() {
        // The United Kingdom is GB; UK is what people type.
        for bad in ["UK", "EU", "XX", "P", "PRY", "", "п"] {
            assert_eq!(code(bad), None, "`{bad}` was taken for a country");
        }
    }

    #[test]
    fn the_list_is_two_upper_case_letters_each_once() {
        let codes: Vec<&str> = ISO_3166.split_whitespace().collect();
        assert!(codes.len() > 240, "the list holds {} codes", codes.len());
        assert!(codes.iter().all(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_uppercase())));
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "a code is listed twice");
    }
}
