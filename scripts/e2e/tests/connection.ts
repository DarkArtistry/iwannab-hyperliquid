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
    logProgress('Gateway is healthy');
  });

  await runTest(ctx, 'Info endpoint available', 'connection', 'Verify /info POST endpoint accepts requests', async () => {
    logProgress('Sending POST /info request...');
    const result = await infoRequest('meta');
    if (!result) throw new Error('Empty response');
    logProgress('Info endpoint responding');
  });

  await runTest(ctx, 'Exchange endpoint available', 'connection', 'Verify /exchange POST endpoint exists', async () => {
    logProgress('Sending POST /exchange probe...');
    const response = await fetch(`${CONFIG.GATEWAY_URL}/exchange`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    // We expect an error response, but the endpoint should exist
    if (response.status === 404) throw new Error('Exchange endpoint not found');
    logProgress('Exchange endpoint exists');
  });

  await runTest(ctx, 'EVM RPC available', 'connection', 'Verify EVM JSON-RPC endpoint is responding', async () => {
    logProgress('Sending eth_chainId request...');
    const response = await fetch(CONFIG.EVM_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'eth_chainId', params: [], id: 1 }),
    });
    if (!response.ok) throw new Error(`EVM RPC failed: ${response.status}`);
    const data = (await response.json()) as { result?: string };
    logProgress(`Chain ID: ${parseInt(data.result || '0', 16)}`);
  });
}
