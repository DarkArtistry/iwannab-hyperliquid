/**
 * E2E Test Library
 *
 * Re-exports all shared utilities for convenient importing.
 *
 * Usage:
 *   import { CONFIG, TEST_ACCOUNTS, infoRequest, signAction, runTest } from '../lib/index.js';
 */

// Configuration
export { CONFIG, MARKETS } from './config.js';

// Test accounts
export { TEST_ACCOUNTS } from './accounts.js';
export type { TestAccount } from './accounts.js';

// Types
export type { TestResult, TestContext, SignatureWire, SignedAction } from './types.js';

// Logging
export { colors, log, logHeader, logSection, logTest, logProgress } from './logging.js';

// API helpers
export { infoRequest, exchangeRequest } from './api.js';

// Signing
export { signAction } from './signing.js';

// Test runner
export { runTest, skipTest, sleep } from './testing.js';
