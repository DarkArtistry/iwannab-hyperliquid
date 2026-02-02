/**
 * HyperCore Multi-Node E2E Test Runner
 *
 * Dedicated runner for comprehensive 5-validator cluster tests.
 * Tests transaction propagation, state sync, EVM consistency,
 * and block progression across all validator nodes.
 *
 * Environment Variables:
 *   GATEWAY_URLS      - Comma-separated gateway URLs (default: localhost:3000-3040)
 *   EVM_RPC_URLS      - Comma-separated EVM RPC URLs (default: localhost:8545-8585)
 *   COMETBFT_RPC_URLS - Comma-separated CometBFT RPC URLs (default: localhost:26657-26697)
 *   VERBOSE           - Enable verbose logging (default: false)
 *
 * Usage:
 *   npx tsx multinode-runner.ts
 */

import { colors, log, logHeader } from './lib/index.js';
import type { TestContext } from './lib/index.js';
import { runMultinodeTests } from './tests/multinode.js';

// =============================================================================
// SUMMARY
// =============================================================================

function printSummary(ctx: TestContext): void {
  const totalDuration = Date.now() - ctx.startTime;

  logHeader('Multi-Node E2E Test Summary');

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
    log(`  ${colors.green}╔════════════════════════════════════════════╗${colors.reset}`);
    log(`  ${colors.green}║   ALL MULTI-NODE TESTS PASSED! ✓          ║${colors.reset}`);
    log(`  ${colors.green}╚════════════════════════════════════════════╝${colors.reset}`);
  } else {
    log(`  ${colors.red}╔════════════════════════════════════════════╗${colors.reset}`);
    log(`  ${colors.red}║   SOME MULTI-NODE TESTS FAILED ✗          ║${colors.reset}`);
    log(`  ${colors.red}╚════════════════════════════════════════════╝${colors.reset}`);
  }
  log('');

  // Machine-readable output for bash script parsing
  console.log(`__TESTS_PASSED=${passed}`);
  console.log(`__TESTS_FAILED=${failed}`);
  console.log(`__TESTS_SKIPPED=${skipped}`);
}

// =============================================================================
// MAIN
// =============================================================================

async function main(): Promise<void> {
  const ctx: TestContext = {
    results: [],
    startTime: Date.now(),
  };

  const gatewayUrls = (process.env.GATEWAY_URLS || 'http://localhost:3000').split(',');
  const evmRpcUrls = (process.env.EVM_RPC_URLS || 'http://localhost:8545').split(',');
  const cometbftRpcUrls = (process.env.COMETBFT_RPC_URLS || 'http://localhost:26657').split(',');

  logHeader('HyperCore Multi-Node E2E Tests (5-Validator Cluster)');

  log(`  ${colors.white}Configuration:${colors.reset}`);
  log(`    Nodes:        ${colors.cyan}${gatewayUrls.length}${colors.reset}`);
  log(`    Gateway URLs: ${colors.cyan}${gatewayUrls.join(', ')}${colors.reset}`);
  log(`    EVM RPC URLs: ${colors.cyan}${evmRpcUrls.join(', ')}${colors.reset}`);
  log(`    CometBFT:     ${colors.cyan}${cometbftRpcUrls.join(', ')}${colors.reset}`);
  log(`    Verbose:      ${colors.cyan}${process.env.VERBOSE === 'true'}${colors.reset}`);
  log('');

  try {
    await runMultinodeTests(ctx);
  } catch (error) {
    log(`${colors.red}Fatal error during test execution:${colors.reset}`);
    log(`${colors.red}${error}${colors.reset}`);
  }

  printSummary(ctx);

  const failed = ctx.results.filter((r) => r.status === 'fail').length;
  process.exit(failed > 0 ? 1 : 0);
}

main().catch((error) => {
  console.error('Fatal error:', error);
  process.exit(1);
});
