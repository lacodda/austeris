# Austeris: Crypto Portfolio Tracker

## Getting Started

1. Install Docker Desktop
2. Clone repository:
   ```bash
   git clone https://github.com/yourusername/crypto-portfolio-tracker.git
   ```
3. Start services:
   ```bash
   docker-compose up -d
   ```
4. Run database migrations:
   ```bash
   sqlx migrate run --database-url postgres://portfolio:portfolio123@localhost:5432/portfolio
   ```
5. Start Core Service:
   ```bash
   cd core_service
   cargo run
   ```

API Endpoints:
- POST /transactions
- GET /portfolio
- Swagger UI: http://localhost:9000/swagger-ui/