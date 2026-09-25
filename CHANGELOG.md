# Changelog

All notable changes to this project are documented in this file.

## [0.8.0] - 2026-09-25

### Bug Fixes
- Speak TLS to price sources and remote installations
- Migrate an installation that already has entries

### Dependencies
- Update the toolchain and dependencies

### Features
- Exchange money between currencies, with the fee on a line of its own
- Read the day's rates from the central banks of Paraguay and Russia


## [0.7.0] - 2026-09-23

### Build
- Derive the dependency layer with cargo-chef

### CI
- Publish amd64 and arm64 images and smoke-test each

### Documentation
- Make the readme a shopfront

### Features
- Run every service in one process
- Seed a made-up household with austeris demo
- Install from the published image as one container
- Update, back up and restore a stand over ssh

### Testing
- Read the configuration table where it now lives


## [0.6.0] - 2026-09-17

### Bug Fixes
- Draw each icon size at the level that reads at it

### Documentation
- Write down why an entry has sides

### Features
- Read the caller the gateway vouched for
- Add the ledger's contract with the modules
- Open the books
- Serve the ledger, and record an entry from a terminal

## [0.5.0] - 2026-09-01

### Breaking Changes
- The gRPC services are now `identity.v1.IdentityService`
and `market.v1.MarketService`. Generated clients follow the new names.

### CI
- Hold the proto gate to breaking changes again

### Features
- Merge the services' OpenAPI into one document

### Refactoring
- Name the services as the linter demands

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

### Bug Fixes
- Update CmcResponse and CmcListing structures for listings/latest
- Improve validation error messages for detailed output
- Handle non-existent wallet_id and asset_id in POST /transactions with 400 error

### CI
- Replace the 2025 workflow with the release gate

### Documentation
- Enhance Swagger documentation for get_transactions and create_transaction
- Enhance Swagger documentation for assets and wallets
- Add comments for better code readability
- Update README and env example
- Add license, ADRs and release tooling for the rebuild
- Rewrite the README for the rebuild
- Regenerate the changelog for v0.1.0

### Features
- Added basic routes and models
- Add assets fetching from CoinMarketCap
- Add filtering and pagination to get_transactions
- Add portfolio value calculation endpoint with CMC quotes
- Add portfolio snapshot model and database table
- Add endpoint to create portfolio snapshot
- Add endpoint to list snapshots and compare with current portfolio
- Add error logging with log and env_logger
- Add DTO validation with validator crate
- Improve error handling with custom error type and JSON response
- Enhance success responses for POST /assets/update and POST /transactions
- Add asset_prices table and update assets with rank and cmc_id as INT
- Update CmcService to fetch prices for existing assets only
- Add AssetPriceRepository to store asset prices
- Add GET /assets/prices endpoint to retrieve latest asset prices
- Add asset_id filter to GET /assets/prices endpoint
- Add historical price retrieval endpoint
- Integrate Redis for caching latest asset prices
- Update PortfolioService to use Redis for portfolio value
- Add periodic price updates using tokio::time
- Add AssetService for asset management
- Add WalletService for wallet management
- Add TransactionService for transaction management
- Add SnapshotService for snapshot management
- Add datetime formatting utility
- Centralize database access in repositories
- Add Docker support for ARM (Raspberry Pi) and x86 (Windows) with configurable .env
- Initialize microservices project structure
- Implement austeris-common library
- Add REST routes, gRPC price endpoint and schema-scoped migrations
- Add the workspace, shared plumbing and the gateway

### Refactoring
- Replace all error types with anyhow
- Introduce repository module for database operations
- Introduce DTO module for API data transfer
- Extract portfolio logic into PortfolioService
- Update data model and add ORDER BY id ASC for assets query
- Add validation for FilterParams in get_transactions
- Simplify routes by delegating to services
- Add From traits for model-to-DTO mapping
- Unify SQL queries with QueryBuilder

