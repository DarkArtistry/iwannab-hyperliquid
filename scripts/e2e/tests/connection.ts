/**
 * Connection & Health Tests
 *
 * Tests for basic connectivity to all HyperCore services.
 */

import { CONFIG, infoRequest, runTest, logSection, log, logProgress } from '../lib/index.js';
import type { TestContext } from '../lib/index.js';

export async function runConnectionTests(ctx: TestContext): Promise<void> {
  logSection('1. Connection & Health Tests');
  log('');
  log('  Testing basic connectivity to all HyperCore services');
  log('');

  await runTest(ctx, 'Gateway health check', 'connection', 'Verify Gateway service is responding to health endpoint', async () => {
    logProgress('Sending GET /health request...');
    const response = await fetch(`${CONFIG.GATEWAY_URL}/health`);
    if (!response.ok) throw new Error(`Health check failed: ${response.status}`);
    const body = await response.text();
    if (!body || body.length === 0) throw new Error('Health check returned empty response');
    logProgress(`Gateway is healthy: ${body}`);
  });

  await runTest(ctx, 'Info endpoint available', 'connection', 'Verify /info POST endpoint accepts requests', async () => {
    logProgress('Sending POST /info request...');
    const result = (await infoRequest('meta')) as { universe?: unknown[] };
    if (!result) throw new Error('Empty response');
    if (!result.universe || !Array.isArray(result.universe)) {
      throw new Error(`Expected meta response with universe array, got: ${JSON.stringify(result).slice(0, 100)}`);
    }
    logProgress(`Info endpoint responding, ${result.universe.length} markets`);
  });

  await runTest(ctx, 'Exchange endpoint available', 'connection', 'Verify /exchange POST endpoint exists and rejects bad requests', async () => {
    logProgress('Sending POST /exchange probe with empty body...');
    const response = await fetch(`${CONFIG.GATEWAY_URL}/exchange`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    // Endpoint should exist (not 404) and reject bad input (400 or 422)
    // axum returns 422 Unprocessable Entity when JSON deserialization fails
    if (response.status === 404) throw new Error('Exchange endpoint not found');
    if (response.status >= 500) throw new Error(`Exchange endpoint returned server error: ${response.status}`);
    if (response.status !== 400 && response.status !== 422) {
      throw new Error(`Expected 400 or 422 for empty body, got ${response.status}`);
    }
    logProgress(`Exchange endpoint returned status ${response.status} (validates input correctly)`);
  });

  await runTest(ctx, 'EVM RPC available', 'connection', 'Verify EVM JSON-RPC endpoint is responding', async () => {
    logProgress('Sending eth_chainId request...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'eth_chainId', params: [], id: 1 }),
    });
    if (!response.ok) throw new Error(`EVM RPC failed: ${response.status}`);
    const data = (await response.json()) as { result?: string; error?: { message: string } };
    if (data.error) throw new Error(`EVM RPC error: ${data.error.message}`);
    if (!data.result) throw new Error('EVM RPC returned no chain ID');
    const chainId = parseInt(data.result, 16);
    if (chainId !== CONFIG.CHAIN_ID) throw new Error(`Expected chain ID ${CONFIG.CHAIN_ID}, got ${chainId}`);
    logProgress(`Chain ID: ${chainId}`);
  });
}
