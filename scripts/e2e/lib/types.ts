/**
 * E2E Test Types
 *
 * Shared TypeScript interfaces used across all test files.
 */

/** Result of a single test execution */
export interface TestResult {
  /** Test name displayed in output */
  name: string;
  /** Test category for grouping (e.g., 'connection', 'orders', 'evm') */
  category: string;
  /** Description of what the test validates */
  description: string;
  /** Test outcome */
  status: 'pass' | 'fail' | 'skip';
  /** Execution time in milliseconds */
  duration: number;
  /** Error message if test failed */
  error?: string;
}

/** Shared context passed to all test functions */
export interface TestContext {
  /** Accumulated test results */
  results: TestResult[];
  /** Timestamp when test suite started */
  startTime: number;
}

/** Signature components for exchange requests */
export interface SignatureWire {
  r: string;
  s: string;
  v: number;
}

/** Result of signing an action */
export interface SignedAction {
  signature: SignatureWire;
  nonce: number;
  address: string;
}
