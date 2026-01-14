# HyperCore Makefile
.PHONY: all build test clean devnet devnet-down seed fmt lint docs

# Default target
all: build

# ============ Build ============

build:
	cargo build --release

build-debug:
	cargo build

# Build specific crates
build-engine:
	cargo build -p hypercore-engine --release

build-node:
	cargo build -p hypercore-node --release

# Build contracts
build-contracts:
	cd contracts && forge build

# ============ Test ============

test:
	cargo test --workspace

test-verbose:
	cargo test --workspace -- --nocapture

test-engine:
	cargo test -p hypercore-engine

test-contracts:
	cd contracts && forge test

test-contracts-verbose:
	cd contracts && forge test -vvvv

# Property-based tests
test-proptest:
	cargo test --workspace -- --ignored proptest

# ============ Formatting & Linting ============

fmt:
	cargo fmt --all
	cd contracts && forge fmt

fmt-check:
	cargo fmt --all --check
	cd contracts && forge fmt --check

lint:
	cargo clippy --workspace -- -D warnings

# ============ Documentation ============

docs:
	cargo doc --workspace --no-deps --open

# ============ Benchmarks ============

bench:
	cargo bench -p hypercore-engine

bench-matching:
	cargo bench -p hypercore-engine -- matching

# ============ Devnet ============

devnet:
	docker compose up -d
	@echo "Waiting for services to start..."
	@sleep 5
	@echo "Seeding accounts..."
	@./scripts/seed-accounts.sh
	@echo "Devnet ready!"
	@echo "  - Gateway:    http://localhost:3000"
	@echo "  - WebSocket:  ws://localhost:3001"
	@echo "  - EVM RPC:    http://localhost:8545"
	@echo "  - CometBFT:   http://localhost:26657"
	@echo "  - Postgres:   localhost:5432"

devnet-down:
	docker compose down -v

devnet-logs:
	docker compose logs -f

devnet-logs-node:
	docker compose logs -f node

devnet-logs-gateway:
	docker compose logs -f gateway

devnet-restart:
	docker compose restart

devnet-rebuild:
	docker compose down -v
	docker compose build --no-cache
	docker compose up -d

# ============ Database ============

migrate:
	sqlx migrate run --source indexer-db/migrations

migrate-create:
	@read -p "Migration name: " name; \
	sqlx migrate add -r $$name --source indexer-db/migrations

# ============ Scripts ============

seed:
	./scripts/seed-accounts.sh

place-orders:
	./scripts/place-orders.sh

# ============ Docker ============

docker-build:
	docker build -f infra/docker/Dockerfile.node -t hypercore-node .
	docker build -f infra/docker/Dockerfile.gateway -t hypercore-gateway .
	docker build -f infra/docker/Dockerfile.indexer -t hypercore-indexer .

docker-push:
	docker push hypercore-node
	docker push hypercore-gateway
	docker push hypercore-indexer

# ============ CI ============

ci: fmt-check lint test test-contracts

# ============ Clean ============

clean:
	cargo clean
	cd contracts && forge clean
	rm -rf target/
	rm -rf contracts/out/
	rm -rf contracts/cache/

# ============ Help ============

help:
	@echo "HyperCore Build System"
	@echo ""
	@echo "Build:"
	@echo "  make build          - Build all crates (release)"
	@echo "  make build-debug    - Build all crates (debug)"
	@echo "  make build-contracts- Build Solidity contracts"
	@echo ""
	@echo "Test:"
	@echo "  make test           - Run all tests"
	@echo "  make test-engine    - Run engine tests"
	@echo "  make test-contracts - Run contract tests"
	@echo ""
	@echo "Devnet:"
	@echo "  make devnet         - Start local development network"
	@echo "  make devnet-down    - Stop devnet"
	@echo "  make devnet-logs    - View devnet logs"
	@echo ""
	@echo "Other:"
	@echo "  make fmt            - Format code"
	@echo "  make lint           - Run linter"
	@echo "  make docs           - Generate documentation"
	@echo "  make bench          - Run benchmarks"
	@echo "  make clean          - Clean build artifacts"
