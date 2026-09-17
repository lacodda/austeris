# Contributing to austeris

## Development

```console
$ cargo fmt --check
$ cargo clippy --all-targets -- -D warnings
$ cargo test
```

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
