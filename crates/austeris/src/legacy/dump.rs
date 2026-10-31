//! Reading a dump of the 2025 tracker's database.
//!
//! A plain `pg_dump` - the SQL script it writes by default - carries each
//! table's rows as a `COPY ... FROM stdin;` block of tab-separated lines. That
//! is all this reads: the rows, by column name, of every table in the file.
//! Nothing in the file is executed, and no database is needed to read it.
//!
//! The other shapes a dump comes in are said rather than guessed at: an
//! archive (`pg_dump -Fc`, `-Ft`, `-Fd`, and pgAdmin's default) is turned into
//! a plain script by `pg_restore --file`, and a script made with `--inserts`
//! by making it again without. What Windows does to a file on the way - a
//! UTF-16 redirect in an old PowerShell, a byte-order mark, CRLF line ends, a
//! gzip around it - is undone here, because the person cannot see it.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// One table of the dump: its columns as the `COPY` named them, and its rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// The column names, in the order the rows carry them.
    pub columns: Vec<String>,
    /// Each row's values; `None` is SQL's null.
    pub rows: Vec<Vec<Option<String>>>,
}

impl Table {
    /// Where a column is in each row.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column == name)
    }
}

/// Every table a dump carries rows for, by schema and name.
#[derive(Debug, Clone, Default)]
pub struct Dump {
    tables: BTreeMap<(String, String), Table>,
    /// Tables whose rows the file inserts one statement at a time.
    inserted: Vec<String>,
}

impl Dump {
    /// Reads a dump from a file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, is an archive rather
    /// than a script, or is not text.
    pub fn read(path: &Path) -> Result<Self> {
        if path.is_dir() {
            if path.join("toc.dat").exists() {
                bail!(archive(path, "a directory-format archive"));
            }
            bail!("{} is a folder; name the dump file itself", path.display());
        }
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut inflated = Vec::new();
            flate2::read::GzDecoder::new(bytes.as_slice())
                .read_to_end(&mut inflated)
                .with_context(|| format!("unpacking {}", path.display()))?;
            inflated
        } else {
            bytes
        };
        if bytes.starts_with(b"PGDMP") {
            bail!(archive(path, "a custom-format archive"));
        }
        if bytes.get(257..262) == Some(b"ustar".as_slice()) {
            bail!(archive(path, "a tar-format archive"));
        }
        let text = decode(&bytes).with_context(|| format!("{} is not a dump pg_dump wrote", path.display()))?;
        Self::parse(&text)
    }

    /// Reads a dump from its text.
    ///
    /// # Errors
    ///
    /// Returns an error when a `COPY` block is malformed or never ends.
    pub fn parse(text: &str) -> Result<Self> {
        let mut dump = Self::default();
        let mut lines = text.lines().enumerate();
        while let Some((number, line)) = lines.next() {
            if let Some(rest) = line.strip_prefix("INSERT INTO ") {
                if let Ok((name, _)) = identifier_path(rest) {
                    dump.inserted.push(name.1);
                }
                continue;
            }
            let Some(rest) = line.strip_prefix("COPY ") else { continue };
            let Some(rest) = rest.strip_suffix(" FROM stdin;") else { continue };
            let ((schema, table), rest) = identifier_path(rest).with_context(|| format!("line {}: a COPY naming no table", number + 1))?;
            let columns = column_list(rest).with_context(|| format!("line {}: the COPY of {table} names no columns", number + 1))?;

            let mut rows = Vec::new();
            let mut ended = false;
            for (row_number, row) in lines.by_ref() {
                if row == "\\." {
                    ended = true;
                    break;
                }
                let values: Vec<Option<String>> = row
                    .split('\t')
                    .map(field)
                    .collect::<Result<_>>()
                    .with_context(|| format!("line {}: a row of {table}", row_number + 1))?;
                if values.len() != columns.len() {
                    bail!(
                        "line {}: a row of {table} has {} values for {} columns",
                        row_number + 1,
                        values.len(),
                        columns.len()
                    );
                }
                rows.push(values);
            }
            if !ended {
                bail!("the rows of {table} never end; the dump is cut short");
            }
            dump.tables.insert((schema, table), Table { columns, rows });
        }
        Ok(dump)
    }

    /// A table of the tracker's own schema, `public`.
    #[must_use]
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.get(&("public".to_owned(), name.to_owned()))
    }

    /// Whether the file inserts a table's rows statement by statement, as
    /// `pg_dump --inserts` writes them, rather than copying them.
    #[must_use]
    pub fn inserts(&self, name: &str) -> bool {
        self.inserted.iter().any(|table| table == name)
    }

    /// The schemas other than `public` that carry rows.
    #[must_use]
    pub fn other_schemas(&self) -> Vec<String> {
        let mut schemas: Vec<String> = self
            .tables
            .keys()
            .filter(|(schema, _)| schema != "public")
            .map(|(schema, _)| schema.clone())
            .collect();
        schemas.dedup();
        schemas
    }
}

/// What to say about an archive: what it is, and the one command that makes
/// it a script.
fn archive(path: &Path, what: &str) -> String {
    format!(
        "{} is {what} of pg_dump, and this reads the plain SQL script. Make one from it - no database is needed: \
         `pg_restore --file legacy.sql \"{}\"`, then import legacy.sql",
        path.display(),
        path.display()
    )
}

/// The text of a file, in whichever of the encodings Windows leaves a dump in.
fn decode(bytes: &[u8]) -> Result<String> {
    // PowerShell 5.1 redirects in UTF-16 with a byte-order mark.
    let utf16 = |rest: &[u8], little: bool| -> Result<String> {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| if little { u16::from_le_bytes(*pair) } else { u16::from_be_bytes(*pair) })
            .collect();
        String::from_utf16(&units).context("UTF-16 that does not decode")
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).context("not UTF-8 text"),
        _ => String::from_utf8(bytes.to_vec()).context("not UTF-8 text"),
    }
}

/// An identifier as `pg_dump` writes it - bare, or in double quotes with
/// quotes doubled - and what follows it.
fn identifier(text: &str) -> Result<(String, &str)> {
    if let Some(mut rest) = text.strip_prefix('"') {
        let mut name = String::new();
        loop {
            let Some(at) = rest.find('"') else { bail!("an unclosed quoted name") };
            name.push_str(&rest[..at]);
            rest = &rest[at + 1..];
            if let Some(after) = rest.strip_prefix('"') {
                name.push('"');
                rest = after;
            } else {
                return Ok((name, rest));
            }
        }
    }
    let end = text.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).unwrap_or(text.len());
    if end == 0 {
        bail!("no name");
    }
    Ok((text[..end].to_lowercase(), &text[end..]))
}

/// A table's name, with its schema - `public` when the dump does not say -
/// and what follows it.
fn identifier_path(text: &str) -> Result<((String, String), &str)> {
    let (first, rest) = identifier(text)?;
    match rest.strip_prefix('.') {
        Some(after) => {
            let (second, rest) = identifier(after)?;
            Ok(((first, second), rest))
        }
        None => Ok((("public".to_owned(), first), rest)),
    }
}

/// The column list of a `COPY`: ` (a, b, "c")`.
fn column_list(text: &str) -> Result<Vec<String>> {
    let Some(mut rest) = text.trim_start().strip_prefix('(') else {
        bail!("no column list");
    };
    let mut columns = Vec::new();
    loop {
        let (name, after) = identifier(rest.trim_start())?;
        columns.push(name);
        let after = after.trim_start();
        if let Some(more) = after.strip_prefix(',') {
            rest = more;
        } else if after.strip_prefix(')').is_some_and(|tail| tail.trim().is_empty()) {
            return Ok(columns);
        } else {
            bail!("a column list that does not close");
        }
    }
}

/// One value of a `COPY` row, unescaped; `\N` is null.
fn field(raw: &str) -> Result<Option<String>> {
    if raw == "\\N" {
        return Ok(None);
    }
    if !raw.contains('\\') {
        return Ok(Some(raw.to_owned()));
    }
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte != b'\\' {
            out.push(byte);
            at += 1;
            continue;
        }
        let Some(&next) = bytes.get(at + 1) else {
            bail!("a value ends in a lone backslash");
        };
        at += 2;
        match next {
            b'b' => out.push(0x08),
            b'f' => out.push(0x0C),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0B),
            b'0'..=b'7' => {
                // Up to three octal digits, the first already read.
                let mut value = u32::from(next - b'0');
                for _ in 0..2 {
                    match bytes.get(at) {
                        Some(digit @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(digit - b'0');
                            at += 1;
                        }
                        _ => break,
                    }
                }
                out.push(u8::try_from(value & 0xFF).unwrap_or_default());
            }
            b'x' => {
                // Up to two hex digits.
                let mut value = 0u32;
                let mut digits = 0;
                while digits < 2 {
                    match bytes.get(at).and_then(|digit| char::from(*digit).to_digit(16)) {
                        Some(digit) => {
                            value = value * 16 + digit;
                            at += 1;
                            digits += 1;
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    out.push(b'x');
                } else {
                    out.push(u8::try_from(value).unwrap_or_default());
                }
            }
            other => out.push(other),
        }
    }
    String::from_utf8(out).map(Some).context("a value that is not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::{Dump, field};

    const DUMP: &str = "--\n-- PostgreSQL database dump\n--\n\nSET client_encoding = 'UTF8';\n\
        COPY public.wallets (id, name, type, address, created_at) FROM stdin;\n\
        1\tBinance\texchange\t\\N\t2025-03-01 10:00:00.123456\n\
        2\tTab\\there\\\\back\\nline\thardware\t\\N\t2025-03-01 10:00:00\n\
        \\.\n\
        COPY public.asset_prices (id, asset_id, price_usd, \"timestamp\") FROM stdin;\n\
        \\.\n\
        COPY assets.assets (id, symbol) FROM stdin;\n1\tBTC\n\\.\n";

    #[test]
    fn rows_are_read_by_column_name_with_their_escapes_undone() {
        let dump = Dump::parse(DUMP).expect("a dump");
        let wallets = dump.table("wallets").expect("the wallets");
        assert_eq!(wallets.column("type"), Some(2));
        assert_eq!(wallets.rows.len(), 2);
        assert_eq!(wallets.rows[0][1].as_deref(), Some("Binance"));
        assert_eq!(wallets.rows[0][3], None, "\\N is null");
        assert_eq!(wallets.rows[1][1].as_deref(), Some("Tab\there\\back\nline"));
        // A keyword column is quoted by pg_dump; its name is still its name.
        assert_eq!(dump.table("asset_prices").expect("prices").column("timestamp"), Some(3));
        assert_eq!(dump.other_schemas(), ["assets"]);
        assert!(dump.table("transactions").is_none());
    }

    #[test]
    fn crlf_line_ends_are_read_as_line_ends() {
        let dump = Dump::parse(&DUMP.replace('\n', "\r\n")).expect("a dump");
        assert_eq!(
            dump.table("wallets").expect("the wallets").rows[0][4].as_deref(),
            Some("2025-03-01 10:00:00.123456")
        );
    }

    #[test]
    fn a_dump_cut_short_or_misshapen_is_said_to_be() {
        let cut = "COPY public.wallets (id, name) FROM stdin;\n1\tBinance\n";
        assert!(format!("{:#}", Dump::parse(cut).expect_err("cut")).contains("never end"));
        let wide = "COPY public.wallets (id, name) FROM stdin;\n1\tBinance\textra\n\\.\n";
        assert!(format!("{:#}", Dump::parse(wide).expect_err("misshapen")).contains("3 values for 2 columns"));
    }

    #[test]
    fn a_dump_of_inserts_is_told_apart() {
        let dump = Dump::parse("INSERT INTO public.transactions VALUES (1, 1, 1, 0.5, 60000, NULL, 'BUY', NULL, '2025-03-01');\n").expect("a dump");
        assert!(dump.inserts("transactions"));
        assert!(dump.table("transactions").is_none());
    }

    #[test]
    fn octal_and_hex_escapes_are_bytes() {
        assert_eq!(field("caf\\303\\251").expect("a value").as_deref(), Some("café"));
        assert_eq!(field("\\x41\\x").expect("a value").as_deref(), Some("Ax"));
        assert!(field("dangling\\").is_err());
    }
}
