"""
Main HyperCore client
"""

from typing import Optional
from hypercore.info import InfoClient
from hypercore.exchange import ExchangeClient
from hypercore.signing import Signer

DEFAULT_BASE_URL = "https://api.hypercore.xyz"
DEFAULT_CHAIN_ID = 1337


class HyperCore:
    """
    Main client for interacting with HyperCore exchange.

    Example:
        >>> client = HyperCore(private_key="0x...")
        >>> state = await client.info.get_account_state(client.address)
    """

    def __init__(
        self,
        base_url: str = DEFAULT_BASE_URL,
        private_key: Optional[str] = None,
        chain_id: int = DEFAULT_CHAIN_ID,
        timeout: float = 30.0,
    ):
        """
        Initialize HyperCore client.

        Args:
            base_url: API base URL
            private_key: Private key for signing (optional)
            chain_id: Chain ID for EIP-712 signing
            timeout: Request timeout in seconds
        """
        self.base_url = base_url
        self.chain_id = chain_id
        self.timeout = timeout

        # Initialize signer if private key provided
        self.signer: Optional[Signer] = None
        if private_key:
            self.signer = Signer(private_key, chain_id)

        # Initialize sub-clients
        self.info = InfoClient(base_url, timeout)
        self.exchange = ExchangeClient(base_url, self.signer, timeout)

    @property
    def address(self) -> Optional[str]:
        """Get the signer's address"""
        return self.signer.address if self.signer else None

    @property
    def is_authenticated(self) -> bool:
        """Check if client has a signer"""
        return self.signer is not None

    async def close(self) -> None:
        """Close HTTP connections"""
        await self.info.close()
        await self.exchange.close()

    async def __aenter__(self) -> "HyperCore":
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb) -> None:
        await self.close()
