/**
 * Logging Utilities
 *
 * Colored console output helpers for test results and progress.
 */

import { CONFIG } from './config.js';
import type { TestResult } from './types.js';

/** ANSI color codes */
export const colors = {
  reset: '\x1b[0m',
  red: '\x1b[31m',
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  blue: '\x1b[34m',
  purple: '\x1b[35m',
  cyan: '\x1b[36m',
  white: '\x1b[37m',
  bold: '\x1b[1m',
};

/** Basic log output */
export function log(message: string): void {
  console.log(message);
}

/** Print a major section header */
export function logHeader(title: string): void {
  log('');
  log(`${colors.purple}${'═'.repeat(70)}${colors.reset}`);
  log(`${colors.bold}${colors.white}  ${title}${colors.reset}`);
  log(`${colors.purple}${'═'.repeat(70)}${colors.reset}`);
  log('');
}

/** Print a section divider */
export function logSection(title: string): void {
  log('');
  log(`${colors.cyan}${'─'.repeat(70)}${colors.reset}`);
  log(`${colors.cyan}  ${title}${colors.reset}`);
  log(`${colors.cyan}${'─'.repeat(70)}${colors.reset}`);
}

/** Print a test result with icon and timing */
export function logTest(result: TestResult): void {
  const icon =
    result.status === 'pass'
      ? `${colors.green}✓`
      : result.status === 'fail'
        ? `${colors.red}✗`
        : `${colors.yellow}○`;
  const duration = `${result.duration}ms`;
  log(`  ${icon} ${colors.white}${result.name}${colors.reset} ${colors.cyan}(${duration})${colors.reset}`);

  if (CONFIG.VERBOSE && result.description) {
    log(`      ${colors.cyan}${result.description}${colors.reset}`);
  }

  if (result.error && (CONFIG.VERBOSE || result.status === 'fail')) {
    log(`      ${colors.red}Error: ${result.error}${colors.reset}`);
  }
}

/** Print progress message (only in verbose mode) */
export function logProgress(message: string): void {
  if (CONFIG.VERBOSE) {
    log(`    ${colors.blue}▸${colors.reset} ${message}`);
  }
}
