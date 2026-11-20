# 19. Every error carries a code

Date: 2026-11-20

## Status

Accepted. Builds on [0011](0011-the-web-interface.md).

## Context

An error was its English sentence, and some errors also carried a code and the
facts the sentence was written from, so the web could say them in its reader's
language. Which ones did was a matter of who had remembered: a Russian screen
showed half its refusals in English, and nothing failed when a new refusal was
written without a code. Requests axum could not read - a malformed body, a path
that was not an id - answered with axum's own plain text, with no body a client
could read at all.

## Decision

The code is part of building an error, not something added after. `AppError`
is made by `bad_request`, `not_found`, `conflict`, `unavailable` or `refused`,
each of which takes a `Code`; there is no constructor without one. A fault the
client did not cause answers `server.internal`. `ErrorBody.code` is always
there, and `params` is always an object - empty when there are no facts.

The services read requests through `austeris_common::extract` - `Json`, `Query`
and `Path` - which reject with `request.malformed` and axum's reason in `why`,
so no endpoint answers anything but the one JSON body.

Codes are declared once, in `Code`, and `Code::ALL` lists them; a test holds the
web's locale files to a sentence for every code in every language, and to none
for a code that is no longer answered. The English `message` stays, for the
command line and the logs, and is what a client falls back on for a code it
does not know.

## Consequences

- A refusal cannot be written without deciding what it is called, and a screen
  says it in its reader's language from the day it exists.
- A code is a contract: renaming one is a breaking change of the API.
- A proxy's error page is still not JSON, and the web falls back on the status
  text for it.
