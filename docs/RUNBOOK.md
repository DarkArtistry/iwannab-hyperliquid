# HyperCore Operator Runbook

This runbook provides procedures for operating, troubleshooting, and recovering
HyperCore nodes in production environments.

## Table of Contents

- [Health Check Procedures](#health-check-procedures)
- [Log Levels and What to Look For](#log-levels-and-what-to-look-for)
- [Common Failure Modes and Recovery](#common-failure-modes-and-recovery)
- [State Export/Import](#state-exportimport)
- [Snapshot Management](#snapshot-management)
- [Node Restart Procedures](#node-restart-procedures)

---

## Health Check Procedures

### Gateway HTTP Health

The gateway exposes a simple health endpoint that returns `OK` when the HTTP
server is accepting connections.

```bash
curl -f http://localhost:3000/health
# Expected response: OK
# HTTP 200 = healthy, non-200 or connection refused = unhealthy
```

The Docker healthcheck uses this endpoint with the following parameters:
- Interval: 5s (compose) or 30s (Dockerfile)
- Timeout: 5-10s
- Retries: 5-10
- Start period: 10s

### CometBFT RPC Status

Check consensus status on any CometBFT node (default port 26657):

```bash
curl -s http://localhost:26657/status | jq '.result.sync_info'
```

Expected output:

```json
{
  "latest_block_hash": "AB12CD34...",
  "latest_app_hash": "EF56GH78...",
  "latest_block_height": "12345",
  "latest_block_time": "2026-01-15T10:30:00.123456789Z",
  "catching_up": false
}
```

Key fields to check:
- `catching_up: false` -- the node is in sync with the network
- `latest_block_height` -- should increase steadily
- `latest_block_time` -- should be within a few seconds of wall clock time

### EVM JSON-RPC Check

Verify the EVM RPC layer is responding:

```bash
curl -s -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

Expected response:

```json
{
  "jsonrpc": "2.0",
  "result": "0x3039",
  "id": 1
}
```

The result is the current block number in hex. A valid response confirms the EVM
executor and JSON-RPC server (jsonrpsee) are operational.

### Block Production Check

Monitor that blocks are being produced by polling height over time:

```bash
# Single check
curl -s -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "meta"}' | jq '.blockHeight'

# Continuous monitoring (watch height increase)
watch -n 1 'curl -s http://localhost:26657/status | \
  jq ".result.sync_info.latest_block_height"'
```

In single-node mode, blocks are produced every 200ms (configurable via
`BLOCK_TIME_MS`). In CometBFT mode, block time is governed by the consensus
timeouts configured in `config.toml` (default: ~1s commit timeout).

### Multi-Node Consensus Check

Verify all validators are participating:

```bash
# Check connected peers
curl -s http://localhost:26657/net_info | jq '.result.n_peers'

# Check validator set
curl -s http://localhost:26657/validators | jq '.result.validators[] | {address, voting_power}'

# Check consensus state (who has prevoted/precommitted)
curl -s http://localhost:26657/dump_consensus_state | \
  jq '.result.round_state.votes[0]'
```

### Automated Health Script

```bash
#!/bin/bash
# health-check.sh - Returns exit code 0 if all checks pass

GATEWAY_URL="${1:-http://localhost:3000}"
COMETBFT_URL="${2:-http://localhost:26657}"
EVM_URL="${3:-http://localhost:8545}"

# Check gateway
if ! curl -sf "$GATEWAY_URL/health" > /dev/null 2>&1; then
    echo "FAIL: Gateway not responding at $GATEWAY_URL"
    exit 1
fi

# Check CometBFT (only in multi-node mode)
if curl -sf "$COMETBFT_URL/status" > /dev/null 2>&1; then
    CATCHING_UP=$(curl -s "$COMETBFT_URL/status" | jq -r '.result.sync_info.catching_up')
    if [ "$CATCHING_UP" = "true" ]; then
        echo "WARN: CometBFT is catching up"
        exit 2
    fi
fi

# Check EVM
EVM_RESP=$(curl -s -X POST "$EVM_URL" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' 2>/dev/null)
if [ -z "$EVM_RESP" ] || echo "$EVM_RESP" | jq -e '.error' > /dev/null 2>&1; then
    echo "FAIL: EVM RPC not responding at $EVM_URL"
    exit 1
fi

echo "OK: All health checks passed"
exit 0
```

---

## Log Levels and What to Look For

### RUST_LOG Configuration

The node uses `tracing_subscriber` with the `RUST_LOG` environment filter.

| Level   | Setting                           | Use Case                        |
|---------|-----------------------------------|---------------------------------|
| Error   | `RUST_LOG=error`                  | Production (minimal output)     |
| Warn    | `RUST_LOG=warn`                   | Production (with warnings)      |
| Info    | `RUST_LOG=info`                   | Default; block production logs  |
| Debug   | `RUST_LOG=info,hypercore=debug`   | Development; detailed state     |
| Trace   | `RUST_LOG=trace`                  | Deep debugging only             |

You can target specific crates:

```bash
# Debug only the chain crate (ABCI/block production)
RUST_LOG=info,hypercore_chain=debug

# Debug persistence operations
RUST_LOG=info,hypercore_persistence=debug

# Debug gateway request handling
RUST_LOG=info,hypercore_gateway=debug

# Debug EVM execution
RUST_LOG=info,hypercore_evm=debug

# Silence noisy third-party crates
RUST_LOG=info,hypercore=debug,tower_http=warn,jsonrpsee=warn
```

### Key Log Messages to Watch For

**Healthy startup sequence:**

```
INFO  Starting HyperCore node
INFO  Chain ID: 1337
INFO  Consensus mode: SingleNode       (or CometBft)
INFO  Gateway HTTP: 0.0.0.0:3000
INFO  EVM RPC: 0.0.0.0:8545
INFO  ABCI: 0.0.0.0:26658
INFO  Created shared unified state for HyperCore and HyperEVM
INFO  Initialized 2 markets: BTC-PERP, ETH-PERP
INFO  Initialized perp engine with matching (BTC-PERP, ETH-PERP)
INFO  WebSocket event broadcasting enabled
INFO  BlockProducer started with 200ms block time
INFO  EVM RPC server started on 0.0.0.0:8545
```

**Persistence-related messages:**

```
INFO  Persistence enabled, data directory: ./data/chain
INFO  Opened persistence at height 5000
INFO  Found persisted state at height 5000, restoring...
INFO  State restored: height=5000, timestamp=..., app_hash=...
INFO  AppHash verified: restored state produces matching hash ab12cd34
```

**Block production (normal):**

```
INFO  Block 1234 produced: 15 txs (15 ok, 0 failed) in 2.3ms, hash: ab12cd34...
```

### Warning Signs

**Immediate action required:**

```
ERROR CRITICAL: AppHash mismatch after state restore!
      stored=abc123... computed=def456...
```
This means the node's state has diverged from the network. See [State Divergence](#state-divergence-apphash-mismatch).

```
ERROR Block production halted due to state divergence!
ERROR Manual intervention required to resume.
```
The P2P attestation layer detected a hash mismatch and halted the node.

**Investigate soon:**

```
WARN  Failed to persist state: ...
```
Persistence writes are failing. Check disk space and RocksDB health.

```
WARN  Transaction failed: ...
```
Individual transaction failures are normal (invalid orders, insufficient balance),
but a sudden spike indicates a problem.

```
WARN  Failed to send events to broadcaster: ...
```
The WebSocket event channel is full (capacity 100). Clients may be missing events.

```
ERROR Failed to open persistence: ...
WARN  Continuing without persistence
```
The node fell back to in-memory mode. Data will be lost on restart.

---

## Common Failure Modes and Recovery

### Node Not Producing Blocks

**Symptoms:** Block height is not increasing. No "Block N produced" log messages.

**Single-node mode diagnosis:**

```bash
# Check if the process is running
pgrep -f hypercore

# Check for port conflicts
lsof -i :3000 -i :8545 -i :26658

# Check recent logs for errors
docker compose logs --tail=50 node
```

**Resolution:**
1. If the process crashed, check for OOM (Out Of Memory) in system logs:
   `dmesg | grep -i oom`
2. If ports are in use, kill the conflicting process or change ports via environment
   variables.
3. Restart the node: `docker compose restart node`

**CometBFT mode diagnosis:**

```bash
# Check if CometBFT is running
curl http://localhost:26657/status

# Check if ABCI connection is alive
docker compose logs cometbft | grep -i "connection\|error\|refused"

# Check peer count
curl -s http://localhost:26657/net_info | jq '.result.n_peers'
```

### CometBFT Consensus Stall

**Symptoms:** Block height frozen across all validators. CometBFT logs show
repeated proposal timeouts.

**Diagnosis:**

```bash
# Check consensus round state
curl -s http://localhost:26657/dump_consensus_state | \
  jq '.result.round_state.height_round_step'

# Check which validators have voted
curl -s http://localhost:26657/dump_consensus_state | \
  jq '.result.round_state.votes'

# Check if >1/3 of validators are down
curl -s http://localhost:26657/validators | \
  jq '[.result.validators[].voting_power | tonumber] | {total: add, count: length}'
```

**Resolution:**
1. CometBFT requires >2/3 voting power online to make progress. If enough
   validators are down, bring them back up.
2. If a single node is stuck, restart both the HyperCore node and CometBFT:
   ```bash
   docker compose restart node-N cometbft-N
   ```
3. If CometBFT WAL is corrupted, you may need to clear the WAL and let the
   node re-sync:
   ```bash
   # CAUTION: Only if other validators are healthy
   rm -rf infra/multinode/validator-N/data/cs.wal/
   docker compose restart cometbft-N
   ```

### State Divergence (AppHash Mismatch)

**Symptoms:** CometBFT logs show `AppHash mismatch` errors. The node is rejected
by peers. The CRITICAL log message appears on startup.

**Cause:** The node computed a different state hash than the rest of the network.
This is typically caused by:
- Non-deterministic operations in state transitions
- Corrupted persistence data
- Version mismatch between nodes

**Diagnosis:**

```bash
# Check the error in logs
docker compose logs node | grep -i "AppHash mismatch"

# Compare heights across nodes
for port in 26657 26667 26677; do
    echo "Port $port:"
    curl -s "http://localhost:$port/status" | \
      jq '{height: .result.sync_info.latest_block_height, hash: .result.sync_info.latest_app_hash}'
done
```

**Resolution:**

1. **State sync from snapshot** (recommended): Import state from a healthy node.
   ```bash
   # On healthy node: export state
   ./target/release/hypercore export --output state.json --data-dir ./data/chain

   # On diverged node: stop, clear, import
   docker compose stop node-N
   rm -rf ./data/chain-N/*
   ./target/release/hypercore import --input state.json --data-dir ./data/chain-N
   docker compose start node-N
   ```

2. **Full re-sync**: Clear all data and let CometBFT replay from genesis.
   ```bash
   docker compose stop node-N cometbft-N
   rm -rf ./data/chain-N/*
   rm -rf infra/multinode/validator-N/data/*
   # Preserve priv_validator_state.json (or CometBFT will double-sign)
   echo '{"height":"0","round":0,"step":0}' > \
     infra/multinode/validator-N/data/priv_validator_state.json
   docker compose start node-N cometbft-N
   ```

### Disk Full

**Symptoms:** Persistence writes fail. RocksDB errors in logs. Node may crash.

**Diagnosis:**

```bash
df -h /data    # or wherever DATA_DIR points
du -sh ./data/chain/*
du -sh infra/multinode/validator-*/data/
```

**Resolution:**

1. **Immediate**: Clear old snapshots to free space:
   ```bash
   ls -la ./data/chain/snapshots/
   rm ./data/chain/snapshots/snapshot_*.json  # Keep latest only
   ```

2. **CometBFT data pruning**: CometBFT stores all blocks by default. Configure
   pruning in `config.toml` if not already set.

3. **Expand storage**: Resize the volume or add a larger disk.

4. **Compact RocksDB**: Trigger a manual compaction (requires node restart):
   ```bash
   # Export state, clear database, re-import
   ./target/release/hypercore export --output /tmp/state_backup.json
   rm -rf ./data/chain/db
   ./target/release/hypercore import --input /tmp/state_backup.json
   ```

### Port Conflicts

**Symptoms:** Node fails to start with "Address already in use" errors.

**Diagnosis:**

```bash
# Check what is using the ports
lsof -i :3000    # Gateway
lsof -i :8545    # EVM RPC
lsof -i :26658   # ABCI
lsof -i :26656   # CometBFT P2P
lsof -i :26657   # CometBFT RPC
```

**Resolution:**
- Kill the conflicting process, or
- Change the HyperCore ports via environment variables:
  ```bash
  HTTP_ADDR=0.0.0.0:3100 EVM_RPC_ADDR=0.0.0.0:8645 \
    ./target/release/hypercore start
  ```

### Lock Contention

**Symptoms:** High latency. Slow block production. Requests timing out. Log messages
about lock contention.

**Cause:** In CometBFT mode, the mock price feed can cause lock contention with the
ABCI app. The node disables it automatically in CometBFT mode, but if you see
contention issues:

**Resolution:**
1. Ensure the node is running in the correct consensus mode (the mock price feed
   is only active in single-node mode).
2. Reduce concurrent gateway requests if the node is overloaded.
3. Check `RUST_LOG=debug` output for "lock" or "contention" messages.

---

## State Export/Import

### Export

Export the current persisted state to a JSON file:

```bash
./target/release/hypercore export --output state.json
```

With a custom data directory:

```bash
./target/release/hypercore export --output state.json --data-dir ./data/chain
```

The export reads directly from RocksDB and does not require the node to be running.
However, for consistency, it is recommended to stop the node before exporting.

The exported JSON includes:
- Block height and timestamp
- App hash
- All account balances (core and EVM views)
- All positions (perpetuals)
- All open orders
- Spot engine state
- EVM state (accounts, storage, code, block hashes)
- Nonces and client order ID mappings

### Import

Import a previously exported state:

```bash
./target/release/hypercore import --input state.json
```

With a custom data directory:

```bash
./target/release/hypercore import --input state.json --data-dir ./data/chain
```

The import process:
1. Reads and parses the JSON file
2. Validates the state (checks structural integrity)
3. Opens (or creates) the RocksDB database
4. Warns if existing state will be overwritten
5. Persists the imported state atomically
6. Flushes to disk

**Important:** The import requires the `persistence` feature to be compiled in.
If built without it, you will see:

```
Export requires the 'persistence' feature. Build with: cargo build --features persistence
```

### State Validation

The import command automatically validates the state before persisting. You can
also validate an exported file manually by attempting a dry-run import to a
temporary directory:

```bash
mkdir -p /tmp/hypercore-validate
./target/release/hypercore import \
  --input state.json \
  --data-dir /tmp/hypercore-validate
rm -rf /tmp/hypercore-validate
```

If validation fails, the error message will indicate what is wrong (e.g., negative
balances, missing fields, inconsistent data).

---

## Snapshot Management

### Automatic Snapshots

When persistence and CometBFT mode are both enabled, the node creates automatic
snapshots every 1000 blocks. Snapshots are stored in `{DATA_DIR}/snapshots/`.

Key constants (defined in `crates/persistence/src/snapshot.rs`):
- **Snapshot interval**: 1000 blocks
- **Chunk size**: 1 MB (1,048,576 bytes)
- **Snapshot format version**: 1
- **Maximum retained snapshots**: 5

Older snapshots are automatically cleaned up when the limit is reached.

### Snapshot Directory Layout

```
./data/chain/snapshots/
  snapshot_1000.json        # State at height 1000
  snapshot_1000.meta        # Metadata (height, format, chunks, hash)
  snapshot_2000.json
  snapshot_2000.meta
  ...
```

### Manual Snapshot Creation

To create a snapshot at a specific height, use the export command:

```bash
# Stop the node for consistency
docker compose stop node

# Export current state
./target/release/hypercore export --output ./snapshots/manual_snapshot.json

# Restart
docker compose start node
```

### State Sync from Snapshot

New nodes can bootstrap quickly using ABCI state sync instead of replaying all
blocks from genesis. This is handled automatically by CometBFT when state sync
is enabled.

To configure a new node for state sync:

1. Get the latest snapshot height and hash from an existing node:
   ```bash
   curl -s http://trusted-node:26657/status | \
     jq '{height: .result.sync_info.latest_block_height, hash: .result.sync_info.latest_block_hash}'
   ```

2. Configure the new node's CometBFT `config.toml`:
   ```toml
   [statesync]
   enable = true
   rpc_servers = "trusted-node-1:26657,trusted-node-2:26657"
   trust_height = 10000        # A recent height from step 1
   trust_hash = "ABCDEF..."   # The hash from step 1
   trust_period = "168h0m0s"   # How far back to trust (7 days)
   ```

3. Start the new node. CometBFT will request snapshot chunks from peers, and
   the ABCI app will apply them to restore state.

### Snapshot Verification

Each snapshot includes a SHA-256 hash of the complete serialized state. During
state sync, the receiving node:
1. Receives all chunks
2. Reassembles the full state
3. Computes the SHA-256 hash
4. Compares against the metadata hash
5. Validates the deserialized state structure

If any step fails, the snapshot is rejected and the node tries another peer.

---

## Node Restart Procedures

### Graceful Shutdown

The node handles both SIGTERM and SIGINT (Ctrl+C) for graceful shutdown. On
receiving either signal:

1. The shutdown signal is propagated to all services
2. The gateway stops accepting new connections
3. In-flight requests are allowed to complete
4. Block production stops (single-node mode) or the ABCI server shuts down
5. Persistence is flushed to disk:
   ```
   INFO  Flushing persistence to disk...
   INFO  Persistence shutdown complete
   INFO  HyperCore node shutdown complete
   ```

**Docker:**

```bash
# Graceful stop (sends SIGTERM, waits 10s, then SIGKILL)
docker compose stop node

# With custom timeout
docker compose stop -t 30 node
```

**Native binary:**

```bash
# Send SIGTERM
kill $(pgrep -f hypercore)

# Or Ctrl+C in the terminal
```

### State Recovery After Crash

If the node crashes (SIGKILL, OOM, power loss), recovery depends on whether
persistence was enabled.

**With persistence enabled:**

The node automatically restores state from RocksDB on startup:

```bash
# Just restart - state will be recovered
docker compose up -d node
```

Watch the logs for:
```
INFO  Found persisted state at height 5000, restoring...
INFO  State restored: height=5000, timestamp=..., app_hash=...
```

In CometBFT mode, the node reports its last committed height to CometBFT via
the `Info` ABCI call. CometBFT then replays only the blocks that occurred after
the persisted height.

**Without persistence:**

All state is lost. The node starts from genesis:
- In single-node mode, all orders and positions are gone.
- In CometBFT mode, the node replays all blocks from height 0 (which can take
  a very long time for a chain with many blocks).

**Recommendation:** Always run with `--enable-persistence` in any environment
where data loss is unacceptable.

### Persistence Verification

After a restart, verify the recovered state is correct:

```bash
# Check the restored height
docker compose logs node | grep "State restored"

# In CometBFT mode, verify AppHash matches
docker compose logs node | grep "AppHash verified"

# Query the node to confirm state is accessible
curl -s -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "meta"}' | jq '.blockHeight'

# Compare with CometBFT's view of the chain
curl -s http://localhost:26657/status | \
  jq '.result.sync_info.latest_block_height'
```

If the `AppHash verified` message appears, the restored state is consistent with
what was previously committed. If you see an `AppHash mismatch` error, follow the
[State Divergence](#state-divergence-apphash-mismatch) recovery procedure.

### Rolling Restart (Multi-Node)

To restart nodes in a multi-node cluster without downtime:

```bash
# Restart one node at a time, waiting for it to catch up
for i in 0 1 2; do
    echo "Restarting validator $i..."
    docker compose restart node-$i cometbft-$i

    # Wait for the node to catch up
    echo "Waiting for validator $i to sync..."
    while true; do
        CATCHING_UP=$(curl -s "http://localhost:$((26657 + i*10))/status" | \
          jq -r '.result.sync_info.catching_up' 2>/dev/null)
        if [ "$CATCHING_UP" = "false" ]; then
            echo "Validator $i is in sync"
            break
        fi
        sleep 2
    done

    # Wait a few more seconds for stability
    sleep 5
done
echo "Rolling restart complete"
```

**Important:** Never restart more than 1/3 of validators simultaneously, or
consensus will stall until they recover.

### Emergency Procedures

**Full cluster restart:**

```bash
# Stop all nodes
docker compose -f docker-compose-multinode.yml down

# Clear CometBFT WAL (if consensus is stuck)
for i in 0 1 2 3 4; do
    rm -rf infra/multinode/validator-$i/data/cs.wal/
done

# Restart
docker compose -f docker-compose-multinode.yml up -d
```

**Reset to genesis (DESTROYS ALL DATA):**

```bash
docker compose -f docker-compose-multinode.yml down -v

# Clear all persisted state
rm -rf ./data/chain*

# Clear CometBFT data (preserve keys)
for i in 0 1 2 3 4; do
    rm -rf infra/multinode/validator-$i/data/
    mkdir -p infra/multinode/validator-$i/data/
    echo '{"height":"0","round":0,"step":0}' > \
      infra/multinode/validator-$i/data/priv_validator_state.json
done

docker compose -f docker-compose-multinode.yml up -d
```
