//! Divergence Handler for State Attestation
//!
//! This module handles divergence alerts from the AttestationCollector.
//! When state divergence is detected, the handler takes appropriate action
//! based on the configured policy.
//!
//! ## Divergence Policies
//!
//! - **Log**: Just log the divergence (for monitoring)
//! - **Halt**: Stop block production/execution to prevent further divergence
//! - **HaltAndResync**: Halt and attempt to resync from the majority
//!
//! ## Safety
//!
//! When we detect that we're in the minority, halting is critical to prevent
//! our node from processing blocks that won't be accepted by the network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::attestation_collector::DivergenceAlert;

/// Policy for handling divergence
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergencePolicy {
    /// Just log the divergence (for monitoring/alerting only)
    Log,
    /// Halt block production/execution
    Halt,
    /// Halt and attempt to resync from majority (requires state sync)
    HaltAndResync,
}

impl Default for DivergencePolicy {
    fn default() -> Self {
        Self::Halt
    }
}

/// Configuration for the divergence handler
#[derive(Debug, Clone, Copy)]
pub struct DivergenceConfig {
    /// Policy to apply when divergence is detected
    pub policy: DivergencePolicy,
    /// Whether to halt only when we're in the minority (default: true)
    /// If false, halt on any divergence even if we're in the majority
    pub halt_only_on_minority: bool,
    // Note: Custom alert callback can be passed separately to the handler
}

impl Default for DivergenceConfig {
    fn default() -> Self {
        Self {
            policy: DivergencePolicy::Halt,
            halt_only_on_minority: true,
        }
    }
}

/// Handler that receives divergence alerts and takes action
pub struct DivergenceHandler {
    /// Atomic flag indicating if the node is halted
    halted: Arc<AtomicBool>,
    /// The height at which divergence was detected (if halted)
    divergence_height: Arc<tokio::sync::RwLock<Option<u64>>>,
    /// Configuration
    config: DivergenceConfig,
    /// Alert history (for debugging)
    alert_history: Arc<tokio::sync::RwLock<Vec<DivergenceAlert>>>,
}

impl DivergenceHandler {
    /// Create a new divergence handler
    pub fn new(config: DivergenceConfig) -> Self {
        Self {
            halted: Arc::new(AtomicBool::new(false)),
            divergence_height: Arc::new(tokio::sync::RwLock::new(None)),
            config,
            alert_history: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Get the halted flag (can be shared with block producer)
    pub fn halted_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.halted)
    }

    /// Check if the node is halted
    pub fn is_halted(&self) -> bool {
        self.halted.load(Ordering::SeqCst)
    }

    /// Get the height at which divergence was detected
    pub async fn divergence_height(&self) -> Option<u64> {
        *self.divergence_height.read().await
    }

    /// Manually clear the halted state (USE WITH CAUTION)
    ///
    /// This should only be used after manual investigation and
    /// potentially resyncing state.
    pub async fn clear_halt(&self) {
        self.halted.store(false, Ordering::SeqCst);
        *self.divergence_height.write().await = None;
        tracing::warn!("Divergence halt cleared manually - ensure state is correct!");
    }

    /// Run the handler loop, processing alerts from the collector
    pub async fn run(&self, mut alert_rx: mpsc::Receiver<DivergenceAlert>) {
        tracing::info!(
            "Divergence handler started with policy: {:?}",
            self.config.policy
        );

        while let Some(alert) = alert_rx.recv().await {
            self.handle_alert(alert).await;
        }

        tracing::info!("Divergence handler stopped");
    }

    /// Handle a single divergence alert
    pub async fn handle_alert(&self, alert: DivergenceAlert) {
        // Store in history
        {
            let mut history = self.alert_history.write().await;
            history.push(alert.clone());
            // Keep last 100 alerts
            if history.len() > 100 {
                history.remove(0);
            }
        }

        // Log the divergence
        tracing::error!(
            "DIVERGENCE ALERT at height {}: {} different hashes",
            alert.height,
            alert.hash_votes.len()
        );

        for (hash, power) in &alert.hash_votes {
            tracing::error!(
                "  Hash {:?}: {} voting power",
                hex::encode(&hash[..8]),
                power
            );
        }

        if alert.we_are_minority {
            tracing::error!(
                "*** WE ARE IN THE MINORITY *** Our hash: {:?}",
                hex::encode(&alert.our_hash[..8])
            );
        }

        // Apply policy
        match self.config.policy {
            DivergencePolicy::Log => {
                tracing::warn!("Divergence logged (policy: Log) - not halting");
            }
            DivergencePolicy::Halt => {
                if self.config.halt_only_on_minority && !alert.we_are_minority {
                    tracing::warn!(
                        "Divergence detected but we're in majority - not halting (halt_only_on_minority=true)"
                    );
                } else {
                    self.halt(alert.height).await;
                }
            }
            DivergencePolicy::HaltAndResync => {
                if self.config.halt_only_on_minority && !alert.we_are_minority {
                    tracing::warn!(
                        "Divergence detected but we're in majority - not halting (halt_only_on_minority=true)"
                    );
                } else {
                    self.halt(alert.height).await;
                    self.initiate_resync(alert.height, alert.majority_hash).await;
                }
            }
        }
    }

    /// Halt the node
    async fn halt(&self, height: u64) {
        tracing::error!("*** HALTING NODE due to state divergence at height {} ***", height);
        tracing::error!("Block production and execution will stop.");
        tracing::error!("Manual intervention required to resume.");

        self.halted.store(true, Ordering::SeqCst);
        *self.divergence_height.write().await = Some(height);
    }

    /// Initiate resync from majority (placeholder for future implementation)
    async fn initiate_resync(&self, height: u64, majority_hash: Option<[u8; 32]>) {
        if let Some(hash) = majority_hash {
            tracing::error!(
                "Resync requested to majority hash {:?} at height {}",
                hex::encode(&hash[..8]),
                height
            );
            // TODO: Trigger state sync to resync from majority
            // This would involve:
            // 1. Finding a peer with the majority hash
            // 2. Requesting a state snapshot
            // 3. Applying the snapshot
            // 4. Resuming from the correct state
            tracing::error!("Automatic resync not yet implemented - manual intervention required");
        } else {
            tracing::error!(
                "Cannot resync - no clear majority hash at height {}",
                height
            );
        }
    }

    /// Get alert history
    pub async fn get_alert_history(&self) -> Vec<DivergenceAlert> {
        self.alert_history.read().await.clone()
    }

    /// Get the last alert
    pub async fn last_alert(&self) -> Option<DivergenceAlert> {
        self.alert_history.read().await.last().cloned()
    }
}

/// Create a complete attestation system (collector + handler)
///
/// Returns the collector, the handler, and a channel for broadcasting attestations.
pub fn create_attestation_system(
    our_pubkey: Option<[u8; 32]>,
    collector_config: super::attestation_collector::AttestationConfig,
    handler_config: DivergenceConfig,
) -> (
    super::attestation_collector::AttestationCollector,
    DivergenceHandler,
) {
    let (alert_tx, alert_rx) = mpsc::channel(100);

    let collector = super::attestation_collector::AttestationCollector::new(
        alert_tx,
        collector_config,
        our_pubkey,
    );

    let handler = DivergenceHandler::new(handler_config);

    // Spawn the handler task
    let handler_clone = handler.halted_flag();
    tokio::spawn(async move {
        let handler = DivergenceHandler {
            halted: handler_clone,
            divergence_height: Arc::new(tokio::sync::RwLock::new(None)),
            config: handler_config,
            alert_history: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        };
        handler.run(alert_rx).await;
    });

    (collector, handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn create_test_alert(height: u64, we_are_minority: bool) -> DivergenceAlert {
        let mut hash_votes = BTreeMap::new();
        hash_votes.insert([0x42u8; 32], 200);
        hash_votes.insert([0x43u8; 32], 100);

        DivergenceAlert {
            height,
            our_hash: if we_are_minority { [0x43u8; 32] } else { [0x42u8; 32] },
            majority_hash: Some([0x42u8; 32]),
            attestations: Vec::new(),
            we_are_minority,
            hash_votes,
        }
    }

    #[tokio::test]
    async fn test_handler_creation() {
        let handler = DivergenceHandler::new(DivergenceConfig::default());
        assert!(!handler.is_halted());
        assert!(handler.divergence_height().await.is_none());
    }

    #[tokio::test]
    async fn test_log_policy_no_halt() {
        let handler = DivergenceHandler::new(DivergenceConfig {
            policy: DivergencePolicy::Log,
            halt_only_on_minority: true,
        });

        let alert = create_test_alert(100, true);
        handler.handle_alert(alert).await;

        assert!(!handler.is_halted(), "Log policy should not halt");
    }

    #[tokio::test]
    async fn test_halt_policy_minority() {
        let handler = DivergenceHandler::new(DivergenceConfig {
            policy: DivergencePolicy::Halt,
            halt_only_on_minority: true,
        });

        let alert = create_test_alert(100, true);
        handler.handle_alert(alert).await;

        assert!(handler.is_halted(), "Should halt when in minority");
        assert_eq!(handler.divergence_height().await, Some(100));
    }

    #[tokio::test]
    async fn test_halt_policy_majority_no_halt() {
        let handler = DivergenceHandler::new(DivergenceConfig {
            policy: DivergencePolicy::Halt,
            halt_only_on_minority: true,
        });

        let alert = create_test_alert(100, false); // We're in majority
        handler.handle_alert(alert).await;

        assert!(!handler.is_halted(), "Should not halt when in majority");
    }

    #[tokio::test]
    async fn test_halt_always_on_divergence() {
        let handler = DivergenceHandler::new(DivergenceConfig {
            policy: DivergencePolicy::Halt,
            halt_only_on_minority: false, // Halt on any divergence
        });

        let alert = create_test_alert(100, false); // Even though we're in majority
        handler.handle_alert(alert).await;

        assert!(handler.is_halted(), "Should halt on any divergence");
    }

    #[tokio::test]
    async fn test_clear_halt() {
        let handler = DivergenceHandler::new(DivergenceConfig::default());

        // Trigger halt
        let alert = create_test_alert(100, true);
        handler.handle_alert(alert).await;
        assert!(handler.is_halted());

        // Clear halt
        handler.clear_halt().await;
        assert!(!handler.is_halted());
        assert!(handler.divergence_height().await.is_none());
    }

    #[tokio::test]
    async fn test_alert_history() {
        let handler = DivergenceHandler::new(DivergenceConfig {
            policy: DivergencePolicy::Log, // Don't halt
            halt_only_on_minority: true,
        });

        handler.handle_alert(create_test_alert(100, false)).await;
        handler.handle_alert(create_test_alert(101, false)).await;
        handler.handle_alert(create_test_alert(102, false)).await;

        let history = handler.get_alert_history().await;
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].height, 100);
        assert_eq!(history[2].height, 102);
    }

    #[tokio::test]
    async fn test_last_alert() {
        let handler = DivergenceHandler::new(DivergenceConfig {
            policy: DivergencePolicy::Log,
            halt_only_on_minority: true,
        });

        assert!(handler.last_alert().await.is_none());

        handler.handle_alert(create_test_alert(100, false)).await;
        handler.handle_alert(create_test_alert(200, false)).await;

        let last = handler.last_alert().await.unwrap();
        assert_eq!(last.height, 200);
    }

    #[tokio::test]
    async fn test_halted_flag_sharing() {
        let handler = DivergenceHandler::new(DivergenceConfig::default());
        let flag = handler.halted_flag();

        assert!(!flag.load(Ordering::SeqCst));

        // Trigger halt
        handler.handle_alert(create_test_alert(100, true)).await;

        // Flag should be updated
        assert!(flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_handler_run_loop() {
        let (tx, rx) = mpsc::channel(10);
        let handler = DivergenceHandler::new(DivergenceConfig {
            policy: DivergencePolicy::Log,
            halt_only_on_minority: true,
        });

        // Spawn handler
        let handler_flag = handler.halted_flag();
        let handler_history = Arc::clone(&handler.alert_history);
        tokio::spawn(async move {
            handler.run(rx).await;
        });

        // Send alerts
        tx.send(create_test_alert(100, false)).await.unwrap();
        tx.send(create_test_alert(101, false)).await.unwrap();

        // Give it time to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Check history was updated
        let history = handler_history.read().await;
        assert_eq!(history.len(), 2);
    }
}
