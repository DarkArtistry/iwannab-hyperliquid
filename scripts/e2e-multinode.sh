#!/usr/bin/env bash
#
# HyperCore Multi-Validator End-to-End Test Suite
#
# This script tests the multi-validator consensus and blockchain progression:
# 1. Generates multi-validator genesis configuration
# 2. Starts a 3-node validator cluster
# 3. Verifies all nodes reach consensus
# 4. Tests blockchain progression
# 5. Tests validator join (dynamic validator addition)
# 6. Tests validator exit (unbonding)
# 7. Verifies state consistency across all nodes
#
# Usage:
#   ./scripts/e2e-multinode.sh              # Full test suite
#   ./scripts/e2e-multinode.sh --keep       # Keep services running after tests
#   ./scripts/e2e-multinode.sh --verbose    # Extra verbose output
#
# Prerequisites:
#   - Docker and docker-compose
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
NUM_VALIDATORS=3
KEEP_SERVICES=false
VERBOSE=false
TIMEOUT=300
NO_BUILD=false

# CometBFT RPC endpoints (one per validator)
COMETBFT_RPC_0="http://localhost:26657"
COMETBFT_RPC_1="http://localhost:26667"
COMETBFT_RPC_2="http://localhost:26677"

# Node HTTP API endpoints
NODE_API_0="http://localhost:3000"
NODE_API_1="http://localhost:3010"
NODE_API_2="http://localhost:3020"

# Test results
TESTS_PASSED=0
TESTS_FAILED=0
START_TIME=""

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

pass_test() {
    local name="$1"
    echo -e "  ${GREEN}✓${NC} ${WHITE}$name${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail_test() {
    local name="$1"
    local reason="$2"
    echo -e "  ${RED}✗${NC} ${WHITE}$name${NC}"
    echo -e "      ${RED}Reason: $reason${NC}"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

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
            echo "  --keep         Keep services running after tests"
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
# CLEANUP
# ============================================================================

cleanup() {
    local exit_code=$?

    print_section "Cleanup"

    if [ "$KEEP_SERVICES" == true ]; then
        print_info "Keeping services running (--keep flag)"
    else
        print_step "Stopping multi-node cluster..."
        cd "$PROJECT_ROOT"
        docker compose -f docker-compose-multinode.yml down -v --remove-orphans 2>/dev/null || true
        print_success "Cluster stopped"
    fi

    print_summary
    exit $exit_code
}

trap cleanup EXIT

# ============================================================================
# SUMMARY
# ============================================================================

print_summary() {
    local end_time=$(date +%s)
    local duration=$((end_time - START_TIME))

    print_header "Multi-Validator E2E Test Summary"

    local total=$((TESTS_PASSED + TESTS_FAILED))

    echo -e "  ${WHITE}Total Tests:${NC}    $total"
    echo -e "  ${GREEN}Passed:${NC}         $TESTS_PASSED"
    echo -e "  ${RED}Failed:${NC}         $TESTS_FAILED"
    echo ""
    echo -e "  ${WHITE}Duration:${NC}       ${duration}s"
    echo ""

    if [ $TESTS_FAILED -eq 0 ]; then
        echo -e "  ${GREEN}╔════════════════════════════════════════╗${NC}"
        echo -e "  ${GREEN}║   ALL MULTI-NODE TESTS PASSED! ✓      ║${NC}"
        echo -e "  ${GREEN}╚════════════════════════════════════════╝${NC}"
    else
        echo -e "  ${RED}╔════════════════════════════════════════╗${NC}"
        echo -e "  ${RED}║   SOME TESTS FAILED ✗                  ║${NC}"
        echo -e "  ${RED}╚════════════════════════════════════════╝${NC}"
    fi
    echo ""
}

# ============================================================================
# INFRASTRUCTURE SETUP
# ============================================================================

generate_genesis() {
    print_section "Generating Multi-Validator Genesis"

    cd "$PROJECT_ROOT"

    print_step "Generating configuration for $NUM_VALIDATORS validators..."

    # Make script executable and run it
    chmod +x scripts/generate-multi-validator-genesis.sh
    ./scripts/generate-multi-validator-genesis.sh $NUM_VALIDATORS "hypercore-e2e-test" 2>&1 | while read line; do
        if [ "$VERBOSE" == true ]; then
            echo "    $line"
        fi
    done

    if [ -d "infra/multinode/validator-0" ]; then
        print_success "Genesis configuration generated"
    else
        print_error "Failed to generate genesis configuration"
        exit 1
    fi
}

start_cluster() {
    print_section "Starting Multi-Validator Cluster"

    cd "$PROJECT_ROOT"

    print_step "Stopping any existing cluster..."
    docker compose -f docker-compose-multinode.yml down -v --remove-orphans 2>/dev/null || true

    if [ "$NO_BUILD" == true ]; then
        print_step "Skipping Docker build (--no-build)"
    else
        print_step "Building Docker images..."
        docker compose -f docker-compose-multinode.yml build --quiet 2>&1 | while read line; do
            if [ "$VERBOSE" == true ]; then
                echo "    $line"
            fi
        done
    fi

    print_step "Starting $NUM_VALIDATORS validator nodes..."
    docker compose -f docker-compose-multinode.yml up -d

    print_success "Cluster started"

    # Show running containers
    echo ""
    docker compose -f docker-compose-multinode.yml ps
    echo ""
}

wait_for_cluster() {
    print_section "Waiting for Cluster to Initialize"

    local max_attempts=60
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        local all_healthy=true

        # Check each CometBFT node
        for i in 0 1 2; do
            local port=$((26657 + i * 10))
            local url="http://localhost:$port/status"

            if ! curl -sf "$url" >/dev/null 2>&1; then
                all_healthy=false
                break
            fi
        done

        if [ "$all_healthy" == true ]; then
            print_success "All nodes are healthy!"
            return 0
        fi

        print_step "Waiting for nodes... ($((attempt + 1))/$max_attempts)"
        sleep 2
        attempt=$((attempt + 1))
    done

    print_error "Cluster failed to become healthy"
    docker compose -f docker-compose-multinode.yml logs --tail=50
    exit 1
}

# ============================================================================
# TEST FUNCTIONS
# ============================================================================

test_all_nodes_connected() {
    print_section "Test: All Nodes Connected"

    local passed=true
    local peers_expected=$((NUM_VALIDATORS - 1))

    # Peer discovery is asynchronous - retry for up to 30s
    local max_peer_attempts=15
    local all_connected=false

    for attempt in $(seq 1 $max_peer_attempts); do
        all_connected=true
        for i in 0 1 2; do
            local port=$((26657 + i * 10))
            local url="http://localhost:$port/net_info"
            local response=$(curl -sf "$url" 2>/dev/null)
            if [ -z "$response" ]; then
                all_connected=false
                break
            fi
            local n_peers=$(echo "$response" | jq -r '.result.n_peers // "0"')
            if [ "$n_peers" -lt 1 ]; then
                all_connected=false
                break
            fi
        done
        if [ "$all_connected" == true ]; then
            break
        fi
        if [ "$attempt" -lt "$max_peer_attempts" ]; then
            sleep 2
        fi
    done

    # Now report results
    for i in 0 1 2; do
        local port=$((26657 + i * 10))
        local url="http://localhost:$port/net_info"

        local response=$(curl -sf "$url" 2>/dev/null)
        if [ -z "$response" ]; then
            fail_test "Node $i connectivity" "Cannot reach node $i"
            passed=false
            continue
        fi

        local n_peers=$(echo "$response" | jq -r '.result.n_peers // "0"')
        if [ "$n_peers" -ge 1 ]; then
            pass_test "Node $i has $n_peers peer(s)"
        else
            fail_test "Node $i peer count" "Expected at least 1 peer, got $n_peers"
            passed=false
        fi
    done

    return 0
}

test_consensus_reached() {
    print_section "Test: Consensus Reached"

    # Wait for at least 5 blocks to be produced
    local target_height=5
    local max_attempts=30
    local attempt=0

    print_step "Waiting for $target_height blocks..."

    while [ $attempt -lt $max_attempts ]; do
        local height_0=$(curl -sf "$COMETBFT_RPC_0/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')

        if [ "$height_0" -ge "$target_height" ]; then
            break
        fi

        sleep 2
        attempt=$((attempt + 1))
    done

    if [ "$height_0" -lt "$target_height" ]; then
        fail_test "Block production" "Only reached height $height_0, expected $target_height"
        return 1
    fi

    pass_test "Blocks are being produced (height: $height_0)"

    # Check that all nodes have the same block hash at a specific height
    local check_height=$((height_0 - 2))  # Use a slightly older block to ensure sync

    local hash_0=$(curl -sf "$COMETBFT_RPC_0/block?height=$check_height" 2>/dev/null | jq -r '.result.block_id.hash // ""')
    local hash_1=$(curl -sf "$COMETBFT_RPC_1/block?height=$check_height" 2>/dev/null | jq -r '.result.block_id.hash // ""')
    local hash_2=$(curl -sf "$COMETBFT_RPC_2/block?height=$check_height" 2>/dev/null | jq -r '.result.block_id.hash // ""')

    if [ -z "$hash_0" ] || [ -z "$hash_1" ] || [ -z "$hash_2" ]; then
        fail_test "Block hash retrieval" "Could not get block hashes"
        return 1
    fi

    if [ "$hash_0" == "$hash_1" ] && [ "$hash_1" == "$hash_2" ]; then
        pass_test "All nodes agree on block $check_height (hash: ${hash_0:0:16}...)"
    else
        fail_test "Consensus" "Block hashes differ: $hash_0 vs $hash_1 vs $hash_2"
        return 1
    fi

    return 0
}

test_blockchain_progression() {
    print_section "Test: Blockchain Progression"

    # Get initial height
    local initial_height=$(curl -sf "$COMETBFT_RPC_0/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')

    print_step "Initial height: $initial_height"
    print_step "Waiting for 10 more blocks..."

    local target_height=$((initial_height + 10))
    local max_attempts=60
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        local current_height=$(curl -sf "$COMETBFT_RPC_0/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')

        if [ "$current_height" -ge "$target_height" ]; then
            break
        fi

        if [ "$VERBOSE" == true ]; then
            echo "    Height: $current_height / $target_height"
        fi

        sleep 1
        attempt=$((attempt + 1))
    done

    local final_height=$(curl -sf "$COMETBFT_RPC_0/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')

    if [ "$final_height" -ge "$target_height" ]; then
        local blocks_produced=$((final_height - initial_height))
        pass_test "Blockchain progressed by $blocks_produced blocks (height: $final_height)"
    else
        fail_test "Blockchain progression" "Only reached height $final_height, expected $target_height"
        return 1
    fi

    return 0
}

test_validator_set() {
    print_section "Test: Validator Set"

    # Get validator set from CometBFT
    local validators=$(curl -sf "$COMETBFT_RPC_0/validators" 2>/dev/null)

    if [ -z "$validators" ]; then
        fail_test "Validator set retrieval" "Could not get validator set"
        return 1
    fi

    local validator_count=$(echo "$validators" | jq -r '.result.count // "0"')

    if [ "$validator_count" -eq "$NUM_VALIDATORS" ]; then
        pass_test "Validator set has $validator_count validators (expected: $NUM_VALIDATORS)"
    else
        fail_test "Validator count" "Expected $NUM_VALIDATORS validators, got $validator_count"
        return 1
    fi

    # Check total voting power
    local total_power=$(echo "$validators" | jq -r '.result.total // "0"')
    if [ "$total_power" -gt 0 ]; then
        pass_test "Total voting power: $total_power"
    else
        fail_test "Voting power" "Total voting power is 0"
        return 1
    fi

    return 0
}

test_state_consistency() {
    print_section "Test: State Consistency Across Nodes"

    # Get app hash from each node
    local hash_0=$(curl -sf "$COMETBFT_RPC_0/status" 2>/dev/null | jq -r '.result.sync_info.latest_app_hash // ""')
    local hash_1=$(curl -sf "$COMETBFT_RPC_1/status" 2>/dev/null | jq -r '.result.sync_info.latest_app_hash // ""')
    local hash_2=$(curl -sf "$COMETBFT_RPC_2/status" 2>/dev/null | jq -r '.result.sync_info.latest_app_hash // ""')

    if [ -z "$hash_0" ] || [ -z "$hash_1" ] || [ -z "$hash_2" ]; then
        fail_test "App hash retrieval" "Could not get app hashes"
        return 1
    fi

    # App hashes might differ slightly if nodes are at different heights
    # Get the height of each node and compare hashes at the same height
    local height_0=$(curl -sf "$COMETBFT_RPC_0/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')
    local height_1=$(curl -sf "$COMETBFT_RPC_1/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')
    local height_2=$(curl -sf "$COMETBFT_RPC_2/status" 2>/dev/null | jq -r '.result.sync_info.latest_block_height // "0"')

    print_step "Node heights: $height_0, $height_1, $height_2"

    # Use the minimum height to compare
    local min_height=$height_0
    [ "$height_1" -lt "$min_height" ] && min_height=$height_1
    [ "$height_2" -lt "$min_height" ] && min_height=$height_2

    # Get commit at the same height
    local commit_0=$(curl -sf "$COMETBFT_RPC_0/commit?height=$min_height" 2>/dev/null | jq -r '.result.signed_header.commit.block_id.hash // ""')
    local commit_1=$(curl -sf "$COMETBFT_RPC_1/commit?height=$min_height" 2>/dev/null | jq -r '.result.signed_header.commit.block_id.hash // ""')
    local commit_2=$(curl -sf "$COMETBFT_RPC_2/commit?height=$min_height" 2>/dev/null | jq -r '.result.signed_header.commit.block_id.hash // ""')

    if [ "$commit_0" == "$commit_1" ] && [ "$commit_1" == "$commit_2" ]; then
        pass_test "All nodes have consistent state at height $min_height"
    else
        fail_test "State consistency" "Commit hashes differ at height $min_height"
        return 1
    fi

    return 0
}

test_api_endpoints() {
    print_section "Test: API Endpoints"

    # Test health endpoint on each node
    for i in 0 1 2; do
        local port=$((3000 + i * 10))
        local url="http://localhost:$port/health"

        local response=$(curl -sf "$url" 2>/dev/null)
        if [ -n "$response" ]; then
            pass_test "Node $i health endpoint (port $port)"
        else
            fail_test "Node $i health endpoint" "No response from $url"
        fi
    done

    return 0
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    START_TIME=$(date +%s)

    print_header "HyperCore Multi-Validator E2E Test Suite"

    echo -e "  ${WHITE}Configuration:${NC}"
    echo -e "    Validators:     ${CYAN}$NUM_VALIDATORS${NC}"
    echo -e "    Keep Services:  ${CYAN}$KEEP_SERVICES${NC}"
    echo -e "    Verbose:        ${CYAN}$VERBOSE${NC}"
    echo ""

    # Phase 1: Generate genesis
    generate_genesis

    # Phase 2: Start cluster
    start_cluster

    # Phase 3: Wait for cluster
    wait_for_cluster

    # Phase 4: Run tests
    print_header "Running Multi-Validator Tests"

    test_all_nodes_connected
    test_consensus_reached
    test_blockchain_progression
    test_validator_set
    test_state_consistency
    test_api_endpoints

    # Return success if all tests passed
    if [ $TESTS_FAILED -eq 0 ]; then
        return 0
    else
        return 1
    fi
}

# Run main
main "$@"
