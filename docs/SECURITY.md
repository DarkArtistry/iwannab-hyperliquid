# Security Model

## Threat Model

### Trust Assumptions
1. **Validators are honest majority**: BFT consensus requires 2/3+ honest validators
2. **Oracle feeds are reliable**: Index prices come from trusted sources (Pyth, Chainlink)
3. **Cryptographic primitives are secure**: secp256k1, keccak256, etc.
4. **No bugs in consensus layer**: CometBFT is battle-tested

### Adversary Capabilities
- Can submit arbitrary signed messages
- Can observe mempool/pending transactions (public network)
- Can run validators (permissionless in future)
- Can deploy arbitrary EVM contracts
- Can create unlimited accounts

### Out of Scope
- Physical attacks on validators
- Social engineering of operators
- Bugs in CometBFT consensus implementation
- 51% attacks (assumed honest majority)

---

## Critical Invariants

### I1: Conservation of Value
```
sum(all_account_balances) + insurance_fund == total_deposits - total_withdrawals + total_fees_collected
```
**Verification**: Assert at end of every block.

### I2: Margin Sufficiency
```
For all accounts with open positions:
  account.equity >= account.maintenance_margin
  OR account is in liquidation queue
```
**Verification**: Check before any position-increasing action.

### I3: Orderbook Integrity
```
best_bid < best_ask (no crossed book)
sum(order.remaining) for price level == level.total_size
all orders reference valid accounts with sufficient margin
```
**Verification**: Assert after every match operation.

### I4: Position Consistency
```
For all markets:
  sum(long_positions) == sum(short_positions) (in size, not value)
  open_interest_long == sum(positive_position_sizes)
  open_interest_short == abs(sum(negative_position_sizes))
```
**Verification**: Assert after every fill.

### I5: Funding Neutrality
```
For each funding settlement:
  sum(funding_payments) == 0 (zero-sum)
```
**Verification**: Assert after funding settlement.

### I6: No Double-Spend
```
Each nonce used at most once per account
```
**Verification**: Track in consensus state.

### I7: No Unauthorized Actions
```
For all actions:
  action.signer == action.account
  signature.recover(action_hash) == action.signer
```
**Verification**: Check before processing any action.

---

## Attack Vectors & Mitigations

### A1: Price Manipulation via Orderbook
**Attack**: Place orders to manipulate mark price, trigger liquidations, profit.

**Mitigation**:
- Mark price uses EWMA of mid price, resists short-term manipulation
- Index price from external oracles provides anchor
- Large position limits prevent outsized manipulation incentive

### A2: Self-Trade Wash Trading
**Attack**: Trade with yourself to generate fake volume, manipulate funding.

**Mitigation**:
- Self-trade prevention: Orders from same account cannot match
- Resting order canceled instead of filled

### A3: Sandwich Attacks (MEV)
**Attack**: Front-run large orders, extract value.

**Mitigation**:
- CoreWriter actions are non-atomic (execute next block)
- No public mempool for direct engine actions
- Block proposer can still extract value (future: encrypted mempool)

### A4: Oracle Manipulation
**Attack**: Manipulate external oracle prices to profit.

**Mitigation**:
- Multiple oracle sources with median aggregation
- Price deviation limits (reject >5% deviation from median)
- Fallback to mark price if oracle stale

### A5: Liquidation Cascades
**Attack**: Trigger mass liquidations by moving price.

**Mitigation**:
- Partial liquidations (25% chunks) reduce market impact
- Insurance fund absorbs bankruptcy losses
- ADL spreads losses to profitable traders (last resort)

### A6: Denial of Service
**Attack**: Spam orders to overwhelm system.

**Mitigation**:
- Rate limiting per account
- Minimum order sizes
- Gas/fee for EVM actions
- Max open orders per market (200)

### A7: Replay Attacks
**Attack**: Resubmit old signed messages.

**Mitigation**:
- Nonces with timestamp-based validation
- Nonces must be greater than last used
- Valid within 1-hour window

### A8: Signature Malleability
**Attack**: Modify signature to create alternate valid signature.

**Mitigation**:
- Use EIP-712 typed data with domain separator
- Canonicalize s-value (require s < secp256k1n / 2)

### A9: Integer Overflow/Underflow
**Attack**: Cause arithmetic errors through edge case values.

**Mitigation**:
- Use checked arithmetic everywhere
- Fixed-point decimal library with overflow checks
- Fuzz testing for arithmetic edge cases

### A10: Reentrancy (EVM)
**Attack**: Reenter during EVM execution to manipulate state.

**Mitigation**:
- Precompiles are read-only, no state changes
- CoreWriter only emits events, execution is deferred
- No callbacks to user contracts during engine operations

### A11: Precompile Data Injection
**Attack**: Craft malicious input to precompiles.

**Mitigation**:
- Strict ABI decoding with length validation
- Return empty/error for invalid inputs
- No arbitrary memory access

### A12: Consensus Fork
**Attack**: Cause validators to disagree on state.

**Mitigation**:
- Deterministic execution (same inputs → same outputs)
- No floating point, no randomness, no external calls
- State commitment in each block for verification

---

## Security Controls

### Input Validation
All inputs validated before processing:
- Signatures verified against typed data hash
- Nonces checked for replay
- Prices validated against tick size
- Sizes validated against lot size
- Accounts validated for sufficient margin

### Access Control
- Actions require valid signature from account owner
- Liquidations executable by anyone (incentive-aligned)
- Admin functions (market config) require multi-sig (future)

### Monitoring & Alerts
- Track insurance fund balance
- Alert on unusual liquidation volume
- Monitor for price deviation from index
- Track validator participation

### Incident Response
1. **Pause markets**: Stop new orders, only allow closes
2. **Emergency settlement**: Force-close all positions at index price
3. **State rollback**: Revert to last known good state (requires validator coordination)

---

## Audit Checklist

### Matching Engine
- [ ] Price-time priority correctly implemented
- [ ] Partial fills handled correctly
- [ ] Self-trade prevention works
- [ ] All order types behave as specified
- [ ] Reduce-only validated correctly
- [ ] Post-only rejected when would cross

### Margin System
- [ ] Equity calculation correct
- [ ] Initial margin calculated correctly
- [ ] Maintenance margin calculated correctly
- [ ] Leverage updates validated
- [ ] Insufficient margin orders rejected

### Liquidation
- [ ] Liquidation trigger accurate
- [ ] Partial liquidation sizing correct
- [ ] Insurance fund properly funded
- [ ] ADL triggers when fund exhausted
- [ ] No race conditions in liquidation

### Funding
- [ ] Funding rate clamped correctly
- [ ] Settlement is zero-sum
- [ ] Lazy settlement works correctly
- [ ] TWAP calculation accurate

### EVM Integration
- [ ] Precompiles return correct data
- [ ] Precompiles handle malformed input
- [ ] CoreWriter events processed correctly
- [ ] No state leakage between environments
- [ ] Gas costs appropriate

### Consensus
- [ ] Block processing deterministic
- [ ] State commitment correct
- [ ] Genesis initialization correct
- [ ] Validator set changes handled (future)

---

## Bug Bounty Scope

### Critical (up to $100,000)
- Fund extraction or creation
- Position manipulation without trading
- Consensus failures leading to chain halt

### High ($10,000 - $50,000)
- Liquidation logic bypass
- Funding miscalculation
- Oracle manipulation vectors

### Medium ($1,000 - $10,000)
- DOS vectors
- Information leakage
- Incorrect order execution

### Low ($100 - $1,000)
- API inconsistencies
- Documentation errors
- Minor UX issues

---

## Cryptographic Standards

### Signature Scheme
- **Algorithm**: ECDSA over secp256k1
- **Hash**: keccak256
- **Format**: EIP-712 typed data

### Address Derivation
- **From public key**: keccak256(pubkey)[12:32]
- **Checksum**: EIP-55 mixed-case

### Commitment Scheme
- **Merkle tree**: Sparse Merkle Tree (SMT)
- **Hash**: keccak256

### Random Number Generation
- **Never** use in consensus-critical code
- Block hash + position for non-critical randomness (e.g., tiebreakers)
