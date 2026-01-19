//! Merkle tree implementation for state commitment
//!
//! This module provides a binary Merkle tree implementation that supports:
//! - Building a tree from sorted key-value entries
//! - Computing Merkle roots
//! - Generating inclusion proofs for individual entries
//! - Verifying proofs without the full tree
//!
//! ## Phase 3C: State Commitment Hardening
//!
//! This replaces the simplified sorted-hash approach with a proper Merkle tree
//! that enables:
//! - Light client verification (prove a balance without full state)
//! - State sync snapshots (verify partial state)
//! - Efficient state proofs for external verification
//!
//! ## Algorithm
//!
//! We use a binary Merkle tree with keccak256 hashing:
//! - Leaves are keccak256(key || value)
//! - Internal nodes are keccak256(left_child || right_child)
//! - If odd number of nodes at a level, duplicate the last node
//! - Empty tree has root = keccak256("")

use sha3::{Digest, Keccak256};

/// A Merkle tree node position in a proof
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofDirection {
    /// The sibling is on the left
    Left,
    /// The sibling is on the right
    Right,
}

/// A Merkle proof for a single leaf
#[derive(Debug, Clone)]
pub struct MerkleProof {
    /// The leaf value being proven (hash of key || value)
    pub leaf_hash: [u8; 32],
    /// The proof path (sibling hashes from leaf to root)
    pub siblings: Vec<[u8; 32]>,
    /// The direction of each sibling (left or right)
    pub directions: Vec<ProofDirection>,
    /// The index of the leaf in the tree
    pub leaf_index: usize,
}

impl MerkleProof {
    /// Verify the proof against a root hash
    pub fn verify(&self, root: &[u8; 32]) -> bool {
        let computed_root = self.compute_root();
        &computed_root == root
    }

    /// Compute the root from this proof
    pub fn compute_root(&self) -> [u8; 32] {
        let mut current = self.leaf_hash;

        for (sibling, direction) in self.siblings.iter().zip(self.directions.iter()) {
            current = match direction {
                ProofDirection::Left => hash_pair(sibling, &current),
                ProofDirection::Right => hash_pair(&current, sibling),
            };
        }

        current
    }

    /// Serialize the proof to bytes (for storage/transmission)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32 + 4 + self.siblings.len() * 33);

        // Leaf hash (32 bytes)
        bytes.extend_from_slice(&self.leaf_hash);

        // Leaf index (4 bytes, little-endian)
        bytes.extend_from_slice(&(self.leaf_index as u32).to_le_bytes());

        // Number of siblings (4 bytes)
        bytes.extend_from_slice(&(self.siblings.len() as u32).to_le_bytes());

        // Each sibling: 32 bytes hash + 1 byte direction
        for (sibling, direction) in self.siblings.iter().zip(self.directions.iter()) {
            bytes.extend_from_slice(sibling);
            bytes.push(match direction {
                ProofDirection::Left => 0,
                ProofDirection::Right => 1,
            });
        }

        bytes
    }

    /// Deserialize a proof from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 40 {
            return None;
        }

        let mut offset = 0;

        // Leaf hash
        let mut leaf_hash = [0u8; 32];
        leaf_hash.copy_from_slice(&bytes[offset..offset + 32]);
        offset += 32;

        // Leaf index
        let leaf_index = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        // Number of siblings
        let num_siblings = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        // Read siblings
        let mut siblings = Vec::with_capacity(num_siblings);
        let mut directions = Vec::with_capacity(num_siblings);

        for _ in 0..num_siblings {
            if offset + 33 > bytes.len() {
                return None;
            }

            let mut sibling = [0u8; 32];
            sibling.copy_from_slice(&bytes[offset..offset + 32]);
            offset += 32;

            let direction = match bytes[offset] {
                0 => ProofDirection::Left,
                1 => ProofDirection::Right,
                _ => return None,
            };
            offset += 1;

            siblings.push(sibling);
            directions.push(direction);
        }

        Some(MerkleProof {
            leaf_hash,
            siblings,
            directions,
            leaf_index,
        })
    }
}

/// A Merkle tree for state commitment
#[derive(Debug, Clone)]
pub struct MerkleTree {
    /// All nodes in the tree, level by level (leaves first)
    /// Level 0: leaves
    /// Level 1: first internal level
    /// ...
    /// Last level: root (single node)
    levels: Vec<Vec<[u8; 32]>>,
}

impl MerkleTree {
    /// Create an empty Merkle tree
    pub fn empty() -> Self {
        MerkleTree {
            levels: vec![vec![]],
        }
    }

    /// Build a Merkle tree from a list of leaf hashes
    ///
    /// The leaves should already be hashed (keccak256 of the entry data).
    /// For an empty list, returns a tree with root = keccak256("").
    /// For a single leaf, returns root = hash(leaf || leaf) to match standard practice.
    pub fn from_leaves(leaves: Vec<[u8; 32]>) -> Self {
        if leaves.is_empty() {
            let empty_root = hash_bytes(&[]);
            return MerkleTree {
                levels: vec![vec![], vec![empty_root]],
            };
        }

        let mut levels = vec![leaves];

        // Build tree levels until we reach root (single node at level > 0)
        // Note: We always hash pairs, so even a single leaf becomes hash(leaf || leaf)
        loop {
            let current_level = levels.last().unwrap();

            // If we have a single node and we're past level 0, that's our root
            if current_level.len() == 1 && levels.len() > 1 {
                break;
            }

            let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

            let mut i = 0;
            while i < current_level.len() {
                let left = &current_level[i];
                // If odd number or single element, duplicate last node
                let right = if i + 1 < current_level.len() {
                    &current_level[i + 1]
                } else {
                    left
                };
                next_level.push(hash_pair(left, right));
                i += 2;
            }

            let is_root = next_level.len() == 1;
            levels.push(next_level);

            if is_root {
                break;
            }
        }

        MerkleTree { levels }
    }

    /// Build a Merkle tree from sorted key-value entries
    ///
    /// Each entry is hashed as keccak256(key || value).
    pub fn from_entries<K: AsRef<[u8]>, V: AsRef<[u8]>>(entries: &[(K, V)]) -> Self {
        let leaves: Vec<[u8; 32]> = entries
            .iter()
            .map(|(k, v)| hash_entry(k.as_ref(), v.as_ref()))
            .collect();
        Self::from_leaves(leaves)
    }

    /// Get the Merkle root
    pub fn root(&self) -> [u8; 32] {
        if self.levels.is_empty() || self.levels.last().unwrap().is_empty() {
            return hash_bytes(&[]);
        }
        self.levels.last().unwrap()[0]
    }

    /// Get the number of leaves
    pub fn leaf_count(&self) -> usize {
        if self.levels.is_empty() {
            0
        } else {
            self.levels[0].len()
        }
    }

    /// Generate a proof for a leaf at the given index
    pub fn prove(&self, leaf_index: usize) -> Option<MerkleProof> {
        if self.levels.is_empty() || leaf_index >= self.levels[0].len() {
            return None;
        }

        let leaf_hash = self.levels[0][leaf_index];
        let mut siblings = Vec::new();
        let mut directions = Vec::new();

        let mut index = leaf_index;
        for level in 0..self.levels.len() - 1 {
            let current_level = &self.levels[level];
            let is_right = index % 2 == 1;

            let sibling_index = if is_right {
                index - 1
            } else {
                // If odd number of nodes and we're the last, duplicate self
                if index + 1 >= current_level.len() {
                    index
                } else {
                    index + 1
                }
            };

            siblings.push(current_level[sibling_index]);
            directions.push(if is_right {
                ProofDirection::Left
            } else {
                ProofDirection::Right
            });

            index /= 2;
        }

        Some(MerkleProof {
            leaf_hash,
            siblings,
            directions,
            leaf_index,
        })
    }

    /// Verify a proof against this tree's root
    pub fn verify_proof(&self, proof: &MerkleProof) -> bool {
        proof.verify(&self.root())
    }
}

/// Hash two child nodes to create parent
#[inline]
fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// Hash a key-value entry
#[inline]
pub fn hash_entry(key: &[u8], value: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(key);
    hasher.update(value);
    hasher.finalize().into()
}

/// Hash arbitrary bytes
#[inline]
pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Verify a Merkle proof without the full tree
///
/// This is the primary method for light client verification.
pub fn verify_proof(proof: &MerkleProof, root: &[u8; 32]) -> bool {
    proof.verify(root)
}

/// Compute the expected leaf hash for a key-value pair
pub fn compute_leaf_hash(key: &[u8], value: &[u8]) -> [u8; 32] {
    hash_entry(key, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let tree = MerkleTree::empty();
        let expected_root = hash_bytes(&[]);
        assert_eq!(tree.root(), expected_root);
        assert_eq!(tree.leaf_count(), 0);
    }

    #[test]
    fn test_single_leaf() {
        let leaf = hash_bytes(b"hello");
        let tree = MerkleTree::from_leaves(vec![leaf]);

        // Single leaf: root = hash(leaf || leaf)
        let expected_root = hash_pair(&leaf, &leaf);
        assert_eq!(tree.root(), expected_root);
        assert_eq!(tree.leaf_count(), 1);

        // Verify proof
        let proof = tree.prove(0).unwrap();
        assert!(proof.verify(&tree.root()));
    }

    #[test]
    fn test_two_leaves() {
        let leaf1 = hash_bytes(b"hello");
        let leaf2 = hash_bytes(b"world");
        let tree = MerkleTree::from_leaves(vec![leaf1, leaf2]);

        let expected_root = hash_pair(&leaf1, &leaf2);
        assert_eq!(tree.root(), expected_root);
        assert_eq!(tree.leaf_count(), 2);

        // Verify proofs for both leaves
        let proof1 = tree.prove(0).unwrap();
        assert!(proof1.verify(&tree.root()));
        assert_eq!(proof1.leaf_hash, leaf1);

        let proof2 = tree.prove(1).unwrap();
        assert!(proof2.verify(&tree.root()));
        assert_eq!(proof2.leaf_hash, leaf2);
    }

    #[test]
    fn test_four_leaves() {
        let leaves: Vec<[u8; 32]> = (0..4)
            .map(|i| hash_bytes(&[i as u8]))
            .collect();
        let tree = MerkleTree::from_leaves(leaves.clone());

        assert_eq!(tree.leaf_count(), 4);

        // Verify all proofs
        for i in 0..4 {
            let proof = tree.prove(i).unwrap();
            assert!(proof.verify(&tree.root()), "Proof {} should verify", i);
            assert_eq!(proof.leaf_hash, leaves[i]);
        }

        // Manually verify tree structure
        let level1_left = hash_pair(&leaves[0], &leaves[1]);
        let level1_right = hash_pair(&leaves[2], &leaves[3]);
        let expected_root = hash_pair(&level1_left, &level1_right);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_odd_leaves() {
        let leaves: Vec<[u8; 32]> = (0..5)
            .map(|i| hash_bytes(&[i as u8]))
            .collect();
        let tree = MerkleTree::from_leaves(leaves.clone());

        assert_eq!(tree.leaf_count(), 5);

        // Verify all proofs
        for i in 0..5 {
            let proof = tree.prove(i).unwrap();
            assert!(proof.verify(&tree.root()), "Proof {} should verify", i);
        }
    }

    #[test]
    fn test_from_entries() {
        let entries = vec![
            (b"key1".to_vec(), b"value1".to_vec()),
            (b"key2".to_vec(), b"value2".to_vec()),
            (b"key3".to_vec(), b"value3".to_vec()),
        ];

        let tree = MerkleTree::from_entries(&entries);
        assert_eq!(tree.leaf_count(), 3);

        // Verify each entry's proof
        for i in 0..3 {
            let proof = tree.prove(i).unwrap();
            assert!(proof.verify(&tree.root()));

            // Verify the leaf hash matches the entry
            let expected_leaf = hash_entry(&entries[i].0, &entries[i].1);
            assert_eq!(proof.leaf_hash, expected_leaf);
        }
    }

    #[test]
    fn test_proof_serialization() {
        let leaves: Vec<[u8; 32]> = (0..8)
            .map(|i| hash_bytes(&[i as u8]))
            .collect();
        let tree = MerkleTree::from_leaves(leaves);

        let proof = tree.prove(3).unwrap();
        let bytes = proof.to_bytes();
        let restored = MerkleProof::from_bytes(&bytes).unwrap();

        assert_eq!(proof.leaf_hash, restored.leaf_hash);
        assert_eq!(proof.leaf_index, restored.leaf_index);
        assert_eq!(proof.siblings.len(), restored.siblings.len());
        assert!(restored.verify(&tree.root()));
    }

    #[test]
    fn test_invalid_proof() {
        let leaves: Vec<[u8; 32]> = (0..4)
            .map(|i| hash_bytes(&[i as u8]))
            .collect();
        let tree = MerkleTree::from_leaves(leaves);

        let mut proof = tree.prove(0).unwrap();
        // Tamper with a sibling
        proof.siblings[0] = [0u8; 32];

        assert!(!proof.verify(&tree.root()));
    }

    #[test]
    fn test_large_tree() {
        let leaves: Vec<[u8; 32]> = (0..1000)
            .map(|i| hash_bytes(&(i as u32).to_le_bytes()))
            .collect();
        let tree = MerkleTree::from_leaves(leaves);

        assert_eq!(tree.leaf_count(), 1000);

        // Verify some random proofs
        for i in [0, 1, 499, 500, 998, 999] {
            let proof = tree.prove(i).unwrap();
            assert!(proof.verify(&tree.root()), "Proof {} should verify", i);
        }
    }

    #[test]
    fn test_deterministic_root() {
        // Same entries should always produce same root
        let entries = vec![
            (b"a".to_vec(), b"1".to_vec()),
            (b"b".to_vec(), b"2".to_vec()),
            (b"c".to_vec(), b"3".to_vec()),
        ];

        let tree1 = MerkleTree::from_entries(&entries);
        let tree2 = MerkleTree::from_entries(&entries);

        assert_eq!(tree1.root(), tree2.root());
    }

    #[test]
    fn test_different_entries_different_roots() {
        let entries1 = vec![(b"a".to_vec(), b"1".to_vec())];
        let entries2 = vec![(b"a".to_vec(), b"2".to_vec())];

        let tree1 = MerkleTree::from_entries(&entries1);
        let tree2 = MerkleTree::from_entries(&entries2);

        assert_ne!(tree1.root(), tree2.root());
    }

    #[test]
    fn test_proof_for_invalid_index() {
        let tree = MerkleTree::from_leaves(vec![hash_bytes(b"test")]);
        assert!(tree.prove(1).is_none());
        assert!(tree.prove(100).is_none());
    }

    #[test]
    fn test_verify_proof_standalone() {
        let leaves: Vec<[u8; 32]> = (0..4)
            .map(|i| hash_bytes(&[i as u8]))
            .collect();
        let tree = MerkleTree::from_leaves(leaves);
        let root = tree.root();

        let proof = tree.prove(2).unwrap();

        // Verify using standalone function
        assert!(verify_proof(&proof, &root));

        // Verify with wrong root
        let wrong_root = [0u8; 32];
        assert!(!verify_proof(&proof, &wrong_root));
    }
}
