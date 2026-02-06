# HyperCore Makefile
.PHONY: all build test clean devnet devnet-down seed fmt lint docs \
	test-multinode test-multinode-keep test-multinode-full \
	test-all all-tests docker-build-node

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
	cargo build -p hypercore-node --release --features cometbft

# Build contracts
build-contracts:
	cd contracts && forge build

# Build the node Docker image (used by all compose files)
docker-build-node:
	@echo "Building hypercore-node Docker image..."
	docker build -f infra/docker/Dockerfile.node -t hypercore-node .

# ============ Test ============

test:
	cargo test --workspace --features cometbft

test-verbose:
	cargo test --workspace --features cometbft -- --nocapture

test-engine:
	cargo test -p hypercore-engine

test-chain:
	cargo test -p hypercore-chain --features cometbft

test-gateway:
	cargo test -p hypercore-gateway

test-primitives:
	cargo test -p hypercore-primitives

test-contracts:
	cd contracts && forge test

test-contracts-verbose:
	cd contracts && forge test -vvvv

# Property-based tests
test-proptest:
	cargo test --workspace -- --ignored proptest

# Quick test: Rust + Solidity only (no Docker required)
test-quick:
	@echo "=========================================="
	@echo "  Quick Tests (Rust + Solidity - No Docker)"
	@echo "=========================================="
	@echo ""
	@echo ">>> [1/2] Rust Unit Tests (556 tests)"
	@echo "------------------------------------------"
	@cargo test --workspace --features cometbft 2>&1 | tail -30
	@echo ""
	@echo ">>> [2/2] Solidity Contract Tests (49 tests)"
	@echo "------------------------------------------"
	@cd contracts && forge test --summary
	@echo ""
	@echo "=========================================="
	@echo "  QUICK TESTS COMPLETE (605 tests)"
	@echo "=========================================="

# Run ALL tests (Rust + Solidity + E2E + Multi-Node)
# Builds Docker image ONCE, then reuses it for all E2E/multinode tests.
test-all:
	@echo "=========================================="
	@echo "  Running ALL Tests"
	@echo "=========================================="
	@echo ""
	@echo "Test Suite Breakdown:"
	@echo "  - Rust Unit Tests:           556 tests"
	@echo "  - Solidity Contracts:         49 tests"
	@echo "  - E2E Integration:           151 tests"
	@echo "  - Multi-Node E2E (3-node):    15 tests"
	@echo "  - Multi-Node Full (5-node):   52 tests"
	@echo "  - Total:                     823 tests"
	@echo ""
	@echo ">>> [1/6] Rust Unit Tests (cargo test)"
	@echo "------------------------------------------"
	cargo test --workspace --features cometbft -- --nocapture
	@echo ""
	@echo ">>> [2/6] Solidity Contract Tests (forge test)"
	@echo "------------------------------------------"
	cd contracts && forge test -vvv
	@echo ""
	@echo ">>> [3/6] Building Docker Image (one-time build)"
	@echo "------------------------------------------"
	docker build -f infra/docker/Dockerfile.node -t hypercore-node .
	@echo ""
	@echo ">>> [4/6] E2E Integration Tests (single-node)"
	@echo "------------------------------------------"
	./scripts/e2e-test.sh --verbose
	@echo ""
	@echo ">>> [5/6] Multi-Validator E2E Tests (3-node cluster)"
	@echo "------------------------------------------"
	./scripts/e2e-multinode.sh --verbose --no-build
	@echo ""
	@echo ">>> [6/6] Comprehensive 5-Validator E2E Tests"
	@echo "------------------------------------------"
	./scripts/e2e-multinode-full.sh --verbose --no-build
	@echo ""
	@echo "=========================================="
	@echo "  Test Results Summary"
	@echo "=========================================="
	@echo ""
	@echo "  [1/6] Rust Unit Tests:            556 tests"
	@echo "  [2/6] Solidity Contract Tests:      49 tests"
	@echo "  [3/6] Docker Image Build:           OK"
	@echo "  [4/6] E2E Integration (single):    151 tests"
	@echo "  [5/6] Multi-Node E2E (3-node):      15 tests"
	@echo "  [6/6] Multi-Node Full (5-node):     52 tests"
	@echo "  ────────────────────────────────────────"
	@echo "  Total:                             823 tests"
	@echo ""
	@echo "=========================================="
	@echo "  ALL TESTS COMPLETE (823 tests)"
	@echo "=========================================="

# Alias for test-all
all-tests: test-all

# Run E2E tests only (requires Docker)
test-e2e:
	./scripts/e2e-test.sh --verbose

# Run E2E tests with existing services (no Docker restart)
test-e2e-quick:
	./scripts/e2e-test.sh --no-docker --verbose

# Run multi-validator E2E tests (requires Docker)
test-multinode:
	./scripts/e2e-multinode.sh --verbose

# Run multi-validator tests and keep cluster running
test-multinode-keep:
	./scripts/e2e-multinode.sh --keep --verbose

# Run comprehensive 5-validator multi-node E2E tests (requires Docker)
test-multinode-full:
	./scripts/e2e-multinode-full.sh --verbose

# SDK Integration tests (requires services running)
test-sdk:
	cd sdk/typescript && pnpm test:integration

# Export/Import CLI tests
test-export-import:
	./scripts/test-export-import.sh

# Persistence layer tests
test-persistence:
	cargo test -p hypercore-persistence --lib

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
	@echo "  make build           - Build all crates (release)"
	@echo "  make build-debug     - Build all crates (debug)"
	@echo "  make build-node      - Build node with CometBFT support"
	@echo "  make build-contracts - Build Solidity contracts"
	@echo ""
	@echo "Test (823 total tests):"
	@echo "  make test              - Run Rust unit tests (556 tests, includes cometbft)"
	@echo "  make test-quick        - Run Rust + Solidity (605 tests, no Docker)"
	@echo "  make test-all          - Run ALL tests (823 tests, requires Docker)"
	@echo "  make all-tests         - Alias for test-all"
	@echo "  make test-engine       - Run engine tests only"
	@echo "  make test-chain        - Run chain/consensus tests only"
	@echo "  make test-gateway      - Run gateway tests only"
	@echo "  make test-primitives   - Run primitives tests only (61 tests)"
	@echo "  make test-persistence  - Run persistence tests (51 tests)"
	@echo "  make test-contracts    - Run Solidity tests (49 tests)"
	@echo "  make test-e2e          - Run E2E integration tests (151 tests)"
	@echo "  make test-e2e-quick    - Run E2E with existing services"
	@echo "  make test-multinode    - Run 3-node multi-validator E2E (15 tests)"
	@echo "  make test-multinode-full - Run 5-node comprehensive E2E (52 tests)"
	@echo "  make test-sdk          - Run SDK integration tests"
	@echo "  make test-export-import - Run export/import CLI tests"
	@echo ""
	@echo "Devnet:"
	@echo "  make devnet          - Start local development network"
	@echo "  make devnet-down     - Stop devnet"
	@echo "  make devnet-logs     - View devnet logs"
	@echo ""
	@echo "Docker:"
	@echo "  make docker-build-node - Build node Docker image"
	@echo "  make docker-build      - Build all Docker images"
	@echo ""
	@echo "Other:"
	@echo "  make fmt             - Format code"
	@echo "  make lint            - Run linter"
	@echo "  make docs            - Generate documentation"
	@echo "  make bench           - Run benchmarks"
	@echo "  make clean           - Clean build artifacts"
