/**
 * E2E Test Modules
 *
 * Re-exports all test category functions for convenient importing.
 *
 * Usage:
 *   import {
 *     runConnectionTests,
 *     runMarketDataTests,
 *     runOrderTests,
 *     // ... etc
 *   } from './tests/index.js';
 */

// 1. Connection & Health Tests
export { runConnectionTests } from './connection.js';

// 2. Market Data Tests
export { runMarketDataTests } from './market-data.js';

// 3. Account State Tests
export { runAccountTests } from './account.js';

// 4. Order Lifecycle Tests
export { runOrderTests } from './orders.js';

// 5. Matching Engine Tests
export { runMatchingTests } from './matching.js';

// 6. Position Management Tests
export { runPositionTests } from './positions.js';

// 7. EVM Integration Tests
export { runEVMTests } from './evm.js';

// 8. Advanced EVM Tests
export { runAdvancedEVMTests } from './evm-advanced.js';

// 9. Token Standards Tests
export { runTokenStandardsTests } from './tokens.js';

// 10. Spot Trading Tests
export { runSpotTests } from './spot.js';

// 12. Unified State Tests
export { runUnifiedStateTests } from './unified.js';

// 13. Stress & Performance Tests
export { runStressTests } from './stress.js';

// 14. Advanced Real-World Scenario Tests
export { runAdvancedTests } from './advanced.js';

// 15. Risk & Margin Tests
export { runRiskTests } from './risk.js';

// 16. State Proof Tests (Phase 3C)
export { runStateProofTests } from './state-proofs.js';

// 17. Liquidation Tests (Phase 8B)
export { runLiquidationTests } from './liquidation.js';

// 18. EVM Precompile Tests (Phase 8C)
export { runEvmPrecompileTests } from './evm-precompile.js';
