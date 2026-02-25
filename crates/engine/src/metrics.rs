//! Application-level Prometheus metrics for HyperCore

use lazy_static::lazy_static;
use prometheus::{
    Histogram, IntCounter, IntGauge, Registry,
    register_histogram_with_registry, register_int_counter_with_registry,
    register_int_gauge_with_registry,
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new_custom(Some("hypercore".to_string()), None).unwrap();

    // Block metrics
    pub static ref BLOCK_HEIGHT: IntGauge = register_int_gauge_with_registry!(
        "block_height", "Current block height", REGISTRY
    ).unwrap();
    pub static ref BLOCK_PRODUCTION_DURATION_MS: Histogram = register_histogram_with_registry!(
        "block_production_duration_ms", "Block production latency in milliseconds",
        vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0],
        REGISTRY
    ).unwrap();

    // Transaction metrics
    pub static ref TX_COUNT_TOTAL: IntCounter = register_int_counter_with_registry!(
        "tx_count_total", "Total transactions processed", REGISTRY
    ).unwrap();
    pub static ref TX_ERRORS_TOTAL: IntCounter = register_int_counter_with_registry!(
        "tx_errors_total", "Total transaction errors", REGISTRY
    ).unwrap();

    // Mempool metrics
    pub static ref MEMPOOL_SIZE: IntGauge = register_int_gauge_with_registry!(
        "mempool_size", "Current mempool transaction count", REGISTRY
    ).unwrap();

    // Engine metrics
    pub static ref MATCH_LATENCY_US: Histogram = register_histogram_with_registry!(
        "match_latency_us", "Order matching latency in microseconds",
        vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0],
        REGISTRY
    ).unwrap();
    pub static ref FILLS_TOTAL: IntCounter = register_int_counter_with_registry!(
        "fills_total", "Total fills executed", REGISTRY
    ).unwrap();
    pub static ref LIQUIDATIONS_TOTAL: IntCounter = register_int_counter_with_registry!(
        "liquidations_total", "Total liquidations executed", REGISTRY
    ).unwrap();
    pub static ref OPEN_ORDERS_TOTAL: IntGauge = register_int_gauge_with_registry!(
        "open_orders_total", "Current total open orders", REGISTRY
    ).unwrap();

    // Persistence metrics
    pub static ref PERSISTENCE_WRITE_DURATION_MS: Histogram = register_histogram_with_registry!(
        "persistence_write_duration_ms", "State persistence write latency in milliseconds",
        vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0],
        REGISTRY
    ).unwrap();
    pub static ref SNAPSHOT_HEIGHT: IntGauge = register_int_gauge_with_registry!(
        "snapshot_height", "Latest snapshot height", REGISTRY
    ).unwrap();

    // Gateway metrics
    pub static ref WEBSOCKET_CONNECTIONS: IntGauge = register_int_gauge_with_registry!(
        "websocket_connections", "Active WebSocket connections", REGISTRY
    ).unwrap();
    pub static ref GATEWAY_REQUESTS_TOTAL: IntCounter = register_int_counter_with_registry!(
        "gateway_requests_total", "Total gateway HTTP requests", REGISTRY
    ).unwrap();
    pub static ref GATEWAY_REQUEST_DURATION_MS: Histogram = register_histogram_with_registry!(
        "gateway_request_duration_ms", "Gateway request duration in milliseconds",
        vec![0.5, 1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0],
        REGISTRY
    ).unwrap();

    // EVM metrics
    pub static ref EVM_TX_TOTAL: IntCounter = register_int_counter_with_registry!(
        "evm_tx_total", "Total EVM transactions processed", REGISTRY
    ).unwrap();
}
