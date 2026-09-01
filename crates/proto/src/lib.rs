//! The gRPC contracts austeris services speak to each other.
//!
//! One crate for both sides of every call: the service that implements a
//! contract and the one that calls it are compiled against the same generated
//! code, so a mismatch is a build error rather than a runtime surprise.
//!
//! A breaking change means a new package (`identity.v2`), never an edit to an
//! existing one (ADR 0001).

/// The market service: what an instrument is worth, now or at an instant.
pub mod market {
    /// Version 1 of the contract.
    pub mod v1 {
        // The generated code is not ours to lint.
        #![allow(clippy::pedantic, clippy::all)]
        tonic::include_proto!("market.v1");
    }
}

/// The identity service: who is calling, without anyone else seeing a password.
pub mod identity {
    /// Version 1 of the contract.
    pub mod v1 {
        // The generated code is not ours to lint.
        #![allow(clippy::pedantic, clippy::all)]
        tonic::include_proto!("identity.v1");
    }
}
