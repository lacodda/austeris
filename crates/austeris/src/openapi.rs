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

        let json = serde_json::to_string(&document).expect("serializing");
        assert!(!json.contains('@'), "an address survived somewhere in the spec");
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
        let json = serde_json::to_string(schemas).expect("serializing the schemas");

        // One field today; written for the list it will become, because the
        // second monetary field is the one nobody thinks to re-check.
        let decimal_fields: &[&str] = &["price"];
        for field in decimal_fields {
            let at = json.find(&format!(r#""{field}":"#)).unwrap_or_else(|| panic!("no `{field}` field in the spec"));
            let described = &json[at..(at + 200).min(json.len())];
            assert!(described.contains(r#""type":"string""#), "`{field}` is not described as a string: {described}");
        }
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
