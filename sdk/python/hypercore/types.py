"""
Type definitions for HyperCore SDK
"""

from enum import Enum
from typing import Optional, List
from pydantic import BaseModel, Field


class OrderSide(str, Enum):
    """Order side"""

    BUY = "buy"
    SELL = "sell"


class OrderType(str, Enum):
    """Order type"""

    LIMIT = "limit"
    MARKET = "market"
    POST_ONLY = "postOnly"
    IOC = "ioc"
    FOK = "fok"


class OrderStatus(str, Enum):
    """Order status"""

    OPEN = "open"
    PARTIAL = "partial"
    FILLED = "filled"
    CANCELED = "canceled"


class TimeInForce(str, Enum):
    """Time in force"""

    GTC = "Gtc"
    IOC = "Ioc"
    FOK = "Fok"
    ALO = "Alo"


class MarketMeta(BaseModel):
    """Market metadata"""

    name: str
    sz_decimals: int = Field(alias="szDecimals")
    max_leverage: int = Field(alias="maxLeverage")
    tick_size: str = Field(alias="tickSize")
    lot_size: str = Field(alias="lotSize")
    maker_fee: str = Field(alias="makerFee")
    taker_fee: str = Field(alias="takerFee")


class Position(BaseModel):
    """Position data"""

    coin: str
    size: str = Field(alias="szi")
    entry_px: str = Field(alias="entryPx")
    unrealized_pnl: str = Field(alias="unrealizedPnl")
    realized_pnl: str = Field(default="0")
    leverage: int
    liquidation_px: Optional[str] = Field(default=None, alias="liquidationPx")
    margin_used: str = Field(default="0", alias="marginUsed")
    return_on_equity: str = Field(default="0", alias="returnOnEquity")


class Order(BaseModel):
    """Order data"""

    coin: str
    oid: int
    side: str
    limit_px: str = Field(alias="limitPx")
    sz: str
    orig_sz: str = Field(alias="origSz")
    timestamp: int
    cloid: Optional[str] = None


class Fill(BaseModel):
    """Fill/trade data"""

    coin: str
    px: str
    sz: str
    side: str
    time: int
    fee: str
    oid: int
    tid: int
    crossed: bool
    closed_pnl: str = Field(alias="closedPnl")


class MarginSummary(BaseModel):
    """Margin summary"""

    account_value: str = Field(alias="accountValue")
    total_ntl_pos: str = Field(alias="totalNtlPos")
    total_raw_usd: str = Field(alias="totalRawUsd")
    total_margin_used: str = Field(alias="totalMarginUsed")
    withdrawable: str


class CrossMarginSummary(BaseModel):
    """Cross margin summary"""

    account_value: str = Field(alias="accountValue")
    total_ntl_pos: str = Field(alias="totalNtlPos")
    total_margin_used: str = Field(alias="totalMarginUsed")


class AssetPosition(BaseModel):
    """Asset position wrapper"""

    position: Position


class AccountState(BaseModel):
    """Full account state"""

    margin_summary: MarginSummary = Field(alias="marginSummary")
    cross_margin_summary: CrossMarginSummary = Field(alias="crossMarginSummary")
    asset_positions: List[AssetPosition] = Field(alias="assetPositions")


class L2Level(BaseModel):
    """Order book level"""

    px: str
    sz: str
    n: int


class L2Book(BaseModel):
    """L2 order book"""

    coin: str
    time: int
    levels: tuple  # [[bids], [asks]]


class FundingRate(BaseModel):
    """Funding rate data"""

    coin: str
    funding_rate: str = Field(alias="fundingRate")
    premium: str
    time: int


class OrderRequest(BaseModel):
    """Order placement request"""

    market: str
    side: OrderSide
    price: Optional[str] = None
    size: str
    order_type: OrderType = OrderType.LIMIT
    reduce_only: bool = False
    client_order_id: Optional[str] = None


class CancelRequest(BaseModel):
    """Order cancellation request"""

    market: str
    order_id: int


class LeverageUpdateRequest(BaseModel):
    """Leverage update request"""

    market: str
    leverage: int
    is_cross: bool = True


class TransferRequest(BaseModel):
    """Transfer request"""

    destination: str
    amount: str


class Signature(BaseModel):
    """ECDSA signature"""

    r: str
    s: str
    v: int


class SignedRequest(BaseModel):
    """Signed request wrapper"""

    action: dict
    nonce: int
    signature: Signature
