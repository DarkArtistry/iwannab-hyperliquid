#!/bin/bash
# Generate multi-validator genesis configuration for HyperCore
#
# This script generates:
# 1. Validator keys (Ed25519) for each validator
# 2. Node keys for P2P identification
# 3. Genesis.json with all validators
# 4. Config files for each validator
# 5. Docker compose template for multi-node setup

set -euo pipefail

# Configuration
NUM_VALIDATORS=${1:-3}
CHAIN_ID=${2:-"hypercore-multinode"}
BASE_DIR="./infra/multinode"
GENESIS_TIME=$(date -u +"%Y-%m-%dT%H:%M:%S.000000000Z")

echo "=== HyperCore Multi-Validator Genesis Generator ==="
echo "Validators: $NUM_VALIDATORS"
echo "Chain ID: $CHAIN_ID"
echo "Output: $BASE_DIR"
echo ""

# Create base directory
rm -rf "$BASE_DIR"
mkdir -p "$BASE_DIR"

# Function to generate Ed25519 key pair
generate_validator_key() {
    local validator_id=$1
    local key_dir="$BASE_DIR/validator-$validator_id"
    mkdir -p "$key_dir/config"
    mkdir -p "$key_dir/data"

    # Use CometBFT to generate proper Ed25519 keys.
    # Priority: native binary > Docker image
    if command -v cometbft &> /dev/null; then
        echo "Using CometBFT binary for validator-$validator_id..."
        cd "$key_dir"
        cometbft init --home . 2>/dev/null || true
        cd - > /dev/null
    elif command -v docker &> /dev/null; then
        echo "Using CometBFT Docker image for validator-$validator_id..."
        local abs_key_dir
        abs_key_dir="$(cd "$(dirname "$key_dir")" && pwd)/$(basename "$key_dir")"
        docker run --rm \
            --user "$(id -u):$(id -g)" \
            -v "$abs_key_dir:/cometbft" \
            cometbft/cometbft:v0.38.5 \
            init --home /cometbft 2>/dev/null || true
    else
        echo "ERROR: Neither cometbft binary nor docker is available."
        echo "Install CometBFT or Docker to generate validator keys."
        exit 1
    fi

    # Remove the auto-generated genesis.json — we build our own multi-validator genesis
    rm -f "$key_dir/config/genesis.json"

    # Create priv_validator_state.json if not present (needed by CometBFT)
    if [ ! -f "$key_dir/data/priv_validator_state.json" ]; then
        cat > "$key_dir/data/priv_validator_state.json" << EOF
{
  "height": "0",
  "round": 0,
  "step": 0
}
EOF
    fi

    echo "  Generated keys for validator-$validator_id"
}

# Generate all validator keys
echo "Generating validator keys..."
for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
    generate_validator_key $i
done

# Extract node IDs for persistent_peers (requires node_key.json from cometbft init)
echo "Extracting node IDs..."
for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
    key_dir="$BASE_DIR/validator-$i"
    abs_key_dir="$(cd "$(dirname "$key_dir")" && pwd)/$(basename "$key_dir")"

    if command -v cometbft &> /dev/null; then
        NODE_ID=$(cd "$key_dir" && cometbft show-node-id --home . 2>/dev/null || echo "")
    elif command -v docker &> /dev/null; then
        NODE_ID=$(docker run --rm \
            -v "$abs_key_dir:/cometbft" \
            cometbft/cometbft:v0.38.5 \
            show-node-id --home /cometbft 2>/dev/null || echo "")
    else
        NODE_ID=""
    fi

    if [ -n "$NODE_ID" ]; then
        echo "$NODE_ID" > "$key_dir/node_id"
        echo "  validator-$i node ID: $NODE_ID"
    else
        echo "  WARNING: Could not extract node ID for validator-$i"
    fi
done

# Build validators array for genesis
echo "Building genesis configuration..."
VALIDATORS_JSON=""
for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
    key_dir="$BASE_DIR/validator-$i"

    if [ -f "$key_dir/config/priv_validator_key.json" ]; then
        ADDRESS=$(jq -r '.address' "$key_dir/config/priv_validator_key.json")
        PUB_KEY=$(jq -r '.pub_key.value' "$key_dir/config/priv_validator_key.json")
    else
        ADDRESS=$(cat "$key_dir/address")
        PUB_KEY=$(cat "$key_dir/pubkey")
    fi

    # Equal voting power for all validators
    POWER=10

    VALIDATOR_ENTRY=$(cat << EOF
    {
      "address": "$ADDRESS",
      "pub_key": {
        "type": "tendermint/PubKeyEd25519",
        "value": "$PUB_KEY"
      },
      "power": "$POWER",
      "name": "validator-$i"
    }
EOF
)

    if [ -n "$VALIDATORS_JSON" ]; then
        VALIDATORS_JSON="${VALIDATORS_JSON},
${VALIDATOR_ENTRY}"
    else
        VALIDATORS_JSON="$VALIDATOR_ENTRY"
    fi
done

# Create genesis.json
cat > "$BASE_DIR/genesis.json" << EOF
{
  "genesis_time": "$GENESIS_TIME",
  "chain_id": "$CHAIN_ID",
  "initial_height": "1",
  "consensus_params": {
    "block": {
      "max_bytes": "22020096",
      "max_gas": "-1"
    },
    "evidence": {
      "max_age_num_blocks": "100000",
      "max_age_duration": "172800000000000",
      "max_bytes": "1048576"
    },
    "validator": {
      "pub_key_types": ["ed25519"]
    },
    "version": {
      "app": "0"
    },
    "abci": {
      "vote_extensions_enable_height": "0"
    }
  },
  "validators": [
$VALIDATORS_JSON
  ],
  "app_hash": "",
  "app_state": {
    "chain_id": "$CHAIN_ID",
    "markets": [
      {
        "id": 0,
        "symbol": "BTC-PERP",
        "max_leverage": 50,
        "initial_mark_price": "65000"
      },
      {
        "id": 1,
        "symbol": "ETH-PERP",
        "max_leverage": 50,
        "initial_mark_price": "3500"
      }
    ],
    "spot_tokens": [
      {
        "index": 1,
        "name": "Test Token",
        "symbol": "TEST",
        "wei_decimals": 18,
        "sz_decimals": 4,
        "max_supply": "1000000000"
      }
    ],
    "balances": [
      {
        "address": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
        "token": 0,
        "amount": "100000000000000",
        "view": "core"
      },
      {
        "address": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        "token": 0,
        "amount": "100000000000000",
        "view": "core"
      },
      {
        "address": "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC",
        "token": 0,
        "amount": "100000000000000",
        "view": "core"
      },
      {
        "address": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
        "token": 1,
        "amount": "10000",
        "view": "core"
      },
      {
        "address": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        "token": 1,
        "amount": "10000",
        "view": "core"
      },
      {
        "address": "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC",
        "token": 1,
        "amount": "10000",
        "view": "core"
      },
      {
        "address": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
        "token": 0,
        "amount": "100000000000000",
        "view": "evm"
      },
      {
        "address": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        "token": 0,
        "amount": "100000000000000",
        "view": "evm"
      },
      {
        "address": "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC",
        "token": 0,
        "amount": "100000000000000",
        "view": "evm"
      }
    ],
    "validators": [
$VALIDATORS_JSON
    ]
  }
}
EOF

# Copy genesis to all validators
for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
    cp "$BASE_DIR/genesis.json" "$BASE_DIR/validator-$i/config/genesis.json"
done

# Generate config.toml for each validator
echo "Generating CometBFT config files..."
for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
    key_dir="$BASE_DIR/validator-$i"

    # All CometBFT containers use STANDARD internal ports.
    # Docker compose handles mapping to unique host ports.
    # P2P=26656, RPC=26657, ABCI=26658, Metrics=26660 (same for all validators)
    INTERNAL_P2P_PORT=26656
    INTERNAL_RPC_PORT=26657
    INTERNAL_ABCI_PORT=26658
    INTERNAL_METRICS_PORT=26660

    # Build persistent_peers list (all other validators)
    # CometBFT format: node_id@host:port
    PERSISTENT_PEERS=""
    for j in $(seq 0 $((NUM_VALIDATORS - 1))); do
        if [ $j -ne $i ]; then
            PEER_HOST="cometbft-$j:$INTERNAL_P2P_PORT"

            # Include node ID if available (required for proper peer identification)
            if [ -f "$BASE_DIR/validator-$j/node_id" ]; then
                PEER_ID=$(cat "$BASE_DIR/validator-$j/node_id")
                PEER_ADDR="${PEER_ID}@${PEER_HOST}"
            else
                PEER_ADDR="$PEER_HOST"
            fi

            if [ -n "$PERSISTENT_PEERS" ]; then
                PERSISTENT_PEERS="${PERSISTENT_PEERS},${PEER_ADDR}"
            else
                PERSISTENT_PEERS="$PEER_ADDR"
            fi
        fi
    done

    cat > "$key_dir/config/config.toml" << EOF
# CometBFT Configuration for validator-$i

proxy_app = "tcp://node-$i:$INTERNAL_ABCI_PORT"
moniker = "validator-$i"

[rpc]
laddr = "tcp://0.0.0.0:$INTERNAL_RPC_PORT"
cors_allowed_origins = ["*"]

[p2p]
laddr = "tcp://0.0.0.0:$INTERNAL_P2P_PORT"
persistent_peers = "$PERSISTENT_PEERS"
addr_book_file = "data/addrbook.json"
addr_book_strict = false
allow_duplicate_ip = true

[mempool]
size = 5000
cache_size = 10000

[consensus]
timeout_propose = "1s"
timeout_propose_delta = "500ms"
timeout_prevote = "1s"
timeout_prevote_delta = "500ms"
timeout_precommit = "1s"
timeout_precommit_delta = "500ms"
timeout_commit = "1s"

[statesync]
enable = false

[instrumentation]
prometheus = true
prometheus_listen_addr = ":$INTERNAL_METRICS_PORT"
EOF
done

echo ""
echo "=== Generation Complete ==="
echo ""
echo "Generated files in $BASE_DIR:"
for i in $(seq 0 $((NUM_VALIDATORS - 1))); do
    echo "  validator-$i/"
    echo "    - config/genesis.json"
    echo "    - config/config.toml"
    echo "    - config/priv_validator_key.json"
    echo "    - config/node_key.json"
done
echo ""
echo "To use these files:"
echo "1. Copy to CometBFT data directories"
echo "2. Use docker-compose-multinode.yml"
echo "3. Run: docker compose -f docker-compose-multinode.yml up"
