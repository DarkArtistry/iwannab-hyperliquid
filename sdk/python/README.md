# HyperCore Python SDK

Python SDK for interacting with the HyperCore perpetual futures exchange.

## Installation

```bash
pip install hypercore-sdk
```

Or install from source:

```bash
pip install -e .
```

## Quick Start

```python
import asyncio
from hypercore import HyperCore

async def main():
    # Initialize client
    client = HyperCore(
        base_url="http://localhost:3000",
        private_key="0x..."  # Your private key
    )

    # Get account state
    state = await client.info.get_account_state(client.address)
    print(f"Balance: {state.margin_summary.account_value}")

    # Place an order
    result = await client.exchange.place_order(
        market="BTC-PERP",
        side="buy",
        size="0.1",
        price="65000",
    )
    print(f"Order result: {result}")

    # Close the client
    await client.close()

asyncio.run(main())
```

## Features

- Full API coverage for info and exchange endpoints
- WebSocket support for real-time updates
- EIP-712 message signing
- Type hints with Pydantic models
- Async/await support

## API Reference

### Info API

```python
# Get exchange metadata
meta = await client.info.get_meta()

# Get orderbook
book = await client.info.get_l2_book("BTC-PERP")

# Get account state
state = await client.info.get_account_state("0x...")

# Get open orders
orders = await client.info.get_open_orders("0x...")

# Get fills
fills = await client.info.get_fills("0x...", start_time=0)
```

### Exchange API

```python
# Place order
await client.exchange.place_order(
    market="BTC-PERP",
    side="buy",
    size="0.1",
    price="65000",
    order_type="limit",
)

# Cancel order
await client.exchange.cancel_order(market="BTC-PERP", order_id=123)

# Cancel all orders
await client.exchange.cancel_all_orders()

# Update leverage
await client.exchange.update_leverage(market="BTC-PERP", leverage=10)

# Transfer USDC
await client.exchange.transfer(destination="0x...", amount="100")
```

## Development

```bash
# Install dev dependencies
pip install -e ".[dev]"

# Run tests
pytest

# Format code
black hypercore
ruff check hypercore

# Type checking
mypy hypercore
```

## License

MIT
