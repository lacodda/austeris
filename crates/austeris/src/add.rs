//! The `add` and `login` subcommands: recording an entry from a terminal.
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

    let entry: Entry = response.json().await.context("reading the entry back")?;
    println!("{}", describe(&entry));
    Ok(())
}

/// Just enough of an entry to say what was recorded.
///
/// Deliberately not the ledger's own type: this is a client of an HTTP API, and
/// sharing a struct across that boundary would make a change to the service's
/// internals a compile error here rather than a versioned change to a contract.
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
    use super::{Entry, Line, describe, trim_zeros};

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
