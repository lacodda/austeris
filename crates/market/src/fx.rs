//! Official exchange rates: what a currency was worth on a day, as a central
//! bank set it.
//!
//! These sources are unlike a crypto price feed in three ways that shape the
//! code. They publish a whole table at once - every currency the bank tracks,
//! against its own - rather than answering for a list of identifiers. Their
//! rates are for a day, not an instant, and asking for a Sunday returns the
//! table in force on Sunday, which was set on Friday. And they need no key, so
//! they are always on: a household in guaranies and roubles has its rates
//! without signing up for anything.
//!
//! No source is contacted in a test. The fixtures are trimmed recordings of
//! each bank's answer, and a test that exercises the request itself serves them
//! from a local listener.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use rust_decimal::Decimal;

/// How long a central bank is given to answer. They are slow on a good day, and
/// a refresh must not hang behind one that has stopped answering at all.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// What austeris says it is when it asks. A bank's firewall refusing a client
/// with no name is a failure that looks like the bank being down.
const USER_AGENT: &str = concat!("austeris/", env!("CARGO_PKG_VERSION"), " (+https://github.com/lacodda/austeris)");

/// One day's table from one bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The day the rates are in force for, as the bank says - not the day that
    /// was asked for, which may be a weekend.
    pub on_date: NaiveDate,
    /// The currency every rate is priced in: the bank's own.
    pub home: &'static str,
    /// Each currency the bank publishes, with how many of `home` one unit of it
    /// is worth.
    pub rates: Vec<(String, Decimal)>,
}

/// A central bank that publishes a daily table.
#[allow(
    async_fn_in_trait,
    reason = "the only implementors are in this crate, and none of them is stored as a trait object"
)]
pub trait RateSource {
    /// The name its rates are recorded under.
    fn name(&self) -> &'static str;

    /// The currency its rates are priced in.
    fn home(&self) -> &'static str;

    /// The table in force on `day`.
    ///
    /// Always for a day, never "the latest": the Bank of Russia publishes
    /// tomorrow's rates in the afternoon, so its latest table is for a day that
    /// has not begun, and today's would never be recorded by a service started
    /// after that hour.
    async fn table(&self, day: NaiveDate) -> Result<Table>;
}

/// The name Banco Central del Paraguay's rates are recorded under.
pub const BCP: &str = "bcp";

/// The name the Bank of Russia's rates are recorded under.
pub const CBR: &str = "cbr";

/// Banco Central del Paraguay: guaranies per unit of each currency it tracks.
///
/// The reference rate, which for the dollar is the weighted average of the
/// interbank spot market that morning. There is no machine-readable feed; the
/// table on its public page is the publication, and it is read as such.
pub struct Bcp {
    client: reqwest::Client,
    base_url: String,
}

impl Bcp {
    /// The real bank.
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: austeris_common::http::client(),
            base_url: "https://www.bcp.gov.py".to_owned(),
        }
    }

    /// Points the source at another base URL, for tests against a recording.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for Bcp {
    fn default() -> Self {
        Self::new()
    }
}

impl RateSource for Bcp {
    fn name(&self) -> &'static str {
        BCP
    }

    fn home(&self) -> &'static str {
        "PYG"
    }

    async fn table(&self, day: NaiveDate) -> Result<Table> {
        // The page posted back with the date picker's field, as a browser
        // would. A day with no table yet - before the morning's interbank
        // average, or a weekend - is answered with the one in force on it.
        let request = self
            .client
            .post(format!("{}/webapps/web/cotizacion/monedas", self.base_url))
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!("fecha={}", day.format("%d%%2F%m%%2F%Y")));
        let body = fetch(request, "Banco Central del Paraguay").await?;
        parse_bcp(&body).context("reading Banco Central del Paraguay's table")
    }
}

/// The Bank of Russia: roubles per unit of each currency it tracks.
pub struct Cbr {
    client: reqwest::Client,
    base_url: String,
}

impl Cbr {
    /// The real bank.
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: austeris_common::http::client(),
            base_url: "https://www.cbr.ru".to_owned(),
        }
    }

    /// Points the source at another base URL, for tests against a recording.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for Cbr {
    fn default() -> Self {
        Self::new()
    }
}

impl RateSource for Cbr {
    fn name(&self) -> &'static str {
        CBR
    }

    fn home(&self) -> &'static str {
        "RUB"
    }

    async fn table(&self, day: NaiveDate) -> Result<Table> {
        // The English edition: the same numbers, with names that are plain
        // ASCII, so nothing depends on decoding windows-1251.
        let request = self
            .client
            .get(format!("{}/scripts/XML_daily_eng.asp", self.base_url))
            .query(&[("date_req", day.format("%d/%m/%Y").to_string())]);
        let body = fetch(request, "the Bank of Russia").await?;
        parse_cbr(&body).context("reading the Bank of Russia's table")
    }
}

/// Sends a request and reads the answer as text, saying who failed.
async fn fetch(request: reqwest::RequestBuilder, who: &str) -> Result<String> {
    let response = request
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(TIMEOUT)
        .send()
        .await
        .with_context(|| format!("asking {who} for its rates"))?;
    let status = response.status();
    let bytes = response.bytes().await.with_context(|| format!("reading {who}'s answer"))?;
    anyhow::ensure!(status.is_success(), "{who} answered {status}");
    // Lossy on purpose: everything read out of either answer is ASCII - codes,
    // digits, dates - and a byte of a currency's name that is not UTF-8 must not
    // cost the whole table.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads the table out of Banco Central del Paraguay's page.
///
/// The page is HTML meant for a browser, so this reads it for the few things
/// that carry the data: the date field the form posts back, and the rows of the
/// table with id `cotizacion-interbancaria`, whose second cell is the currency
/// code and whose fourth is guaranies per unit. Twenty lines of reading rather
/// than an HTML parser, because those are the only four things taken from it.
fn parse_bcp(body: &str) -> Result<Table> {
    // The date the table is in force for, as the bank filled it in: asking for
    // a Sunday returns Friday's table with Friday in this field.
    let date = between(body, r#"name="fecha" id="fecha" value=""#, "\"").context("the page carries no date")?;
    let on_date = NaiveDate::parse_from_str(date, "%d/%m/%Y").with_context(|| format!("`{date}` is not a date"))?;

    let table = between(body, r#"id="cotizacion-interbancaria""#, "</table>").context("the page carries no rates table")?;
    let rows = between(table, "<tbody>", "</tbody>").context("the rates table has no body")?;
    let rows = without_comments(rows);

    let mut rates = Vec::new();
    for row in rows.split("<tr").skip(1) {
        let cells = cells(row);
        let (Some(code), Some(per_unit)) = (cells.get(1), cells.get(3)) else {
            continue;
        };
        if !is_code(code) {
            continue;
        }
        // Spanish figures: `5.956,04` is five thousand nine hundred and
        // fifty-six guaranies and four céntimos.
        let rate = decimal(&per_unit.replace('.', "").replace(',', ".")).with_context(|| format!("{code} has no rate: `{per_unit}`"))?;
        rates.push((code.clone(), rate));
    }

    if rates.is_empty() {
        bail!("the rates table has no rows");
    }
    Ok(Table { on_date, home: "PYG", rates })
}

/// Reads the table out of the Bank of Russia's `XML_daily` answer.
fn parse_cbr(body: &str) -> Result<Table> {
    let date = between(body, r#"<ValCurs Date=""#, "\"").context("the answer carries no date")?;
    let on_date = NaiveDate::parse_from_str(date, "%d.%m.%Y").with_context(|| format!("`{date}` is not a date"))?;

    let mut rates = Vec::new();
    for valute in body.split("<Valute ").skip(1) {
        let code = between(valute, "<CharCode>", "</CharCode>").context("a currency with no code")?;
        // `VunitRate` is the rate for one unit; `Value` is for `Nominal` units -
        // a hundred yen, ten thousand rupiah. The per-unit figure is used when
        // the bank gives it, since dividing again would add digits it did not
        // publish.
        let rate = if let Some(per_unit) = between(valute, "<VunitRate>", "</VunitRate>") {
            decimal(&per_unit.replace(',', "."))?
        } else {
            let value = decimal(&between(valute, "<Value>", "</Value>").context("a currency with no value")?.replace(',', "."))?;
            let nominal = decimal(between(valute, "<Nominal>", "</Nominal>").context("a currency with no nominal")?)?;
            anyhow::ensure!(!nominal.is_zero(), "{code} is quoted per zero units");
            value / nominal
        };
        if is_code(code) {
            rates.push((code.to_owned(), rate));
        }
    }

    if rates.is_empty() {
        bail!("the answer lists no currencies");
    }
    Ok(Table { on_date, home: "RUB", rates })
}

/// The text between the first `start` and the next `end` after it.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let to = text[from..].find(end)? + from;
    Some(&text[from..to])
}

/// HTML with its comments removed: the bank's page carries a commented-out
/// fifth cell in every row, and reading it as a cell would shift nothing today
/// but would the day a column is added in front of it.
fn without_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = rest[start..].find("-->").map_or("", |end| &rest[start + end + 3..]);
    }
    out.push_str(rest);
    out
}

/// The text of each `<td>` in a row, with tags inside it dropped.
fn cells(row: &str) -> Vec<String> {
    row.split("<td")
        .skip(1)
        .filter_map(|cell| {
            let content = &cell[cell.find('>')? + 1..];
            let content = &content[..content.find("</td>")?];
            Some(text_of(content))
        })
        .collect()
}

/// The visible text of a fragment: tags removed, whitespace collapsed.
fn text_of(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for character in fragment.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            other if !in_tag => text.push(other),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a cell is a currency code.
fn is_code(text: &str) -> bool {
    text.len() == 3 && text.chars().all(|c| c.is_ascii_uppercase())
}

/// A decimal from a figure, or which figure was not one.
fn decimal(text: &str) -> Result<Decimal> {
    text.trim().parse().with_context(|| format!("`{text}` is not a number"))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    use super::{Bcp, Cbr, RateSource, parse_bcp, parse_cbr};

    const BCP_PAGE: &str = include_str!("../fixtures/bcp-2026-09-18.html");

    fn cbr_answer() -> String {
        // The recording is windows-1251, as the bank sends it; read the way
        // `fetch` reads it.
        String::from_utf8_lossy(include_bytes!("../fixtures/cbr-2026-09-19.xml")).into_owned()
    }

    fn rate(table: &super::Table, code: &str) -> Decimal {
        table.rates.iter().find(|(c, _)| c == code).map(|(_, r)| *r).expect("the currency")
    }

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    #[test]
    fn the_paraguayan_table_is_guaranies_per_unit_for_the_day_the_bank_names() {
        let table = parse_bcp(BCP_PAGE).expect("parsing");
        // Asked for on Sunday the 20th; the bank answered with Friday's table.
        assert_eq!(table.on_date, NaiveDate::from_ymd_opt(2026, 9, 18).expect("a date"));
        assert_eq!(table.home, "PYG");
        assert_eq!(rate(&table, "USD"), decimal("5956.04"));
        assert_eq!(rate(&table, "EUR"), decimal("6826.22"));
        // Below a thousand, the figure has no thousands dot to drop.
        assert_eq!(rate(&table, "JPY"), decimal("37.83"));
        assert_eq!(table.rates.len(), 5, "{:?}", table.rates);
    }

    #[test]
    fn the_russian_table_is_roubles_per_single_unit() {
        let table = parse_cbr(&cbr_answer()).expect("parsing");
        assert_eq!(table.on_date, NaiveDate::from_ymd_opt(2026, 9, 19).expect("a date"));
        assert_eq!(table.home, "RUB");
        assert_eq!(rate(&table, "USD"), decimal("84.1975"));
        // Quoted per hundred yen; stored per one.
        assert_eq!(rate(&table, "JPY"), decimal("0.535948"));
        assert_eq!(rate(&table, "AMD"), decimal("0.231668"));
    }

    #[test]
    fn without_the_per_unit_figure_the_value_is_divided_by_the_nominal() {
        let answer = r#"<ValCurs Date="19.09.2026" name="Foreign Currency Market"><Valute ID="R01820"><CharCode>JPY</CharCode><Nominal>100</Nominal><Value>53,5948</Value></Valute></ValCurs>"#;
        let table = parse_cbr(answer).expect("parsing");
        assert_eq!(rate(&table, "JPY"), decimal("0.535948"));
    }

    #[test]
    fn a_page_that_is_not_the_table_is_an_error_rather_than_no_rates() {
        // A maintenance page or a changed layout must fail the refresh loudly;
        // an empty table recorded as "nothing changed" is how rates stop
        // arriving with nobody told.
        assert!(parse_bcp("<html><body>Sitio en mantenimiento</body></html>").is_err());
        assert!(parse_cbr("<html>Service unavailable</html>").is_err());
        let no_rows = BCP_PAGE.replace("<tr", "<xx");
        assert!(parse_bcp(&no_rows).is_err());
    }

    /// Serves a recorded answer on a local port and says what was asked.
    async fn recording(body: Vec<u8>) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
        use axum::extract::OriginalUri;
        use axum::http::Method;

        let (tell, heard) = tokio::sync::mpsc::unbounded_channel();
        let app = axum::Router::new().fallback(move |method: Method, OriginalUri(uri): OriginalUri, form: axum::body::Bytes| {
            let body = body.clone();
            let tell = tell.clone();
            async move {
                let _ = tell.send(format!("{method} {uri} {}", String::from_utf8_lossy(&form)));
                body
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("binding");
        let address = listener.local_addr().expect("an address");
        tokio::spawn(async move { axum::serve(listener, app).await });
        (format!("http://{address}"), heard)
    }

    #[tokio::test]
    async fn another_days_paraguayan_table_is_asked_for_the_way_the_page_asks() {
        let (url, mut heard) = recording(BCP_PAGE.as_bytes().to_vec()).await;
        let source = Bcp::new().with_base_url(url);

        let table = source.table(NaiveDate::from_ymd_opt(2026, 9, 20).expect("a date")).await.expect("a table");
        assert_eq!(table.rates.len(), 5);
        assert_eq!(
            heard.recv().await.expect("a request"),
            "POST /webapps/web/cotizacion/monedas fecha=20%2F09%2F2026"
        );
    }

    #[tokio::test]
    async fn another_days_russian_table_is_asked_for_by_date() {
        let (url, mut heard) = recording(include_bytes!("../fixtures/cbr-2026-09-19.xml").to_vec()).await;
        let source = Cbr::new().with_base_url(url);

        let table = source.table(NaiveDate::from_ymd_opt(2026, 9, 20).expect("a date")).await.expect("a table");
        assert_eq!(table.on_date, NaiveDate::from_ymd_opt(2026, 9, 19).expect("a date"));
        assert_eq!(
            heard.recv().await.expect("a request"),
            "GET /scripts/XML_daily_eng.asp?date_req=20%2F09%2F2026 "
        );
    }
}
