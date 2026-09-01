---
title: Sessions
description: How signing in works, and what a session is.
---

## Passwords

A password is human-chosen and therefore guessable at scale, so it is stored as
an Argon2id hash with a per-password salt. The same password chosen by two
people produces two different hashes, which is what makes a database dump
useless for finding accounts that share one.

Session tokens are treated differently: they are 256-bit strings the server
generated, so there is no dictionary to slow anyone down with, and they get
SHA-256 - cheap enough to verify on every request.

## Sessions are server-side

A session is a row, not a signed self-contained token. A stateless token cannot
be withdrawn, and the blocklist that would fix that is this table under another
name - while an installation must be able to end access **now**, on the
afternoon a laptop goes missing.

Only the hash of the token is stored. A database dump, or a row in a log line,
hands out nothing usable.

The cookie is `HttpOnly` (no script can read it, even one that got injected) and
`SameSite=Strict` (it is never sent from another origin, which also makes CSRF
tokens unnecessary). It is marked `Secure` only when `AUSTERIS_SECURE_COOKIES`
says so: a self-hosted install on a home network is often plain HTTP, where an
unconditional flag makes every sign-in fail with nothing in the response saying
why.

A session expires a fortnight after it was last used. Using it extends it, so
someone working through the day is not signed out mid-afternoon.

## Being refused

An unknown address, a deactivated account and a wrong password are one answer:
`401` with the same message. Distinguishing them turns the sign-in form into a
way to learn who has an account here. The work of verifying a password is done
even when the address is unknown, so the two do not differ by a measurable
pause.

Five refusals within fifteen minutes lock an address out, and the lockout is
keyed to the **address that was attempted**, not to an account. An address with
no account behind it locks out the same way - otherwise the lockout itself would
answer the question the refusal refuses to answer.

A successful sign-in forgets the failures before it, so a typo last week cannot
combine with a typo today.

## What other services see

Nothing but a user id. A service asks the gateway's header - or, between
services, `identity.v1.ValidateSession` - and gets back who is calling. No
service other than `identity` ever sees a password, and none of them can tell an
expired session from a deactivated account, because neither changes what they
should do.
