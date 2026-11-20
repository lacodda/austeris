//! How long a trash keeps what is put in it.

/// How many days something deleted - an entry, a file taken off one, a trade -
/// can still come back. Then it is erased for good, with its history. One
/// number for every module's trash: a person learns one promise, not one per
/// screen.
pub const KEEP_DAYS: i32 = 30;
