# Directory where the Rust service is located
SERVICE_DIR = core_service

# Load environment variables from .env
include .env.example
export

# Detect OS for compatibility (Linux vs macOS)
ifeq ($(shell uname), Darwin)
	SED_INPLACE = sed -i ''
else
	SED_INPLACE = sed -i.bak
endif

# Run sqlx prepare in check mode to verify that the query cache is up-to-date
check_prepare:
	cd $(SERVICE_DIR) && DATABASE_URL=$(DATABASE_URL_LOCALHOST) cargo sqlx prepare --check

# Update sqlx query cache
prepare:
	cd $(SERVICE_DIR) && DATABASE_URL=$(DATABASE_URL_LOCALHOST) cargo sqlx prepare

# Auto-prepare and commit the cache if it's outdated
auto_prepare:
	cd $(SERVICE_DIR) && DATABASE_URL=$(DATABASE_URL_LOCALHOST) cargo sqlx prepare
	git add $(SERVICE_DIR)/sqlx-data.json $(SERVICE_DIR)/.sqlx
	git commit -m "chore(sqlx): update query cache" || true

# Setup git hooks path and make them executable
setup:
	git config core.hooksPath .git-hooks
	chmod +x .git-hooks/*
	@echo "Git hooks path set to .git-hooks and all hooks made executable."
	@echo "Run 'make prepare' or just commit to trigger sqlx auto-caching"

# Initialize .env from .env.example and insert the API key
init-env:
ifndef COINMARKETCAP_API_KEY
	$(error Usage: make init-env COINMARKETCAP_API_KEY=your_api_key_here)
endif
	if [ ! -f .env ]; then \
		cp .env.example .env && \
		$(SED_INPLACE) "s|^COINMARKETCAP_API_KEY=.*|COINMARKETCAP_API_KEY=$(COINMARKETCAP_API_KEY)|" .env && \
		rm -f .env.bak; \
	else \
		echo ".env already exists, skipping initialization"; \
	fi

# Run database migrations
migrate:
	cd $(SERVICE_DIR) && DATABASE_URL=$(DATABASE_URL_LOCALHOST) cargo sqlx migrate run