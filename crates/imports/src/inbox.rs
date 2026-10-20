//! The folder receipts are dropped in.
//!
//! A person saves a receipt from an e-mail into a folder - on the machine
//! austeris runs on, or one kept in step with it - and it is in the books
//! half a minute later, with nothing opened and nothing typed. The folder is
//! `AUSTERIS_INBOX`, laid out by whose and which account:
//!
//! ```text
//! inbox/
//!   owner@example.com/          a person's own folder, named by the address they sign in with
//!     factura.xml               paid from wherever the receipt says, or asked
//!     Visa Debit/               an account of theirs, by name
//!       factura.xml             paid from that account
//!     done/                     what was taken in: booked, or waiting on the screen
//!     refused/                  what cannot be read into the books
//! ```
//!
//! A file is taken once it has stopped changing for a few seconds, so one
//! still being copied in is not read half-written. Taking a file in is the
//! same as uploading it ([`crate::receive`]): the same file or receipt again is
//! answered with what it already is, so a folder synchronised back in full is
//! harmless.

use std::collections::HashSet;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use austeris_ledger::attachment::MAX_BYTES;
use sqlx::PgPool;

use crate::book::Said;
use crate::ledger::{Books, People};
use crate::model::{Origin, State};
use crate::receive;

/// Where what was taken in goes.
pub const DONE: &str = "done";
/// Where what cannot be read into the books goes.
pub const REFUSED: &str = "refused";

/// How often the folder is looked at.
const EVERY: Duration = Duration::from_secs(30);

/// How long a file has to have stood still before it is read: longer than a
/// copy pauses between writes.
const SETTLED: Duration = Duration::from_secs(5);

/// What one look at the folder did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Swept {
    /// Files taken in: booked, waiting, or already in.
    pub taken: usize,
    /// Files that cannot be read into the books.
    pub refused: usize,
}

/// Looks at the folder every half minute, for as long as the service runs.
pub async fn every_half_minute(pool: PgPool, books: impl Books, people: impl People, root: PathBuf) {
    let mut ticker = tokio::time::interval(EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut warned = HashSet::new();
    loop {
        ticker.tick().await;
        match sweep(&pool, &books, &people, &root, SETTLED, &mut warned).await {
            Ok(swept) if swept != Swept::default() => {
                tracing::info!(taken = swept.taken, refused = swept.refused, "took documents in from the folder");
            }
            Ok(_) => {}
            Err(error) => tracing::error!(error = format!("{error:#}"), "looking at the folder failed"),
        }
    }
}

/// Takes in every settled file in the folder, once.
///
/// `warned` holds what has been said about already - a folder named after
/// nobody, a file that cannot be moved - so a look every half minute does not
/// say it every half minute.
///
/// # Errors
///
/// Returns an error when the folder cannot be read, or this service's
/// database fails. A file that cannot be moved after it is taken in is said
/// once and left alone after that; were it read again - after a restart - it
/// is answered with what it already is.
pub async fn sweep<S: BuildHasher>(
    pool: &PgPool,
    books: &impl Books,
    people: &impl People,
    root: &Path,
    settled: Duration,
    warned: &mut HashSet<PathBuf, S>,
) -> Result<Swept> {
    let mut swept = Swept::default();
    if !root.is_dir() {
        if warned.insert(root.to_owned()) {
            tracing::warn!(folder = %root.display(), "the folder documents are taken in from is not there");
        }
        return Ok(swept);
    }

    for person in subfolders(root)? {
        let Some(email) = person.file_name().and_then(|name| name.to_str()).map(ToOwned::to_owned) else {
            continue;
        };
        let owner = match people.person(&email).await {
            Ok(Some(owner)) => owner,
            Ok(None) => {
                if warned.insert(person.clone()) {
                    tracing::warn!(folder = %person.display(), "nobody signs in as this folder's name; its files are left where they are");
                }
                continue;
            }
            Err(status) => {
                tracing::warn!(%status, "identity did not answer; the folder is looked at again shortly");
                return Ok(swept);
            }
        };

        // The person's own files, then each account folder's.
        let mut places = vec![(person.clone(), None)];
        for account in subfolders(&person)? {
            let name = account.file_name().and_then(|name| name.to_str()).map(ToOwned::to_owned);
            if let Some(name) = name.filter(|name| name != DONE && name != REFUSED) {
                places.push((account, Some(name)));
            }
        }

        for (folder, account_name) in places {
            for file in settled_files(&folder, settled)? {
                // Already said about: taken in and left where it could not be
                // moved from, or not readable at all. Reading it every half
                // minute would only say the same again.
                if warned.contains(&file) {
                    continue;
                }
                let name = file.file_name().and_then(|name| name.to_str()).unwrap_or("document").to_owned();
                let size = std::fs::metadata(&file).map_or(0, |meta| meta.len());
                if size > u64::try_from(MAX_BYTES).unwrap_or(u64::MAX) {
                    // Not read at all: a file this large is not a receipt, and
                    // reading it would cost a small machine its memory.
                    tracing::warn!(file = %file.display(), size, "too large to be a receipt; moved aside unread");
                    move_into(&file, &person.join(REFUSED), warned);
                    swept.refused += 1;
                    continue;
                }
                let content = match std::fs::read(&file) {
                    Ok(content) => content,
                    Err(error) => {
                        if warned.insert(file.clone()) {
                            tracing::warn!(file = %file.display(), %error, "a file in the folder cannot be read");
                        }
                        continue;
                    }
                };
                let said = Said {
                    account_name: account_name.clone(),
                    ..Said::default()
                };
                let received = receive::receive(pool, books, owner, &name, &content, Origin::Folder, said).await?;
                if received.document.state == State::Refused {
                    move_into(&file, &person.join(REFUSED), warned);
                    swept.refused += 1;
                } else {
                    move_into(&file, &person.join(DONE), warned);
                    swept.taken += 1;
                }
            }
        }
    }
    Ok(swept)
}

/// A folder's own folders, by name, without the hidden ones.
fn subfolders(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(folder)
        .with_context(|| format!("reading {}", folder.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| !is_hidden(path))
        .collect();
    out.sort();
    Ok(out)
}

/// A folder's files that have stood still for `settled`, oldest first.
///
/// Hidden files and the names a download or a copy uses while it is still
/// writing are left alone: they are not documents yet.
fn settled_files(folder: &Path, settled: Duration) -> Result<Vec<PathBuf>> {
    let now = SystemTime::now();
    let mut out: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(folder)
        .with_context(|| format!("reading {}", folder.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .filter(|(modified, path)| !is_hidden(path) && !is_partial(path) && now.duration_since(*modified).is_ok_and(|age| age >= settled))
        .collect();
    out.sort();
    Ok(out.into_iter().map(|(_, path)| path).collect())
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with('.'))
}

fn is_partial(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_lowercase();
    [".part", ".partial", ".tmp", ".crdownload", ".download", "~"]
        .iter()
        .any(|ending| name.ends_with(ending))
}

/// Moves a file into a folder, beside any file of the same name rather than
/// over it.
fn move_into<S: BuildHasher>(file: &Path, folder: &Path, warned: &mut HashSet<PathBuf, S>) {
    let moved = std::fs::create_dir_all(folder).and_then(|()| {
        let target = free_name(folder, file);
        std::fs::rename(file, &target)
    });
    if let Err(error) = moved
        && warned.insert(file.to_owned())
    {
        tracing::warn!(file = %file.display(), %error, "a file taken in cannot be moved out of the folder; it is left, and answered as already in");
    }
}

/// A name in `folder` no file has yet: the file's own, or with ` (2)`, ` (3)`
/// before its extension.
fn free_name(folder: &Path, file: &Path) -> PathBuf {
    let name = file.file_name().map_or_else(|| "document".into(), std::ffi::OsStr::to_os_string);
    let first = folder.join(&name);
    if !first.exists() {
        return first;
    }
    let stem = file.file_stem().and_then(|stem| stem.to_str()).unwrap_or("document");
    let extension = file.extension().and_then(|extension| extension.to_str());
    // Bounded: a folder with a thousand files of one name has something
    // else wrong with it, and the next look tries again.
    (2..1000)
        .map(|n| match extension {
            Some(extension) => folder.join(format!("{stem} ({n}).{extension}")),
            None => folder.join(format!("{stem} ({n})")),
        })
        .find(|candidate| !candidate.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{free_name, is_partial};

    #[test]
    fn a_file_still_being_written_is_not_a_document_yet() {
        assert!(is_partial(Path::new("factura.xml.part")));
        assert!(is_partial(Path::new("factura.xml.crdownload")));
        assert!(is_partial(Path::new("factura.xml~")));
        assert!(!is_partial(Path::new("factura.xml")));
    }

    #[test]
    fn a_moved_file_never_lands_on_another() {
        let folder = std::env::temp_dir().join(format!("austeris-free-name-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        std::fs::write(folder.join("factura.xml"), b"one").expect("a file");
        std::fs::write(folder.join("factura (2).xml"), b"two").expect("a file");
        assert_eq!(free_name(&folder, Path::new("in/factura.xml")), folder.join("factura (3).xml"));
        assert_eq!(free_name(&folder, Path::new("in/other.xml")), folder.join("other.xml"));
        std::fs::remove_dir_all(&folder).expect("cleaning up");
    }
}
