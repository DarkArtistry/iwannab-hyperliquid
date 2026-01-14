"""
HyperCore Python SDK

A Python client library for interacting with the HyperCore perpetual exchange.

Example:
    >>> from hypercore import HyperCore
    >>> client = HyperCore(private_key="0x...")
    >>>
    >>> # Get account state
    >>> state = await client.info.get_account_state(client.address)
    >>> print(f"Equity: {state.margin_summary.account_value}")
    >>>
    >>> # Place an order
    >>> result = await client.exchange.place_order(
    ...     market="BTC-PERP",
    ...     side="buy",
    ...     price="65000",
    ...     size="0.1"
    ... )
"""

from hypercore.client import HyperCore
from hypercore.info import InfoClient
from hypercore.exchange import ExchangeClient
from hypercore.signing import Signer
from hypercore.types import (
    OrderSide,
    OrderType,
    OrderStatus,
    Position,
    Order,
    Fill,
    AccountState,
    MarginSummary,
)

__version__ = "0.1.0"
__all__ = [
    "HyperCore",
    "InfoClient",
    "ExchangeClient",
    "Signer",
    "OrderSide",
    "OrderType",
    "OrderStatus",
    "Position",
    "Order",
    "Fill",
    "AccountState",
    "MarginSummary",
]
