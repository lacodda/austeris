//! The `add`, `exchange` and `login` subcommands: recording from a terminal.
//!
//! `austeris add "45000 food lunch"` is the fastest way there is to record an
//! expense, and it is the point of the one-line form - a ledger someone keeps
//! is one where recording a coffee costs a sentence.
//!
//! The line is *not* parsed here. It goes to the running installation as text,
//! and the ledger's own parser reads it: the terminal, the web page and the
//! phone must agree on what `45000 food from cash lunch` means, and three
//! parsers agreeing is three parsers that will one day disagree.
//!
//! Nor does this touch the database. It calls the gateway like any other
//! client, because the gateway is the only public surface (ADR 0001) and a CLI
//! with its own pool would be a second way into the books, with its own copy of
//! every rule about whose account is whose.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

/// Arguments to `austeris add`.
#[derive(Debug, clap::Args)]
pub struct AddArgs {
    /// The entry, as you would say it: `45000 food lunch`.
    ///
    /// Taken as several words rather than one quoted argument, so both
    /// `austeris add 45000 food lunch` and `austeris add "45000 food lunch"`
    /// work - a person typing quickly should not have to think about quoting.
    #[arg(required = true, num_args = 1..)]
    pub words: Vec<String>,

    /// The day it happened, as `YYYY-MM-DD`. Today when omitted.
    #[arg(long, value_name = "DATE")]
    pub on: Option<String>,
}

/// Arguments to `austeris exchange`.
#[derive(Debug, clap::Args)]
pub struct ExchangeArgs {
    /// How much you handed over, in the currency of the account it left.
    #[arg(value_name = "GIVEN")]
    pub given: String,
    /// The account it left, by name.
    #[arg(value_name = "FROM")]
    pub from: String,
    /// How much you received, in the currency of the account it went into.
    #[arg(value_name = "GOT")]
    pub got: String,
    /// The account it went into, by name.
    #[arg(value_name = "TO")]
    pub to: String,

    /// The day it happened, as `YYYY-MM-DD`. Today when omitted.
    #[arg(long, value_name = "DATE")]
    pub on: Option<String>,

    /// What to call it: the exchange office, the bank.
    #[arg(long)]
    pub note: Option<String>,
}

/// Arguments to `austeris login`.
#[derive(Debug, clap::Args)]
pub struct LoginArgs {
    /// The address to sign in as.
    #[arg(long)]
    pub email: String,

    /// The password. Read from the terminal when omitted, so it does not land
    /// in the shell's history or in `ps`.
    #[arg(long)]
    pub password: Option<String>,
}

/// Signs in and keeps the session for `add` to use.
///
/// # Errors
///
/// Returns an error when the installation cannot be reached, the credentials
/// are refused, or the session cannot be stored.
pub async fn login(args: &LoginArgs) -> Result<()> {
    let password = match &args.password {
        Some(password) => password.clone(),
        None => rpassword::prompt_password("password: ").context("reading the password")?,
    };

    let client = austeris_common::http::client();
    let response = client
        .post(format!("{}/api/v1/auth/login", base_url()))
        .json(&serde_json::json!({ "email": args.email, "password": password }))
        .send()
        .await
        .with_context(|| format!("reaching austeris at {}", base_url()))?;

    if !response.status().is_success() {
        bail!("{}", message_from(response).await);
    }

    // The cookie the server set is the session; it is stored rather than the
    // password, so this file being read gives an attacker something the owner
    // can revoke with `logout-everywhere`.
    let cookie = response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .context("the installation did not set a session cookie")?
        .to_owned();

    store_session(&cookie)?;
    println!("Signed in as {}. The session is kept in {}.", args.email, session_path()?.display());
    Ok(())
}

/// Records an entry from one typed line.
///
/// # Errors
///
/// Returns an error when there is no session, the installation cannot be
/// reached, or it refuses the line.
pub async fn add(args: &AddArgs) -> Result<()> {
    let session = read_session()?;
    let text = args.words.join(" ");

    let mut body = serde_json::json!({ "text": text });
    if let Some(on) = &args.on {
        body["occurred_on"] = serde_json::Value::String(on.clone());
    }

    let response = austeris_common::http::client()
        .post(format!("{}/api/v1/ledger/entries/quick", base_url()))
        .header(reqwest::header::COOKIE, session)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("reaching austeris at {}", base_url()))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("that session is no longer good; run `austeris login` again");
    }
    if !response.status().is_success() {
        // The message is the ledger's own - "you have no category called
        // `yachts`", "say which account" - and it is what the person needs.
        bail!("{}", message_from(response).await);
    }

    let recorded: Recorded = response.json().await.context("reading the entry back")?;
    println!("{}", describe(&recorded.entry));
    if let Some(conversion) = &recorded.conversion {
        println!("  {}", describe_conversion(conversion));
    }
    Ok(())
}

/// Records money changed from one account's currency into another's.
///
/// The accounts are named the way `add` names them, and matched the way the
/// ledger matches a name: the whole name, in any case.
///
/// # Errors
///
/// Returns an error when there is no session, when an amount is not one, when
/// an account name matches nothing, or when the installation refuses the
/// exchange.
pub async fn exchange(args: &ExchangeArgs) -> Result<()> {
    let session = read_session()?;
    // The ledger's own reading of an amount, so `600.000` means here what it
    // means in a typed line.
    let amount = |text: &str| austeris_ledger::parse::amount(text).with_context(|| format!("`{text}` is not an amount"));
    let (given, got) = (amount(&args.given)?, amount(&args.got)?);

    let client = austeris_common::http::client();
    let response = client
        .get(format!("{}/api/v1/ledger/accounts", base_url()))
        .header(reqwest::header::COOKIE, &session)
        .send()
        .await
        .with_context(|| format!("reaching austeris at {}", base_url()))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("that session is no longer good; run `austeris login` again");
    }
    if !response.status().is_success() {
        bail!("{}", message_from(response).await);
    }
    let accounts: Vec<Account> = response.json().await.context("reading the accounts")?;
    let (from, to) = (named(&accounts, &args.from)?, named(&accounts, &args.to)?);

    let mut body = serde_json::json!({
        "from_account": from.id,
        "given": given.to_string(),
        "to_account": to.id,
        "got": got.to_string(),
        "description": args.note.clone().unwrap_or_default(),
    });
    if let Some(on) = &args.on {
        body["occurred_on"] = serde_json::Value::String(on.clone());
    }

    let response = client
        .post(format!("{}/api/v1/ledger/exchanges", base_url()))
        .header(reqwest::header::COOKIE, &session)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("reaching austeris at {}", base_url()))?;
    if !response.status().is_success() {
        bail!("{}", message_from(response).await);
    }

    let recorded: Recorded = response.json().await.context("reading the exchange back")?;
    println!(
        "Exchanged {} {} from {} for {} {} into {} on {}",
        trim_zeros(&given.to_string()),
        from.currency,
        from.name,
        trim_zeros(&got.to_string()),
        to.currency,
        to.name,
        recorded.entry.occurred_on
    );
    if let Some(conversion) = &recorded.conversion {
        println!("  {}", describe_conversion(conversion));
    }
    Ok(())
}

/// Just enough of an account to name it and say its currency.
#[derive(Debug, serde::Deserialize)]
struct Account {
    id: String,
    name: String,
    currency: String,
}

/// The account a name refers to: the whole name, in any case - the rule the
/// ledger applies to `from cash` in a typed line.
fn named<'a>(accounts: &'a [Account], name: &str) -> Result<&'a Account> {
    accounts.iter().find(|account| account.name.eq_ignore_ascii_case(name.trim())).with_context(|| {
        format!(
            "you have no account called `{name}`; you have {}",
            accounts.iter().map(|account| account.name.as_str()).collect::<Vec<_>>().join(", ")
        )
    })
}

/// What the installation answers after recording: the entry, and the
/// conversion when money changed currency.
#[derive(Debug, serde::Deserialize)]
struct Recorded {
    entry: Entry,
    conversion: Option<Conversion>,
}

/// Just enough of a conversion to say what it cost.
#[derive(Debug, serde::Deserialize)]
struct Conversion {
    deal: Deal,
    reference: Option<Reference>,
    fee: Option<Money>,
    no_fee: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Deal {
    base: String,
    quote: String,
    rate: String,
}

#[derive(Debug, serde::Deserialize)]
struct Reference {
    base_currency: String,
    quote_currency: String,
    rate: String,
    on_date: String,
    source: String,
    age_days: i64,
    stale: bool,
}

#[derive(Debug, serde::Deserialize)]
struct Money {
    amount: String,
    currency: String,
}

/// One line on what a conversion cost: the deal's rate, the day's rate it was
/// measured against and how old that was, and the fee.
fn describe_conversion(conversion: &Conversion) -> String {
    let deal = &conversion.deal;
    let mut said = vec![format!("at {} {} per {}", trim_zeros(&deal.rate), deal.quote, deal.base)];

    if let Some(reference) = &conversion.reference {
        let age = match reference.age_days {
            0 => String::new(),
            1 => ", a day old".to_owned(),
            days => format!(", {days} days old"),
        };
        let stale = if reference.stale { " - stale" } else { "" };
        said.push(format!(
            "the day's rate was {} {} per {} ({}, {}{age}{stale})",
            trim_zeros(&reference.rate),
            reference.quote_currency,
            reference.base_currency,
            reference.source,
            reference.on_date
        ));
    }

    match (&conversion.fee, conversion.no_fee.as_deref()) {
        (Some(fee), _) => match fee.amount.strip_prefix('-') {
            Some(gained) => said.push(format!("{} {} better than it", trim_zeros(gained), fee.currency)),
            None => said.push(format!("the exchange kept {} {}", trim_zeros(&fee.amount), fee.currency)),
        },
        (None, Some("no_reference")) => said.push("no rate for the day is known, so no fee was measured".to_owned()),
        (None, Some("stale_reference")) => said.push("that rate is too old to measure a fee against".to_owned()),
        (None, _) => {}
    }
    said.join("; ")
}

/// Just enough of an entry to say what was recorded.
///
/// Deliberately not the ledger's own type: this is a client of an HTTP API, and
/// sharing a struct across that boundary would make a change to the service's
/// internals a compile error here rather than a versioned change to a contract.
/// The amount reader above is the exception, because it is not a type of the
/// contract but the one reading of `1.500,50` every surface has to agree on.
#[derive(Debug, serde::Deserialize)]
struct Entry {
    occurred_on: String,
    description: String,
    lines: Vec<Line>,
}

#[derive(Debug, serde::Deserialize)]
struct Line {
    account_id: Option<String>,
    amount: String,
    currency: String,
}

/// One line saying what was recorded, for someone who typed one line.
fn describe(entry: &Entry) -> String {
    // The account's side is the money that actually moved; its sign says which
    // way, and printing it back is how a person sees the line was understood.
    let moved = entry
        .lines
        .iter()
        .find(|line| line.account_id.is_some())
        .map_or_else(|| "?".to_owned(), |line| format!("{} {}", trim_zeros(&line.amount), line.currency));

    let description = if entry.description.is_empty() {
        String::new()
    } else {
        format!(" - {}", entry.description)
    };

    format!("Recorded {} on {}{}", moved, entry.occurred_on, description)
}

/// Drops the trailing zeros a `NUMERIC(38, 18)` comes back with.
///
/// Only for printing. The value keeps every digit everywhere else - this is the
/// one place a person reads it, and `-45000.000000000000000000` is not how
/// anyone says forty-five thousand.
fn trim_zeros(amount: &str) -> String {
    if !amount.contains('.') {
        return amount.to_owned();
    }
    let trimmed = amount.trim_end_matches('0').trim_end_matches('.');
    // "-0.000000000000000000" trims to "-0", and negative zero is not a thing
    // anyone is shown. The empty and bare-minus cases cannot arise from a
    // well-formed amount, but a zero of any spelling prints as one.
    match trimmed {
        "" | "-" | "-0" => "0".to_owned(),
        kept => kept.to_owned(),
    }
}

/// What the installation said went wrong.
async fn message_from(response: reqwest::Response) -> String {
    let status = response.status();
    let Ok(body) = response.text().await else {
        return format!("austeris answered {status}");
    };

    // The shared error body carries a message meant for a person; anything else
    // is shown as it came, rather than swallowed.
    serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|json| json.get("message").and_then(|m| m.as_str().map(ToOwned::to_owned)))
        .unwrap_or_else(|| format!("austeris answered {status}: {body}"))
}

/// Where this installation is.
fn base_url() -> String {
    std::env::var("AUSTERIS_URL").unwrap_or_else(|_| "http://127.0.0.1:8084".to_owned())
}

/// Where the session is kept.
///
/// Under the user's own data directory rather than beside the binary: several
/// people share a machine, and a session is one person's.
fn session_path() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
    }
    .context("no directory to keep the session in; set APPDATA or HOME")?;

    Ok(base.join("austeris").join("session"))
}

/// Writes the session where only its owner can read it.
fn store_session(cookie: &str) -> Result<()> {
    let path = session_path()?;
    std::fs::create_dir_all(path.parent().context("the session path has no directory")?).context("creating the session directory")?;
    std::fs::write(&path, cookie).with_context(|| format!("writing {}", path.display()))?;

    // A session token readable by every account on the machine is a session
    // token anyone on the machine has. Windows has no mode bits; there the
    // directory's own ACL is what protects it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).context("restricting the session file")?;
    }

    Ok(())
}

/// Reads the stored session.
fn read_session() -> Result<String> {
    let path = session_path()?;
    match std::fs::read_to_string(&path) {
        Ok(cookie) if !cookie.trim().is_empty() => Ok(cookie.trim().to_owned()),
        // Not signed in is the ordinary state on a fresh machine, and saying
        // what to do about it beats an io error nobody can act on.
        _ => bail!("not signed in; run `austeris login --email you@example.com`"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Conversion, Deal, Entry, Line, Money, Reference, describe, describe_conversion, trim_zeros};

    fn conversion(fee: Option<&str>, no_fee: Option<&str>, stale: bool) -> Conversion {
        Conversion {
            deal: Deal {
                base: "USD".to_owned(),
                quote: "PYG".to_owned(),
                rate: "5965.000000000000000000".to_owned(),
            },
            reference: Some(Reference {
                base_currency: "USD".to_owned(),
                quote_currency: "PYG".to_owned(),
                rate: "5900.280000000000000000".to_owned(),
                on_date: "2026-09-24".to_owned(),
                source: "bcp".to_owned(),
                age_days: if stale { 9 } else { 1 },
                stale,
            }),
            fee: fee.map(|amount| Money {
                amount: amount.to_owned(),
                currency: "PYG".to_owned(),
            }),
            no_fee: no_fee.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn a_conversion_says_its_rate_the_days_rate_and_what_was_kept() {
        assert_eq!(
            describe_conversion(&conversion(Some("647"), None, false)),
            "at 5965 PYG per USD; the day's rate was 5900.28 PYG per USD (bcp, 2026-09-24, a day old); the exchange kept 647 PYG"
        );
    }

    #[test]
    fn a_stale_rate_is_called_stale_where_it_is_printed() {
        let said = describe_conversion(&conversion(None, Some("stale_reference"), true));
        assert!(said.contains("9 days old - stale"), "{said}");
        assert!(said.contains("too old to measure a fee"), "{said}");
    }

    #[test]
    fn a_deal_better_than_the_days_rate_is_not_printed_as_a_negative_fee() {
        let said = describe_conversion(&conversion(Some("-1003"), None, false));
        assert!(said.ends_with("1003 PYG better than it"), "{said}");
    }

    #[test]
    fn a_stored_amount_is_printed_the_way_a_person_says_it() {
        // `NUMERIC(38, 18)` hands back the scale it stores at. That is right in
        // the API, where a client may need every digit, and wrong in a terminal.
        assert_eq!(trim_zeros("-45000.000000000000000000"), "-45000");
        assert_eq!(trim_zeros("0.123456789012345678"), "0.123456789012345678");
        assert_eq!(trim_zeros("1.500000000000000000"), "1.5");
        assert_eq!(trim_zeros("45000"), "45000");
    }

    #[test]
    fn trimming_never_turns_an_amount_into_nothing() {
        // A value that is all zeros must not print as an empty string or a
        // bare minus sign.
        assert_eq!(trim_zeros("0.000000000000000000"), "0");
        assert_eq!(trim_zeros("-0.000000000000000000"), "0");
    }

    #[test]
    fn what_is_printed_back_is_the_money_that_moved() {
        // The account's side, not the category's: its sign is what tells the
        // person the line was read the way they meant it.
        let entry = Entry {
            occurred_on: "2026-09-17".to_owned(),
            description: "lunch".to_owned(),
            lines: vec![
                Line {
                    account_id: Some("a".to_owned()),
                    amount: "-45000.000000000000000000".to_owned(),
                    currency: "PYG".to_owned(),
                },
                Line {
                    account_id: None,
                    amount: "45000.000000000000000000".to_owned(),
                    currency: "PYG".to_owned(),
                },
            ],
        };
        assert_eq!(describe(&entry), "Recorded -45000 PYG on 2026-09-17 - lunch");
    }

    #[test]
    fn an_entry_with_no_note_does_not_print_a_dangling_dash() {
        let entry = Entry {
            occurred_on: "2026-09-17".to_owned(),
            description: String::new(),
            lines: vec![Line {
                account_id: Some("a".to_owned()),
                amount: "-45000.000000000000000000".to_owned(),
                currency: "PYG".to_owned(),
            }],
        };
        assert_eq!(describe(&entry), "Recorded -45000 PYG on 2026-09-17");
    }
}
