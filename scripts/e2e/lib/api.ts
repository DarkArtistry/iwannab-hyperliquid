/**
 * API Request Helpers
 *
 * Functions for making requests to HyperCore Gateway API.
 */

import { CONFIG } from './config.js';
import type { SignatureWire } from './types.js';

/**
 * Make a request to the /info endpoint
 *
 * @param type - The info request type (e.g., 'meta', 'l2Book', 'clearinghouseState')
 * @param params - Additional parameters for the request
 * @returns The JSON response from the server
 */
export async function infoRequest(type: string, params: Record<string, unknown> = {}): Promise<unknown> {
  const response = await fetch(`${CONFIG.GATEWAY_URL}/info`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type, ...params }),
  });

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${await response.text()}`);
  }

  return response.json();
}

/**
 * Make a request to the /exchange endpoint
 *
 * Uses proper EIP-712 signature verification - no bypasses or workarounds.
 *
 * @param action - The exchange action (e.g., order, cancel, viewTransfer)
 * @param signature - The signature components (r, s, v)
 * @param nonce - The request nonce (timestamp-based)
 * @returns The JSON response from the server
 */
export async function exchangeRequest(
  action: Record<string, unknown>,
  signature: SignatureWire,
  nonce: number
): Promise<unknown> {
  const body = { action, signature, nonce };

  const response = await fetch(`${CONFIG.GATEWAY_URL}/exchange`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${await response.text()}`);
  }

  return response.json();
}
