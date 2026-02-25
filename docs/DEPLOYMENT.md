# HyperCore Deployment Guide

This document covers building, configuring, and deploying HyperCore — a spot exchange
built on CometBFT consensus with an EVM execution environment.

## Table of Contents

- [Quick Start (Full Stack)](#quick-start-full-stack)
- [Single-Node Mode](#single-node-mode)
- [5-Validator Cluster (Docker)](#5-validator-cluster-docker)
- [Frontend](#frontend)
- [Restarting / Resetting State](#restarting--resetting-state)
- [Hardware Requirements](#hardware-requirements)
- [Environment Variables Reference](#environment-variables-reference)
- [SSL/TLS Termination](#ssltls-termination)

---

## Quick Start (Full Stack)

The fastest way to get everything running with 5 validators:

```bash
# 1. Build Docker image (one-time, ~3-5 minutes)
docker build -f infra/docker/Dockerfile.node -t hypercore-node .

# 2. Start the 5-validator cluster
docker compose -f docker-compose-multinode-5.yml up -d

# 3. Wait for nodes to be healthy (~30 seconds)
docker compose -f docker-compose-multinode-5.yml ps

# 4. Verify the API is responding
curl http://localhost:3000/health
curl -s -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type":"spotMeta"}' | python3 -m json.tool

# 5. Start the frontend (separate terminal)
cd frontend && npm install && npm run dev
```

Open **http://localhost:3001** in your browser. Connect your wallet to trade.

### Default Accounts

| Account | Address | Balances |
|---------|---------|----------|
| Admin | `0xdeA3c06EEe614bF84e74d505173822236c8Ad135` | 10M USDC, 100 BTC, 1000 ETH |
| Alice (hardhat #0) | `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266` | 100K USDC, 10 BTC, 100 ETH |
| Bob (hardhat #1) | `0x70997970C51812dc3A010C7d01b50e0d17dc79C8` | 100K USDC, 10 BTC, 100 ETH |
| Charlie (hardhat #2) | `0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC` | 100K USDC, 10 BTC, 100 ETH |

### Default Markets

| Market | ID | Base Token | Quote Token |
|--------|----|------------|-------------|
| BTC-USDC | 128 | BTC (index 1) | USDC (index 0) |
| ETH-USDC | 129 | ETH (index 2) | USDC (index 0) |

---

## Single-Node Mode

For fast local development without Docker or CometBFT.

### Prerequisites

- **Rust 1.75+** (edition 2021)
- **RocksDB dependencies**: `libclang-dev`, `libssl-dev`, `pkg-config`

On macOS:
```bash
brew install llvm pkg-config openssl
```

On Debian/Ubuntu:
```bash
sudo apt-get update && sudo apt-get install -y \
    pkg-config libssl-dev libclang-dev build-essential
```

### Build & Run

```bash
# Build
cargo build --release -p hypercore-node

# Run (single-node, no CometBFT needed)
./target/release/hypercore start

# Or with cargo directly
cargo run --release -p hypercore-node -- start
```

This starts three servers in the same process:

| Service | Default Address | Description |
|---------|-----------------|-------------|
| Gateway HTTP | `0.0.0.0:3000` | REST API and WebSocket |
| ABCI | `0.0.0.0:26658` | CometBFT ABCI (unused in single-node) |
| EVM JSON-RPC | `0.0.0.0:8545` | Ethereum-compatible RPC |

With persistence enabled:
```bash
./target/release/hypercore start \
    --consensus-mode single-node \
    --enable-persistence \
    --data-dir ./data/chain
```

### Verify

```bash
# Health check
curl http://localhost:3000/health

# Spot metadata
curl -s -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type":"spotMeta"}'

# Check spot balances
curl -s -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type":"spotBalances","user":"0xdeA3c06EEe614bF84e74d505173822236c8Ad135"}'

# EVM block number
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

---

## 5-Validator Cluster (Docker)

Runs 5 HyperCore nodes + 5 CometBFT processes with BFT consensus (tolerates 1 Byzantine node).

### Prerequisites

- Docker and Docker Compose
- ~8 GB RAM (for 5 nodes + CometBFT + PostgreSQL)

### Start

```bash
# Build the node image (one-time)
docker build -f infra/docker/Dockerfile.node -t hypercore-node .

# Start the cluster
docker compose -f docker-compose-multinode-5.yml up -d

# Check all services are healthy
docker compose -f docker-compose-multinode-5.yml ps
```

### Port Scheme

Each validator gets ports at base + (validator_id * 10):

| Validator | Gateway (HTTP) | EVM RPC | CometBFT RPC | CometBFT P2P | ABCI |
|-----------|----------------|---------|---------------|---------------|------|
| 0 | 3000 | 8545 | 26657 | 26656 | 26658 |
| 1 | 3010 | 8555 | 26667 | 26666 | 26668 |
| 2 | 3020 | 8565 | 26677 | 26676 | 26678 |
| 3 | 3030 | 8575 | 26687 | 26686 | 26688 |
| 4 | 3040 | 8585 | 26697 | 26696 | 26698 |

The frontend connects to validator-0's gateway at `localhost:3000` by default.

### View Logs

```bash
# All services
docker compose -f docker-compose-multinode-5.yml logs -f

# Specific node
docker compose -f docker-compose-multinode-5.yml logs -f node-0

# CometBFT consensus
docker compose -f docker-compose-multinode-5.yml logs -f cometbft-0
```

### Check Consensus

```bash
# CometBFT status
curl -s http://localhost:26657/status | python3 -c "
import sys, json
d = json.load(sys.stdin)['result']['sync_info']
print(f\"Latest block: {d['latest_block_height']}  Time: {d['latest_block_time']}\")
"

# Check all nodes are at same height
for port in 26657 26667 26677 26687 26697; do
  height=$(curl -s http://localhost:$port/status | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['sync_info']['latest_block_height'])")
  echo "Port $port: height $height"
done
```

### Stop

```bash
# Stop and keep data
docker compose -f docker-compose-multinode-5.yml down

# Stop and delete all data (full reset)
docker compose -f docker-compose-multinode-5.yml down -v
```

---

## Frontend

The frontend is a Next.js app that connects to the gateway API.

### Prerequisites

- Node.js 18+
- npm

### Install & Run

```bash
cd frontend
npm install
npm run dev
```

Opens at **http://localhost:3001**.

### Configuration

The frontend connects to `localhost:3000` for the API and `localhost:8545` for EVM RPC.
To change these, edit `frontend/src/lib/constants.ts`:

```typescript
export const API_BASE_URL = "http://localhost:3000";
export const WS_URL = "ws://localhost:3000/ws";
export const EVM_RPC_URL = "http://localhost:8545";
export const CHAIN_ID = 1337;
```

### Pages

| URL | Description |
|-----|-------------|
| `/` | Homepage — lists all spot markets |
| `/trade/BTC-USDC` | Trading page for BTC-USDC |
| `/trade/ETH-USDC` | Trading page for ETH-USDC |
| `/admin` | Admin panel (only visible to admin wallet) |

### Production Build

```bash
cd frontend
npm run build
npm start    # serves on port 3001
```

---

## Restarting / Resetting State

### Restart without losing state

```bash
# Single-node: just Ctrl+C and re-run
cargo run --release -p hypercore-node -- start

# Docker cluster: restart containers
docker compose -f docker-compose-multinode-5.yml restart
```

### Full reset (clean state)

When changing genesis config (tokens, balances, markets), you must reset all state:

```bash
# 1. Stop everything
docker compose -f docker-compose-multinode-5.yml down -v

# 2. Reset CometBFT validator state
for i in 0 1 2 3 4; do
  dir="infra/multinode/validator-$i/data"
  rm -rf "$dir/blockstore.db" "$dir/state.db" "$dir/tx_index.db" "$dir/evidence.db" "$dir/cs.wal"
  echo '{"height": "0", "round": 0, "step": 0}' > "$dir/priv_validator_state.json"
  echo '{}' > "$dir/addrbook.json"
done

# 3. Clean local data (single-node mode)
rm -rf data/

# 4. Rebuild and start fresh
docker build -f infra/docker/Dockerfile.node -t hypercore-node .
docker compose -f docker-compose-multinode-5.yml up -d
```

### Reset single-node only

```bash
rm -rf data/
cargo run --release -p hypercore-node -- start
```

---

## Hardware Requirements

| Tier | CPU Cores | RAM | Storage | Network | Notes |
|------|-----------|-----|---------|---------|-------|
| Development | 4 | 8 GB | 50 GB SSD | 100 Mbps | Single-node mode |
| Staging | 8 | 32 GB | 500 GB NVMe | 1 Gbps | Multi-node, persistence |
| Production | 16+ | 64+ GB | 1+ TB NVMe | 10 Gbps | Full cluster, snapshots, indexer |

---

## Environment Variables Reference

All environment variables can also be passed as CLI flags (replace `_` with `-` and
lowercase, e.g., `HTTP_ADDR` becomes `--http-addr`).

| Variable | Default | Description |
|----------|---------|-------------|
| `HTTP_ADDR` | `0.0.0.0:3000` | Gateway HTTP API listen address |
| `ABCI_ADDR` | `0.0.0.0:26658` | CometBFT ABCI listen address |
| `EVM_RPC_ADDR` | `0.0.0.0:8545` | EVM JSON-RPC server listen address |
| `CHAIN_ID` | `1337` | Chain ID (used in genesis and EVM chain ID) |
| `CONSENSUS_MODE` | `single-node` | Consensus mode: `single-node` or `comet-bft` |
| `BLOCK_TIME_MS` | `200` | Block time in milliseconds (single-node mode only) |
| `RUST_LOG` | `info` | Log level filter (e.g., `info,hypercore=debug`) |
| `COMETBFT_RPC_URL` | `http://localhost:26657` | CometBFT RPC URL for tx broadcast (CometBFT mode only) |
| `DATA_DIR` | `./data/chain` | Data directory for RocksDB persistence |
| `DATABASE_URL` | _(none)_ | PostgreSQL URL for indexer |

### Cargo Feature Flags

| Feature | Description |
|---------|-------------|
| `cometbft` | CometBFT ABCI server for multi-node consensus |
| `persistence` | RocksDB-backed state persistence and snapshots |
| `p2p` | libp2p attestation gossip layer |

---

## SSL/TLS Termination

HyperCore does not handle TLS natively. Use a reverse proxy for production deployments.

### Nginx Example

```nginx
upstream hypercore_gateway {
    server 127.0.0.1:3000;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate     /etc/ssl/certs/api.example.com.pem;
    ssl_certificate_key /etc/ssl/private/api.example.com-key.pem;

    location / {
        proxy_pass http://hypercore_gateway;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://hypercore_gateway;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 3600s;
    }
}
```

### Load Balancer (Multi-Node)

```nginx
upstream hypercore_read {
    least_conn;
    server node-0:3000;
    server node-1:3000;
    server node-2:3000;
    server node-3:3000;
    server node-4:3000;
}

upstream hypercore_write {
    server node-0:3000;
    server node-1:3000 backup;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    location /info {
        proxy_pass http://hypercore_read;
    }

    location /exchange {
        proxy_pass http://hypercore_write;
    }
}
```
