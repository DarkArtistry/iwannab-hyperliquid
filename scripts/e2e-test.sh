#!/usr/bin/env bash
#
# HyperCore End-to-End Integration Test Suite
#
# This script provides a complete E2E testing workflow:
# 1. Stops any existing services
# 2. Starts fresh Docker environment
# 3. Waits for all services to be healthy
# 4. Deploys contracts (if needed)
# 5. Runs comprehensive integration tests
# 6. Displays detailed results and summary
# 7. Cleans up on exit
#
# Usage:
#   ./scripts/e2e-test.sh              # Full run with Docker
#   ./scripts/e2e-test.sh --no-docker  # Skip Docker, use existing services
#   ./scripts/e2e-test.sh --keep       # Keep services running after tests
#   ./scripts/e2e-test.sh --verbose    # Extra verbose output
#
# Environment Variables:
#   GATEWAY_URL     - Override gateway endpoint (default: http://localhost:3000)
#   CHAIN_ID        - Override chain ID (default: 1337)
#   SKIP_CONTRACTS  - Skip contract deployment (default: false)
#
# ============================================================================
# ARCHITECTURE OVERVIEW
# ============================================================================
#
# HyperCore runs two main services in Docker:
#   1. Gateway (port 3000) - REST/WebSocket API for trading
#   2. Node (port 8545)    - EVM JSON-RPC endpoint (Ethereum-compatible)
#
# The gateway provides HyperCore-specific APIs:
#   - POST /info     - Read-only queries (orderbook, balances, positions)
#   - POST /exchange - Signed actions (orders, cancels, transfers)
#   - GET /health    - Health check endpoint
#
# The EVM RPC provides standard Ethereum JSON-RPC:
#   - eth_* methods for transactions, blocks, accounts
#   - Custom precompiles at 0x0800-0x0808 for reading exchange state
#
# Test accounts use deterministic private keys from Foundry/Anvil, which
# are pre-funded by the node's initialization code. See runner.ts for
# account details and how balances are credited.
#

set -e

# ============================================================================
# CONFIGURATION
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
NC='\033[0m' # No Color

# Defaults
USE_DOCKER=true
KEEP_SERVICES=false
VERBOSE=false

# Gateway URL - HyperCore API endpoint for trading operations
# Port 3000 is the default gateway port, matching Hyperliquid's API format
GATEWAY_URL="${GATEWAY_URL:-http://localhost:3000}"

# EVM RPC URL - Ethereum-compatible JSON-RPC endpoint
# Port 8545 is the standard Ethereum RPC port (same as Geth, Anvil, etc.)
# Used for deploying contracts and EVM interactions
EVM_RPC_URL="${EVM_RPC_URL:-http://localhost:8545}"

# Chain ID - Network identifier for EIP-155 transaction signing
# 1337 is the default local development chain ID (used by Anvil/Hardhat)
# This MUST match the chain ID configured in the node, or signatures will fail
CHAIN_ID="${CHAIN_ID:-1337}"

SKIP_CONTRACTS="${SKIP_CONTRACTS:-false}"

# Timeouts for waiting on service health
DOCKER_TIMEOUT=120
HEALTH_CHECK_INTERVAL=2
MAX_HEALTH_CHECKS=60

# Test results
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0
START_TIME=""
END_TIME=""

# ============================================================================
# HELPER FUNCTIONS
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

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_info() {
    echo -e "${CYAN}ℹ${NC} $1"
}

print_test_result() {
    local name="$1"
    local status="$2"
    local duration="$3"
    local description="$4"

    if [ "$status" == "pass" ]; then
        echo -e "  ${GREEN}✓${NC} ${WHITE}$name${NC} ${CYAN}($duration)${NC}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    elif [ "$status" == "fail" ]; then
        echo -e "  ${RED}✗${NC} ${WHITE}$name${NC} ${RED}($duration)${NC}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    else
        echo -e "  ${YELLOW}○${NC} ${WHITE}$name${NC} ${YELLOW}(skipped)${NC}"
        TESTS_SKIPPED=$((TESTS_SKIPPED + 1))
    fi

    if [ -n "$description" ] && [ "$VERBOSE" == true ]; then
        echo -e "      ${CYAN}$description${NC}"
    fi
}

# ============================================================================
# PARSE ARGUMENTS
# ============================================================================

while [[ $# -gt 0 ]]; do
    case $1 in
        --no-docker)
            USE_DOCKER=false
            shift
            ;;
        --keep)
            KEEP_SERVICES=true
            shift
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --no-docker    Skip Docker setup, use existing services"
            echo "  --keep         Keep services running after tests"
            echo "  --verbose, -v  Extra verbose output"
            echo "  --help, -h     Show this help message"
            echo ""
            echo "Environment Variables:"
            echo "  GATEWAY_URL     Override gateway endpoint"
            echo "  EVM_RPC_URL     Override EVM RPC endpoint"
            echo "  CHAIN_ID        Override chain ID"
            echo "  SKIP_CONTRACTS  Skip contract deployment"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# ============================================================================
# CLEANUP HANDLER
# ============================================================================

cleanup() {
    local exit_code=$?

    print_section "Cleanup"

    if [ "$KEEP_SERVICES" == true ]; then
        print_info "Keeping services running (--keep flag)"
    elif [ "$USE_DOCKER" == true ]; then
        print_step "Stopping Docker services..."
        cd "$PROJECT_ROOT"
        docker-compose down -v --remove-orphans 2>/dev/null || true
        print_success "Services stopped"
    fi

    # Print final summary
    print_summary

    exit $exit_code
}

trap cleanup EXIT

# ============================================================================
# DOCKER MANAGEMENT
# ============================================================================

stop_existing_services() {
    print_section "Stopping Existing Services"

    cd "$PROJECT_ROOT"

    print_step "Stopping Docker containers..."
    docker-compose down -v --remove-orphans 2>/dev/null || true

    # Also kill any local processes that might be using our ports
    print_step "Checking for processes on required ports..."

    # Port assignments in HyperCore:
    # -------------------------------------------------------------------------
    # 3000  - Gateway HTTP API (REST + WebSocket for trading)
    # 3001  - Gateway alternate port (reserved for future use)
    # 5432  - PostgreSQL database (for indexer storage)
    # 8545  - EVM JSON-RPC (Ethereum-compatible API)
    # 8546  - EVM WebSocket RPC (reserved for future use)
    # 26656 - CometBFT P2P port (consensus node communication)
    # 26657 - CometBFT RPC port (consensus queries)
    # 26658 - CometBFT ABCI port (app <-> consensus communication)
    # -------------------------------------------------------------------------
    local ports=(3000 3001 5432 8545 8546 26656 26657 26658)
    for port in "${ports[@]}"; do
        local pid=$(lsof -ti:$port 2>/dev/null || true)
        if [ -n "$pid" ]; then
            print_warning "Killing process on port $port (PID: $pid)"
            kill -9 $pid 2>/dev/null || true
        fi
    done

    # Wait a moment for ports to be released
    sleep 2

    print_success "Existing services stopped"
}

start_docker_services() {
    print_section "Starting Docker Services"

    cd "$PROJECT_ROOT"

    print_step "Building Docker images..."
    docker-compose build --quiet 2>&1 | while read line; do
        if [ "$VERBOSE" == true ]; then
            echo "    $line"
        fi
    done

    print_step "Starting containers..."
    docker-compose up -d

    print_success "Docker containers started"

    # List running containers
    echo ""
    docker-compose ps
    echo ""
}

wait_for_services() {
    print_section "Waiting for Services"

    # IMPORTANT: For Phase 2A Unified State, both the Gateway API (port 3000) and
    # EVM RPC (port 8545) run in the same node process. This ensures they share
    # the same unified state for consistent balance views between HyperCore and EVM.

    local checks=0
    local all_healthy=false

    while [ $checks -lt $MAX_HEALTH_CHECKS ]; do
        all_healthy=true

        # Check PostgreSQL
        if ! docker-compose exec -T postgres pg_isready -U hypercore >/dev/null 2>&1; then
            all_healthy=false
            print_step "Waiting for PostgreSQL... ($checks/$MAX_HEALTH_CHECKS)"
        fi

        # Check Gateway (runs in node process for shared unified state)
        if ! curl -sf "$GATEWAY_URL/health" >/dev/null 2>&1; then
            all_healthy=false
            print_step "Waiting for Gateway (node)... ($checks/$MAX_HEALTH_CHECKS)"
        fi

        # Check EVM RPC (runs in same node process)
        if ! curl -sf -X POST -H "Content-Type: application/json" \
             -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
             "$EVM_RPC_URL" >/dev/null 2>&1; then
            all_healthy=false
            print_step "Waiting for EVM RPC (node)... ($checks/$MAX_HEALTH_CHECKS)"
        fi

        if [ "$all_healthy" == true ]; then
            break
        fi

        sleep $HEALTH_CHECK_INTERVAL
        checks=$((checks + 1))
    done

    if [ "$all_healthy" == true ]; then
        print_success "All services are healthy!"
        print_info "Gateway and EVM RPC share unified state (same process)"
    else
        print_error "Services failed to become healthy within timeout"
        docker-compose logs --tail=50
        exit 1
    fi
}

# ============================================================================
# CONTRACT DEPLOYMENT
# ============================================================================

deploy_contracts() {
    if [ "$SKIP_CONTRACTS" == "true" ]; then
        print_info "Skipping contract deployment (SKIP_CONTRACTS=true)"
        return 0
    fi

    print_section "Deploying Contracts"

    cd "$PROJECT_ROOT/contracts"

    # Check if Foundry is installed
    if ! command -v forge &> /dev/null; then
        print_warning "Foundry not installed, skipping contract deployment"
        return 0
    fi

    print_step "Building contracts..."
    forge build --quiet

    print_step "Deploying to local EVM..."

    # =========================================================================
    # PRIVATE KEY EXPLANATION
    # =========================================================================
    # This private key (0xac0974...) is the FIRST deterministic test account
    # from Foundry/Anvil. It is a well-known test key used across the Ethereum
    # development ecosystem:
    #
    #   Private Key: 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
    #   Address:     0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
    #   Alias:       "Alice" in our test suite
    #
    # HOW IT GETS FUNDED:
    # The HyperCore node initializes test accounts with balances on startup.
    # See crates/gateway/src/main.rs:initialize_spot_markets() which credits:
    #   - 100,000 USDC (token index 0) for spot trading
    #   - 10,000 TEST tokens (token index 1) for testing
    #
    # For EVM/native ETH balance, the node credits accounts in the EVM state
    # during initialization (see crates/node/src/main.rs).
    #
    # WARNING: NEVER use this key on mainnet! It is public and has no security.
    # =========================================================================
    forge script script/Deploy.s.sol \
        --rpc-url "$EVM_RPC_URL" \
        --broadcast \
        --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
        --tc DeployScript \
        2>&1 | while read line; do
            if [ "$VERBOSE" == true ]; then
                echo "    $line"
            fi
        done

    print_success "Contracts deployed"
}

# ============================================================================
# TEST EXECUTION
# ============================================================================

run_e2e_tests() {
    print_section "Running E2E Integration Tests"

    cd "$PROJECT_ROOT/scripts/e2e"

    # Check if node_modules exists
    if [ ! -d "node_modules" ]; then
        print_step "Installing dependencies..."
        npm install --silent 2>/dev/null || npm install
    fi

    print_step "Starting test execution..."
    echo ""

    # Create temp file for output
    local output_file=$(mktemp)

    # Run the TypeScript E2E test runner
    GATEWAY_URL="$GATEWAY_URL" \
    EVM_RPC_URL="$EVM_RPC_URL" \
    CHAIN_ID="$CHAIN_ID" \
    VERBOSE="$VERBOSE" \
    npx tsx runner.ts 2>&1 | tee "$output_file"

    local test_exit_code=${PIPESTATUS[0]}

    # Parse results from output
    TESTS_PASSED=$(grep -o '__TESTS_PASSED=[0-9]*' "$output_file" | cut -d= -f2 || echo "0")
    TESTS_FAILED=$(grep -o '__TESTS_FAILED=[0-9]*' "$output_file" | cut -d= -f2 || echo "0")
    TESTS_SKIPPED=$(grep -o '__TESTS_SKIPPED=[0-9]*' "$output_file" | cut -d= -f2 || echo "0")

    # Clean up
    rm -f "$output_file"

    return $test_exit_code
}

# ============================================================================
# SUMMARY
# ============================================================================

print_summary() {
    END_TIME=$(date +%s)
    local duration=$((END_TIME - START_TIME))

    print_header "E2E Test Summary"

    local total=$((TESTS_PASSED + TESTS_FAILED + TESTS_SKIPPED))

    echo -e "  ${WHITE}Total Tests:${NC}    $total"
    echo -e "  ${GREEN}Passed:${NC}         $TESTS_PASSED"
    echo -e "  ${RED}Failed:${NC}         $TESTS_FAILED"
    echo -e "  ${YELLOW}Skipped:${NC}        $TESTS_SKIPPED"
    echo ""
    echo -e "  ${WHITE}Duration:${NC}       ${duration}s"
    echo ""

    if [ $TESTS_FAILED -eq 0 ]; then
        echo -e "  ${GREEN}╔════════════════════════════════════╗${NC}"
        echo -e "  ${GREEN}║     ALL TESTS PASSED! ✓           ║${NC}"
        echo -e "  ${GREEN}╚════════════════════════════════════╝${NC}"
    else
        echo -e "  ${RED}╔════════════════════════════════════╗${NC}"
        echo -e "  ${RED}║     SOME TESTS FAILED ✗            ║${NC}"
        echo -e "  ${RED}╚════════════════════════════════════╝${NC}"
    fi
    echo ""
    # Machine-readable marker for parent scripts (e.g., make test-all)
    echo "__TESTS_TOTAL=$total"
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    START_TIME=$(date +%s)

    print_header "HyperCore E2E Integration Test Suite"

    echo -e "  ${WHITE}Configuration:${NC}"
    echo -e "    Gateway URL:    ${CYAN}$GATEWAY_URL${NC}"
    echo -e "    EVM RPC URL:    ${CYAN}$EVM_RPC_URL${NC}"
    echo -e "    Chain ID:       ${CYAN}$CHAIN_ID${NC}"
    echo -e "    Use Docker:     ${CYAN}$USE_DOCKER${NC}"
    echo -e "    Keep Services:  ${CYAN}$KEEP_SERVICES${NC}"
    echo -e "    Verbose:        ${CYAN}$VERBOSE${NC}"
    echo ""

    # Phase 1: Stop existing services
    if [ "$USE_DOCKER" == true ]; then
        stop_existing_services
    fi

    # Phase 2: Start Docker services
    if [ "$USE_DOCKER" == true ]; then
        start_docker_services
        wait_for_services
    fi

    # Phase 3: Deploy contracts
    deploy_contracts

    # Phase 4: Run E2E tests
    run_e2e_tests
}

# Run main function
main "$@"
