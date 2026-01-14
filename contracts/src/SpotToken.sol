// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "./interfaces/IERC20.sol";

/**
 * @title SpotToken
 * @notice HIP-1 style ERC20 token that bridges to HyperCore spot trading
 * @dev Each SpotToken has a system address that acts as a bridge.
 *      Transfers to the system address credit the sender's HyperCore balance.
 *      Transfers from the system address (by the bridge) debit HyperCore balance.
 */
contract SpotToken is IERC20 {
    // Token metadata
    string public name;
    string public symbol;
    uint8 public immutable decimals;

    // Token index in HyperCore (used to derive system address)
    uint8 public immutable tokenIndex;

    // System address for bridging (derived from tokenIndex)
    address public immutable systemAddress;

    // Total and circulating supply
    uint256 public totalSupply;

    // Balances
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    // Bridge events
    event BridgeToCore(address indexed from, uint256 amount);
    event BridgeFromCore(address indexed to, uint256 amount);

    /**
     * @notice Deploy a new SpotToken
     * @param _name Token name
     * @param _symbol Token symbol (max 6 characters per HIP-1)
     * @param _decimals Decimal places (weiDecimals in HIP-1)
     * @param _tokenIndex Unique token index in HyperCore (1-255, 0 is USDC)
     * @param _initialSupply Initial supply to mint to deployer
     */
    constructor(
        string memory _name,
        string memory _symbol,
        uint8 _decimals,
        uint8 _tokenIndex,
        uint256 _initialSupply
    ) {
        require(bytes(_symbol).length <= 6, "Symbol too long");
        require(_tokenIndex > 0, "Index 0 reserved for USDC");

        name = _name;
        symbol = _symbol;
        decimals = _decimals;
        tokenIndex = _tokenIndex;

        // Derive system address: keccak256("HyperCore.SpotToken.SystemAddress" + tokenIndex)[12:32]
        systemAddress = _deriveSystemAddress(_tokenIndex);

        // Mint initial supply to deployer
        if (_initialSupply > 0) {
            _mint(msg.sender, _initialSupply);
        }
    }

    /**
     * @notice Derive the system address for a token index
     * @dev Must match the Rust implementation in primitives/src/spot.rs
     */
    function _deriveSystemAddress(uint8 _index) internal pure returns (address) {
        return address(uint160(uint256(keccak256(
            abi.encodePacked("HyperCore.SpotToken.SystemAddress", _index)
        ))));
    }

    // ERC20 Implementation

    function transfer(address to, uint256 amount) external returns (bool) {
        return _transfer(msg.sender, to, amount);
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= amount, "Insufficient allowance");
            allowance[from][msg.sender] = allowed - amount;
        }
        return _transfer(from, to, amount);
    }

    // Internal functions

    function _transfer(address from, address to, uint256 amount) internal returns (bool) {
        require(balanceOf[from] >= amount, "Insufficient balance");

        balanceOf[from] -= amount;
        balanceOf[to] += amount;

        emit Transfer(from, to, amount);

        // Check if this is a bridge transfer
        if (to == systemAddress) {
            // Bridging to HyperCore
            emit BridgeToCore(from, amount);
        }

        return true;
    }

    function _mint(address to, uint256 amount) internal {
        totalSupply += amount;
        balanceOf[to] += amount;
        emit Transfer(address(0), to, amount);
    }

    /**
     * @notice Bridge tokens from HyperCore to EVM
     * @dev Can only be called by the bridge mechanism (via system address transfer)
     *      In production, this would be triggered by CoreWriter or precompile
     * @param to Recipient address
     * @param amount Amount to bridge
     */
    function bridgeFromCore(address to, uint256 amount) external {
        // In production, this would verify the caller is authorized (e.g., CoreWriter)
        // For now, only the system address can initiate this
        require(msg.sender == systemAddress || balanceOf[systemAddress] >= amount, "Unauthorized");

        if (msg.sender != systemAddress) {
            // Transfer from system address balance
            balanceOf[systemAddress] -= amount;
            balanceOf[to] += amount;
            emit Transfer(systemAddress, to, amount);
        } else {
            // Direct mint (system address is calling)
            _mint(to, amount);
        }

        emit BridgeFromCore(to, amount);
    }

    /**
     * @notice Get the system address for this token
     * @return The bridge system address
     */
    function getSystemAddress() external view returns (address) {
        return systemAddress;
    }

    /**
     * @notice Check the bridge balance (tokens waiting to be bridged)
     * @return Amount held by system address
     */
    function bridgeBalance() external view returns (uint256) {
        return balanceOf[systemAddress];
    }
}

/**
 * @title SpotTokenFactory
 * @notice Factory for deploying HIP-1 style SpotTokens
 */
contract SpotTokenFactory {
    // Deployed tokens by index
    mapping(uint8 => address) public tokens;

    // Next available token index
    uint8 public nextTokenIndex = 1;

    // Events
    event TokenDeployed(
        uint8 indexed tokenIndex,
        address indexed tokenAddress,
        string name,
        string symbol,
        address deployer
    );

    /**
     * @notice Deploy a new SpotToken
     * @param name Token name
     * @param symbol Token symbol
     * @param decimals Token decimals
     * @param initialSupply Initial supply (minted to msg.sender)
     * @return tokenAddress The deployed token address
     * @return tokenIndex The assigned token index
     */
    function deployToken(
        string memory name,
        string memory symbol,
        uint8 decimals,
        uint256 initialSupply
    ) external returns (address tokenAddress, uint8 tokenIndex) {
        tokenIndex = nextTokenIndex++;
        require(tokenIndex != 0, "Index overflow");

        SpotToken token = new SpotToken(
            name,
            symbol,
            decimals,
            tokenIndex,
            initialSupply
        );

        tokenAddress = address(token);
        tokens[tokenIndex] = tokenAddress;

        emit TokenDeployed(tokenIndex, tokenAddress, name, symbol, msg.sender);
    }

    /**
     * @notice Get token address by index
     */
    function getToken(uint8 index) external view returns (address) {
        return tokens[index];
    }
}
