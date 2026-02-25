# HyperCore Monitoring Guide

This document covers metrics collection, dashboards, and alerting for HyperCore
nodes in production environments.

## Table of Contents

- [Key Metrics to Track](#key-metrics-to-track)
- [Prometheus Metrics Endpoints](#prometheus-metrics-endpoints)
- [Grafana Dashboard Templates](#grafana-dashboard-templates)
- [Alerting Rules](#alerting-rules)

---

## Key Metrics to Track

### Block Production

| Metric                 | Target           | Description                                     |
|------------------------|------------------|-------------------------------------------------|
| Block time             | 200ms (single)   | Time between consecutive blocks                 |
| Block time             | ~1s (CometBFT)   | Governed by consensus timeout_commit             |
| Block height           | Monotonic        | Should always increase; stall = problem          |
| Transactions per block | Varies           | Max 10,000 per block (configurable)              |
| Failed transactions    | < 5%             | Spike indicates bad client requests or bugs      |
| Block production duration | < 50ms        | Time to execute BeginBlock/DeliverTx/EndBlock    |

### Transaction Throughput

| Metric                  | Target           | Description                                    |
|-------------------------|------------------|------------------------------------------------|
| Orders per second       | > 1,000          | Total order submission rate                    |
| Fills per second        | Varies           | Matched trade rate                             |
| Mempool depth           | < 1,000          | Pending transactions waiting for inclusion     |
| Mempool drain rate      | > submission rate| Must keep up with incoming transactions         |

### Latency

| Metric                  | Target           | Description                                    |
|-------------------------|------------------|------------------------------------------------|
| P50 order latency       | < 10ms           | Median time from submission to execution       |
| P99 order latency       | < 100ms          | Tail latency for order execution               |
| Gateway response time   | < 20ms           | HTTP request round-trip for info queries       |
| EVM RPC response time   | < 50ms           | JSON-RPC call round-trip                       |

### Resource Usage

| Metric                  | Alert Threshold  | Description                                    |
|-------------------------|------------------|------------------------------------------------|
| CPU utilization         | > 80% sustained  | Per-core and total usage                       |
| Memory (RSS)            | > 80% of limit   | Resident set size of the hypercore process     |
| Disk I/O (write)        | > 500 MB/s       | RocksDB writes and compaction                  |
| Disk I/O (IOPS)         | > 10,000         | Random read/write operations                   |
| Disk space free         | < 20%            | Available space on data volume                 |
| Open file descriptors   | > 80% of limit   | RocksDB and network sockets                    |

### Network (CometBFT)

| Metric                  | Target           | Description                                    |
|-------------------------|------------------|------------------------------------------------|
| Connected peers         | >= 2/3 validators| CometBFT P2P peer count                        |
| Consensus rounds        | 0 (mostly)       | Extra rounds indicate slow validators          |
| Missed blocks           | 0                | Validator not signing blocks                   |

---

## Prometheus Metrics Endpoints

### CometBFT Metrics (Available Now)

CometBFT exposes Prometheus metrics when `[instrumentation] prometheus = true` is
set in `config.toml`. The endpoint is at the configured listen address (default
`:26660`).

```yaml
# prometheus.yml scrape config for CometBFT
scrape_configs:
  - job_name: 'cometbft'
    static_configs:
      - targets:
          - 'cometbft-0:26660'
          - 'cometbft-1:26660'
          - 'cometbft-2:26660'
    scrape_interval: 5s
```

Key CometBFT metrics:

| Metric                                        | Type      | Description                          |
|-----------------------------------------------|-----------|--------------------------------------|
| `cometbft_consensus_height`                   | Gauge     | Current consensus height             |
| `cometbft_consensus_rounds`                   | Gauge     | Number of rounds in current height   |
| `cometbft_consensus_validators`               | Gauge     | Number of validators                 |
| `cometbft_consensus_validators_power`         | Gauge     | Total validator voting power         |
| `cometbft_consensus_missing_validators`       | Gauge     | Number of missing validators         |
| `cometbft_consensus_missing_validators_power` | Gauge     | Missing validator voting power       |
| `cometbft_consensus_block_size_bytes`         | Gauge     | Size of latest block in bytes        |
| `cometbft_consensus_num_txs`                  | Gauge     | Number of txs in latest block        |
| `cometbft_consensus_total_txs`                | Counter   | Total transactions committed         |
| `cometbft_consensus_block_interval_seconds`   | Histogram | Time between blocks                  |
| `cometbft_p2p_peers`                          | Gauge     | Number of connected peers            |
| `cometbft_mempool_size`                       | Gauge     | Number of uncommitted transactions   |
| `cometbft_mempool_tx_size_bytes`              | Histogram | Size of transactions in mempool      |

### HyperCore Application Metrics (Planned)

The following metrics are planned for future HyperCore releases. They would be
exposed on a dedicated metrics endpoint (e.g., `:9100/metrics`).

```
# HELP hypercore_block_height Current block height
# TYPE hypercore_block_height gauge
hypercore_block_height{chain_id="hypercore-1337"} 12345

# HELP hypercore_block_production_duration_ms Time to produce a block
# TYPE hypercore_block_production_duration_ms histogram
hypercore_block_production_duration_ms_bucket{le="1"} 500
hypercore_block_production_duration_ms_bucket{le="5"} 950
hypercore_block_production_duration_ms_bucket{le="10"} 990
hypercore_block_production_duration_ms_bucket{le="50"} 999
hypercore_block_production_duration_ms_bucket{le="100"} 1000
hypercore_block_production_duration_ms_bucket{le="+Inf"} 1000

# HELP hypercore_tx_count_total Total transactions processed
# TYPE hypercore_tx_count_total counter
hypercore_tx_count_total{status="success"} 50000
hypercore_tx_count_total{status="failed"} 120

# HELP hypercore_mempool_size Current mempool size
# TYPE hypercore_mempool_size gauge
hypercore_mempool_size 42

# HELP hypercore_match_latency_ms Order matching latency
# TYPE hypercore_match_latency_ms histogram
hypercore_match_latency_ms_bucket{market="BTC-PERP",le="1"} 800
hypercore_match_latency_ms_bucket{market="BTC-PERP",le="5"} 950
hypercore_match_latency_ms_bucket{market="BTC-PERP",le="10"} 999
hypercore_match_latency_ms_bucket{market="BTC-PERP",le="+Inf"} 1000

# HELP hypercore_positions_total Total open positions
# TYPE hypercore_positions_total gauge
hypercore_positions_total{market="BTC-PERP"} 1500
hypercore_positions_total{market="ETH-PERP"} 800

# HELP hypercore_open_orders_total Total open orders in orderbook
# TYPE hypercore_open_orders_total gauge
hypercore_open_orders_total{market="BTC-PERP",side="bid"} 3000
hypercore_open_orders_total{market="BTC-PERP",side="ask"} 2800

# HELP hypercore_fills_total Total fills executed
# TYPE hypercore_fills_total counter
hypercore_fills_total{market="BTC-PERP"} 25000
hypercore_fills_total{market="ETH-PERP"} 12000

# HELP hypercore_liquidations_total Total liquidations executed
# TYPE hypercore_liquidations_total counter
hypercore_liquidations_total{market="BTC-PERP"} 150

# HELP hypercore_persistence_write_duration_ms RocksDB write latency
# TYPE hypercore_persistence_write_duration_ms histogram
hypercore_persistence_write_duration_ms_bucket{le="1"} 900
hypercore_persistence_write_duration_ms_bucket{le="10"} 990
hypercore_persistence_write_duration_ms_bucket{le="100"} 1000

# HELP hypercore_snapshot_height Latest snapshot height
# TYPE hypercore_snapshot_height gauge
hypercore_snapshot_height 10000

# HELP hypercore_websocket_connections Active WebSocket connections
# TYPE hypercore_websocket_connections gauge
hypercore_websocket_connections 150

# HELP hypercore_evm_tx_total Total EVM transactions processed
# TYPE hypercore_evm_tx_total counter
hypercore_evm_tx_total 5000

# HELP hypercore_gateway_requests_total Total gateway HTTP requests
# TYPE hypercore_gateway_requests_total counter
hypercore_gateway_requests_total{endpoint="/info",status="200"} 100000
hypercore_gateway_requests_total{endpoint="/exchange",status="200"} 50000
hypercore_gateway_requests_total{endpoint="/exchange",status="400"} 500

# HELP hypercore_gateway_request_duration_ms Gateway request duration
# TYPE hypercore_gateway_request_duration_ms histogram
hypercore_gateway_request_duration_ms_bucket{endpoint="/info",le="5"} 95000
hypercore_gateway_request_duration_ms_bucket{endpoint="/exchange",le="10"} 48000
```

### Prometheus Configuration Example

Full `prometheus.yml` for monitoring a 3-validator cluster:

```yaml
global:
  scrape_interval: 5s
  evaluation_interval: 5s

rule_files:
  - "rules/hypercore_alerts.yml"

scrape_configs:
  # CometBFT consensus metrics (per validator)
  - job_name: 'cometbft'
    static_configs:
      - targets:
          - 'cometbft-0:26660'
          - 'cometbft-1:26660'
          - 'cometbft-2:26660'
        labels:
          cluster: 'hypercore-prod'

  # HyperCore application metrics (planned, per node)
  - job_name: 'hypercore'
    static_configs:
      - targets:
          - 'node-0:9100'
          - 'node-1:9100'
          - 'node-2:9100'
        labels:
          cluster: 'hypercore-prod'

  # Node Exporter (system metrics)
  - job_name: 'node-exporter'
    static_configs:
      - targets:
          - 'node-0:9101'
          - 'node-1:9101'
          - 'node-2:9101'
```

### Interim Monitoring (Without Native Metrics)

Until native Prometheus metrics are implemented, use these approaches to
collect key data:

**Poll block height via CometBFT RPC:**

```bash
# Script to track block production rate
while true; do
    HEIGHT=$(curl -s http://localhost:26657/status | \
      jq -r '.result.sync_info.latest_block_height')
    echo "$(date +%s) $HEIGHT" >> /var/log/hypercore/block_heights.log
    sleep 1
done
```

**Poll gateway for state metrics:**

```bash
# Get orderbook depth
curl -s -X POST http://localhost:3000/info \
  -H "Content-Type: application/json" \
  -d '{"type": "l2Book", "coin": "BTC-PERP"}' | jq '.levels | length'
```

**Use Node Exporter for system metrics:**

```yaml
# docker-compose addition
node-exporter:
  image: prom/node-exporter:latest
  pid: host
  volumes:
    - /proc:/host/proc:ro
    - /sys:/host/sys:ro
  command:
    - '--path.procfs=/host/proc'
    - '--path.sysfs=/host/sys'
  ports:
    - "9101:9100"
```

---

## Grafana Dashboard Templates

### Enabling the Monitoring Stack

```bash
# Start with monitoring profile (includes Grafana + Prometheus)
docker compose --profile monitoring up -d

# Access Grafana
open http://localhost:3002
# Default credentials: admin / admin
```

Grafana datasources and dashboards are auto-provisioned from:
- `infra/grafana/datasources/` -- Prometheus datasource
- `infra/grafana/dashboards/` -- Dashboard JSON files

### Dashboard JSON Template

The following Grafana dashboard template provides panels for the key operational
metrics. Save this as `infra/grafana/dashboards/hypercore-operations.json` and
it will be auto-provisioned.

```json
{
  "dashboard": {
    "id": null,
    "uid": "hypercore-ops",
    "title": "HyperCore Operations",
    "tags": ["hypercore", "operations"],
    "timezone": "browser",
    "refresh": "5s",
    "time": {
      "from": "now-1h",
      "to": "now"
    },
    "panels": [
      {
        "title": "Block Height",
        "type": "stat",
        "gridPos": { "h": 4, "w": 6, "x": 0, "y": 0 },
        "targets": [
          {
            "expr": "cometbft_consensus_height",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "thresholds": {
              "steps": [
                { "color": "green", "value": null }
              ]
            }
          }
        }
      },
      {
        "title": "Block Production Rate (blocks/min)",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 0, "y": 4 },
        "targets": [
          {
            "expr": "rate(cometbft_consensus_height[1m]) * 60",
            "legendFormat": "{{instance}}"
          }
        ]
      },
      {
        "title": "Transactions Per Block",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 12, "y": 4 },
        "targets": [
          {
            "expr": "cometbft_consensus_num_txs",
            "legendFormat": "{{instance}}"
          }
        ]
      },
      {
        "title": "Total Transactions (cumulative)",
        "type": "stat",
        "gridPos": { "h": 4, "w": 6, "x": 6, "y": 0 },
        "targets": [
          {
            "expr": "cometbft_consensus_total_txs",
            "legendFormat": "{{instance}}"
          }
        ]
      },
      {
        "title": "Connected Peers",
        "type": "stat",
        "gridPos": { "h": 4, "w": 6, "x": 12, "y": 0 },
        "targets": [
          {
            "expr": "cometbft_p2p_peers",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "thresholds": {
              "steps": [
                { "color": "red", "value": 0 },
                { "color": "yellow", "value": 1 },
                { "color": "green", "value": 2 }
              ]
            }
          }
        }
      },
      {
        "title": "Mempool Size",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 0, "y": 12 },
        "targets": [
          {
            "expr": "cometbft_mempool_size",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "thresholds": {
              "steps": [
                { "color": "green", "value": null },
                { "color": "yellow", "value": 500 },
                { "color": "red", "value": 4000 }
              ]
            }
          }
        }
      },
      {
        "title": "Block Interval (seconds)",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 12, "y": 12 },
        "targets": [
          {
            "expr": "rate(cometbft_consensus_block_interval_seconds_sum[5m]) / rate(cometbft_consensus_block_interval_seconds_count[5m])",
            "legendFormat": "avg block interval {{instance}}"
          }
        ]
      },
      {
        "title": "Consensus Rounds",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 0, "y": 20 },
        "targets": [
          {
            "expr": "cometbft_consensus_rounds",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "thresholds": {
              "steps": [
                { "color": "green", "value": 0 },
                { "color": "yellow", "value": 1 },
                { "color": "red", "value": 3 }
              ]
            }
          }
        }
      },
      {
        "title": "Missing Validators",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 12, "y": 20 },
        "targets": [
          {
            "expr": "cometbft_consensus_missing_validators",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "thresholds": {
              "steps": [
                { "color": "green", "value": 0 },
                { "color": "red", "value": 1 }
              ]
            }
          }
        }
      },
      {
        "title": "Block Size (bytes)",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 0, "y": 28 },
        "targets": [
          {
            "expr": "cometbft_consensus_block_size_bytes",
            "legendFormat": "{{instance}}"
          }
        ]
      },
      {
        "title": "System CPU Usage",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 12, "y": 28 },
        "targets": [
          {
            "expr": "100 - (avg by(instance) (rate(node_cpu_seconds_total{mode=\"idle\"}[5m])) * 100)",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "unit": "percent",
            "max": 100
          }
        }
      },
      {
        "title": "System Memory Usage",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 0, "y": 36 },
        "targets": [
          {
            "expr": "(1 - (node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)) * 100",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "unit": "percent",
            "max": 100
          }
        }
      },
      {
        "title": "Disk Usage",
        "type": "timeseries",
        "gridPos": { "h": 8, "w": 12, "x": 12, "y": 36 },
        "targets": [
          {
            "expr": "(1 - (node_filesystem_avail_bytes{mountpoint=\"/data\"} / node_filesystem_size_bytes{mountpoint=\"/data\"})) * 100",
            "legendFormat": "{{instance}}"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "unit": "percent",
            "max": 100
          }
        }
      }
    ]
  }
}
```

### Panel Summary

| Panel                      | Data Source    | Purpose                                         |
|----------------------------|---------------|-------------------------------------------------|
| Block Height               | CometBFT      | Current chain height (stat)                     |
| Block Production Rate      | CometBFT      | Blocks per minute over time                     |
| Transactions Per Block     | CometBFT      | Throughput per block                            |
| Total Transactions         | CometBFT      | Cumulative transaction count                    |
| Connected Peers            | CometBFT      | Network connectivity (red/yellow/green)         |
| Mempool Size               | CometBFT      | Pending transaction backlog                     |
| Block Interval             | CometBFT      | Average time between blocks                     |
| Consensus Rounds           | CometBFT      | Extra rounds = slow validators                  |
| Missing Validators         | CometBFT      | Validators not participating                    |
| Block Size                 | CometBFT      | Block payload size in bytes                     |
| CPU Usage                  | Node Exporter | Host CPU utilization                            |
| Memory Usage               | Node Exporter | Host memory utilization                         |
| Disk Usage                 | Node Exporter | Data volume disk utilization                    |

---

## Alerting Rules

### Prometheus Alert Rules

Save the following as `infra/prometheus/rules/hypercore_alerts.yml`:

```yaml
groups:
  - name: hypercore.block_production
    interval: 5s
    rules:
      # Block production stall: no new block in 5 seconds
      - alert: BlockProductionStalled
        expr: |
          changes(cometbft_consensus_height[30s]) == 0
        for: 5s
        labels:
          severity: critical
        annotations:
          summary: "Block production stalled on {{ $labels.instance }}"
          description: |
            No new blocks have been produced in the last 30 seconds.
            Current height: {{ $value }}.
            Check CometBFT consensus state and ABCI connection.

      # Slow block production: blocks taking more than 5s each
      - alert: SlowBlockProduction
        expr: |
          rate(cometbft_consensus_block_interval_seconds_sum[5m])
          / rate(cometbft_consensus_block_interval_seconds_count[5m])
          > 5
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "Slow block production on {{ $labels.instance }}"
          description: |
            Average block interval is {{ $value }}s (target: ~1s in CometBFT mode).
            This may indicate network issues or slow ABCI processing.

  - name: hypercore.consensus
    rules:
      # Missing validators: any validator not signing
      - alert: ValidatorMissing
        expr: cometbft_consensus_missing_validators > 0
        for: 30s
        labels:
          severity: warning
        annotations:
          summary: "{{ $value }} validators missing on {{ $labels.instance }}"
          description: |
            One or more validators are not participating in consensus.
            If missing_validators_power exceeds 1/3, consensus will stall.

      # Consensus requires multiple rounds (indicates slow validators)
      - alert: ConsensusMultipleRounds
        expr: cometbft_consensus_rounds > 0
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "Consensus taking multiple rounds on {{ $labels.instance }}"
          description: |
            Consensus round {{ $value }} > 0 for over 1 minute.
            This indicates one or more validators are slow to respond.

      # No peers connected
      - alert: NoPeersConnected
        expr: cometbft_p2p_peers == 0
        for: 30s
        labels:
          severity: critical
        annotations:
          summary: "No peers connected on {{ $labels.instance }}"
          description: |
            This node has zero P2P peers. It cannot participate in consensus.
            Check network connectivity and CometBFT P2P configuration.

  - name: hypercore.mempool
    rules:
      # Mempool overflow: approaching CometBFT mempool limit (5000)
      - alert: MempoolOverflow
        expr: cometbft_mempool_size > 4000
        for: 30s
        labels:
          severity: warning
        annotations:
          summary: "Mempool near capacity on {{ $labels.instance }}"
          description: |
            Mempool size is {{ $value }} (limit: 5000).
            Incoming transactions may be dropped.
            Check if block production is keeping up with submission rate.

      # Mempool full
      - alert: MempoolFull
        expr: cometbft_mempool_size >= 5000
        for: 10s
        labels:
          severity: critical
        annotations:
          summary: "Mempool full on {{ $labels.instance }}"
          description: |
            Mempool is at capacity ({{ $value }}/5000).
            New transactions are being rejected.

  - name: hypercore.resources
    rules:
      # Disk space low (< 20% free)
      - alert: DiskSpaceLow
        expr: |
          (node_filesystem_avail_bytes{mountpoint="/data"}
          / node_filesystem_size_bytes{mountpoint="/data"}) < 0.2
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Low disk space on {{ $labels.instance }}"
          description: |
            Only {{ $value | humanizePercentage }} disk space remaining.
            Consider cleaning old snapshots or expanding storage.

      # Disk space critical (< 5% free)
      - alert: DiskSpaceCritical
        expr: |
          (node_filesystem_avail_bytes{mountpoint="/data"}
          / node_filesystem_size_bytes{mountpoint="/data"}) < 0.05
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Critical disk space on {{ $labels.instance }}"
          description: |
            Only {{ $value | humanizePercentage }} disk space remaining.
            The node may crash if disk fills completely. Immediate action required.

      # High memory usage (> 80%)
      - alert: HighMemoryUsage
        expr: |
          (1 - (node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)) > 0.8
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High memory usage on {{ $labels.instance }}"
          description: |
            Memory usage is {{ $value | humanizePercentage }}.
            The node may be OOM-killed if usage continues to grow.

      # Critical memory usage (> 95%)
      - alert: CriticalMemoryUsage
        expr: |
          (1 - (node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)) > 0.95
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Critical memory usage on {{ $labels.instance }}"
          description: |
            Memory usage is {{ $value | humanizePercentage }}.
            OOM kill is imminent. Consider restarting the node or adding memory.

      # High CPU usage (> 90% sustained)
      - alert: HighCpuUsage
        expr: |
          100 - (avg by(instance) (rate(node_cpu_seconds_total{mode="idle"}[5m])) * 100) > 90
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "High CPU usage on {{ $labels.instance }}"
          description: |
            CPU usage is {{ $value }}% over the last 10 minutes.
            Check for runaway processes or unexpectedly high transaction volume.

  - name: hypercore.latency
    rules:
      # High block interval (potential consensus or ABCI issues)
      - alert: HighBlockInterval
        expr: |
          rate(cometbft_consensus_block_interval_seconds_sum[5m])
          / rate(cometbft_consensus_block_interval_seconds_count[5m])
          > 3
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "High block interval on {{ $labels.instance }}"
          description: |
            Average block interval is {{ $value }}s over the last 5 minutes.
            Normal CometBFT block time with timeout_commit=1s should be ~1-2s.
```

### Alertmanager Integration

Configure Alertmanager to route alerts to appropriate channels:

```yaml
# alertmanager.yml
global:
  resolve_timeout: 5m

route:
  receiver: 'default'
  group_by: ['alertname', 'cluster']
  group_wait: 10s
  group_interval: 5m
  repeat_interval: 1h

  routes:
    - match:
        severity: critical
      receiver: 'pager'
      repeat_interval: 5m

    - match:
        severity: warning
      receiver: 'slack'
      repeat_interval: 30m

receivers:
  - name: 'default'
    webhook_configs:
      - url: 'http://alertmanager-webhook:9095/alert'

  - name: 'pager'
    pagerduty_configs:
      - service_key: '<PAGERDUTY_SERVICE_KEY>'

  - name: 'slack'
    slack_configs:
      - api_url: '<SLACK_WEBHOOK_URL>'
        channel: '#hypercore-alerts'
        title: '{{ .GroupLabels.alertname }}'
        text: '{{ range .Alerts }}{{ .Annotations.description }}{{ end }}'
```

### Alert Severity Guide

| Severity  | Response Time | Examples                                             |
|-----------|---------------|------------------------------------------------------|
| Critical  | Immediate     | Block stall, no peers, disk full, AppHash mismatch   |
| Warning   | Within 1 hour | Slow blocks, high memory, missing validator          |
| Info      | Next business day | Approaching thresholds, maintenance reminders    |

### Quick Reference: What Each Alert Means

| Alert                     | Likely Cause                            | First Action                              |
|---------------------------|-----------------------------------------|-------------------------------------------|
| BlockProductionStalled    | ABCI crash, CometBFT down, network split | Check `docker compose ps` and logs        |
| SlowBlockProduction       | Slow ABCI processing, network latency   | Check node resource usage                 |
| ValidatorMissing          | Validator node down or partitioned      | Restart the missing validator             |
| ConsensusMultipleRounds   | Slow validator, high latency            | Check the slowest validator's resources   |
| NoPeersConnected          | Network issue, wrong P2P config         | Check firewall and persistent_peers       |
| MempoolOverflow           | Block production too slow               | Check block production rate               |
| DiskSpaceLow              | Snapshots accumulating, logs growing    | Clean snapshots, prune CometBFT data      |
| HighMemoryUsage           | Large state, memory leak                | Monitor trend, consider restart           |
| HighCpuUsage              | High tx volume, compaction storm        | Check if temporary; scale if sustained    |
