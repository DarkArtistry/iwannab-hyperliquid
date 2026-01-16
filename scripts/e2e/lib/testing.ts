/**
 * Test Runner Utilities
 *
 * Core test execution and helper functions.
 */

import { logTest } from './logging.js';
import type { TestContext, TestResult } from './types.js';

/**
 * Run a single test and record the result
 *
 * @param ctx - Test context to accumulate results
 * @param name - Test name displayed in output
 * @param category - Category for grouping results
 * @param description - What the test validates
 * @param testFn - Async function that performs the test
 */
export async function runTest(
  ctx: TestContext,
  name: string,
  category: string,
  description: string,
  testFn: () => Promise<void>
): Promise<void> {
  const start = Date.now();

  try {
    await testFn();
    const duration = Date.now() - start;
    const result: TestResult = { name, category, description, status: 'pass', duration };
    ctx.results.push(result);
    logTest(result);
  } catch (error) {
    const duration = Date.now() - start;
    const errorMessage = error instanceof Error ? error.message : String(error);
    const result: TestResult = { name, category, description, status: 'fail', duration, error: errorMessage };
    ctx.results.push(result);
    logTest(result);
  }
}

/**
 * Skip a test and record the reason
 *
 * @param ctx - Test context to accumulate results
 * @param name - Test name displayed in output
 * @param category - Category for grouping results
 * @param description - What the test would validate
 * @param reason - Why the test is being skipped
 */
export function skipTest(
  ctx: TestContext,
  name: string,
  category: string,
  description: string,
  reason: string
): void {
  const result: TestResult = { name, category, description, status: 'skip', duration: 0, error: reason };
  ctx.results.push(result);
  logTest(result);
}

/**
 * Sleep for a specified duration
 *
 * @param ms - Milliseconds to sleep
 */
export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
