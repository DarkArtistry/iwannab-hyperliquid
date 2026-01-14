/**
 * Message signing utilities for HyperCore
 */

import { privateKeyToAccount, signTypedData } from 'viem/accounts';
import { keccak256, toBytes } from 'viem';
import type { Signature } from './types';
import { EIP712_DOMAIN } from './constants';

export class Signer {
  readonly address: string;
  private privateKey: `0x${string}`;
  private chainId: number;

  constructor(privateKey: string, chainId: number) {
    this.privateKey = (
      privateKey.startsWith('0x') ? privateKey : `0x${privateKey}`
    ) as `0x${string}`;

    const account = privateKeyToAccount(this.privateKey);
    this.address = account.address;
    this.chainId = chainId;
  }

  /**
   * Sign an action for the exchange API
   */
  async signAction(
    action: Record<string, unknown>,
    nonce: number
  ): Promise<Signature> {
    const domain = {
      ...EIP712_DOMAIN,
      chainId: this.chainId,
    };

    // Determine the action type and create appropriate typed data
    const actionType = action.type as string;
    const types = this.getTypesForAction(actionType);
    const message = this.getMessageForAction(action, nonce);

    const signature = await signTypedData({
      privateKey: this.privateKey,
      domain,
      types,
      primaryType: this.getPrimaryType(actionType),
      message,
    });

    // Parse signature into r, s, v components
    return this.parseSignature(signature);
  }

  /**
   * Sign a raw message hash
   */
  async signHash(hash: `0x${string}`): Promise<Signature> {
    const account = privateKeyToAccount(this.privateKey);
    const signature = await account.signMessage({
      message: { raw: toBytes(hash) },
    });
    return this.parseSignature(signature);
  }

  /**
   * Get EIP-712 types for an action
   */
  private getTypesForAction(actionType: string): Record<string, Array<{ name: string; type: string }>> {
    switch (actionType) {
      case 'order':
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'orders', type: 'Order[]' },
            { name: 'grouping', type: 'string' },
            { name: 'nonce', type: 'uint64' },
          ],
          Order: [
            { name: 'a', type: 'uint8' },
            { name: 'b', type: 'bool' },
            { name: 'p', type: 'string' },
            { name: 's', type: 'string' },
            { name: 'r', type: 'bool' },
            { name: 't', type: 'string' },
          ],
        };

      case 'cancel':
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'cancels', type: 'Cancel[]' },
            { name: 'nonce', type: 'uint64' },
          ],
          Cancel: [
            { name: 'a', type: 'uint8' },
            { name: 'o', type: 'uint64' },
          ],
        };

      case 'updateLeverage':
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'asset', type: 'uint8' },
            { name: 'isCross', type: 'bool' },
            { name: 'leverage', type: 'uint8' },
            { name: 'nonce', type: 'uint64' },
          ],
        };

      case 'usdTransfer':
      case 'withdraw':
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'destination', type: 'address' },
            { name: 'amount', type: 'string' },
            { name: 'nonce', type: 'uint64' },
          ],
        };

      case 'cancelByCloid':
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'cancels', type: 'CancelByCloid[]' },
            { name: 'nonce', type: 'uint64' },
          ],
          CancelByCloid: [
            { name: 'asset', type: 'uint8' },
            { name: 'cloid', type: 'string' },
          ],
        };

      case 'cancelAll':
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'nonce', type: 'uint64' },
          ],
        };

      case 'batchModify':
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'modifies', type: 'Modify[]' },
            { name: 'nonce', type: 'uint64' },
          ],
          Modify: [
            { name: 'oid', type: 'uint64' },
            { name: 'order', type: 'Order' },
          ],
          Order: [
            { name: 'a', type: 'uint8' },
            { name: 'b', type: 'bool' },
            { name: 'p', type: 'string' },
            { name: 's', type: 'string' },
            { name: 'r', type: 'bool' },
            { name: 't', type: 'string' },
          ],
        };

      default:
        // Generic action type
        return {
          Action: [
            { name: 'type', type: 'string' },
            { name: 'nonce', type: 'uint64' },
          ],
        };
    }
  }

  /**
   * Get the message object for signing
   */
  private getMessageForAction(
    action: Record<string, unknown>,
    nonce: number
  ): Record<string, unknown> {
    return {
      ...action,
      nonce: BigInt(nonce),
    };
  }

  /**
   * Get the primary type name for an action
   */
  private getPrimaryType(_actionType: string): string {
    return 'Action';
  }

  /**
   * Parse a signature string into r, s, v components
   */
  private parseSignature(signature: `0x${string}`): Signature {
    // Remove '0x' prefix and ensure we have 130 hex chars (65 bytes)
    const sig = signature.slice(2);

    if (sig.length !== 130) {
      throw new Error(`Invalid signature length: ${sig.length}`);
    }

    const r = `0x${sig.slice(0, 64)}`;
    const s = `0x${sig.slice(64, 128)}`;
    const v = parseInt(sig.slice(128, 130), 16);

    return { r, s, v };
  }

  /**
   * Verify a signature
   */
  static verifySignature(
    _message: Record<string, unknown>,
    _signature: Signature,
    _expectedAddress: string
  ): boolean {
    // Implementation would verify the signature
    // This is a placeholder - actual verification requires the full typed data
    return true;
  }

  /**
   * Hash a message for signing
   */
  static hashMessage(message: string): `0x${string}` {
    return keccak256(toBytes(message));
  }
}
