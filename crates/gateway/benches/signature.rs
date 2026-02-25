//! Signature verification benchmarks
//!
//! Measures the performance of cryptographic operations in the gateway:
//! - EIP-712 signature recovery (ecrecover)
//! - Keccak256 hashing (used for EIP-712 typed data hashing)
//! - Hex decoding of signature components
//! - Signature construction from raw bytes

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use alloy_primitives::B256;
use hypercore_primitives::Signature;
use sha3::{Digest, Keccak256};

/// A pre-computed message hash (32 bytes) for benchmarking.
/// This is a Keccak256 hash of some arbitrary data.
fn sample_message_hash() -> [u8; 32] {
    Keccak256::digest(b"benchmark test message for signature verification").into()
}

/// A valid-format (but not necessarily valid-recovery) signature for benchmarking.
/// The r and s components are non-zero 32-byte values with v=27.
fn sample_signature() -> Signature {
    // These are syntactically valid signature components (non-zero, correct length)
    // but may not recover to a valid address. That is fine for benchmarking the
    // computational cost of the recovery attempt itself.
    let r = B256::from([
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
    ]);
    let s = B256::from([
        0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
        0x0f, 0x1e, 0x2d, 0x3c, 0x4b, 0x5a, 0x69, 0x78,
        0x87, 0x96, 0xa5, 0xb4, 0xc3, 0xd2, 0xe1, 0xf0,
        0x13, 0x24, 0x35, 0x46, 0x57, 0x68, 0x79, 0x8a,
    ]);
    Signature::new(r, s, 27)
}

/// Benchmark: Signature::recover() - the most expensive crypto operation.
///
/// This is the core of EIP-712 signature verification. It performs:
/// 1. Combine r || s into 64-byte signature
/// 2. Parse k256 signature
/// 3. Compute recovery ID from v
/// 4. Recover verifying key (secp256k1 point recovery - the expensive part)
/// 5. Hash public key to Ethereum address
///
/// Note: We benchmark the full recover() call including error handling.
/// Some calls may fail with invalid signatures, but the computational
/// cost is dominated by the elliptic curve operation which runs regardless.
fn bench_signature_recover(c: &mut Criterion) {
    let hash = sample_message_hash();
    let sig = sample_signature();

    c.bench_function("signature_recover", |b| {
        b.iter(|| {
            // The recover call exercises the full ecrecover path.
            // Even if this particular signature doesn't recover to a valid address,
            // it exercises the k256 library's point recovery code path.
            let _ = black_box(sig.recover(black_box(&hash)));
        });
    });
}

/// Benchmark: Keccak256 hashing at various input sizes.
///
/// Keccak256 is used extensively in EIP-712:
/// - Domain separator computation
/// - Type hash computation
/// - Struct hash computation
/// - Final typed data hash (\x19\x01 || domain || struct_hash)
fn bench_keccak256(c: &mut Criterion) {
    let mut group = c.benchmark_group("keccak256");

    for &size in &[32usize, 64, 128, 256, 512] {
        let data = vec![0xabu8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &data,
            |b, data| {
                b.iter(|| {
                    let _: [u8; 32] = black_box(Keccak256::digest(black_box(data)).into());
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: EIP-712 domain separator computation.
///
/// The domain separator is computed once per chain ID and cached,
/// but this measures the raw cost.
fn bench_domain_separator(c: &mut Criterion) {
    c.bench_function("eip712_domain_separator", |b| {
        b.iter(|| {
            // Simulate domain separator computation:
            // keccak256(abi.encode(typeHash, nameHash, versionHash, chainId))
            let type_hash = Keccak256::digest(
                b"EIP712Domain(string name,string version,uint256 chainId)"
            );
            let name_hash = Keccak256::digest(b"HyperCore");
            let version_hash = Keccak256::digest(b"1");

            let mut hasher = Keccak256::new();
            hasher.update(type_hash);
            hasher.update(name_hash);
            hasher.update(version_hash);
            // chain_id as 32-byte big-endian
            let mut chain_id_bytes = [0u8; 32];
            chain_id_bytes[24..32].copy_from_slice(&421614u64.to_be_bytes());
            hasher.update(chain_id_bytes);
            let _: [u8; 32] = black_box(hasher.finalize().into());
        });
    });
}

/// Benchmark: hex decoding of signature r/s components.
///
/// Every incoming exchange request has r and s as hex strings that must
/// be decoded to bytes before signature verification.
fn bench_hex_decode_signature(c: &mut Criterion) {
    let r_hex = "0x123456789abcdef011223344556677889900aabbccddeeff0011223344556677";
    let s_hex = "0xfedcba987654321001234567890abcdef0fedcba987654321001234567890abcd";

    c.bench_function("hex_decode_r_s", |b| {
        b.iter(|| {
            let r_clean = r_hex.strip_prefix("0x").unwrap_or(r_hex);
            let s_clean = s_hex.strip_prefix("0x").unwrap_or(s_hex);
            let r_bytes = black_box(hex::decode(r_clean).unwrap());
            let s_bytes = black_box(hex::decode(s_clean).unwrap());
            let _ = black_box(B256::from_slice(&r_bytes));
            let _ = black_box(B256::from_slice(&s_bytes));
        });
    });
}

/// Benchmark: Signature construction from components.
///
/// Measures the cost of creating a Signature struct from decoded bytes.
fn bench_signature_construction(c: &mut Criterion) {
    let r_bytes = [0x12u8; 32];
    let s_bytes = [0x34u8; 32];

    c.bench_function("signature_new", |b| {
        b.iter(|| {
            let r = B256::from(black_box(r_bytes));
            let s = B256::from(black_box(s_bytes));
            let _ = black_box(Signature::new(r, s, 27));
        });
    });
}

/// Benchmark: Signature to_bytes / from_bytes round-trip.
///
/// Serialization format: 65 bytes (r || s || v).
fn bench_signature_serialization(c: &mut Criterion) {
    let sig = sample_signature();
    let bytes = sig.to_bytes();

    let mut group = c.benchmark_group("signature_serde");

    group.bench_function("to_bytes", |b| {
        b.iter(|| {
            let _ = black_box(sig.to_bytes());
        });
    });

    group.bench_function("from_bytes", |b| {
        b.iter(|| {
            let _ = black_box(Signature::from_bytes(black_box(&bytes)));
        });
    });

    group.finish();
}

/// Benchmark: Full EIP-712 typed data hash computation.
///
/// Simulates the complete hash computation for a CancelAll action:
/// keccak256("\x19\x01" || domainSeparator || structHash)
fn bench_eip712_full_hash(c: &mut Criterion) {
    // Pre-compute domain separator (would be cached in production)
    let domain_separator = {
        let type_hash = Keccak256::digest(
            b"EIP712Domain(string name,string version,uint256 chainId)"
        );
        let name_hash = Keccak256::digest(b"HyperCore");
        let version_hash = Keccak256::digest(b"1");
        let mut hasher = Keccak256::new();
        hasher.update(type_hash);
        hasher.update(name_hash);
        hasher.update(version_hash);
        let mut chain_id_bytes = [0u8; 32];
        chain_id_bytes[24..32].copy_from_slice(&421614u64.to_be_bytes());
        hasher.update(chain_id_bytes);
        let result: [u8; 32] = hasher.finalize().into();
        result
    };

    c.bench_function("eip712_full_typed_data_hash", |b| {
        b.iter(|| {
            // Struct hash for CancelAll: keccak256(typeHash || encode("cancelAll") || encode(nonce))
            let action_type_hash = Keccak256::digest(b"Action(string type,uint64 nonce)");
            let type_string_hash = Keccak256::digest(b"cancelAll");
            let mut nonce_bytes = [0u8; 32];
            nonce_bytes[24..32].copy_from_slice(&12345u64.to_be_bytes());

            let mut struct_hasher = Keccak256::new();
            struct_hasher.update(action_type_hash);
            struct_hasher.update(type_string_hash);
            struct_hasher.update(nonce_bytes);
            let struct_hash: [u8; 32] = struct_hasher.finalize().into();

            // Final hash: keccak256("\x19\x01" || domainSeparator || structHash)
            let mut final_hasher = Keccak256::new();
            final_hasher.update(b"\x19\x01");
            final_hasher.update(black_box(domain_separator));
            final_hasher.update(black_box(struct_hash));
            let _: [u8; 32] = black_box(final_hasher.finalize().into());
        });
    });
}

/// Benchmark: batch signature verification throughput.
///
/// Measures how many signatures can be verified per second,
/// simulating a burst of incoming exchange requests.
fn bench_batch_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("signature_batch_verify");

    for &batch_size in &[10u64, 50, 100] {
        let hashes: Vec<[u8; 32]> = (0..batch_size)
            .map(|i| {
                let data = format!("message {}", i);
                Keccak256::digest(data.as_bytes()).into()
            })
            .collect();
        let sigs: Vec<Signature> = (0..batch_size).map(|_| sample_signature()).collect();

        group.throughput(Throughput::Elements(batch_size));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for (hash, sig) in hashes.iter().zip(sigs.iter()) {
                        let _ = black_box(sig.recover(black_box(hash)));
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_signature_recover,
    bench_keccak256,
    bench_domain_separator,
    bench_hex_decode_signature,
    bench_signature_construction,
    bench_signature_serialization,
    bench_eip712_full_hash,
    bench_batch_verify,
);

criterion_main!(benches);
