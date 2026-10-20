//! The `import` subcommand: receipts read into the books from a terminal.
//!
//! `austeris import Downloads/factura.xml` hands the file to the running
//! installation, which reads it the way it reads an upload or a file dropped in
//! its folder; this says what it became. Like `add`, it reads nothing itself
//! and touches no database - the installation is the one place a receipt means
//! something (see `add.rs`).
//!
//! A receipt that waits - for which account paid, or what it was for - is
//! finished by importing it again with `--from` or `--for`: the same file is
//! the same document, and what is said the second time is what it waited for.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::add::{Account, answered, base_url, message_from, named, read_session, trim_zeros};

/// Arguments to `austeris import`.
#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    /// The receipts: files, or folders whose files are each one.
    #[arg(required = true, num_args = 1.., value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// The account they were paid from, by name - for receipts that do not
    /// say, as one paid by card does not say which card.
    #[arg(long, value_name = "ACCOUNT")]
    pub from: Option<String>,

    /// What they were for, by category name or path (`living/food`) - for a
    /// shop that has no usual category yet. The shop takes it as its usual
    /// one, so its next receipt needs nothing said.
    #[arg(long = "for", value_name = "CATEGORY")]
    pub category: Option<String>,
}

/// Imports receipts.
///
/// # Errors
///
/// Returns an error when there is no session, a path cannot be read, an
/// account or category named matches none, or the installation cannot be
/// reached or refuses a file outright. A receipt that waits or cannot be read
/// into the books is said, not an error.
pub async fn import(args: &ImportArgs) -> Result<()> {
    let session = read_session()?;
    let files = files(&args.paths)?;
    if files.is_empty() {
        bail!("there are no files there to import");
    }

    let client = austeris_common::http::client();
    let account = match &args.from {
        Some(name) => {
            let accounts: Vec<Account> = get(&client, &session, "/api/v1/ledger/accounts").await?;
            Some(named(&accounts, name)?.id.clone())
        }
        None => None,
    };
    let category = match &args.category {
        Some(name) => {
            let categories: Vec<Category> = get(&client, &session, "/api/v1/ledger/categories").await?;
            Some(category_named(&categories, name)?)
        }
        None => None,
    };

    let mut tally = Tally::default();
    for file in &files {
        let name = file.file_name().and_then(|name| name.to_str()).unwrap_or("document").to_owned();
        let content = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        let mut query = vec![("name", name.clone())];
        if let Some(account) = &account {
            query.push(("account", account.clone()));
        }
        if let Some(category) = &category {
            query.push(("category", category.clone()));
        }
        let response = client
            .post(format!("{}/api/v1/imports/documents", base_url()))
            .query(&query)
            .header(reqwest::header::COOKIE, &session)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(content)
            .send()
            .await
            .with_context(|| format!("reaching austeris at {}", base_url()))?;
        let response = answered(response).await.with_context(|| format!("importing {name}"))?;
        let upload: Upload = response.json().await.context("reading what the receipt became")?;
        println!("{name}: {}", said(&upload));
        tally.count(&upload.document.state);
    }

    if files.len() > 1 {
        println!("{}", tally.say());
    }
    if tally.waiting > 0 {
        println!("To finish a waiting one, import it again with --from ACCOUNT and --for CATEGORY, or book it on the Import screen.");
    }
    Ok(())
}

/// The files the paths name: each file, and each file directly in a folder,
/// without hidden ones.
fn files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in paths {
        let meta = std::fs::metadata(path).with_context(|| format!("{} is not there", path.display()))?;
        if meta.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(path)
                .with_context(|| format!("reading {}", path.display()))?
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .map(|entry| entry.path())
                .filter(|path| !is_hidden(path))
                .collect();
            inside.sort();
            out.extend(inside);
        } else {
            out.push(path.clone());
        }
    }
    Ok(out)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with('.'))
}

/// A list the installation answers with.
async fn get<T: serde::de::DeserializeOwned>(client: &reqwest::Client, session: &str, path: &str) -> Result<T> {
    let response = client
        .get(format!("{}{path}", base_url()))
        .header(reqwest::header::COOKIE, session)
        .send()
        .await
        .with_context(|| format!("reaching austeris at {}", base_url()))?;
    if !response.status().is_success() {
        bail!("{}", message_from(response).await);
    }
    response.json().await.with_context(|| format!("reading {path}"))
}

/// Just enough of a category to find it by name.
#[derive(Debug, serde::Deserialize)]
struct Category {
    id: String,
    parent_id: Option<String>,
    name: String,
}

/// The category a name or a path names: `food`, or `living/food` when two
/// share the name - the rule a typed line follows.
fn category_named(categories: &[Category], typed: &str) -> Result<String> {
    let path: Vec<String> = typed
        .split('/')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    let Some(leaf) = path.last() else {
        bail!("say what it was for, by a category's name");
    };
    let found: Vec<&Category> = categories
        .iter()
        .filter(|category| category.name.to_lowercase() == *leaf)
        .filter(|category| {
            // Walk up as far as the path says, matching each parent.
            let mut at = category.parent_id.as_deref();
            for parent in path.iter().rev().skip(1) {
                let Some(id) = at else { return false };
                let Some(above) = categories.iter().find(|candidate| candidate.id == id) else {
                    return false;
                };
                if above.name.to_lowercase() != *parent {
                    return false;
                }
                at = above.parent_id.as_deref();
            }
            true
        })
        .collect();
    match found.as_slice() {
        [only] => Ok(only.id.clone()),
        [] => bail!("you have no category called `{typed}`"),
        several => bail!(
            "`{typed}` means {} categories; say which by its path, as in `parent/{}`",
            several.len(),
            several[0].name
        ),
    }
}

/// What the installation answered for one file.
#[derive(Debug, serde::Deserialize)]
struct Upload {
    document: Document,
    already: bool,
}

/// Just enough of a document to say what it became.
#[derive(Debug, serde::Deserialize)]
struct Document {
    state: String,
    #[serde(default)]
    shop: Option<String>,
    #[serde(default)]
    receipt: Option<Receipt>,
    #[serde(default)]
    needs: Vec<String>,
    #[serde(default)]
    reason: Option<Reason>,
}

#[derive(Debug, serde::Deserialize)]
struct Receipt {
    issued_at: String,
    issuer: Issuer,
    currency: String,
    total: String,
}

#[derive(Debug, serde::Deserialize)]
struct Issuer {
    name: String,
    trade_name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Reason {
    message: String,
}

/// One line on what a file became.
fn said(upload: &Upload) -> String {
    let document = &upload.document;
    let receipt = document.receipt.as_ref().map(|receipt| {
        let day = receipt.issued_at.get(..10).unwrap_or(&receipt.issued_at);
        let shop = document
            .shop
            .as_deref()
            .or(receipt.issuer.trade_name.as_deref())
            .unwrap_or(&receipt.issuer.name);
        format!("{shop}, {} {} on {day}", trim_zeros(&receipt.total), receipt.currency)
    });
    let receipt = receipt.unwrap_or_default();
    match document.state.as_str() {
        // Arrived before: booked then, or now with what was said here.
        "booked" if upload.already => format!("in the books - {receipt}"),
        "booked" => format!("booked - {receipt}"),
        "waiting" => {
            let mut missing: Vec<&str> = Vec::new();
            if document.needs.iter().any(|need| need == "account") {
                missing.push("which account paid (--from)");
            }
            if document.needs.iter().any(|need| need == "category") {
                missing.push("what it was for (--for)");
            }
            let mut line = format!("waiting - {receipt}");
            if !missing.is_empty() {
                line.push_str("; say ");
                line.push_str(&missing.join(" and "));
            }
            if let Some(reason) = &document.reason {
                line.push_str("; ");
                line.push_str(&reason.message);
            }
            line
        }
        _ => format!(
            "not read into the books - {}",
            document.reason.as_ref().map_or("it is not a receipt", |reason| reason.message.as_str())
        ),
    }
}

/// How many files came to what.
#[derive(Debug, Default)]
struct Tally {
    booked: usize,
    waiting: usize,
    refused: usize,
}

impl Tally {
    fn count(&mut self, state: &str) {
        match state {
            "booked" => self.booked += 1,
            "waiting" => self.waiting += 1,
            _ => self.refused += 1,
        }
    }

    fn say(&self) -> String {
        format!("{} booked, {} waiting, {} not read", self.booked, self.waiting, self.refused)
    }
}

#[cfg(test)]
mod tests {
    use super::{Category, Upload, category_named, said};

    fn category(id: &str, parent: Option<&str>, name: &str) -> Category {
        Category {
            id: id.to_owned(),
            parent_id: parent.map(ToOwned::to_owned),
            name: name.to_owned(),
        }
    }

    #[test]
    fn a_category_is_named_as_a_typed_line_names_it() {
        let tree = [
            category("1", None, "Living"),
            category("2", Some("1"), "Food"),
            category("3", None, "Travel"),
            category("4", Some("3"), "Food"),
            category("5", None, "Еда вне дома"),
        ];
        // A name with spaces is one argument here, not three words.
        assert_eq!(category_named(&tree, "еда вне дома").expect("found"), "5");
        assert_eq!(category_named(&tree, "living/food").expect("found"), "2");
        assert_eq!(category_named(&tree, "Travel / Food").expect("found"), "4");
        // Two leaves share the name: which one is asked, never guessed.
        assert!(category_named(&tree, "food").is_err());
        assert!(category_named(&tree, "yachts").is_err());
    }

    fn upload(json: serde_json::Value) -> Upload {
        serde_json::from_value(json).expect("an upload")
    }

    #[test]
    fn each_file_is_said_as_what_it_became() {
        let receipt = serde_json::json!({
            "issued_at": "2026-10-18T16:12:43",
            "issuer": { "name": "SOL ALIMENTOS S.A.", "trade_name": "SUPERMERCADO SOL" },
            "currency": "PYG",
            "total": "32950"
        });
        let booked =
            upload(serde_json::json!({ "already": false, "document": { "state": "booked", "shop": "Supermercado Sol", "receipt": receipt, "needs": [] } }));
        assert_eq!(said(&booked), "booked - Supermercado Sol, 32950 PYG on 2026-10-18");

        let waiting = upload(serde_json::json!({ "already": false, "document": { "state": "waiting", "receipt": receipt, "needs": ["account", "category"] } }));
        let line = said(&waiting);
        assert!(line.contains("--from") && line.contains("--for"), "{line}");

        let refused = upload(serde_json::json!({ "already": false, "document": {
            "state": "refused", "needs": [],
            "reason": { "code": "document.not_a_receipt", "params": {}, "message": "this XML document is not an electronic document issued through SIFEN" }
        } }));
        assert!(said(&refused).ends_with("issued through SIFEN"));
    }
}
