/**
 * HyperCore E2E Integration Tests
 *
 * Main test runner that orchestrates all test categories.
 *
 * Structure:
 *   lib/       - Shared utilities (config, signing, API helpers, etc.)
 *   tests/     - Test modules organized by category
 *   runner.ts  - This file: main orchestrator
 *
 * Usage:
 *   npx tsx runner.ts
 *
 * Environment Variables:
 *   GATEWAY_URL  - Gateway API URL (default: http://localhost:8080)
 *   EVM_RPC_URL  - EVM RPC URL (default: http://localhost:8545)
 *   CHAIN_ID     - Chain ID (default: 999)
 *   VERBOSE      - Enable verbose logging (default: false)
 */

// Shared utilities
import { CONFIG, colors, log, logHeader } from './lib/index.js';
import type { TestContext } from './lib/index.js';

// Test modules
import {
  runConnectionTests,
  runMarketDataTests,
  runAccountTests,
  runOrderTests,
  runMatchingTests,
  runPositionTests,
  runEVMTests,
  runAdvancedEVMTests,
  runTokenStandardsTests,
  runSpotTests,
  runUnifiedStateTests,
  runStressTests,
  runAdvancedTests,
  runRiskTests,
  runStateProofTests,
} from './tests/index.js';

// ============================================================================
// SUMMARY
// ============================================================================

function printSummary(ctx: TestContext): void {
  const totalDuration = Date.now() - ctx.startTime;

  logHeader('E2E Test Summary');

  const passed = ctx.results.filter((r) => r.status === 'pass').length;
  const failed = ctx.results.filter((r) => r.status === 'fail').length;
  const skipped = ctx.results.filter((r) => r.status === 'skip').length;
  const total = ctx.results.length;

  log(`  ${colors.white}Total Tests:${colors.reset}    ${total}`);
  log(`  ${colors.green}Passed:${colors.reset}         ${passed}`);
  log(`  ${colors.red}Failed:${colors.reset}         ${failed}`);
  log(`  ${colors.yellow}Skipped:${colors.reset}        ${skipped}`);
  log('');
  log(`  ${colors.white}Duration:${colors.reset}       ${(totalDuration / 1000).toFixed(2)}s`);
  log('');

  // Group by category
  const categories = [...new Set(ctx.results.map((r) => r.category))];

  log(`  ${colors.white}Results by Category:${colors.reset}`);
  for (const category of categories) {
    const catResults = ctx.results.filter((r) => r.category === category);
    const catPassed = catResults.filter((r) => r.status === 'pass').length;
    const catTotal = catResults.length;
    const icon = catPassed === catTotal ? `${colors.green}✓` : `${colors.red}✗`;
    log(`    ${icon} ${category}: ${catPassed}/${catTotal}${colors.reset}`);
  }
  log('');

  // Print failed tests
  const failedTests = ctx.results.filter((r) => r.status === 'fail');
  if (failedTests.length > 0) {
    log(`  ${colors.red}Failed Tests:${colors.reset}`);
    for (const test of failedTests) {
      log(`    ${colors.red}✗ ${test.name}${colors.reset}`);
      if (test.error) {
        log(`      ${colors.yellow}${test.error}${colors.reset}`);
      }
    }
    log('');
  }

  if (failed === 0) {
    log(`  ${colors.green}╔════════════════════════════════════╗${colors.reset}`);
    log(`  ${colors.green}║     ALL TESTS PASSED! ✓            ║${colors.reset}`);
    log(`  ${colors.green}╚════════════════════════════════════╝${colors.reset}`);
  } else {
    log(`  ${colors.red}╔════════════════════════════════════╗${colors.reset}`);
    log(`  ${colors.red}║     SOME TESTS FAILED ✗            ║${colors.reset}`);
    log(`  ${colors.red}╚════════════════════════════════════╝${colors.reset}`);
  }
  log('');

  // Write results to file for bash script to parse
  console.log(`__TESTS_PASSED=${passed}`);
  console.log(`__TESTS_FAILED=${failed}`);
  console.log(`__TESTS_SKIPPED=${skipped}`);
}

// ============================================================================
// MAIN
// ============================================================================

async function main(): Promise<void> {
  const ctx: TestContext = {
    results: [],
    startTime: Date.now(),
  };

  logHeader('HyperCore E2E Integration Tests');

  log(`  ${colors.white}Configuration:${colors.reset}`);
  log(`    Gateway:  ${colors.cyan}${CONFIG.GATEWAY_URL}${colors.reset}`);
  log(`    EVM RPC:  ${colors.cyan}${CONFIG.EVM_RPC_URL}${colors.reset}`);
  log(`    Chain ID: ${colors.cyan}${CONFIG.CHAIN_ID}${colors.reset}`);
  log(`    Verbose:  ${colors.cyan}${CONFIG.VERBOSE}${colors.reset}`);
  log('');

  try {
    // Run all test categories
    await runConnectionTests(ctx);
    await runMarketDataTests(ctx);
    await runAccountTests(ctx);
    await runOrderTests(ctx);
    await runMatchingTests(ctx);
    await runPositionTests(ctx);
    await runEVMTests(ctx);
    await runAdvancedEVMTests(ctx);
    await runTokenStandardsTests(ctx);
    await runSpotTests(ctx);
    await runUnifiedStateTests(ctx);
    await runStressTests(ctx);
    await runAdvancedTests(ctx);
    await runRiskTests(ctx);
    await runStateProofTests(ctx);
  } catch (error) {
    log(`${colors.red}Fatal error during test execution:${colors.reset}`);
    log(`${colors.red}${error}${colors.reset}`);
  }

  // Print summary
  printSummary(ctx);

  // Exit with appropriate code
  const failed = ctx.results.filter((r) => r.status === 'fail').length;
  process.exit(failed > 0 ? 1 : 0);
}

// Run
main().catch((error) => {
  console.error('Fatal error:', error);
  process.exit(1);
});
