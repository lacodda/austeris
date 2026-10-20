//! What this service stores and answers with.

use austeris_common::Code;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sifen::Receipt;

/// A document handed to austeris to be read into the books.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Document {
    /// Stable identifier.
    pub id: Uuid,
    /// The file's name, as it arrived.
    pub name: String,
    /// How large it was, in bytes.
    pub size: i64,
    /// How it arrived.
    pub origin: Origin,
    /// Where it stands.
    pub state: State,
    /// What it was read as, when it could be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Receipt>,
    /// The name its shop is filed under in the books - the one it trades
    /// under, as a person writes it - when it was read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shop: Option<String>,
    /// The account the person said it was paid from, while it waits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Uuid>,
    /// What the person said it was for, while it waits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<Uuid>,
    /// What it waits for the person to say.
    pub needs: Vec<Needed>,
    /// Why it waits or was refused, when there is more to say than `needs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// The entry it became, once it is in the books.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<Uuid>,
    /// When it arrived.
    pub received_at: DateTime<Utc>,
    /// When it went into the books.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub booked_at: Option<DateTime<Utc>>,
}

/// How a document arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "document_origin", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Uploaded: from the web interface or `austeris import`.
    Upload,
    /// Dropped in the installation's folder.
    Folder,
}

/// Where a document stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "document_state", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Read, and not in the books yet: `needs` says what is missing, or
    /// `reason` why it could not be booked.
    Waiting,
    /// In the books, as `entry_id`.
    Booked,
    /// Not something that can be read into the books; `reason` says why.
    Refused,
}

/// What a waiting document needs the person to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Needed {
    /// Which account it was paid from.
    Account,
    /// What it was for.
    Category,
}

impl Needed {
    /// As it is stored.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Category => "category",
        }
    }

    /// As it was stored; `None` for a word this build does not know.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        match stored {
            "account" => Some(Self::Account),
            "category" => Some(Self::Category),
            _ => None,
        }
    }
}

/// Why a document waits or was refused: a code a screen says in its own
/// language, and the facts its sentence is made from - the same codes an
/// error answers with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Reason {
    /// The code: `document.not_a_receipt`.
    pub code: String,
    /// The facts, by name.
    #[schema(value_type = Object)]
    pub params: serde_json::Value,
    /// The sentence, in English, for a terminal and the logs.
    pub message: String,
}

impl Reason {
    /// A reason by its code.
    #[must_use]
    pub fn new(code: Code, params: serde_json::Value, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().to_owned(),
            params,
            message: message.into(),
        }
    }
}
