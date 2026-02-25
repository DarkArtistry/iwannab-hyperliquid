#!/usr/bin/env bash
#
# HyperCore 5-Validator Comprehensive Multi-Node E2E Test Suite
#
# This script runs a full multi-node E2E test with real transactions:
#
# 1. Generates 5-validator genesis configuration
# 2. Starts a 5-node validator cluster (Docker)
# 3. Waits for all nodes to be healthy
# 4. Runs comprehensive TypeScript E2E tests:
#    - Connectivity verification (health, peers, validators)
#    - Transaction propagation (orders, leverage, cancels across nodes)
#    - EVM state sync (block numbers, balances, chain ID)
#    - Invalid transaction handling (rejection, no state corruption)
#    - Block progression over time (15s+ sustained)
#    - State consistency (appHash, clearinghouse, unified balances)
# 5. Reports results
#
# Usage:
#   ./scripts/e2e-multinode-full.sh              # Full test
#   ./scripts/e2e-multinode-full.sh --keep        # Keep cluster after tests
#   ./scripts/e2e-multinode-full.sh --verbose     # Verbose output
#
# Prerequisites:
#   - Docker and docker-compose
#   - Node.js 18+ with pnpm or npm
#   - curl, jq
#
# ============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
NC='\033[0m'

# Configuration
NUM_VALIDATORS=5
COMPOSE_FILE="docker-compose-multinode-5.yml"
KEEP_SERVICES=false
VERBOSE=false
NO_BUILD=false

# URLs (matching docker-compose-multinode-5.yml port scheme)
GATEWAY_URLS="http://localhost:3000,http://localhost:3010,http://localhost:3020,http://localhost:3030,http://localhost:3040"
EVM_RPC_URLS="http://localhost:8545,http://localhost:8555,http://localhost:8565,http://localhost:8575,http://localhost:8585"
COMETBFT_RPC_URLS="http://localhost:26657,http://localhost:26667,http://localhost:26677,http://localhost:26687,http://localhost:26697"

# ============================================================================
# PARSE ARGUMENTS
# ============================================================================

while [[ $# -gt 0 ]]; do
    case $1 in
        --keep)
            KEEP_SERVICES=true
            shift
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --no-build)
            NO_BUILD=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --keep         Keep Docker services running after tests"
            echo "  --verbose, -v  Extra verbose output"
            echo "  --no-build     Skip Docker image build (use existing images)"
            echo "  --help, -h     Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# ============================================================================
# HELPERS
# ============================================================================

print_header() {
    echo ""
    echo -e "${PURPLE}════════════════════════════════════════════════════════════════════${NC}"
    echo -e "${WHITE}  $1${NC}"
    echo -e "${PURPLE}════════════════════════════════════════════════════════════════════${NC}"
    echo ""
}

print_section() {
    echo ""
    echo -e "${CYAN}────────────────────────────────────────────────────────────────────${NC}"
    echo -e "${CYAN}  $1${NC}"
    echo -e "${CYAN}────────────────────────────────────────────────────────────────────${NC}"
}

print_step() {
    echo -e "${BLUE}▸${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_info() {
    echo -e "${CYAN}ℹ${NC} $1"
}

# ============================================================================
# CLEANUP
# ============================================================================

cleanup() {
    local exit_code=$?

    print_section "Cleanup"

    if [ "$KEEP_SERVICES" == true ]; then
        print_info "Keeping services running (--keep flag)"
        print_info "To stop: docker compose -f $COMPOSE_FILE down -v"
    else
        print_step "Stopping 5-validator cluster..."
        cd "$PROJECT_ROOT"
        docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
        print_success "Cluster stopped"
    fi

    exit $exit_code
}

trap cleanup EXIT

# ============================================================================
# PHASE 1: GENERATE GENESIS
# ============================================================================

generate_genesis() {
    print_section "Phase 1: Generating $NUM_VALIDATORS-Validator Genesis"

    cd "$PROJECT_ROOT"

    print_step "Generating configuration for $NUM_VALIDATORS validators..."

    chmod +x scripts/generate-multi-validator-genesis.sh
    if [ "$VERBOSE" == true ]; then
        ./scripts/generate-multi-validator-genesis.sh $NUM_VALIDATORS "hypercore-5v-test" 2>&1
    else
        ./scripts/generate-multi-validator-genesis.sh $NUM_VALIDATORS "hypercore-5v-test" 2>&1 | tail -5
    fi

    # Verify all validator dirs exist
    for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
        if [ ! -d "infra/multinode/validator-$i" ]; then
            print_error "Missing validator-$i configuration"
            exit 1
        fi
    done

    print_success "Genesis configuration generated for $NUM_VALIDATORS validators"
}

# ============================================================================
# PHASE 2: START CLUSTER
# ============================================================================

start_cluster() {
    print_section "Phase 2: Starting $NUM_VALIDATORS-Validator Cluster"

    cd "$PROJECT_ROOT"

    print_step "Stopping any existing cluster..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true

    if [ "$NO_BUILD" == true ]; then
        print_step "Skipping Docker build (--no-build)"
    else
        print_step "Building Docker images..."
        if [ "$VERBOSE" == true ]; then
            docker compose -f "$COMPOSE_FILE" build 2>&1
        else
            docker compose -f "$COMPOSE_FILE" build --quiet 2>&1
        fi
    fi

    print_step "Starting $NUM_VALIDATORS validator nodes..."
    docker compose -f "$COMPOSE_FILE" up -d

    print_success "Cluster started"

    echo ""
    docker compose -f "$COMPOSE_FILE" ps
    echo ""
}

# ============================================================================
# PHASE 3: WAIT FOR CLUSTER
# ============================================================================

wait_for_cluster() {
    print_section "Phase 3: Waiting for Cluster Health"

    local max_attempts=90
    local attempt=0

    # Wait for all CometBFT nodes
    while [ $attempt -lt $max_attempts ]; do
        local all_healthy=true

        for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
            local port=$((26657 + i * 10))
            local url="http://localhost:$port/status"

            if ! curl -sf "$url" >/dev/null 2>&1; then
                all_healthy=false
                break
            fi
        done

        if [ "$all_healthy" == true ]; then
            print_success "All CometBFT nodes are responding"
            break
        fi

        if [ $((attempt % 10)) -eq 0 ]; then
            print_step "Waiting for CometBFT nodes... ($((attempt + 1))/$max_attempts)"
        fi
        sleep 2
        attempt=$((attempt + 1))
    done

    if [ $attempt -ge $max_attempts ]; then
        print_error "Cluster failed to become healthy after $max_attempts attempts"
        docker compose -f "$COMPOSE_FILE" logs --tail=30
        exit 1
    fi

    # Wait for Gateway health on all nodes
    attempt=0
    while [ $attempt -lt 60 ]; do
        local all_gateways=true

        for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
            local port=$((3000 + i * 10))
            local url="http://localhost:$port/health"

            if ! curl -sf "$url" >/dev/null 2>&1; then
                all_gateways=false
                break
            fi
        done

        if [ "$all_gateways" == true ]; then
            print_success "All Gateway APIs are healthy"
            break
        fi

        if [ $((attempt % 10)) -eq 0 ]; then
            print_step "Waiting for Gateways... ($((attempt + 1))/60)"
        fi
        sleep 2
        attempt=$((attempt + 1))
    done

    if [ $attempt -ge 60 ]; then
        print_error "Gateways failed to start"
        docker compose -f "$COMPOSE_FILE" logs --tail=30
        exit 1
    fi

    # Wait for at least 3 blocks to be produced
    print_step "Waiting for initial blocks..."
    local blocks=0
    for i in $(seq 1 30); do
        blocks=$(curl -sf "http://localhost:26657/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')
        if [ "$blocks" -ge 3 ]; then
            break
        fi
        sleep 2
    done

    print_success "Cluster ready at height $blocks"
}

# ============================================================================
# PHASE 4: RUN TYPESCRIPT E2E TESTS
# ============================================================================

run_tests() {
    print_section "Phase 4: Running Multi-Node E2E Tests"

    cd "$PROJECT_ROOT/scripts/e2e"

    # Ensure dependencies are installed
    if [ ! -d "node_modules" ]; then
        print_step "Installing test dependencies..."
        npm install --quiet 2>&1 | tail -3
    fi

    # Run the multi-node test runner
    print_step "Running comprehensive multi-node tests against $NUM_VALIDATORS validators..."
    echo ""

    local output_file="/tmp/hypercore-multinode-full-e2e.log"

    GATEWAY_URLS="$GATEWAY_URLS" \
    EVM_RPC_URLS="$EVM_RPC_URLS" \
    COMETBFT_RPC_URLS="$COMETBFT_RPC_URLS" \
    VERBOSE="$VERBOSE" \
    npx tsx multinode-runner.ts 2>&1 | tee "$output_file"

    # Parse results
    local passed=$(grep -o '__TESTS_PASSED=[0-9]*' "$output_file" | cut -d= -f2)
    local failed=$(grep -o '__TESTS_FAILED=[0-9]*' "$output_file" | cut -d= -f2)

    passed=${passed:-0}
    failed=${failed:-0}

    echo ""
    print_section "Phase 4 Results"
    echo -e "  ${GREEN}Passed:${NC} $passed"
    echo -e "  ${RED}Failed:${NC} $failed"
    # Machine-readable marker for parent scripts (e.g., make test-all)
    echo "__TESTS_TOTAL=$((passed + failed))"

    if [ "$failed" -gt 0 ]; then
        return 1
    fi
    return 0
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    local start_time=$(date +%s)

    print_header "HyperCore 5-Validator Comprehensive E2E Test Suite"

    echo -e "  ${WHITE}Configuration:${NC}"
    echo -e "    Validators:     ${CYAN}$NUM_VALIDATORS${NC}"
    echo -e "    Compose File:   ${CYAN}$COMPOSE_FILE${NC}"
    echo -e "    Keep Services:  ${CYAN}$KEEP_SERVICES${NC}"
    echo -e "    Verbose:        ${CYAN}$VERBOSE${NC}"
    echo ""

    # Phase 1: Generate genesis
    generate_genesis

    # Phase 2: Start cluster
    start_cluster

    # Phase 3: Wait for health
    wait_for_cluster

    # Phase 4: Run tests
    if run_tests; then
        local end_time=$(date +%s)
        local duration=$((end_time - start_time))

        print_header "5-Validator E2E Test Complete"
        echo -e "  ${GREEN}╔════════════════════════════════════════════════╗${NC}"
        echo -e "  ${GREEN}║   ALL 5-VALIDATOR TESTS PASSED! ✓             ║${NC}"
        echo -e "  ${GREEN}╚════════════════════════════════════════════════╝${NC}"
        echo ""
        echo -e "  ${WHITE}Duration:${NC} ${duration}s"
        echo ""
        return 0
    else
        local end_time=$(date +%s)
        local duration=$((end_time - start_time))

        print_header "5-Validator E2E Test Complete"
        echo -e "  ${RED}╔════════════════════════════════════════════════╗${NC}"
        echo -e "  ${RED}║   SOME 5-VALIDATOR TESTS FAILED ✗             ║${NC}"
        echo -e "  ${RED}╚════════════════════════════════════════════════╝${NC}"
        echo ""
        echo -e "  ${WHITE}Duration:${NC} ${duration}s"
        echo ""
        return 1
    fi
}

main "$@"
