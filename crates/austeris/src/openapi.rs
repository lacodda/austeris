//! The platform's `OpenAPI` document, and the page that renders it.
//!
//! Each service describes its own share; the gateway merges them into one
//! document, because one document is what a client of austeris actually faces.
//! The merge is done at compile time from the service crates rather than by
//! asking the running services for their pieces: the binary already contains
//! every service (ADR 0005), so a spec assembled over the network could only
//! ever be the same answer arrived at less reliably - and would go blank
//! whenever a service was down.

use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

/// The document's own frame: what this API is, and what version of it.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "austeris",
        description = "Self-hosted home finance: accounts and entries in several currencies, what you own and what you owe, and the whole picture on top.",
        license(name = "MIT", url = "https://github.com/lacodda/austeris/blob/main/LICENSE"),
    ),
    servers((url = "/", description = "This installation")),
)]
struct Frame;

/// The whole surface, every service merged.
///
/// Paths are the public ones - `/api/v1/auth/login`, not the `/auth/login` the
/// identity service listens on internally. What a reader needs is the address
/// they can actually call.
#[must_use]
pub fn document() -> Document {
    let mut document = Frame::openapi();
    env!("CARGO_PKG_VERSION").clone_into(&mut document.info.version);
    // utoipa fills `contact` from the manifest's `authors`, which carries a
    // personal address. Authorship belongs in the manifest; a published spec is
    // not the place to hand it to whoever fetches it.
    document.info.contact = None;

    document.merge(austeris_identity::routes::ApiDoc::openapi());
    document.merge(austeris_ledger::routes::ApiDoc::openapi());
    document.merge(austeris_market::routes::ApiDoc::openapi());

    document
}

#[cfg(test)]
mod tests {
    use super::document;

    #[test]
    fn every_service_contributes_its_paths() {
        let document = document();
        let paths: Vec<&str> = document.paths.paths.keys().map(String::as_str).collect();

        // A merge that silently dropped a service would leave a document that
        // looks complete and documents half the product.
        assert!(paths.contains(&"/api/v1/auth/login"), "identity is missing: {paths:?}");
        assert!(paths.contains(&"/api/v1/ledger/accounts"), "ledger is missing: {paths:?}");
        assert!(paths.contains(&"/api/v1/market/instruments"), "market is missing: {paths:?}");
    }

    #[test]
    fn the_document_describes_the_public_paths_not_the_internal_ones() {
        let document = document();
        for path in document.paths.paths.keys() {
            assert!(
                path.starts_with("/api/v1/"),
                "`{path}` is an internal path; a reader cannot call it from outside the compose network"
            );
        }
    }

    #[test]
    fn the_spec_carries_no_personal_contact() {
        // utoipa fills `contact` from the manifest's `authors`. A published
        // spec handing out the owner's address is the kind of leak that
        // reappears the moment someone regenerates the frame.
        let document = document();
        assert!(document.info.contact.is_none(), "the spec carries a contact");

        // An address rather than any `@`: the docs themselves write `@5965`
        // for a rate stated in a typed line.
        let json = serde_json::to_string(&document).expect("serializing");
        let address = json.match_indices('@').find(|(at, _)| {
            let is_part = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
            let local = json[..*at].chars().next_back().is_some_and(is_part);
            let domain: String = json[at + 1..].chars().take_while(|c| is_part(*c)).collect();
            local && domain.contains('.') && !domain.ends_with('.')
        });
        assert!(
            address.is_none(),
            "an address survived in the spec: {}",
            &json[address.map_or(0, |(at, _)| at.saturating_sub(40))..][..80]
        );
    }

    #[test]
    fn an_address_is_told_apart_from_a_rate() {
        // The check above, on the two things it has to tell apart.
        let has_address = |text: &str| {
            text.match_indices('@').any(|(at, _)| {
                let is_part = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
                let local = text[..at].chars().next_back().is_some_and(is_part);
                let domain: String = text[at + 1..].chars().take_while(|c| is_part(*c)).collect();
                local && domain.contains('.') && !domain.ends_with('.')
            })
        };
        assert!(has_address(r#""email":"owner@example.com""#));
        assert!(!has_address("state it, as in `@5965`."));
    }

    #[test]
    fn the_document_carries_this_build_s_version() {
        // A spec stamped with a stale version is worse than an unstamped one:
        // it tells a reader they are looking at something they are not.
        assert_eq!(document().info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn every_decimal_in_the_spec_is_described_as_a_string() {
        // The schema is the contract a client generates code from. Described as
        // a number, every generated client parses it into a double and loses
        // the value before rendering it (ADR 0004).
        //
        // Two things keep this true and either suffices: utoipa's `decimal`
        // feature, which knows what a `Decimal` is, and an explicit
        // `value_type = String` on the field. Removing both does not produce a
        // wrong spec - it fails to compile, because `Decimal` then has no
        // schema at all. This test therefore guards the shape rather than the
        // mechanism, and would catch a future field described as a number some
        // other way.
        let document = document();
        let schemas = &document.components.as_ref().expect("components").schemas;
        let json = serde_json::to_value(schemas).expect("serializing the schemas");

        // Every property by these names, in every schema - not the first one
        // found, which is how a second `rate` described as a number would slip
        // past. A property that refers to another schema is an object whose
        // own fields are checked where that schema is.
        let money: &[&str] = &["price", "amount", "opening_balance", "rate", "converted", "given", "got"];
        let mut seen: Vec<&str> = Vec::new();
        let mut stack: Vec<&serde_json::Value> = vec![&json];
        while let Some(value) = stack.pop() {
            match value {
                serde_json::Value::Object(map) => {
                    if let Some(serde_json::Value::Object(properties)) = map.get("properties") {
                        for (name, schema) in properties {
                            if let Some(field) = money.iter().find(|field| **field == name) {
                                assert!(is_string(schema) || is_reference(schema), "`{name}` is not described as a string: {schema}");
                                seen.push(field);
                            }
                        }
                    }
                    stack.extend(map.values());
                }
                serde_json::Value::Array(items) => stack.extend(items),
                _ => {}
            }
        }
        for field in money {
            assert!(seen.contains(field), "no `{field}` field in the spec; the list above is out of date");
        }
    }

    /// Whether a schema describes a string, possibly an optional one.
    fn is_string(schema: &serde_json::Value) -> bool {
        match schema.get("type") {
            Some(serde_json::Value::String(kind)) => kind == "string",
            Some(serde_json::Value::Array(kinds)) => kinds.iter().any(|kind| kind == "string") && kinds.iter().all(|kind| kind == "string" || kind == "null"),
            _ => schema
                .get("oneOf")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|options| options.iter().any(is_string) && options.iter().all(|option| is_string(option) || option["type"] == "null")),
        }
    }

    /// Whether a schema is another schema by reference, possibly an optional one.
    fn is_reference(schema: &serde_json::Value) -> bool {
        schema.get("$ref").is_some()
            || schema.get("oneOf").and_then(serde_json::Value::as_array).is_some_and(|options| {
                options.iter().any(|option| option.get("$ref").is_some())
                    && options.iter().all(|option| option.get("$ref").is_some() || option["type"] == "null")
            })
    }

    #[test]
    fn every_documented_path_belongs_to_a_service_the_gateway_routes() {
        // A path in the spec that the gateway does not forward is a promise
        // nothing keeps.
        let document = document();
        for path in document.paths.paths.keys() {
            let prefix = path.trim_start_matches("/api/v1/").split('/').next().unwrap_or_default();
            assert!(
                crate::service::Service::routed().iter().any(|service| service.prefix() == prefix),
                "`{path}` is documented but no service answers on `{prefix}`"
            );
        }
    }
}
