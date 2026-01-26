//! P2P Gossip Layer for State Attestations
//!
//! This module implements the networking layer for broadcasting state attestations
//! between validators using LibP2P's GossipSub protocol.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      HyperCore Node                             │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │  ┌─────────────────┐     broadcast_tx      ┌────────────────┐  │
//! │  │  BlockProducer  │ ─────────────────────►│ AttestationGossip│ │
//! │  │  (creates       │                       │                 │  │
//! │  │   attestations) │                       │  ┌───────────┐  │  │
//! │  └─────────────────┘                       │  │ GossipSub │  │  │
//! │                                            │  │  (LibP2P) │  │  │
//! │                                            │  └─────┬─────┘  │  │
//! │  ┌─────────────────┐   incoming_rx         │        │        │  │
//! │  │  Attestation    │◄─────────────────────│        │        │  │
//! │  │   Collector     │                       │        │        │  │
//! │  └─────────────────┘                       └────────┼────────┘  │
//! │                                                     │           │
//! └─────────────────────────────────────────────────────┼───────────┘
//!                                                       │
//!                                               LibP2P P2P Network
//!                                           (attestation topic gossip)
//! ```
//!
//! ## Features
//!
//! - **GossipSub Protocol**: Efficient pub/sub for attestation messages
//! - **Bootstrap Peers**: Manual peer list for production deployments
//! - **Noise Protocol**: Encrypted connections between peers
//! - **Identify Protocol**: Peer identification and capability exchange
//!
//! ## Usage
//!
//! ```ignore
//! let config = GossipConfig {
//!     listen_addr: "/ip4/0.0.0.0/tcp/26670".parse()?,
//!     bootstrap_peers: vec!["/ip4/1.2.3.4/tcp/26670/p2p/QmPeer...".parse()?],
//!     ..Default::default()
//! };
//!
//! let (gossip, broadcast_tx, incoming_rx) = AttestationGossip::new(config).await?;
//!
//! // Start the gossip service
//! tokio::spawn(gossip.run());
//!
//! // Send attestations via broadcast_tx
//! broadcast_tx.send(attestation).await?;
//!
//! // Receive attestations via incoming_rx
//! while let Some(attestation) = incoming_rx.recv().await {
//!     collector.process_attestation(attestation).await?;
//! }
//! ```

#[cfg(feature = "p2p")]
pub mod gossip {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::Duration;

    use futures::StreamExt;
    use libp2p::{
        gossipsub::{self, IdentTopic, MessageAuthenticity, MessageId, ValidationMode},
        identity::Keypair,
        noise,
        swarm::{NetworkBehaviour, SwarmEvent},
        tcp, yamux, Multiaddr, PeerId, Swarm,
    };
    use tokio::sync::mpsc;

    use crate::attestation::StateAttestation;

    /// GossipSub topic for state attestations
    const ATTESTATION_TOPIC: &str = "hypercore/attestations/1.0.0";

    /// Configuration for the attestation gossip layer
    #[derive(Debug, Clone)]
    pub struct GossipConfig {
        /// Address to listen on (e.g., "/ip4/0.0.0.0/tcp/26670")
        pub listen_addr: Multiaddr,
        /// Bootstrap peers to connect to on startup
        pub bootstrap_peers: Vec<Multiaddr>,
        /// GossipSub heartbeat interval
        pub heartbeat_interval: Duration,
        /// Maximum number of mesh peers per topic
        pub mesh_n: usize,
        /// Minimum number of mesh peers per topic
        pub mesh_n_low: usize,
        /// Maximum number of mesh peers per topic
        pub mesh_n_high: usize,
        /// Channel buffer size for outgoing attestations
        pub broadcast_buffer: usize,
        /// Channel buffer size for incoming attestations
        pub incoming_buffer: usize,
    }

    impl Default for GossipConfig {
        fn default() -> Self {
            Self {
                listen_addr: "/ip4/0.0.0.0/tcp/26670".parse().unwrap(),
                bootstrap_peers: Vec::new(),
                heartbeat_interval: Duration::from_secs(1),
                mesh_n: 6,
                mesh_n_low: 4,
                mesh_n_high: 12,
                broadcast_buffer: 256,
                incoming_buffer: 256,
            }
        }
    }

    /// Combined network behaviour for attestation gossip
    #[derive(NetworkBehaviour)]
    struct AttestationBehaviour {
        gossipsub: gossipsub::Behaviour,
        identify: libp2p::identify::Behaviour,
    }

    /// The attestation gossip service
    pub struct AttestationGossip {
        swarm: Swarm<AttestationBehaviour>,
        topic: IdentTopic,
        broadcast_rx: mpsc::Receiver<StateAttestation>,
        incoming_tx: mpsc::Sender<StateAttestation>,
        local_peer_id: PeerId,
        config: GossipConfig,
    }

    impl AttestationGossip {
        /// Create a new attestation gossip service
        ///
        /// Returns the gossip service, a sender for broadcasting attestations,
        /// and a receiver for incoming attestations from the network.
        pub async fn new(
            config: GossipConfig,
        ) -> Result<
            (
                Self,
                mpsc::Sender<StateAttestation>,
                mpsc::Receiver<StateAttestation>,
            ),
            GossipError,
        > {
            // Generate a new keypair for this node
            let local_key = Keypair::generate_ed25519();
            let local_peer_id = PeerId::from(local_key.public());

            tracing::info!("Local peer ID: {}", local_peer_id);

            // Create the GossipSub behaviour
            let gossipsub = Self::create_gossipsub(&local_key, &config)?;

            // Create identify behaviour for peer identification
            let identify = libp2p::identify::Behaviour::new(
                libp2p::identify::Config::new(
                    "/hypercore/1.0.0".to_string(),
                    local_key.public(),
                ),
            );

            // Combine behaviours
            let behaviour = AttestationBehaviour {
                gossipsub,
                identify,
            };

            // Create the swarm
            let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
                .with_tokio()
                .with_tcp(
                    tcp::Config::default(),
                    noise::Config::new,
                    yamux::Config::default,
                )
                .map_err(|e| GossipError::SwarmError(e.to_string()))?
                .with_behaviour(|_| behaviour)
                .map_err(|e| GossipError::SwarmError(e.to_string()))?
                .with_swarm_config(|cfg| {
                    cfg.with_idle_connection_timeout(Duration::from_secs(60))
                })
                .build();

            // Create channels
            let (broadcast_tx, broadcast_rx) = mpsc::channel(config.broadcast_buffer);
            let (incoming_tx, incoming_rx) = mpsc::channel(config.incoming_buffer);

            let topic = IdentTopic::new(ATTESTATION_TOPIC);

            let gossip = Self {
                swarm,
                topic,
                broadcast_rx,
                incoming_tx,
                local_peer_id,
                config,
            };

            Ok((gossip, broadcast_tx, incoming_rx))
        }

        /// Create a new attestation gossip service with an existing keypair
        pub async fn with_keypair(
            keypair: Keypair,
            config: GossipConfig,
        ) -> Result<
            (
                Self,
                mpsc::Sender<StateAttestation>,
                mpsc::Receiver<StateAttestation>,
            ),
            GossipError,
        > {
            let local_peer_id = PeerId::from(keypair.public());

            tracing::info!("Local peer ID: {}", local_peer_id);

            // Create the GossipSub behaviour
            let gossipsub = Self::create_gossipsub(&keypair, &config)?;

            // Create identify behaviour
            let identify = libp2p::identify::Behaviour::new(
                libp2p::identify::Config::new(
                    "/hypercore/1.0.0".to_string(),
                    keypair.public(),
                ),
            );

            let behaviour = AttestationBehaviour {
                gossipsub,
                identify,
            };

            let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
                .with_tokio()
                .with_tcp(
                    tcp::Config::default(),
                    noise::Config::new,
                    yamux::Config::default,
                )
                .map_err(|e| GossipError::SwarmError(e.to_string()))?
                .with_behaviour(|_| behaviour)
                .map_err(|e| GossipError::SwarmError(e.to_string()))?
                .with_swarm_config(|cfg| {
                    cfg.with_idle_connection_timeout(Duration::from_secs(60))
                })
                .build();

            let (broadcast_tx, broadcast_rx) = mpsc::channel(config.broadcast_buffer);
            let (incoming_tx, incoming_rx) = mpsc::channel(config.incoming_buffer);

            let topic = IdentTopic::new(ATTESTATION_TOPIC);

            let gossip = Self {
                swarm,
                topic,
                broadcast_rx,
                incoming_tx,
                local_peer_id,
                config,
            };

            Ok((gossip, broadcast_tx, incoming_rx))
        }

        fn create_gossipsub(
            local_key: &Keypair,
            config: &GossipConfig,
        ) -> Result<gossipsub::Behaviour, GossipError> {
            // Custom message ID function to prevent duplicate messages
            let message_id_fn = |message: &gossipsub::Message| {
                let mut hasher = DefaultHasher::new();
                message.data.hash(&mut hasher);
                MessageId::from(hasher.finish().to_be_bytes().to_vec())
            };

            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(config.heartbeat_interval)
                .validation_mode(ValidationMode::Strict)
                .mesh_n(config.mesh_n)
                .mesh_n_low(config.mesh_n_low)
                .mesh_n_high(config.mesh_n_high)
                .message_id_fn(message_id_fn)
                .build()
                .map_err(|e| GossipError::GossipsubError(e.to_string()))?;

            gossipsub::Behaviour::new(
                MessageAuthenticity::Signed(local_key.clone()),
                gossipsub_config,
            )
            .map_err(|e| GossipError::GossipsubError(e.to_string()))
        }

        /// Get the local peer ID
        pub fn local_peer_id(&self) -> PeerId {
            self.local_peer_id
        }

        /// Run the gossip service
        ///
        /// This method runs indefinitely, handling:
        /// - Listening for connections
        /// - Connecting to bootstrap peers
        /// - Publishing attestations from the broadcast channel
        /// - Receiving attestations from the network
        pub async fn run(mut self) -> Result<(), GossipError> {
            // Start listening
            self.swarm
                .listen_on(self.config.listen_addr.clone())
                .map_err(|e| GossipError::ListenError(e.to_string()))?;

            tracing::info!(
                "Attestation gossip listening on {}",
                self.config.listen_addr
            );

            // Subscribe to attestation topic
            self.swarm
                .behaviour_mut()
                .gossipsub
                .subscribe(&self.topic)
                .map_err(|e| GossipError::SubscribeError(e.to_string()))?;

            tracing::info!("Subscribed to topic: {}", ATTESTATION_TOPIC);

            // Connect to bootstrap peers
            for addr in &self.config.bootstrap_peers {
                tracing::info!("Dialing bootstrap peer: {}", addr);
                if let Err(e) = self.swarm.dial(addr.clone()) {
                    tracing::warn!("Failed to dial bootstrap peer {}: {}", addr, e);
                }
            }

            // Main event loop
            loop {
                tokio::select! {
                    // Handle outgoing attestations
                    Some(attestation) = self.broadcast_rx.recv() => {
                        self.publish_attestation(attestation).await;
                    }

                    // Handle swarm events
                    event = self.swarm.select_next_some() => {
                        self.handle_swarm_event(event).await;
                    }
                }
            }
        }

        /// Publish an attestation to the network
        async fn publish_attestation(&mut self, attestation: StateAttestation) {
            let data = attestation.to_bytes();

            match self
                .swarm
                .behaviour_mut()
                .gossipsub
                .publish(self.topic.clone(), data)
            {
                Ok(message_id) => {
                    tracing::debug!(
                        "Published attestation for height {} (message_id: {:?})",
                        attestation.height,
                        message_id
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to publish attestation for height {}: {:?}",
                        attestation.height,
                        e
                    );
                }
            }
        }

        /// Handle a swarm event
        async fn handle_swarm_event(&mut self, event: SwarmEvent<AttestationBehaviourEvent>) {
            match event {
                SwarmEvent::Behaviour(AttestationBehaviourEvent::Gossipsub(
                    gossipsub::Event::Message {
                        propagation_source,
                        message_id,
                        message,
                    },
                )) => {
                    self.handle_gossipsub_message(propagation_source, message_id, message)
                        .await;
                }

                SwarmEvent::Behaviour(AttestationBehaviourEvent::Gossipsub(
                    gossipsub::Event::Subscribed { peer_id, topic },
                )) => {
                    tracing::debug!("Peer {} subscribed to {}", peer_id, topic);
                }

                SwarmEvent::Behaviour(AttestationBehaviourEvent::Identify(
                    libp2p::identify::Event::Received { peer_id, info, .. },
                )) => {
                    tracing::debug!(
                        "Identified peer {}: {} ({:?})",
                        peer_id,
                        info.protocol_version,
                        info.protocols
                    );
                    // Add identified peer to gossipsub mesh
                    self.swarm
                        .behaviour_mut()
                        .gossipsub
                        .add_explicit_peer(&peer_id);
                }

                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!("Listening on {}/p2p/{}", address, self.local_peer_id);
                }

                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    tracing::info!(
                        "Connection established with {} via {:?}",
                        peer_id,
                        endpoint
                    );
                }

                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    tracing::debug!("Connection closed with {}: {:?}", peer_id, cause);
                }

                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    tracing::warn!(
                        "Outgoing connection error to {:?}: {}",
                        peer_id,
                        error
                    );
                }

                _ => {}
            }
        }

        /// Handle a received GossipSub message
        async fn handle_gossipsub_message(
            &self,
            source: PeerId,
            message_id: MessageId,
            message: gossipsub::Message,
        ) {
            // Verify the message is from the attestation topic
            if message.topic != self.topic.hash() {
                return;
            }

            // Deserialize the attestation
            match StateAttestation::from_bytes(&message.data) {
                Some(attestation) => {
                    tracing::debug!(
                        "Received attestation from {} for height {} (msg_id: {:?})",
                        source,
                        attestation.height,
                        message_id
                    );

                    // Forward to the attestation collector
                    if let Err(e) = self.incoming_tx.send(attestation).await {
                        tracing::warn!("Failed to forward attestation to collector: {}", e);
                    }
                }
                None => {
                    tracing::warn!(
                        "Failed to deserialize attestation from {} (msg_id: {:?})",
                        source,
                        message_id
                    );
                }
            }
        }
    }

    /// Errors that can occur in the gossip layer
    #[derive(Debug, thiserror::Error)]
    pub enum GossipError {
        #[error("Failed to create GossipSub: {0}")]
        GossipsubError(String),
        #[error("Failed to create swarm: {0}")]
        SwarmError(String),
        #[error("Failed to listen: {0}")]
        ListenError(String),
        #[error("Failed to subscribe to topic: {0}")]
        SubscribeError(String),
        #[error("Failed to dial peer: {0}")]
        DialError(String),
    }

    /// Statistics about the gossip network
    #[derive(Debug, Clone, Default)]
    pub struct GossipStats {
        /// Number of connected peers
        pub connected_peers: usize,
        /// Number of mesh peers for attestation topic
        pub mesh_peers: usize,
        /// Number of attestations published
        pub attestations_published: u64,
        /// Number of attestations received
        pub attestations_received: u64,
    }
}

#[cfg(feature = "p2p")]
pub use gossip::*;

#[cfg(test)]
#[cfg(feature = "p2p")]
mod tests {
    use super::gossip::*;
    use crate::attestation::{AttestationKeyPair, StateAttestation};

    #[tokio::test]
    async fn test_gossip_config_default() {
        let config = GossipConfig::default();
        assert_eq!(config.listen_addr.to_string(), "/ip4/0.0.0.0/tcp/26670");
        assert_eq!(config.mesh_n, 6);
    }

    #[tokio::test]
    async fn test_gossip_creation() {
        let config = GossipConfig {
            listen_addr: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
            ..Default::default()
        };

        let result = AttestationGossip::new(config).await;
        assert!(result.is_ok());

        let (gossip, _tx, _rx) = result.unwrap();
        assert!(!gossip.local_peer_id().to_string().is_empty());
    }

    #[tokio::test]
    async fn test_attestation_serialization_for_gossip() {
        let key = AttestationKeyPair::generate();
        let attestation = key.sign_attestation(100, [0x42u8; 32]);

        let bytes = attestation.to_bytes();
        let restored = StateAttestation::from_bytes(&bytes).unwrap();

        assert_eq!(attestation, restored);
    }
}
