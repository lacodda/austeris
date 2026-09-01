# Changelog

All notable changes to this project are documented in this file.

## [0.4.0] - 2026-09-01

### Breaking Changes
- `/api/v1/*` other than `/api/v1/auth/*` now answers 401
without a session cookie. Nothing consumed those paths yet.

### Bug Fixes
- Require a session for everything but signing in

### Features
- Add instruments and their prices


## [0.3.0] - 2026-09-01

### Documentation
- Add the documentation site

### Features
- Add people, passwords and sessions
- Forward to services and vouch for the caller


## [0.2.0] - 2026-09-01

### Build
- Package the workspace as one image and a compose stack

### Features
- Add reversible migrations scoped to one service's schema


## [0.1.0] - 2026-09-01

### CI
- Replace the 2025 workflow with the release gate

### Documentation
- Add license, ADRs and release tooling for the rebuild
- Rewrite the README for the rebuild

### Features
- Add the workspace, shared plumbing and the gateway

