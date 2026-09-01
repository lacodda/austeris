---
title: Authentication
description: The endpoints for signing in, signing out, and asking who you are.
---

Every path is served by the gateway under `/api/v1/auth`. These are the only
paths reachable without a session - everything else behind the gateway answers
`401` until you have one. See
[Sessions](/austeris/concepts/sessions/) for what a session is and how a refusal
behaves.

## `POST /api/v1/auth/login`

Signs in and sets the session cookie.

```json
{ "email": "owner@austeris.local", "password": "..." }
```

| Status | Meaning |
| --- | --- |
| `200` | Signed in. The response carries `Set-Cookie: austeris_session=...`. |
| `401` | Wrong email or password - or the account is deactivated, or does not exist. One answer for all four. |
| `429` | The address is locked out after five refusals in fifteen minutes. |

```console
$ curl -c jar -X POST http://127.0.0.1:8084/api/v1/auth/login \
    -H 'Content-Type: application/json' \
    -d '{"email":"owner@austeris.local","password":"..."}'
{"status":"ok"}
```

## `GET /api/v1/auth/me`

Who the session belongs to. An interface calls this on load to decide whether to
show a sign-in form.

```console
$ curl -b jar http://127.0.0.1:8084/api/v1/auth/me
{"id":"f7325b2d-d1f8-486f-baa9-9927fe451309","email":"owner@austeris.local","display_name":"owner@austeris.local"}
```

`401` when there is no session, or it has expired, or the account behind it was
deactivated.

## `POST /api/v1/auth/logout`

Ends **this** session and clears the cookie. The token stops working
immediately - it is a row, and the row is gone.

## `POST /api/v1/auth/logout-everywhere`

Ends every session the account has - the answer to a laptop left on a train.
Reports how many were ended:

```json
{ "status": "ok", "ended": 3 }
```

The account itself is untouched: signing in again works.

## Between services

Services do not call these paths. They ask `identity` over gRPC:

```proto
service Identity {
  rpc ValidateSession(ValidateSessionRequest) returns (ValidateSessionResponse);
}
```

An empty `user_id` in the response means the token is unknown, expired, or its
account is deactivated - three cases the caller has no business telling apart.

Breaking this contract means a new `identity.v2` package, never an edit to `v1`.
