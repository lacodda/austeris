---
title: Getting Started
description: Install austeris with Docker and sign in for the first time.
---

austeris is one image. Installing it means pulling that image and a
PostgreSQL next to it; nothing is built on your machine. The image is published
for amd64 and arm64, so a Raspberry Pi is a fine place for it.

## Install

```console
$ curl -o docker-compose.yml \
    https://raw.githubusercontent.com/lacodda/austeris/main/docker-compose.install.yml
$ printf 'COMPOSE_PROFILES=single\nPOSTGRES_PASSWORD=%s\n' "$(openssl rand -hex 24)" > .env
$ chmod 600 .env
$ docker compose up -d
```

The `.env` holds the one secret an installation has - the database password -
generated on the spot, so there is no default password to forget to change.
Pin a version there too once you are past a first look:

```ini
AUSTERIS_VERSION=0.7.0
```

Two containers start: PostgreSQL, and austeris running every service in one
process. The public port (`8084` unless `AUSTERIS_PORT` says otherwise) opens
only once every schema is migrated, so a container reporting healthy can serve.

## The first account

An installation with no accounts creates one on first start and prints its
password - once, into the log:

```console
$ docker compose logs austeris

  An account was created, because this installation had none:

      email:    owner@austeris.local
      password: mjmr4evx7svaue82nvzn

  This is the only time it is shown. Sign in and change it.
```

Set `AUSTERIS_FIRST_USER` before the first start to use your own address. Once
an account exists, restarting creates nothing and prints nothing - a restart is
not a way to mint credentials.

## Look around first

`AUSTERIS_DEMO=true` in `.env` fills an empty installation with a made-up
household instead: four accounts, a tree of categories, four months of
spending, a second currency and two invented instruments with prices. Sign in
as `demo@austeris.local` with the password `austeris-demo`. See
[Demo](/austeris/reference/demo/) for what it will and will not do.

From a clone of the repository, `docker compose up` builds from source and
starts with the demo - the quickest way to see a change working:

```console
$ git clone https://github.com/lacodda/austeris && cd austeris
$ docker compose up
```

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

## A container per service

The install file has a second shape, for someone who wants to restart, watch or
scale one service on its own:

```console
$ docker compose --profile split up -d
```

Both shapes use the same image and the same database volume, so switching keeps
the books. Run one at a time: both publish the same port. See
[Architecture](/austeris/concepts/architecture/) for what differs between them -
very little.

## Without Docker

Every service is the same binary, so the whole installation is one command
against a PostgreSQL you already have:

```console
$ AUSTERIS_DATABASE_URL=postgres://austeris:austeris@localhost:5434/austeris \
  cargo run -- serve
```

or one service at a time, while the rest run elsewhere:

```console
$ cargo run -- serve identity
$ cargo run -- serve gateway      # owns no schema, needs no database
```
