# Contributing to HyperCore

Thank you for your interest in contributing to HyperCore! This document provides guidelines and instructions for contributing.

## Code of Conduct

- Be respectful and inclusive
- Focus on constructive feedback
- Help others learn and grow

## Getting Started

### Development Environment

1. **Clone the repository**
   ```bash
   git clone <repo-url>
   cd iwannab-hyperliquid
   ```

2. **Install Rust**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup default stable
   ```

3. **Install Foundry**
   ```bash
   curl -L https://foundry.paradigm.xyz | bash
   foundryup
   ```

4. **Install Node.js dependencies**
   ```bash
   cd sdk/typescript
   pnpm install
   ```

5. **Build the project**
   ```bash
   cargo build
   ```

6. **Run tests**
   ```bash
   cargo test --workspace --features cometbft   # Rust tests (556 tests)
   cd contracts && forge test                    # Solidity tests (49 tests)
   ./scripts/e2e-test.sh                        # E2E integration (151 tests)
   ```

## Project Structure

```
iwannab-hyperliquid/
├── crates/                    # Rust workspace (8 crates)
│   ├── primitives/            # Core types (Decimal, Order, Position, UnifiedState)
│   ├── engine/                # Matching engine, risk, funding, liquidation
│   ├── chain/                 # CometBFT ABCI app, Merkle proofs, attestation
│   ├── evm/                   # HyperEVM with precompiles and JSON-RPC
│   ├── gateway/               # HTTP/WebSocket API server with rate limiting
│   ├── indexer/               # PostgreSQL data indexing and candle aggregation
│   ├── persistence/           # RocksDB state persistence (24 column families)
│   └── node/                  # Main binary (single-node & CometBFT modes)
├── contracts/                 # Solidity
├── sdk/
│   ├── typescript/            # TypeScript SDK
│   └── python/                # Python SDK
└── docs/                      # Documentation
```

## Development Workflow

### 1. Create a Branch

```bash
# Feature branch
git checkout -b feature/your-feature-name

# Bug fix branch
git checkout -b fix/bug-description

# Documentation branch
git checkout -b docs/what-you-are-documenting
```

### 2. Make Changes

Follow the coding standards below and ensure your changes:
- Include appropriate tests
- Pass all existing tests
- Include documentation updates if needed

### 3. Test Your Changes

```bash
# Run all Rust tests (556 tests)
cargo test --workspace --features cometbft

# Run Solidity tests (49 tests)
cd contracts && forge test -vvv

# Run E2E integration tests (151 tests, requires Docker)
./scripts/e2e-test.sh

# Run multi-node tests (52 tests, requires Docker)
make test-multinode-full

# Run everything (823 tests total)
make test-all
```

### 4. Commit Your Changes

We follow conventional commits:

```bash
# Format
<type>(<scope>): <description>

# Examples
feat(engine): add partial fill support
fix(gateway): handle websocket reconnection
docs(api): update order placement examples
test(contracts): add integration tests
refactor(primitives): simplify decimal parsing
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `test`: Adding/updating tests
- `refactor`: Code refactoring
- `perf`: Performance improvement
- `chore`: Maintenance tasks

### 5. Submit a Pull Request

1. Push your branch:
   ```bash
   git push origin feature/your-feature-name
   ```

2. Open a PR on GitHub with:
   - Clear title following conventional commits
   - Description of changes
   - Link to any related issues
   - Screenshots if UI changes

## Coding Standards

### Rust

- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Document public APIs with doc comments

```rust
/// Process an order and return any fills.
///
/// # Arguments
/// * `order` - The order to process
/// * `market_id` - The market identifier
///
/// # Returns
/// A tuple of (modified order, fills)
///
/// # Errors
/// Returns `Error::MarketNotFound` if market doesn't exist
pub fn process_order(
    &mut self,
    order: Order,
    market_id: MarketId,
) -> Result<(Order, Vec<Fill>)> {
    // Implementation
}
```

### Solidity

- Use Foundry for development
- Follow [Solidity Style Guide](https://docs.soliditylang.org/en/latest/style-guide.html)
- Use `forge fmt` for formatting
- Document with NatSpec

```solidity
/// @notice Place a limit order
/// @param params Order parameters
/// @return actionId Unique identifier for tracking
function placeOrder(OrderParams calldata params)
    external
    returns (bytes32 actionId)
{
    // Implementation
}
```

### TypeScript

- Use TypeScript strict mode
- Use `pnpm lint` for linting
- Use `pnpm format` for formatting
- Document with JSDoc

```typescript
/**
 * Place a limit order on the exchange.
 *
 * @param request - Order parameters
 * @returns Order result with status and fills
 * @throws HyperCoreError if order is rejected
 *
 * @example
 * ```typescript
 * const result = await client.exchange.placeOrder({
 *   market: 'BTC-PERP',
 *   side: OrderSide.Buy,
 *   price: '65000',
 *   size: '0.1',
 * });
 * ```
 */
async placeOrder(request: OrderRequest): Promise<OrderResponse> {
  // Implementation
}
```

## Testing Guidelines

### Unit Tests

- Test one thing per test
- Use descriptive test names
- Include edge cases

```rust
#[test]
fn test_orderbook_should_match_crossing_orders() {
    // Setup
    let mut book = OrderBook::new(market_id);

    // Add orders
    book.add_order(bid_order);
    book.add_order(ask_order);

    // Assert match occurred
    assert_eq!(book.bid_count(), 0);
    assert_eq!(fills.len(), 1);
}
```

### Integration Tests

- Test complete workflows
- Use realistic scenarios
- Document the test purpose

```typescript
/**
 * @scenario Place and fill a limit order
 * @given Alice has sufficient margin
 * @when Alice places limit buy for 0.1 BTC
 * @and Bob places market sell for 0.1 BTC
 * @then Orders match at Alice's price
 */
test('should match limit and market orders', async () => {
  // Test implementation
});
```

### Solidity Tests

- Use Foundry's testing framework
- Test success and failure cases
- Use fuzz tests for edge cases

```solidity
function test_placeOrder_success() public {
    // Success case
}

function test_placeOrder_revert_zeroSize() public {
    vm.expectRevert("size must be positive");
    coreWriter.placeOrder(zeroSizeOrder);
}

function testFuzz_placeOrder(uint128 price, uint128 size) public {
    vm.assume(price > 0 && size > 0);
    // Fuzz test
}
```

## Documentation

### Code Documentation

- Document all public APIs
- Include examples where helpful
- Explain non-obvious behavior

### User Documentation

- Update relevant docs with code changes
- Keep examples working and tested
- Use clear, concise language

### Architecture Documentation

- Document design decisions
- Explain trade-offs
- Include diagrams where helpful

## Review Process

1. **Automated Checks**
   - All tests must pass
   - Code must be formatted
   - No clippy warnings

2. **Code Review**
   - At least one approval required
   - Address all feedback
   - Keep discussions constructive

3. **Merge**
   - Squash commits for clean history
   - Use conventional commit for merge message

## Areas for Contribution

### Good First Issues

- Documentation improvements
- Test coverage increases
- Minor bug fixes
- Code cleanup

### Intermediate

- New API endpoints
- SDK features
- Performance improvements
- Additional test scenarios

### Advanced

- Core engine features
- Consensus integration
- EVM precompiles
- Security improvements

## Getting Help

- Open an issue for bugs or features
- Use discussions for questions
- Tag maintainers if blocked

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
