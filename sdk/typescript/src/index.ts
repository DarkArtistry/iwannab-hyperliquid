/**
 * HyperCore TypeScript SDK
 *
 * @example
 * ```typescript
 * import { HyperCore, OrderSide, OrderType } from '@hypercore/sdk';
 *
 * const client = new HyperCore({
 *   baseUrl: 'https://api.hypercore.xyz',
 *   privateKey: '0x...'
 * });
 *
 * // Get account state
 * const state = await client.info.getAccountState('0x...');
 *
 * // Place an order
 * const result = await client.exchange.placeOrder({
 *   market: 'BTC-PERP',
 *   side: OrderSide.Buy,
 *   price: '65000',
 *   size: '0.1',
 *   type: OrderType.Limit
 * });
 * ```
 */

export { HyperCore } from './client';
export { InfoClient } from './info';
export { ExchangeClient } from './exchange';
export { WebSocketClient } from './websocket';
export { Signer } from './signing';

export * from './types';
export * from './constants';
