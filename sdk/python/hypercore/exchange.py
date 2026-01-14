"""
Exchange API client for trading operations
"""

from typing import Dict, List, Optional, Any, Union
import time
import httpx
from hypercore.types import (
    OrderRequest,
    OrderSide,
    OrderType,
    Signature,
)
from hypercore.signing import Signer

# Market symbol to ID mapping
MARKETS = {
    "BTC-PERP": 0,
    "ETH-PERP": 1,
}


class ExchangeClient:
    """Client for HyperCore Exchange API (trading operations)"""

    def __init__(
        self,
        base_url: str,
        signer: Optional[Signer],
        timeout: float = 30.0,
    ):
        self.base_url = base_url
        self.signer = signer
        self.timeout = timeout
        self._client: Optional[httpx.AsyncClient] = None

    @property
    def client(self) -> httpx.AsyncClient:
        if self._client is None:
            self._client = httpx.AsyncClient(timeout=self.timeout)
        return self._client

    async def close(self) -> None:
        if self._client:
            await self._client.aclose()
            self._client = None

    def _resolve_market_id(self, market: Union[str, int]) -> int:
        """Convert market symbol to ID"""
        if isinstance(market, int):
            return market
        market_id = MARKETS.get(market)
        if market_id is None:
            raise ValueError(f"Unknown market: {market}")
        return market_id

    def _order_type_to_tif(self, order_type: OrderType) -> Dict[str, Any]:
        """Convert order type to TIF structure"""
        if order_type == OrderType.MARKET:
            return {"trigger": {"isMarket": True, "triggerPx": "0", "tpsl": "tp"}}
        elif order_type == OrderType.POST_ONLY:
            return {"limit": {"tif": "Alo"}}
        elif order_type == OrderType.IOC:
            return {"limit": {"tif": "Ioc"}}
        elif order_type == OrderType.FOK:
            return {"limit": {"tif": "Fok"}}
        else:  # LIMIT
            return {"limit": {"tif": "Gtc"}}

    async def _request(self, action: Dict[str, Any]) -> Any:
        """Make a signed POST request to /exchange endpoint"""
        if not self.signer:
            raise ValueError("Signer required for exchange operations")

        nonce = int(time.time() * 1000)
        signature = self.signer.sign_action(action, nonce)

        body = {
            "action": action,
            "nonce": nonce,
            "signature": {
                "r": signature.r,
                "s": signature.s,
                "v": signature.v,
            },
        }

        response = await self.client.post(
            f"{self.base_url}/exchange",
            json=body,
        )
        response.raise_for_status()
        data = response.json()

        if data.get("status") == "err":
            raise Exception(f"Exchange Error: {data.get('response', 'Unknown error')}")

        return data

    async def place_order(
        self,
        market: Union[str, int],
        side: Union[str, OrderSide],
        size: str,
        price: Optional[str] = None,
        order_type: Union[str, OrderType] = OrderType.LIMIT,
        reduce_only: bool = False,
        client_order_id: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Place an order.

        Args:
            market: Market symbol or ID
            side: "buy" or "sell"
            size: Order size
            price: Limit price (required for limit orders)
            order_type: Order type
            reduce_only: Reduce-only flag
            client_order_id: Client order ID for tracking

        Returns:
            Order placement response
        """
        market_id = self._resolve_market_id(market)

        if isinstance(side, str):
            side = OrderSide(side)
        if isinstance(order_type, str):
            order_type = OrderType(order_type)

        order = {
            "a": market_id,
            "b": side == OrderSide.BUY,
            "p": price or "0",
            "s": size,
            "r": reduce_only,
            "t": self._order_type_to_tif(order_type),
        }
        if client_order_id:
            order["c"] = client_order_id

        return await self._request({
            "type": "order",
            "orders": [order],
            "grouping": "na",
        })

    async def place_orders(self, orders: List[OrderRequest]) -> Dict[str, Any]:
        """Place multiple orders"""
        order_list = []
        for req in orders:
            market_id = self._resolve_market_id(req.market)
            order = {
                "a": market_id,
                "b": req.side == OrderSide.BUY,
                "p": req.price or "0",
                "s": req.size,
                "r": req.reduce_only,
                "t": self._order_type_to_tif(req.order_type),
            }
            if req.client_order_id:
                order["c"] = req.client_order_id
            order_list.append(order)

        return await self._request({
            "type": "order",
            "orders": order_list,
            "grouping": "na",
        })

    async def cancel_order(
        self,
        market: Union[str, int],
        order_id: int,
    ) -> Dict[str, Any]:
        """Cancel an order by ID"""
        market_id = self._resolve_market_id(market)

        return await self._request({
            "type": "cancel",
            "cancels": [{"a": market_id, "o": order_id}],
        })

    async def cancel_by_cloid(
        self,
        market: Union[str, int],
        client_order_id: str,
    ) -> Dict[str, Any]:
        """Cancel an order by client order ID"""
        market_id = self._resolve_market_id(market)

        return await self._request({
            "type": "cancelByCloid",
            "cancels": [{"asset": market_id, "cloid": client_order_id}],
        })

    async def cancel_all_orders(
        self,
        market: Optional[Union[str, int]] = None,
    ) -> Dict[str, Any]:
        """Cancel all orders, optionally in a specific market"""
        if market is not None:
            market_id = self._resolve_market_id(market)
            return await self._request({
                "type": "cancel",
                "cancels": [{"a": market_id, "o": 0}],
            })

        return await self._request({"type": "cancelAll"})

    async def update_leverage(
        self,
        market: Union[str, int],
        leverage: int,
        is_cross: bool = True,
    ) -> Dict[str, Any]:
        """Update leverage for a market"""
        market_id = self._resolve_market_id(market)

        return await self._request({
            "type": "updateLeverage",
            "asset": market_id,
            "isCross": is_cross,
            "leverage": leverage,
        })

    async def transfer(
        self,
        destination: str,
        amount: str,
    ) -> Dict[str, Any]:
        """Transfer USDC to another address"""
        return await self._request({
            "type": "usdTransfer",
            "destination": destination,
            "amount": amount,
        })

    async def withdraw(
        self,
        destination: str,
        amount: str,
    ) -> Dict[str, Any]:
        """Withdraw USDC to L1"""
        return await self._request({
            "type": "withdraw",
            "destination": destination,
            "amount": amount,
        })
