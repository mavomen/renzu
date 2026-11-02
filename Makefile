SHELL := /bin/bash
.DEFAULT_GOAL := help

CARGO ?= cargo
DOCKER_COMPOSE ?= docker compose
RUST_TOOLCHAIN ?= 1.97.0

APP_BINS := gateway orders renzu
COMPOSE_FILE := compose.yaml

.PHONY: all check fmt fmt-fix clippy test build build-debug clean \
	run-gateway run-orders run-renzu \
	infra-up infra-down infra-logs infra-ps \
	e2e ci-local docker-build docker-build-% help

all: check ## Run formatting, clippy, and tests (default target)

check: fmt clippy test ## Run all standard verification checks

fmt: ## Check code formatting
	@$(CARGO) fmt --all -- --check

fmt-fix: ## Apply code formatting
	@$(CARGO) fmt --all

clippy: ## Run clippy lints
	@$(CARGO) clippy --workspace --all-targets -- -D warnings

test: ## Run workspace tests
	@$(CARGO) test --workspace

build: ## Build all workspace crates in release mode
	@$(CARGO) build --release --workspace

build-debug: ## Build all workspace crates in debug mode
	@$(CARGO) build --workspace

run-gateway: build-debug ## Run the gateway service (debug)
	@$(CARGO) run --bin gateway

run-orders: build-debug ## Run the orders service (debug)
	@$(CARGO) run --bin orders

run-renzu: build-debug ## Run the renzu CLI (debug)
	@$(CARGO) run --bin renzu

clean: ## Remove build artifacts
	@$(CARGO) clean

infra-up: ## Start local observability infrastructure
	@$(DOCKER_COMPOSE) -f $(COMPOSE_FILE) up -d

infra-down: ## Stop local observability infrastructure
	@$(DOCKER_COMPOSE) -f $(COMPOSE_FILE) down

infra-logs: ## Stream infrastructure logs
	@$(DOCKER_COMPOSE) -f $(COMPOSE_FILE) logs -f

infra-ps: ## Show infrastructure container status
	@$(DOCKER_COMPOSE) -f $(COMPOSE_FILE) ps

e2e: infra-up ## Run end-to-end trace visualization verification
	@./scripts/e2e-trace-visualization.sh

ci-local: ## Run local CI mirror (fmt, clippy, test)
	@./scripts/ci-local.sh

docker-build: $(addprefix docker-build-,$(APP_BINS)) ## Build all app Docker images

docker-build-gateway: ## Build gateway Docker image
	@docker build -t renzu-gateway:$(RUST_TOOLCHAIN) -f apps/gateway/Dockerfile .

docker-build-orders: ## Build orders Docker image
	@docker build -t renzu-orders:$(RUST_TOOLCHAIN) -f apps/orders/Dockerfile .

docker-build-renzu: ## Build renzu CLI Docker image
	@docker build -t renzu-renzu:$(RUST_TOOLCHAIN) -f apps/renzu/Dockerfile .

help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'
