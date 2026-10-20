//! A document arriving, from wherever it arrived: read it, keep it while it
//! waits, and book it as soon as everything it needs is known.
//!
//! The one path an upload, the folder and `austeris import` share, so a
//! receipt means the same thing however it got here - and the same file or the
//! same receipt arriving twice is answered with what it already is.

use anyhow::Result;
use austeris_common::Code;
use austeris_ledger::attachment;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::book::{self, Outcome, Proof, Said};
use crate::ledger::Books;
use crate::model::{Document, Origin, Reason, State};
use crate::repository::{self, Arriving, Stored};
use crate::sifen::{self, Unread};

/// What handing a document over came to.
#[derive(Debug, Clone)]
pub struct Received {
    /// The document, as it now stands.
    pub document: Document,
    /// It had arrived before - this file, or this receipt as another file -
    /// and this is that document, booked now if what was said here was what
    /// it waited for.
    pub already: bool,
}

/// Takes a document in.
///
/// # Errors
///
/// Returns an error when this service's own database fails. Anything about
/// the document - unreadable, not a receipt, waiting for an account, a
/// ledger out of reach - is in the document returned.
pub async fn receive(pool: &PgPool, books: &impl Books, owner_id: Uuid, name: &str, content: &[u8], origin: Origin, said: Said) -> Result<Received> {
    let sha256 = Sha256::digest(content).to_vec();
    let arriving = Arriving {
        owner_id,
        name,
        content,
        sha256: &sha256,
        origin,
    };

    if let Some(known) = repository::by_sha256(pool, owner_id, &sha256).await? {
        match known.state() {
            State::Booked => {
                return Ok(Received {
                    document: known.document(),
                    already: true,
                });
            }
            State::Waiting => {
                let document = book_waiting(pool, books, owner_id, known, said).await?;
                return Ok(Received { document, already: true });
            }
            // Read again: a newer build may read what an older one could not.
            State::Refused => repository::forget_refused(pool, owner_id, known.id()).await?,
        }
    }

    let receipt = match readable(content) {
        Ok(receipt) => receipt,
        Err(reason) => {
            let document = repository::refuse(pool, &arriving, &reason).await?.document();
            return Ok(Received { document, already: false });
        }
    };

    if let Some(code) = &receipt.code
        && let Some(known) = repository::by_code(pool, owner_id, code).await?
    {
        let document = match known.state() {
            State::Waiting => book_waiting(pool, books, owner_id, known, said).await?,
            _ => known.document(),
        };
        return Ok(Received { document, already: true });
    }

    let Some(waiting) = repository::arrive(pool, &arriving, &receipt, &said).await? else {
        // The same file or receipt arrived at this very moment by another
        // way; that arrival is this document.
        let known = match repository::by_sha256(pool, owner_id, &sha256).await? {
            Some(known) => Some(known),
            None => match &receipt.code {
                Some(code) => repository::by_code(pool, owner_id, code).await?,
                None => None,
            },
        };
        let known = known.ok_or_else(|| anyhow::anyhow!("a document collided with one that is not there"))?;
        return Ok(Received {
            document: known.document(),
            already: true,
        });
    };
    let document = book_waiting(pool, books, owner_id, waiting, Said::default()).await?;
    Ok(Received { document, already: false })
}

/// Books a waiting document with what was said about it now, on top of what
/// was said before; the document as it then stands, `None` when there is no
/// such document of this person's.
///
/// # Errors
///
/// Returns an error when this service's own database fails.
pub async fn book_again(pool: &PgPool, books: &impl Books, owner_id: Uuid, id: Uuid, said: Said) -> Result<Option<Document>> {
    let Some(known) = repository::document(pool, owner_id, id).await? else {
        return Ok(None);
    };
    if known.state() != State::Waiting {
        return Ok(Some(known.document()));
    }
    Ok(Some(book_waiting(pool, books, owner_id, known, said).await?))
}

/// Tries to book a waiting document.
async fn book_waiting(pool: &PgPool, books: &impl Books, owner_id: Uuid, known: Stored, said: Said) -> Result<Document> {
    let before = known.said();
    // What is said now wins; what was said before stays for what is not. An
    // account chosen by hand replaces the folder's name for one.
    let said = Said {
        account: said.account.or(before.account),
        account_name: if said.account.is_some() {
            None
        } else {
            said.account_name.or(before.account_name)
        },
        category: said.category.or(before.category),
    };

    let id = known.id();
    let (Some(receipt), Some(content)) = (known.receipt().cloned(), repository::content(pool, owner_id, id).await?) else {
        // Booked by another attempt between reading it and getting here.
        return Ok(repository::document(pool, owner_id, id).await?.unwrap_or(known).document());
    };
    let sha256 = hex(&Sha256::digest(&content));
    let proof = Proof {
        name: known.name(),
        content: &content,
        sha256: &sha256,
    };

    let stored = match book::book(books, owner_id, &receipt, proof, &said).await {
        Ok(Outcome::Booked { entry_id }) => repository::booked(pool, owner_id, id, entry_id).await?,
        Ok(Outcome::Waiting { needs, reason }) => repository::waits(pool, owner_id, id, &said, &needs, reason.as_ref()).await?,
        Err(status) => {
            // The ledger could not be asked. Nothing is lost: the document
            // waits, and booking it again asks again.
            tracing::warn!(%status, document = %id, "the ledger did not answer while booking a document");
            let reason = Reason::new(
                Code::DocumentLedgerUnavailable,
                json!({}),
                "the books could not be reached; try booking it again in a moment",
            );
            repository::waits(pool, owner_id, id, &said, &[], Some(&reason)).await?
        }
    };
    Ok(stored.unwrap_or(known).document())
}

/// A receipt, or why the bytes are not one that can be booked.
fn readable(content: &[u8]) -> Result<sifen::Receipt, Reason> {
    if content.len() > attachment::MAX_BYTES {
        let limit = attachment::MAX_BYTES / (1024 * 1024);
        return Err(Reason::new(
            Code::DocumentTooLarge,
            json!({ "limit_mb": limit }),
            format!("a file can be {limit} MB at most"),
        ));
    }
    match attachment::media_type(content) {
        Some("application/xml") => {}
        other => {
            return Err(Reason::new(
                Code::DocumentUnsupported,
                other.map_or_else(|| json!({}), |media_type| json!({ "media_type": media_type })),
                "only a receipt's XML is read into the books; a photo or a PDF is attached to its entry instead",
            ));
        }
    }
    sifen::read(content).map_err(|unread| {
        let message = unread.to_string();
        match unread {
            Unread::NotXml(why) => Reason::new(Code::DocumentUnreadable, json!({ "why": why }), message),
            Unread::NotSifen => Reason::new(Code::DocumentNotAReceipt, json!({}), message),
            Unread::Several(count) => Reason::new(Code::DocumentSeveral, json!({ "count": count }), message),
            Unread::NotAPayment(kind) => Reason::new(Code::DocumentNotAPayment, json!({ "kind": kind }), message),
            Unread::Missing(what) | Unread::Malformed(what, _) => Reason::new(Code::DocumentIncomplete, json!({ "what": what }), message),
        }
    })
}

/// Bytes as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}
