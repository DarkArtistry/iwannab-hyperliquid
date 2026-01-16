/**
 * Test script to verify EIP-712 hash computation matches between TypeScript and Rust
 */

import { hashTypedData, keccak256, toBytes, encodeAbiParameters, parseAbiParameters } from 'viem';

// Domain from our SDK
const domain = {
  name: 'HyperCore',
  version: '1',
  chainId: 1337,
  verifyingContract: '0x0000000000000000000000000000000000000000' as `0x${string}`,
};

// Type definition for usdTransfer
const types = {
  Action: [
    { name: 'type', type: 'string' },
    { name: 'destination', type: 'address' },
    { name: 'amount', type: 'string' },
    { name: 'nonce', type: 'uint64' },
  ],
};

// Test message
const message = {
  type: 'usdTransfer',
  destination: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8' as `0x${string}`,
  amount: '10',
  nonce: BigInt(1234567890),
};

// Compute the hash using viem
const typedDataHash = hashTypedData({
  domain,
  types,
  primaryType: 'Action',
  message,
});

console.log('=== TypeScript EIP-712 Hash Computation ===\n');

// Compute individual components for comparison
const typeString = 'Action(string type,address destination,string amount,uint64 nonce)';
const typeHash = keccak256(toBytes(typeString));
console.log(`Type hash: ${typeHash}`);
console.log(`Expected:  0x041360a30ca50a93c33ed1aa1bf450088b97ffb4f3c5ae3524618a207a30716c`);
console.log(`Match: ${typeHash === '0x041360a30ca50a93c33ed1aa1bf450088b97ffb4f3c5ae3524618a207a30716c'}\n`);

// keccak256 of 'usdTransfer'
const typeValueHash = keccak256(toBytes('usdTransfer'));
console.log(`Encoded 'usdTransfer': ${typeValueHash}`);
console.log(`Expected:              0x5ece99db8c94c6b90e6d2adcb8644d06099ea05d4f32f9ad0bd00fdf338a08e0`);
console.log(`Match: ${typeValueHash === '0x5ece99db8c94c6b90e6d2adcb8644d06099ea05d4f32f9ad0bd00fdf338a08e0'}\n`);

// keccak256 of '10'
const amountHash = keccak256(toBytes('10'));
console.log(`Encoded amount '10': ${amountHash}`);
console.log(`Expected:            0x1a192fabce13988b84994d4296e6cdc418d55e2f1d7f942188d4040b94fc57ac`);
console.log(`Match: ${amountHash === '0x1a192fabce13988b84994d4296e6cdc418d55e2f1d7f942188d4040b94fc57ac'}\n`);

// Final typed data hash
console.log(`Full typed data hash: ${typedDataHash}`);

// Compute domain separator
const domainTypeHash = keccak256(toBytes('EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)'));
console.log(`\nDomain type hash: ${domainTypeHash}`);

const nameHash = keccak256(toBytes('HyperCore'));
const versionHash = keccak256(toBytes('1'));
console.log(`Name hash: ${nameHash}`);
console.log(`Version hash: ${versionHash}`);

// Encode domain separator
const domainSeparator = keccak256(
  encodeAbiParameters(
    parseAbiParameters('bytes32, bytes32, bytes32, uint256, address'),
    [domainTypeHash, nameHash, versionHash, BigInt(1337), '0x0000000000000000000000000000000000000000']
  )
);
console.log(`\nDomain separator: ${domainSeparator}`);
console.log(`Expected:         0x92aaf364dd0f262cc209ea8386d47271db0c8f54586f165ba16c3bcb65a1e85d`);
console.log(`Match: ${domainSeparator === '0x92aaf364dd0f262cc209ea8386d47271db0c8f54586f165ba16c3bcb65a1e85d'}\n`);

// Now let's verify what viem actually computes for the struct hash
// By encoding it manually
const structHashManual = keccak256(
  encodeAbiParameters(
    parseAbiParameters('bytes32, bytes32, address, bytes32, uint64'),
    [
      typeHash as `0x${string}`,
      typeValueHash as `0x${string}`,
      '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
      amountHash as `0x${string}`,
      BigInt(1234567890),
    ]
  )
);
console.log(`Manual struct hash: ${structHashManual}`);
console.log(`Expected (Rust):    0xc67a5811c2a3691aef8057d15ee740069892e267716451a195b93be98994fa75\n`);

// Check if they match
console.log('=== Summary ===');
console.log(`Type hash matches: ${typeHash === '0x041360a30ca50a93c33ed1aa1bf450088b97ffb4f3c5ae3524618a207a30716c'}`);
console.log(`Domain separator matches: ${domainSeparator === '0x92aaf364dd0f262cc209ea8386d47271db0c8f54586f165ba16c3bcb65a1e85d'}`);
console.log(`Struct hash matches: ${structHashManual === '0xc67a5811c2a3691aef8057d15ee740069892e267716451a195b93be98994fa75'}`);
