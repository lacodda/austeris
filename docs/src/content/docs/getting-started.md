---
title: Getting Started
description: Bring austeris up with Docker and sign in for the first time.
---

austeris runs as a small set of processes behind one gateway. The quickest way
to see it is the compose file in the repository.

## Bring it up

```console
$ git clone https://github.com/lacodda/austeris && cd austeris
$ docker compose up -d
```

Three containers start: PostgreSQL, the `identity` service, and the gateway.
Each service waits for the one it depends on to report healthy, so there is no
ordering to get right by hand.

## The first account

An installation with no accounts creates one on first start and prints its
password - once, into the log of the `identity` container:

```console
$ docker compose logs identity

  An account was created, because this installation had none:

      email:    owner@austeris.local
      password: mjmr4evx7svaue82nvzn

  This is the only time it is shown. Sign in and change it.
```

Set `AUSTERIS_FIRST_USER` before the first start to use your own address. Once
an account exists, restarting creates nothing and prints nothing - a restart is
not a way to mint credentials.

## Sign in

```console
$ curl -c jar -X POST http://127.0.0.1:8084/api/v1/auth/login \
    -H 'Content-Type: application/json' \
    -d '{"email":"owner@austeris.local","password":"mjmr4evx7svaue82nvzn"}'
{"status":"ok"}

$ curl -b jar http://127.0.0.1:8084/api/v1/auth/me
{"id":"f7325b2d-...","email":"owner@austeris.local","display_name":"owner@austeris.local"}
```

The session lives in a cookie the browser carries and the server can end. See
[Authentication](/austeris/reference/auth/) for the whole surface.

## Without Docker

Every service is the same binary with a different subcommand, so a service can
be run outside compose while the rest stay in it:

```console
$ AUSTERIS_DATABASE_URL=postgres://austeris:austeris@localhost:5434/austeris \
  cargo run -- serve identity
```

The gateway owns no schema and needs no database at all:

```console
$ cargo run -- serve gateway
```
