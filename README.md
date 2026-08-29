> **Status (2026-08-28):** austeris is being rebuilt as a self-hosted **home-finance platform** — a bookkeeping core with modules for market prices, portfolios, loans and mortgages, deposits, recurring income and forecasts — on a microservice architecture (axum + gRPC + PostgreSQL, React UI). The decisions are in [`docs/adr/`](https://github.com/lacodda/austeris/tree/main/docs/adr). The text below describes the 2025 crypto-tracker prototype, kept in the tree until the first release of the rebuild (`v0.1.0`) and preserved at the tag `legacy-2025`.

# Austeris: Crypto Portfolio Tracker 📊💸

Austeris is a robust and modern cryptocurrency portfolio tracker built with **Rust** 🦀, leveraging **Docker** 🐳 for containerization, **PostgreSQL** 🐘 for persistent storage, **Redis** 🔴 for caching, and the **CoinMarketCap API** 📈 for real-time market data. It provides a RESTful API to manage assets, wallets, transactions, and portfolio snapshots, with Swagger UI for easy API exploration.

---

## ✨ Features

- **Asset Management** 🪙: Create, update, and retrieve cryptocurrency assets with details like symbol, name, and CoinMarketCap ID.
- **Wallet Management** 💼: Manage multiple wallets (e.g., exchange or hardware wallets) with customizable names and addresses.
- **Transaction Tracking** 📒: Record buy/sell transactions with validation for assets and wallets.
- **Portfolio Valuation** 💰: Calculate the total portfolio value in USD using real-time prices.
- **Portfolio Snapshots** 📸: Capture snapshots of your portfolio's asset holdings at specific times and compare them with the current state.
- **Price Updates** ⏰: Automatically fetch and cache asset prices from CoinMarketCap every 15 minutes.
- **Swagger UI** 📜: Interactive API documentation at `http://localhost:9000/swagger-ui/`.
- **SQLx Integration** 🗃️: Type-safe SQL queries with compile-time validation using SQLx.
- **Dockerized Setup** 🐳: Easy deployment with PostgreSQL, Redis, and pgAdmin containers.
- **Git Hooks** 🔗: Automated SQLx query cache updates on commits to prevent runtime errors.

---

## 🛠️ Tech Stack

- **Rust** 🦀: High-performance backend with Actix Web for the API.
- **PostgreSQL** 🐘: Relational database for storing assets, wallets, transactions, and snapshots.
- **Redis** 🔴: In-memory caching for asset prices with a 1-hour TTL.
- **CoinMarketCap API** 📈: Fetches real-time cryptocurrency data.
- **Docker & Docker Compose** 🐳: Containerized services for easy setup and deployment.
- **SQLx** 🗃️: Async database access with compile-time query validation.
- **Utoipa** 📖: Generates OpenAPI documentation for Swagger UI.
- **Actix Web Validator** ✅: Validates incoming API requests.
- **Redis-rs** 🔗: Async Redis client for caching.

---

## 📋 Prerequisites

To run Austeris, ensure you have the following installed:

- [Docker Desktop](https://www.docker.com/products/docker-desktop) 🐳
- [Git](https://git-scm.com/) 📂
- A valid **CoinMarketCap API key** 🔑 (sign up at [CoinMarketCap](https://coinmarketcap.com/api/))

---

## 🚀 Getting Started

Follow these steps to set up and run Austeris locally:

### 1. Clone the Repository 📂

```bash
git clone https://github.com/lacodda/austeris.git
cd austeris
```

### 2. Initialize Environment Variables ⚙️

Create a `.env` file from the provided `.env.example` and set your CoinMarketCap API key:

```bash
make init-env COINMARKETCAP_API_KEY=your_api_key_here
```

Edit `.env` to customize settings if needed (e.g., database credentials, ports). Example `.env`:

```env
APP_PORT=9000
RUST_LOG=info
COINMARKETCAP_API_KEY=your_api_key_here
POSTGRES_USER=user
POSTGRES_PASSWORD=password
POSTGRES_DB=austeris
POSTGRES_PORT=5432
DATABASE_URL=postgres://user:password@postgres:5432/austeris
DATABASE_URL_LOCALHOST=postgresql://user:password@localhost:5432/austeris
SQLX_MAX_CONNECTIONS=5
SQLX_ACQUIRE_TIMEOUT=30
REDIS_HOST=redis
REDIS_PORT=6379
REDIS_URL=redis://redis:6379
PGADMIN_DEFAULT_EMAIL=admin@admin.com
PGADMIN_DEFAULT_PASSWORD=admin
PGADMIN_PORT=5050
```

### 3. Build and Start Services 🐳

Build and run the Docker containers:

```bash
docker-compose up -d --build
```

This will:
- Build the Rust core service.
- Start **PostgreSQL**, **Redis**, and **pgAdmin** containers.
- Run database migrations automatically.
- Expose the API at `http://localhost:9000`.

### 4. Verify Setup ✅

- Access the **Swagger UI** at [http://localhost:9000/swagger-ui/](http://localhost:9000/swagger-ui/) to explore the API.
- Use **pgAdmin** at [http://localhost:5050/](http://localhost:5050/) to manage the PostgreSQL database (login with `PGADMIN_DEFAULT_EMAIL` and `PGADMIN_DEFAULT_PASSWORD` from `.env`).

### 5. Run Migrations Manually (Optional) 🗃️

If you need to run migrations separately:

```bash
make migrate
```

Or restart the core service to apply migrations:

```bash
docker-compose restart core_service
```

### 6. Run Development Mode Manually (Optional) 💻

To run the Rust service without Docker:

```bash
cd core_service
cargo run
```

Ensure PostgreSQL and Redis are running locally or via Docker.

---

## 🌐 API Endpoints

Austeris provides a RESTful API with the following endpoints:

### Assets 🪙
- **GET /assets**: Retrieve all assets.
- **POST /assets**: Create a new asset.
- **POST /assets/update**: Sync assets with CoinMarketCap data.
- **GET /assets/prices**: Get latest asset prices (optionally filtered by asset IDs).
- **GET /assets/prices/history**: Get historical asset prices.

### Wallets 💼
- **GET /wallets**: Retrieve all wallets.
- **POST /wallets**: Create a new wallet.

### Transactions 📒
- **GET /transactions**: Retrieve transactions with optional filters (asset ID, wallet ID, start date, limit, offset).
- **POST /transactions**: Create a new transaction.
- **GET /transactions/portfolio/value**: Calculate the total portfolio value in USD.

### Snapshots 📸
- **GET /snapshots**: Retrieve all portfolio snapshots with differences from the current state.
- **POST /snapshots**: Create a new portfolio snapshot.

Explore the full API documentation via **Swagger UI** at [http://localhost:9000/swagger-ui/](http://localhost:9000/swagger-ui/).

---

## 🛠️ Project Structure

The project is organized as follows:

```
austeris/
├── core_service/               # Rust backend source code
│   ├── src/                    # Application source files
│   │   ├── dto/                # Data Transfer Objects for API
│   │   ├── models/             # Database models
│   │   ├── repository/         # Database query logic
│   │   ├── routes/             # API route handlers
│   │   ├── services/           # Business logic
│   │   └── utils/              # Utility functions (e.g., datetime formatting)
│   ├── .sqlx/                  # SQLx query cache
│   ├── Cargo.toml              # Rust dependencies
│   └── Dockerfile              # Docker configuration for core_service
├── migrations/                 # PostgreSQL migration scripts
├── .env.example                # Example environment configuration
├── .git-hooks/                 # Git hooks for SQLx query cache
├── .github/workflows/          # GitHub Actions CI configuration
├── .gitignore                  # Git ignore rules
├── docker-compose.yml          # Docker Compose configuration
├── Makefile                    # Build and setup commands
├── scripts/                    # Development scripts
└── README.md                   # Project documentation
```

---

## ⚙️ Configuration

### Environment Variables 🌍

The `.env` file configures the application. Key variables include:

| Variable                   | Description                                  | Default Value             |
|----------------------------|----------------------------------------------|---------------------------|
| `APP_PORT`                | Port for the Rust API server                | `9000`                   |
| `RUST_LOG`                | Logging level for Rust                     | `info`                   |
| `COINMARKETCAP_API_KEY`   | CoinMarketCap API key                       | (Required, no default)    |
| `POSTGRES_USER`           | PostgreSQL username                         | `user`                   |
| `POSTGRES_PASSWORD`       | PostgreSQL password                         | `password`               |
| `POSTGRES_DB`             | PostgreSQL database name                    | `austeris`               |
| `POSTGRES_PORT`           | PostgreSQL port                             | `5432`                   |
| `DATABASE_URL`            | PostgreSQL connection URL (Docker)          | `postgres://user:password@postgres:5432/austeris` |
| `DATABASE_URL_LOCALHOST`  | PostgreSQL connection URL (localhost)       | `postgresql://user:password@localhost:5432/austeris` |
| `SQLX_MAX_CONNECTIONS`    | Maximum PostgreSQL connections              | `5`                      |
| `SQLX_ACQUIRE_TIMEOUT`    | Timeout for acquiring a database connection | `30`                     |
| `REDIS_HOST`              | Redis host                                  | `redis`                  |
| `REDIS_PORT`              | Redis port                                  | `6379`                   |
| `REDIS_URL`               | Redis connection URL                        | `redis://redis:6379`     |
| `PGADMIN_DEFAULT_EMAIL`   | pgAdmin login email                         | `admin@admin.com`        |
| `PGADMIN_DEFAULT_PASSWORD`| pgAdmin login password                      | `admin`                  |
| `PGADMIN_PORT`            | pgAdmin port                                | `5050`                   |

### Database Schema 🗃️

The PostgreSQL database includes the following tables:

- **assets**: Stores cryptocurrency details (symbol, name, CoinMarketCap ID, decimals, rank).
- **wallets**: Stores wallet information (name, type, address).
- **transactions**: Records buy/sell transactions with references to assets and wallets.
- **portfolio_snapshots**: Stores JSONB snapshots of portfolio holdings.
- **asset_prices**: Tracks historical and current asset prices in USD.

Migrations are located in the `migrations/` directory and are applied automatically on container startup.

---

## 🧪 Development Workflow

### Makefile Commands 📜

| Command             | Description                                              |
|---------------------|----------------------------------------------------------|
| `make init-env COINMARKETCAP_API_KEY=your_key` | Initialize `.env` with your API key |
| `make setup`        | Set up Git hooks for SQLx query cache                    |
| `make check_prepare`| Check if SQLx query cache is up-to-date                 |
| `make prepare`      | Regenerate SQLx query cache                             |
| `make auto_prepare` | Regenerate and commit SQLx query cache                  |
| `make migrate`      | Run database migrations                                 |

### Git Hooks 🔗

The project includes a pre-commit hook (`.git-hooks/pre-commit`) that ensures the SQLx query cache (`.sqlx/` and `sqlx-data.json`) is up-to-date before each commit. To enable:

```bash
make setup
```

This hook:
1. Checks the SQLx query cache using `cargo sqlx prepare --check`.
2. Regenerates the cache with `cargo sqlx prepare` if outdated.
3. Commits the updated cache automatically.

### CI Pipeline 🚀

A GitHub Actions workflow (`.github/workflows/rust-ci.yml`) runs on every push to:
- Build the Rust project with `cargo build`.
- Run tests with `cargo test`.

---

## 🐳 Docker Setup

The `docker-compose.yml` file defines four services:

- **core_service**: The Rust API server, built from `core_service/Dockerfile`.
- **postgres**: PostgreSQL 17.5-alpine, with health checks and persistent data volume.
- **redis**: Redis 8-alpine, with health checks and persistent data volume.
- **pgadmin**: pgAdmin4 for database management.

All services are connected via a custom `austeris-network` bridge network.

---

## 🔍 Notes

- **SQLx Query Cache**: The `.sqlx/` directory contains cached queries for compile-time validation. Always keep it updated using `make prepare` or the pre-commit hook.
- **CoinMarketCap API Limits**: The API key must support the `/v1/cryptocurrency/listings/latest` and `/v2/cryptocurrency/quotes/latest` endpoints. Ensure your plan has sufficient credits.
- **Redis Caching**: Asset prices are cached for 1 hour to reduce API calls and improve performance.
- **Error Handling**: The API uses custom `AppError` responses with detailed JSON messages (e.g., validation errors, database failures).

---

## 📜 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for details.

---

## 🙌 Contributing

Contributions are welcome! Please:
1. Fork the repository.
2. Create a feature branch (`git checkout -b feature/your-feature`).
3. Commit your changes (`git commit -m "Add your feature"`).
4. Push to the branch (`git push origin feature/your-feature`).
5. Open a pull request.

Ensure you run `make setup` to enable Git hooks before committing.

---

## 📬 Contact

For questions or support, open an issue on the [GitHub repository](https://github.com/lacodda/austeris) or contact the maintainers.

Happy tracking! 🚀💰