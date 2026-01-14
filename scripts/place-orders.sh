#!/bin/bash
# Place sample orders for testing
# Compatible with bash 3.x (macOS default)

set -e

GATEWAY_URL="${GATEWAY_URL:-http://localhost:3000}"

echo "=== Placing Sample Orders ==="

# Test private key (Alice - Hardhat account 0)
PRIVKEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

# Get current timestamp for nonce (macOS compatible)
# Note: macOS date doesn't support %N for nanoseconds
if date +%s%N 2>/dev/null | grep -q 'N'; then
    # macOS - use seconds and append random digits
    NONCE=$(($(date +%s) * 1000 + RANDOM % 1000))
else
    # Linux - use milliseconds
    NONCE=$(($(date +%s%N) / 1000000))
fi

# Function to sign and submit order
place_order() {
    local market=$1
    local side=$2
    local price=$3
    local size=$4
    local is_buy="false"

    if [ "$side" = "buy" ]; then
        is_buy="true"
    fi

    echo "Placing $side order: $size @ $price on market $market"

    # Build order payload
    local payload="{
    \"action\": {
        \"type\": \"order\",
        \"orders\": [{
            \"a\": $market,
            \"b\": $is_buy,
            \"p\": \"$price\",
            \"s\": \"$size\",
            \"r\": false,
            \"t\": {\"limit\": {\"tif\": \"Gtc\"}}
        }],
        \"grouping\": \"na\"
    },
    \"nonce\": $NONCE,
    \"signature\": {
        \"r\": \"0x0000000000000000000000000000000000000000000000000000000000000000\",
        \"s\": \"0x0000000000000000000000000000000000000000000000000000000000000000\",
        \"v\": 27
    }
}"

    # Note: In production, you would properly sign this request
    # For testing, the gateway may accept unsigned requests in dev mode

    response=$(curl -s -X POST "$GATEWAY_URL/exchange" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || true

    if [ -n "$response" ]; then
        # Try to pretty-print with jq, fall back to raw output
        if command -v jq &> /dev/null; then
            echo "$response" | jq . 2>/dev/null || echo "  Response: $response"
        else
            echo "  Response: $response"
        fi
    else
        echo "  (no response)"
    fi

    # Increment nonce
    NONCE=$((NONCE + 1))
}

# BTC-PERP orders (market_id = 0)
echo ""
echo "--- BTC-PERP Orders ---"

# Place buy orders (bids)
place_order 0 "buy" "64500.0" "0.1"
place_order 0 "buy" "64000.0" "0.2"
place_order 0 "buy" "63500.0" "0.3"
place_order 0 "buy" "63000.0" "0.5"

# Place sell orders (asks)
place_order 0 "sell" "65500.0" "0.1"
place_order 0 "sell" "66000.0" "0.2"
place_order 0 "sell" "66500.0" "0.3"
place_order 0 "sell" "67000.0" "0.5"

# ETH-PERP orders (market_id = 1)
echo ""
echo "--- ETH-PERP Orders ---"

# Place buy orders
place_order 1 "buy" "3450.00" "1.0"
place_order 1 "buy" "3400.00" "2.0"
place_order 1 "buy" "3350.00" "3.0"
place_order 1 "buy" "3300.00" "5.0"

# Place sell orders
place_order 1 "sell" "3550.00" "1.0"
place_order 1 "sell" "3600.00" "2.0"
place_order 1 "sell" "3650.00" "3.0"
place_order 1 "sell" "3700.00" "5.0"

echo ""
echo "=== Sample Orders Placed ==="
echo "Check the orderbook:"
echo "  curl -X POST $GATEWAY_URL/info -H 'Content-Type: application/json' -d '{\"type\":\"l2Book\",\"coin\":\"BTC-PERP\"}'"
