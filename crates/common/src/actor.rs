//! Who is writing to a person's books.
//!
//! A service writes on someone's behalf: the person the gateway vouched for,
//! or a module posting what happened in its own terms. What remembers a
//! change - the ledger's history of an entry - is kept by the database, so the
//! database has to be told who it is. [`Actor::begin`] opens a transaction
//! that says so, and every write a service makes to remembered rows goes
//! through one.
//!
//! The setting is local to the transaction: a pooled connection handed to the
//! next request carries nothing over from this one.

use anyhow::{Context, Result};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::Caller;

/// The setting the database reads who is writing from.
pub const SETTING: &str = "austeris.actor";

/// On whose behalf a write is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    /// A person, through the gateway.
    Person(Uuid),
    /// A module posting for a person, by the name it signs with - when it
    /// gave one.
    Module(Option<String>),
}

impl From<Caller> for Actor {
    fn from(caller: Caller) -> Self {
        Self::Person(caller.id())
    }
}

impl Actor {
    /// A module, by the name it signs with; an empty name is no name.
    #[must_use]
    pub fn module(name: Option<&str>) -> Self {
        Self::Module(name.map(str::trim).filter(|name| !name.is_empty()).map(ToOwned::to_owned))
    }

    /// The value of [`SETTING`]: `person:<id>` or `module:<name>`.
    #[must_use]
    pub fn setting(&self) -> String {
        match self {
            Self::Person(id) => format!("person:{id}"),
            Self::Module(name) => format!("module:{}", name.as_deref().unwrap_or_default()),
        }
    }

    /// Opens a transaction in which every write is this actor's.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot be opened or the setting
    /// cannot be made.
    pub async fn begin<'a>(&self, pool: &'a PgPool) -> Result<Transaction<'a, Postgres>> {
        let mut transaction = pool.begin().await.context("opening a transaction")?;
        self.bind(&mut transaction).await?;
        Ok(transaction)
    }

    /// Says, for the rest of an open transaction, that its writes are this
    /// actor's.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting cannot be made.
    pub async fn bind(&self, transaction: &mut Transaction<'_, Postgres>) -> Result<()> {
        // `true`: local to the transaction, so it ends with it.
        sqlx::query("SELECT set_config($1, $2, true)")
            .bind(SETTING)
            .bind(self.setting())
            .execute(&mut **transaction)
            .await
            .context("saying who is writing")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::Actor;

    #[test]
    fn a_person_and_a_module_are_told_apart_in_the_setting() {
        let id = Uuid::nil();
        assert_eq!(Actor::Person(id).setting(), format!("person:{id}"));
        assert_eq!(Actor::module(Some("recurring")).setting(), "module:recurring");
    }

    #[test]
    fn a_module_without_a_name_is_still_a_module() {
        // An unnamed module is not "nobody": the database records it as a
        // module that did not sign, not as a write from outside austeris.
        assert_eq!(Actor::module(None), Actor::Module(None));
        assert_eq!(Actor::module(Some("  ")), Actor::Module(None));
        assert_eq!(Actor::module(None).setting(), "module:");
    }
}
