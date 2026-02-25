export const API_BASE_URL = "http://localhost:3000";
export const WS_URL = "ws://localhost:3000/ws";
export const EVM_RPC_URL = "http://localhost:8545";
export const CHAIN_ID = 1337;

export const EIP712_DOMAIN = {
  name: "HyperCore",
  version: "1",
  chainId: BigInt(CHAIN_ID),
  verifyingContract:
    "0x0000000000000000000000000000000000000000" as `0x${string}`,
} as const;

export const ADMIN_ADDRESS = "0xdeA3c06EEe614bF84e74d505173822236c8Ad135";
