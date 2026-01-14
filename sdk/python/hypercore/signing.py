"""
Message signing utilities for HyperCore
"""

from typing import Any, Dict
from eth_account import Account
from eth_account.messages import encode_typed_data
from hypercore.types import Signature

# EIP-712 Domain
EIP712_DOMAIN = {
    "name": "HyperCore",
    "version": "1",
    "chainId": 1337,
    "verifyingContract": "0x0000000000000000000000000000000000000000",
}


class Signer:
    """
    Signer for HyperCore exchange actions.

    Uses EIP-712 typed data signing for secure message authentication.
    """

    def __init__(self, private_key: str, chain_id: int = 1337):
        """
        Initialize signer.

        Args:
            private_key: Private key (hex string, with or without 0x prefix)
            chain_id: Chain ID for EIP-712 domain
        """
        if not private_key.startswith("0x"):
            private_key = f"0x{private_key}"

        self._private_key = private_key
        self._account = Account.from_key(private_key)
        self._chain_id = chain_id

    @property
    def address(self) -> str:
        """Get the signer's address"""
        return self._account.address

    def sign_action(self, action: Dict[str, Any], nonce: int) -> Signature:
        """
        Sign an action for the exchange API.

        Args:
            action: Action payload to sign
            nonce: Nonce for replay protection

        Returns:
            Signature object with r, s, v components
        """
        action_type = action.get("type", "")
        types = self._get_types_for_action(action_type)

        domain = {
            **EIP712_DOMAIN,
            "chainId": self._chain_id,
        }

        message = {
            **action,
            "nonce": nonce,
        }

        # Encode and sign
        signable = encode_typed_data(
            domain_data=domain,
            message_types=types,
            message_data=message,
        )

        signed = self._account.sign_message(signable)

        return Signature(
            r=hex(signed.r),
            s=hex(signed.s),
            v=signed.v,
        )

    def _get_types_for_action(self, action_type: str) -> Dict[str, Any]:
        """Get EIP-712 types for an action"""
        if action_type == "order":
            return {
                "Action": [
                    {"name": "type", "type": "string"},
                    {"name": "orders", "type": "Order[]"},
                    {"name": "grouping", "type": "string"},
                    {"name": "nonce", "type": "uint64"},
                ],
                "Order": [
                    {"name": "a", "type": "uint8"},
                    {"name": "b", "type": "bool"},
                    {"name": "p", "type": "string"},
                    {"name": "s", "type": "string"},
                    {"name": "r", "type": "bool"},
                    {"name": "t", "type": "string"},
                ],
            }

        elif action_type == "cancel":
            return {
                "Action": [
                    {"name": "type", "type": "string"},
                    {"name": "cancels", "type": "Cancel[]"},
                    {"name": "nonce", "type": "uint64"},
                ],
                "Cancel": [
                    {"name": "a", "type": "uint8"},
                    {"name": "o", "type": "uint64"},
                ],
            }

        elif action_type == "updateLeverage":
            return {
                "Action": [
                    {"name": "type", "type": "string"},
                    {"name": "asset", "type": "uint8"},
                    {"name": "isCross", "type": "bool"},
                    {"name": "leverage", "type": "uint8"},
                    {"name": "nonce", "type": "uint64"},
                ],
            }

        elif action_type in ("usdTransfer", "withdraw"):
            return {
                "Action": [
                    {"name": "type", "type": "string"},
                    {"name": "destination", "type": "address"},
                    {"name": "amount", "type": "string"},
                    {"name": "nonce", "type": "uint64"},
                ],
            }

        # Generic fallback
        return {
            "Action": [
                {"name": "type", "type": "string"},
                {"name": "data", "type": "string"},
                {"name": "nonce", "type": "uint64"},
            ],
        }

    def sign_hash(self, message_hash: bytes) -> Signature:
        """
        Sign a raw message hash.

        Args:
            message_hash: 32-byte hash to sign

        Returns:
            Signature object
        """
        signed = self._account.signHash(message_hash)

        return Signature(
            r=hex(signed.r),
            s=hex(signed.s),
            v=signed.v,
        )

    @staticmethod
    def recover_address(message_hash: bytes, signature: Signature) -> str:
        """
        Recover signer address from signature.

        Args:
            message_hash: Original message hash
            signature: Signature to verify

        Returns:
            Recovered address
        """
        return Account.recoverHash(
            message_hash,
            vrs=(signature.v, int(signature.r, 16), int(signature.s, 16)),
        )
