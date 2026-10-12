//! Holds the web interface to a sentence for every error code the services
//! answer with.
//!
//! A code exists so a screen can say what went wrong in its reader's language;
//! a code the interface has no sentence for falls back to the server's English
//! and nobody notices until a Russian screen shows half a message. The list is
//! the one the services answer from (`Code::ALL`), not a scan of their source,
//! so a code cannot be answered without being listed here too.

use std::path::PathBuf;

use austeris_common::Code;

/// The interface's languages, as its locale files are named.
const LANGUAGES: [&str; 2] = ["en", "ru"];

fn locale(language: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../web/src/i18n/locales/{language}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()))
}

#[test]
fn every_code_a_service_answers_with_has_a_sentence_in_every_language() {
    for language in LANGUAGES {
        let strings = locale(language);
        let missing: Vec<&str> = Code::ALL
            .iter()
            .map(|code| code.as_str())
            .filter(|code| {
                // i18next reads the dots of a key as nesting.
                let found = code.split('.').fold(&strings["errors"], |at, part| &at[part]);
                found.as_str().is_none_or(|sentence| sentence.trim().is_empty())
            })
            .collect();
        assert!(missing.is_empty(), "{language}.json has no `errors.` sentence for {missing:?}");
    }
}

#[test]
fn the_interface_has_no_sentence_for_a_code_no_service_answers_with() {
    // A sentence left behind by a code that was renamed is a translation of
    // nothing, and the new name goes untranslated beside it.
    fn leaves(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                    leaves(child, &path, out);
                }
            }
            _ => out.push(prefix.to_owned()),
        }
    }
    let known: Vec<&str> = Code::ALL.iter().map(|code| code.as_str()).collect();
    for language in LANGUAGES {
        let mut found = Vec::new();
        leaves(&locale(language)["errors"], "", &mut found);
        let stray: Vec<&String> = found.iter().filter(|code| !known.contains(&code.as_str())).collect();
        assert!(stray.is_empty(), "{language}.json translates codes no service answers with: {stray:?}");
    }
}
