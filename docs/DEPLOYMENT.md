# HyperCore Deployment Guide

This guide covers deploying HyperCore for testnet and production environments.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Single-Node Development](#single-node-development)
3. [Multi-Node Testnet](#multi-node-testnet)
4. [Production Deployment](#production-deployment)
5. [Monitoring](#monitoring)
6. [Troubleshooting](#troubleshooting)

---

## Prerequisites

### Required Software

| Software | Version | Purpose |
|----------|---------|---------|
| Rust | 1.85+ | Build HyperCore binaries |
| Docker | 24.0+ | Container runtime |
| Docker Compose | 2.20+ | Service orchestration |
| PostgreSQL | 16+ | Indexer database |

### Hardware Requirements

| Component | Development | Testnet | Production |
|-----------|-------------|---------|------------|
| CPU | 2 cores | 4 cores | 8+ cores |
| RAM | 4 GB | 8 GB | 32+ GB |
| Storage | 10 GB SSD | 100 GB SSD | 500+ GB NVMe |
| Network | 10 Mbps | 100 Mbps | 1 Gbps |

---

## Single-Node Development

Quick setup for local development and testing.

### Option 1: Docker Compose (Recommended)

```bash
# Clone repository
git clone <repo-url>
cd iwannab-hyperliquid

# Start all services
docker-compose up -d

# Check service health
docker-compose ps

# View logs
docker-compose logs -f node
```

**Services started:**
| Service | Port | Description |
|---------|------|-------------|
| node | 3000, 8545, 26658 | HyperCore node (Gateway + EVM + ABCI) |
| postgres | 5432 | PostgreSQL database |
| cometbft | 26656, 26657 | CometBFT consensus |
| indexer | - | Block indexer |

### Option 2: Native Binary

```bash
# Build with persistence
cargo build -p hypercore-node --features persistence --release

# Start PostgreSQL
docker-compose up -d postgres

# Run node
./target/release/hypercore-node start \
    --enable-persistence \
    --data-dir ./data \
    --http-addr 0.0.0.0:3000 \
    --chain-id 1337
```

### Verify Deployment

```bash
# Check health
curl http://localhost:3000/health

# Get exchange metadata
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "meta"}'

# Check EVM RPC
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

---

## Multi-Node Testnet

Deploy a multi-validator testnet with CometBFT consensus.

### Step 1: Generate Validator Keys

```bash
# Create directory structure for 4 validators
mkdir -p testnet/{node0,node1,node2,node3}

# Generate CometBFT node keys for each validator
for i in 0 1 2 3; do
    cometbft init --home testnet/node$i
done

# Collect validator public keys
for i in 0 1 2 3; do
    cat testnet/node$i/config/priv_validator_key.json | jq -r '.pub_key.value'
done
```

### Step 2: Create Genesis File

Create `testnet/genesis.json`:

```json
{
  "genesis_time": "2026-01-01T00:00:00.000000000Z",
  "chain_id": "hypercore-testnet",
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
    }
  },
  "validators": [
    {
      "address": "<VALIDATOR_0_ADDRESS>",
      "pub_key": {"type": "tendermint/PubKeyEd25519", "value": "<VALIDATOR_0_PUBKEY>"},
      "power": "10",
      "name": "validator-0"
    },
    {
      "address": "<VALIDATOR_1_ADDRESS>",
      "pub_key": {"type": "tendermint/PubKeyEd25519", "value": "<VALIDATOR_1_PUBKEY>"},
      "power": "10",
      "name": "validator-1"
    },
    {
      "address": "<VALIDATOR_2_ADDRESS>",
      "pub_key": {"type": "tendermint/PubKeyEd25519", "value": "<VALIDATOR_2_PUBKEY>"},
      "power": "10",
      "name": "validator-2"
    },
    {
      "address": "<VALIDATOR_3_ADDRESS>",
      "pub_key": {"type": "tendermint/PubKeyEd25519", "value": "<VALIDATOR_3_PUBKEY>"},
      "power": "10",
      "name": "validator-3"
    }
  ],
  "app_hash": ""
}
```

### Step 3: Configure CometBFT

For each validator, update `testnet/node$i/config/config.toml`:

```toml
[p2p]
laddr = "tcp://0.0.0.0:26656"
persistent_peers = "node0_id@node0:26656,node1_id@node1:26656,node2_id@node2:26656,node3_id@node3:26656"

[consensus]
timeout_propose = "3s"
timeout_prevote = "1s"
timeout_precommit = "1s"
timeout_commit = "5s"

[rpc]
laddr = "tcp://0.0.0.0:26657"
cors_allowed_origins = ["*"]
```

### Step 4: Create docker-compose.testnet.yml

```yaml
version: "3.8"

services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: hypercore
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-hypercore}
      POSTGRES_DB: hypercore
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U hypercore"]
      interval: 5s
      timeout: 5s
      retries: 5

  node0:
    build:
      context: .
      dockerfile: infra/docker/Dockerfile.node
    environment:
      RUST_LOG: info,hypercore=debug
      DATABASE_URL: postgres://hypercore:${POSTGRES_PASSWORD:-hypercore}@postgres:5432/hypercore
      ABCI_ADDR: 0.0.0.0:26658
      HTTP_ADDR: 0.0.0.0:3000
      EVM_RPC_ADDR: 0.0.0.0:8545
    volumes:
      - node0_data:/data
    ports:
      - "3000:3000"
      - "8545:8545"
    depends_on:
      postgres:
        condition: service_healthy

  cometbft0:
    image: cometbft/cometbft:v0.38.5
    volumes:
      - ./testnet/node0:/cometbft
      - ./testnet/genesis.json:/cometbft/config/genesis.json:ro
    command: start --proxy_app=tcp://node0:26658
    ports:
      - "26656:26656"
      - "26657:26657"
    depends_on:
      - node0

  # Repeat for node1, node2, node3...
  # (See full example in infra/docker-compose.testnet.yml)

volumes:
  postgres_data:
  node0_data:
  node1_data:
  node2_data:
  node3_data:
```

### Step 5: Start Testnet

```bash
# Build images
docker-compose -f docker-compose.testnet.yml build

# Start services
docker-compose -f docker-compose.testnet.yml up -d

# Check consensus status
curl http://localhost:26657/status | jq '.result.sync_info'

# Check validator set
curl http://localhost:26657/validators | jq '.result.validators'
```

### Step 6: Verify Multi-Node Operation

```bash
# Check block production
watch -n 1 'curl -s http://localhost:26657/status | jq ".result.sync_info.latest_block_height"'

# Verify all validators are signing
curl http://localhost:26657/dump_consensus_state | jq '.result.round_state.validators'

# Test order placement
curl -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "meta"}'
```

---

## Production Deployment

### Security Checklist

- [ ] Change default PostgreSQL password
- [ ] Enable TLS for all external endpoints
- [ ] Configure firewall rules
- [ ] Set up rate limiting
- [ ] Enable WAL archiving for PostgreSQL
- [ ] Configure validator key protection
- [ ] Set up monitoring and alerting
- [ ] Enable persistence with backup strategy

### Environment Variables

```bash
# Required
POSTGRES_PASSWORD=<strong-password>
DATABASE_URL=postgres://hypercore:${POSTGRES_PASSWORD}@postgres:5432/hypercore
CHAIN_ID=hypercore-mainnet

# Optional
RUST_LOG=info
HTTP_ADDR=0.0.0.0:3000
EVM_RPC_ADDR=0.0.0.0:8545
ABCI_ADDR=0.0.0.0:26658
```

### TLS Configuration

For production, use a reverse proxy (nginx/traefik) with TLS:

```nginx
server {
    listen 443 ssl http2;
    server_name api.hypercore.example.com;

    ssl_certificate /etc/letsencrypt/live/hypercore.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/hypercore.example.com/privkey.pem;

    location / {
        proxy_pass http://node:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

### Backup Strategy

```bash
# RocksDB checkpoint (consistent snapshot)
# The node exposes create_checkpoint() API for consistent backups

# PostgreSQL backup
pg_dump -h localhost -U hypercore hypercore > backup_$(date +%Y%m%d).sql

# Full data directory backup (stop node first)
tar -czf hypercore_backup_$(date +%Y%m%d).tar.gz /data/chain
```

### State Sync (New Node Bootstrap)

To bootstrap a new node from an existing validator:

```bash
# On existing validator: create checkpoint
curl -X POST http://validator0:3000/admin/checkpoint \
  -d '{"path": "/data/snapshots/checkpoint_latest"}'

# Copy checkpoint to new node
rsync -av validator0:/data/snapshots/checkpoint_latest/ new_node:/data/chain/

# Start new node (will sync from checkpoint)
./hypercore-node start \
    --enable-persistence \
    --data-dir /data/chain \
    --consensus-mode cometbft
```

---

## Monitoring

### Health Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Basic health check |
| `/metrics` | GET | Prometheus metrics |
| `/status` | GET | Detailed node status |

### Prometheus Metrics

Add to `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'hypercore'
    static_configs:
      - targets: ['node:9090']
    scrape_interval: 15s
```

### Key Metrics to Monitor

| Metric | Alert Threshold | Description |
|--------|-----------------|-------------|
| `hypercore_block_height` | No increase in 30s | Block production stalled |
| `hypercore_order_latency_ms` | > 100ms | Order processing slow |
| `hypercore_pending_tx_count` | > 1000 | Mempool backlog |
| `hypercore_validator_missed_blocks` | > 10 | Validator not signing |

### Grafana Dashboard

Import the pre-built dashboard from `infra/grafana/dashboards/hypercore.json`.

```bash
# Start monitoring stack
docker-compose --profile monitoring up -d

# Access Grafana at http://localhost:3002
# Default credentials: admin/admin
```

---

## Troubleshooting

### Common Issues

#### Node Won't Start

```bash
# Check logs
docker-compose logs node

# Common causes:
# 1. Port already in use
lsof -i :3000 :8545 :26658

# 2. Database not ready
docker-compose up -d postgres
sleep 5
docker-compose up -d node

# 3. Corrupted data
rm -rf ./data/chain
# Warning: This deletes all state!
```

#### Consensus Stalled

```bash
# Check validator status
curl http://localhost:26657/dump_consensus_state | jq '.result.round_state'

# Check if validators are connected
curl http://localhost:26657/net_info | jq '.result.peers'

# Restart CometBFT
docker-compose restart cometbft
```

#### High Memory Usage

```bash
# Check memory stats
docker stats hypercore-node

# Tune RocksDB (in environment):
ROCKSDB_MAX_OPEN_FILES=256
ROCKSDB_BLOCK_CACHE_MB=256
```

#### Database Connection Issues

```bash
# Check PostgreSQL status
docker-compose exec postgres pg_isready -U hypercore

# Reset connection pool
docker-compose restart node
```

### Diagnostic Commands

```bash
# Get node version
curl http://localhost:3000/info -d '{"type": "meta"}' | jq '.version'

# Check block production
curl http://localhost:26657/status | jq '.result.sync_info'

# List validators
curl http://localhost:26657/validators | jq '.result.validators'

# Get orderbook state
curl http://localhost:3000/info -d '{"type": "l2Book", "coin": "BTC-PERP"}' | jq

# Check EVM block
curl http://localhost:8545 -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

### Log Analysis

```bash
# Search for errors
docker-compose logs node 2>&1 | grep -i error

# Follow logs with timestamp
docker-compose logs -f --timestamps node

# Export logs
docker-compose logs node > node.log 2>&1
```

---

## Quick Reference

### Start/Stop Commands

```bash
# Development (single-node)
docker-compose up -d
docker-compose down

# Testnet (multi-node)
docker-compose -f docker-compose.testnet.yml up -d
docker-compose -f docker-compose.testnet.yml down

# With monitoring
docker-compose --profile monitoring up -d
```

### Port Reference

| Port | Service | Protocol |
|------|---------|----------|
| 3000 | Gateway API | HTTP/WS |
| 8545 | EVM RPC | JSON-RPC |
| 8546 | EVM WS | WebSocket |
| 26656 | CometBFT P2P | TCP |
| 26657 | CometBFT RPC | HTTP |
| 26658 | ABCI | TCP |
| 5432 | PostgreSQL | TCP |
| 9090 | Prometheus | HTTP |
| 3002 | Grafana | HTTP |

### Environment Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `HTTP_ADDR` | `0.0.0.0:3000` | Gateway listen address |
| `EVM_RPC_ADDR` | `0.0.0.0:8545` | EVM RPC listen address |
| `ABCI_ADDR` | `0.0.0.0:26658` | ABCI listen address |
| `DATABASE_URL` | - | PostgreSQL connection URL |
| `CHAIN_ID` | `1337` | Network chain ID |
| `RUST_LOG` | `info` | Log level |
| `DATA_DIR` | `/data` | Persistence directory |

---

## Test Coverage Summary

Before deploying, ensure all tests pass:

```bash
# Run all Rust tests (337 tests)
cargo test --workspace --lib

# Test breakdown:
# - engine: 105 tests (matching, risk, liquidation, funding)
# - chain: 54 tests (state, determinism, block production)
# - primitives: 59 tests (types, positions, orders)
# - gateway: 66 tests (API, validation, rate limiting)
# - persistence: 26 tests (RocksDB, state save/restore)
# - evm: 23 tests (execution, precompiles)
# - indexer: 4 tests (database operations)

# Run E2E integration tests
./scripts/e2e-test.sh

# Run contract tests
cd contracts && forge test
```

---

## Support

- **Documentation**: See `docs/` directory
- **Issues**: GitHub Issues
- **Architecture**: `docs/ARCHITECTURE.md`
- **Implementation Status**: `docs/IMPLEMENTATION_STATUS.md`
