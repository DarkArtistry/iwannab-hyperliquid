#!/bin/bash
# Seed test accounts with initial balances
# Compatible with bash 3.x (macOS default)

set -e

GATEWAY_URL="${GATEWAY_URL:-http://localhost:3000}"

echo "=== Seeding Test Accounts ==="

# Test accounts (for development only - DO NOT use these keys in production!)
# These are well-known Hardhat/Foundry test private keys
ACCOUNT_NAMES=(alice bob charlie dave eve)
ACCOUNT_KEYS=(
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
    "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
    "0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6"
    "0x47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a"
)

# Well-known addresses corresponding to the keys above (Hardhat accounts 0-4)
ACCOUNT_ADDRESSES=(
    "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
    "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
    "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    "0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65"
)

# Derive address from private key (if cast is available)
get_address() {
    local index=$1
    local privkey="${ACCOUNT_KEYS[$index]}"

    # Use cast to derive address (requires foundry)
    if command -v cast &> /dev/null; then
        cast wallet address --private-key "$privkey" 2>/dev/null
    else
        # Fall back to hardcoded addresses
        echo "${ACCOUNT_ADDRESSES[$index]}"
    fi
}

# Deposit function
deposit() {
    local index=$1
    local amount=$2
    local name="${ACCOUNT_NAMES[$index]}"
    local address=$(get_address "$index")

    echo "Depositing $amount USDC to $name ($address)..."

    # In a real setup, this would call the deposit endpoint
    # For now, we'll use a direct database insert for testing
    response=$(curl -s -X POST "$GATEWAY_URL/internal/seed" \
        -H "Content-Type: application/json" \
        -d "{
            \"account\": \"$address\",
            \"balance\": $amount
        }" 2>/dev/null) || true

    if [ -n "$response" ]; then
        echo "  Response: $response"
    else
        echo "  (endpoint not available or no response)"
    fi
}

# Seed each account with initial balance
INITIAL_BALANCE=100000000000  # 100,000 USDC (6 decimals)

for i in "${!ACCOUNT_NAMES[@]}"; do
    deposit "$i" "$INITIAL_BALANCE"
done

echo ""
echo "=== Test Accounts ==="
for i in "${!ACCOUNT_NAMES[@]}"; do
    name="${ACCOUNT_NAMES[$i]}"
    address=$(get_address "$i")
    echo "$name: $address"
done

echo ""
echo "=== Seed Complete ==="
echo "Each account has been credited with 100,000 USDC"
echo ""
echo "Note: The /internal/seed endpoint must be implemented in the gateway"
echo "for actual balance seeding. Currently this is a placeholder."
