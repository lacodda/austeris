//! Guards the facts that must agree before a version is published.
//!
//! austeris ships to GitHub and, eventually, to crates.io and GHCR, and each
//! renders its own copy of the README. Drift is only visible after publishing,
//! when it is too late to take back, so these checks run in CI instead.

use std::fs;
use std::path::{Path, PathBuf};

/// The workspace root, two levels up from this crate.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: impl AsRef<Path>) -> String {
    let path = repo_root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Reads a top-level `key = "value"` from one section of a manifest.
///
/// Deliberately naive: it stops at the next section, which is all these checks
/// need, and avoids a TOML parser as a dev-dependency.
fn manifest_field(manifest: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if inside {
                break;
            }
            inside = line == section;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else { continue };
        // Exact match, so `rust-version` cannot answer a lookup for `version`.
        if name.trim() != key {
            continue;
        }
        return Some(value.trim().trim_matches('"').to_string());
    }
    None
}

/// The version the workspace is releasing.
fn workspace_version() -> String {
    manifest_field(&read("Cargo.toml"), "[workspace.package]", "version").expect("[workspace.package] has no version")
}

/// Every crate in the workspace, by its directory.
fn member_manifests() -> Vec<(String, String)> {
    let workspace = read("Cargo.toml");
    let members = workspace
        .lines()
        .find(|l| l.trim_start().starts_with("members"))
        .expect("the workspace manifest has no members list");

    members
        .split('"')
        .filter(|part| part.starts_with("crates/"))
        .map(|dir| (dir.to_string(), read(format!("{dir}/Cargo.toml"))))
        .collect()
}

#[test]
fn every_crate_ships_the_workspace_version() {
    // A crate that pins its own version drifts silently: the tag says one
    // thing, the published crate another, and only a stranger's `cargo add`
    // finds out.
    let expected = workspace_version();
    let members = member_manifests();
    assert!(!members.is_empty(), "no workspace members were found; the parser above is wrong");

    for (dir, manifest) in members {
        // Members inherit through `version.workspace = true`, which this naive
        // parser sees as the key `version.workspace`; an explicit `version` is
        // the case worth catching.
        let declared = manifest_field(&manifest, "[package]", "version")
            .or_else(|| manifest_field(&manifest, "[package]", "version.workspace").map(|_| expected.clone()))
            .unwrap_or_else(|| panic!("{dir} declares no version at all"));
        assert!(declared == expected, "{dir} declares version {declared}, the workspace is at {expected}");
    }
}

#[test]
fn readme_links_resolve_off_github() {
    // The same file is rendered on crates.io, where a relative path has no
    // repository to resolve against: the banner turns into a broken image and
    // the links 404.
    let readme = read("README.md");

    for (line_no, line) in readme.lines().enumerate() {
        for (marker, kind) in [("src=\"", "image"), ("](", "link")] {
            let mut rest = line;
            while let Some(at) = rest.find(marker) {
                let target = &rest[at + marker.len()..];
                let end = if marker == "](" { ')' } else { '"' };
                let target = &target[..target.find(end).unwrap_or(target.len())];

                let relative = !target.starts_with("http") && !target.starts_with('#') && !target.is_empty();
                assert!(
                    !relative,
                    "README line {}: relative {kind} `{target}` breaks on crates.io; use an absolute URL",
                    line_no + 1
                );

                rest = &rest[at + marker.len()..];
            }
        }
    }
}

#[test]
fn readme_is_not_duplicated() {
    // One README for every storefront. A second copy is where descriptions
    // start to drift; the docs and web milestones must reuse the root file
    // rather than fork it.
    for candidate in ["docs/README.md", "web/README.md", "crates/austeris/README.md", "crates/common/README.md"] {
        assert!(
            !repo_root().join(candidate).exists(),
            "{candidate} exists; it will drift from the root README, which is the single source"
        );
    }
}

#[test]
fn the_readme_documents_every_environment_variable() {
    // The configuration table is the only place an operator learns these
    // exist. A variable read by the code and missing from the table is
    // invisible until someone reads the source, which is not what a
    // self-hosted product can ask of them.
    //
    // The whole workspace is searched, not one file: the settings live wherever
    // they are used - the first-user address in the binary, the cookie flag in
    // identity, the pool size in common - and a gate that only reads `config.rs`
    // is blind to every one added anywhere else.
    let readme = read("README.md");
    let mut checked = 0;

    for source in rust_sources(&repo_root().join("crates")) {
        let text = fs::read_to_string(&source).expect("reading a source file");
        for name in variables_in(&text) {
            assert!(
                readme.contains(&name),
                "{name} is read by {} but missing from the README's configuration table",
                source.display()
            );
            checked += 1;
        }
    }

    assert!(
        checked > 0,
        "no AUSTERIS_ variable was found anywhere; the search is looking in the wrong place"
    );
}

/// Every `AUSTERIS_*` name a source file names, whole or as a suffix format.
fn variables_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();

    // Literal names: `"AUSTERIS_BIND"`, `format!("AUSTERIS_{}_ADDR", ...)`.
    let mut rest = text;
    while let Some(at) = rest.find("AUSTERIS_") {
        let tail = &rest[at..];
        let name: String = tail.chars().take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_').collect();
        // A name built by interpolation ends at the `{`; what survives is the
        // stem, and the README must at least carry that.
        if name.len() > "AUSTERIS_".len() {
            found.push(name);
        }
        rest = &tail["AUSTERIS_".len()..];
    }

    // Names assembled from a bare suffix, as `config.rs` does through its own
    // prefixing helpers.
    for call in ["optional(\"", "parsed(\""] {
        let mut rest = text;
        while let Some(at) = rest.find(call) {
            let tail = &rest[at + call.len()..];
            if let Some((name, _)) = tail.split_once('"')
                && !name.is_empty()
                && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            {
                found.push(format!("AUSTERIS_{name}"));
            }
            rest = tail;
        }
    }

    found.sort_unstable();
    found.dedup();
    found
}

/// Every `.rs` file under a directory.
fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![root.to_path_buf()];

    while let Some(dir) = queue.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }

    found
}

#[test]
fn the_changelog_covers_the_version_being_shipped() {
    // The tag drives an irreversible publish, and the release notes are cut
    // from the changelog. A manifest bumped without a changelog entry ships a
    // version nobody can read the changes of.
    let version = workspace_version();
    let changelog = read("CHANGELOG.md");
    let heading = format!("## [{version}]");

    assert!(
        changelog.contains(&heading),
        "CHANGELOG.md has no `{heading}` section; run `git-cliff --tag v{version}` before tagging"
    );
}

#[test]
fn the_adr_index_has_no_gaps() {
    // The ADRs are the product's memory and the README links them by number.
    // A skipped number means a decision was written and lost.
    let dir = repo_root().join("docs/adr");
    let mut numbers: Vec<u32> = fs::read_dir(&dir)
        .expect("docs/adr is missing")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.split_once('-')?.0.parse().ok()
        })
        .collect();
    numbers.sort_unstable();

    assert!(!numbers.is_empty(), "docs/adr holds no decisions");
    for (index, number) in numbers.iter().enumerate() {
        let expected = u32::try_from(index).unwrap() + 1;
        assert_eq!(*number, expected, "ADR {expected:04} is missing from docs/adr");
    }
}

#[test]
fn every_migration_can_be_undone() {
    // Rolling back a bad release is a migration, not a restore from backup
    // (v0.2.0). That only holds while every `.up.sql` has its `.down.sql`:
    // one missing pair makes the whole rollback refuse, and it is found at the
    // worst possible moment - during the rollback.
    let mut checked = 0;
    for dir in migration_dirs(&repo_root()) {
        for entry in fs::read_dir(&dir).expect("reading a migrations directory") {
            let name = entry.expect("a directory entry").file_name().into_string().expect("a UTF-8 file name");
            let Some(stem) = name.strip_suffix(".up.sql") else { continue };

            let down = dir.join(format!("{stem}.down.sql"));
            assert!(
                down.exists(),
                "{} has no .down.sql; a release carrying it cannot be rolled back",
                dir.join(&name).display()
            );
            checked += 1;
        }
    }

    // A gate that checks nothing passes silently. Until a service owns a
    // schema, the only migrations are the test fixtures - but there must be
    // some, or this test is measuring an empty search.
    assert!(checked > 0, "no .up.sql was found anywhere; the search below is looking in the wrong place");
}

/// Every directory in the workspace holding sqlx migrations.
fn migration_dirs(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![root.join("crates")];

    while let Some(dir) = queue.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.file_name().is_some_and(|n| n == "migrations") {
                found.push(path);
            } else {
                queue.push(path);
            }
        }
    }

    found
}
