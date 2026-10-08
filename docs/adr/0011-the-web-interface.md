# 11. The web interface: compiled into the binary, built from the line's kit

Date: 2026-10-08

## Status

Accepted. Builds on [0005](0005-one-binary-many-services.md) and
[0008](0008-one-process-by-default.md): the gateway is the only surface, and the
interface is one more thing it serves.

## Context

Until 0.10 every endpoint answered JSON and the only client was the command
line. The web interface forces three questions that outlive the version that
introduces it: how the built page reaches a browser, what its components are
made of, and how money crosses from the API into a screen without changing on
the way.

## Decision

**The built single-page app is compiled into the binary.** `rust-embed` takes
`web/dist` into the `austeris` crate, and the gateway's fallback serves it: a
file when one exists at the path, `index.html` for any other path, so a reload
on `/entries` is answered by the app that owns the route. Everything under
`/api` is the exception - a path there that no service owns is a client's
mistake, and answering it with a page would hide that behind a 200.

One file stays the whole product for a self-hosted install. The interface can
never be from another build than the API it calls, and "which build is this" is
a question about the version rather than about a directory beside the binary.
`allow_missing` lets a clean checkout compile without Node; `release_consistency`
refuses a tree without `web/dist/index.html`, so a binary built that way cannot
be released looking complete.

**The document is held to its own origin.** It is served with a content
security policy that loads scripts, styles, images and API calls from the
installation alone and forbids framing by another site, plus `nosniff` and a
same-origin referrer. Inline styles are allowed: the components set layout
through `style`, and their overlays insert rules as they open. Bundles are
fingerprinted and cached as immutable; the document is never cached.

**The kit is dowel, copied from its registry.** The components are dowel's
primitives installed with `shadcn` from the line's registry and kept identical
to it - `check-registry` fails on a copy that drifted, and screens are composed
from them rather than from hand-made controls. The accent is austeris's orchid
from the registry of marks; no colour is written in a component, and the theme
follows the operating system through tokens, so there is no `dark:` utility
anywhere.

**Money stays a decimal string from the API to the glyph.** A JSON number in a
browser is a double ([0004](0004-money-as-numeric-decimal.md)), so the API
already sends amounts as strings; the interface keeps them so.
`Intl.NumberFormat` formats a decimal string exactly, which puts the language's
separators, the currency's own digits (no kopecks on guaraníes) and its symbol
on the screen without a conversion. The one sum a form needs - the account's
side of a split - is done on scaled integers, so an entry balances by
construction rather than by a float's rounding.

**The session is the API's.** The page signs in with the same HttpOnly cookie
the command line uses, `SameSite=Strict`; nothing is kept in `localStorage`
but a person's choice of language and of the currency a total is shown in.

## Consequences

Node and pnpm join the toolchain. The gate builds and checks the interface
before the Rust suite, and CI builds it ahead of every job that compiles the
binary: the image, the release archives and the tests all carry the interface
they will ship.

In development Vite serves the page and forwards `/api` to a running austeris,
so a change is a reload rather than a Rust build.

A release published to crates.io would need `web/dist` inside the `austeris`
package, which `rust-embed` reads from outside it today. That is the day this
decision is revisited; nothing is published there yet.
