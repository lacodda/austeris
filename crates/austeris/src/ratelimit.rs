//! A ceiling on how fast one client can ask.
//!
//! The stand is reachable from a home network, so the gateway - the only thing
//! outside can talk to - keeps a fixed-window counter per client. It is in the
//! process's memory rather than in a store: austeris runs one gateway, and a
//! limiter that needs its own database to say "slow down" is a second thing to
//! keep running for a job a `HashMap` does.
//!
//! Deliberately coarse. It is here to blunt a script hammering the sign-in
//! form, not to meter a paying API - the per-address lockout in identity is
//! what actually stops a password being guessed.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// How many requests one client may make per window.
///
/// A person using the web UI makes a handful a second at most; a script
/// guessing passwords makes thousands.
const REQUESTS_PER_WINDOW: u32 = 120;

/// How long a window lasts.
const WINDOW: Duration = Duration::from_secs(60);

/// How many clients are remembered before the table is swept.
///
/// A bound rather than none: the key comes from outside, so an unbounded map is
/// a way to exhaust a Raspberry Pi's memory by varying the source address.
const MAX_TRACKED: usize = 10_000;

/// The counters, shared by every request.
#[derive(Clone, Default)]
pub struct Limiter {
    clients: Arc<Mutex<HashMap<IpAddr, Window>>>,
}

/// One client's count within the current window.
#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    requests: u32,
}

impl Limiter {
    /// Records a request and says whether it is over the limit.
    fn over_limit(&self, client: IpAddr) -> bool {
        let now = Instant::now();
        // A poisoned lock means another thread panicked while holding it. The
        // counters are not worth failing a request over, so the limiter starts
        // fresh rather than propagating the panic.
        let mut clients = self.clients.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        if clients.len() >= MAX_TRACKED {
            clients.retain(|_, window| now.duration_since(window.started) < WINDOW);
            // Still full even after dropping every finished window: every entry
            // is live, so this is either genuine load or a distributed flood,
            // and neither is helped by growing the table further.
            if clients.len() >= MAX_TRACKED {
                return true;
            }
        }

        let window = clients.entry(client).or_insert(Window { started: now, requests: 0 });
        if now.duration_since(window.started) >= WINDOW {
            *window = Window { started: now, requests: 0 };
        }
        window.requests += 1;

        window.requests > REQUESTS_PER_WINDOW
    }
}

/// Middleware that refuses a client asking too fast.
pub async fn limit(State(limiter): State<Limiter>, ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>, request: Request, next: Next) -> Response {
    // The peer address, not a forwarded header: anything in front of austeris
    // is the operator's own reverse proxy, and trusting a header a client can
    // set would let one client claim to be thousands.
    if limiter.over_limit(peer.ip()) {
        tracing::warn!(client = %peer.ip(), "refused a client asking too fast");
        return (StatusCode::TOO_MANY_REQUESTS, "too many requests\n").into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{Limiter, REQUESTS_PER_WINDOW};

    fn client(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, last))
    }

    #[test]
    fn a_client_within_the_limit_is_let_through() {
        let limiter = Limiter::default();
        for _ in 0..REQUESTS_PER_WINDOW {
            assert!(!limiter.over_limit(client(1)));
        }
    }

    #[test]
    fn the_request_after_the_limit_is_refused() {
        let limiter = Limiter::default();
        for _ in 0..REQUESTS_PER_WINDOW {
            limiter.over_limit(client(1));
        }
        assert!(limiter.over_limit(client(1)));
    }

    #[test]
    fn one_client_hitting_the_limit_does_not_refuse_another() {
        // A shared counter would make one script on the network lock everyone
        // else out of their own finances.
        let limiter = Limiter::default();
        for _ in 0..=REQUESTS_PER_WINDOW {
            limiter.over_limit(client(1));
        }
        assert!(limiter.over_limit(client(1)));
        assert!(!limiter.over_limit(client(2)));
    }
}
