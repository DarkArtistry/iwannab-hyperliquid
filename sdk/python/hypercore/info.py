"""
Info API client for read-only queries
"""

from typing import Dict, List, Optional, Any
import httpx
from hypercore.types import (
    AccountState,
    L2Book,
    MarketMeta,
    Order,
    Fill,
    FundingRate,
)


class InfoClient:
    """Client for HyperCore Info API (read-only queries)"""

    def __init__(self, base_url: str, timeout: float = 30.0):
        self.base_url = base_url
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

    async def _request(self, body: Dict[str, Any]) -> Any:
        """Make a POST request to /info endpoint"""
        response = await self.client.post(
            f"{self.base_url}/info",
            json=body,
        )
        response.raise_for_status()
        data = response.json()

        if isinstance(data, dict) and data.get("status") == "err":
            raise Exception(f"API Error: {data.get('response', 'Unknown error')}")

        return data

    async def get_meta(self) -> Dict[str, List[MarketMeta]]:
        """Get exchange metadata (markets, fees, etc.)"""
        data = await self._request({"type": "meta"})
        return {
            "universe": [MarketMeta(**m) for m in data.get("universe", [])]
        }

    async def get_all_mids(self) -> Dict[str, str]:
        """Get all mid prices"""
        return await self._request({"type": "allMids"})

    async def get_l2_book(self, coin: str, n_sig_figs: int = 5) -> L2Book:
        """Get L2 order book snapshot"""
        data = await self._request({
            "type": "l2Book",
            "coin": coin,
            "nSigFigs": n_sig_figs,
        })
        return L2Book(**data)

    async def get_account_state(self, user: str) -> AccountState:
        """Get account state (positions, margin, etc.)"""
        data = await self._request({
            "type": "clearinghouseState",
            "user": user,
        })
        return AccountState(**data)

    async def get_open_orders(self, user: str) -> List[Order]:
        """Get open orders for a user"""
        data = await self._request({
            "type": "openOrders",
            "user": user,
        })
        return [Order(**o) for o in data]

    async def get_user_fills(
        self,
        user: str,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
    ) -> List[Fill]:
        """Get user's trade history"""
        request = {"type": "userFills", "user": user}
        if start_time:
            request["startTime"] = start_time
        if end_time:
            request["endTime"] = end_time

        data = await self._request(request)
        return [Fill(**f) for f in data]

    async def get_funding_history(
        self,
        coin: str,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
    ) -> List[FundingRate]:
        """Get funding rate history for a market"""
        request = {"type": "fundingHistory", "coin": coin}
        if start_time:
            request["startTime"] = start_time
        if end_time:
            request["endTime"] = end_time

        data = await self._request(request)
        return [FundingRate(**f) for f in data]

    async def get_user_funding_history(
        self,
        user: str,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
    ) -> List[Dict[str, Any]]:
        """Get user's funding payment history"""
        request = {"type": "userFundingHistory", "user": user}
        if start_time:
            request["startTime"] = start_time
        if end_time:
            request["endTime"] = end_time

        return await self._request(request)

    async def get_recent_trades(
        self,
        coin: str,
        limit: int = 100,
    ) -> List[Dict[str, Any]]:
        """Get recent trades for a market"""
        return await self._request({
            "type": "recentTrades",
            "coin": coin,
            "limit": limit,
        })

    async def get_candles(
        self,
        coin: str,
        interval: str,
        start_time: Optional[int] = None,
        end_time: Optional[int] = None,
    ) -> List[Dict[str, Any]]:
        """
        Get OHLCV candles.

        Args:
            coin: Market symbol (e.g., "BTC-PERP")
            interval: Candle interval ("1m", "5m", "15m", "1h", "4h", "1d")
            start_time: Start timestamp in ms
            end_time: End timestamp in ms
        """
        request = {
            "type": "candleSnapshot",
            "coin": coin,
            "interval": interval,
        }
        if start_time:
            request["startTime"] = start_time
        if end_time:
            request["endTime"] = end_time

        return await self._request(request)
