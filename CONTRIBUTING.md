# Contributing to austeris

## Development

The web interface is compiled into the binary, so it is built first; the
suite refuses a tree without it.

```console
$ pnpm --dir web install
$ pnpm --dir web lint     # eslint, types, locales, dowel copies, licenses, tests
$ pnpm --dir web build
$ cargo fmt --check
$ cargo clippy --all-targets -- -D warnings
$ cargo test
```

The database tests run only with a database - `docker compose up -d db` and
`AUSTERIS_DATABASE_URL=postgres://austeris:austeris@localhost:5434/austeris` -
and skip themselves without one.

Working on the interface, let Vite serve it and forward `/api` to a running
austeris (`docker compose up` publishes one on 8084; `AUSTERIS_DEV_API` points
elsewhere):

```console
$ pnpm --dir web dev
```

The components in `web/src/components/ui/` are copies of
[dowel](https://lacodda.github.io/dowel/)'s primitives and are not edited:
`pnpm --dir web registry` fails on one that differs from the installed
`dowel-ui`. A fix belongs in dowel; a screen's own components are
PascalCase files beside them. Interface text lives in
`web/src/i18n/locales/` - English is the source, and every other locale has
to carry the same keys.

## Repository layout

Microservices in one Cargo workspace, one PostgreSQL, a schema per service.
The shape is fixed by decisions written down in
[`docs/adr/`](https://github.com/lacodda/austeris/tree/main/docs/adr); see also
[Architecture](https://lacodda.github.io/austeris/concepts/architecture/).

## Commits

English, [Conventional Commits](https://www.conventionalcommits.org/), no
trailers.

## License

By contributing, you agree that your contributions will be licensed under the
project's MIT license.
