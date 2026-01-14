// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console} from "forge-std/Script.sol";
import {CoreWriter} from "../src/CoreWriter.sol";

contract DeployScript is Script {
    // Known USDC addresses
    address constant USDC_MAINNET = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;
    address constant USDC_SEPOLIA = 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238;

    function run() external returns (CoreWriter) {
        // Get USDC address from env or use default for local devnet
        address usdc = vm.envOr("USDC_ADDRESS", address(0));

        if (usdc == address(0)) {
            // For local devnet, deploy a mock USDC
            console.log("No USDC_ADDRESS set, deploying mock USDC for devnet...");
            usdc = deployMockUSDC();
        }

        console.log("Deploying CoreWriter with USDC:", usdc);

        vm.startBroadcast();
        CoreWriter coreWriter = new CoreWriter(usdc);
        vm.stopBroadcast();

        console.log("CoreWriter deployed at:", address(coreWriter));

        return coreWriter;
    }

    function deployMockUSDC() internal returns (address) {
        vm.startBroadcast();
        MockUSDC usdc = new MockUSDC();
        vm.stopBroadcast();
        console.log("Mock USDC deployed at:", address(usdc));
        return address(usdc);
    }
}

/// @dev Simple mock USDC for devnet testing
contract MockUSDC {
    string public constant name = "USD Coin";
    string public constant symbol = "USDC";
    uint8 public constant decimals = 6;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    function mint(address to, uint256 amount) external {
        totalSupply += amount;
        balanceOf[to] += amount;
        emit Transfer(address(0), to, amount);
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        return _transfer(msg.sender, to, amount);
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= amount, "USDC: insufficient allowance");
            allowance[from][msg.sender] = allowed - amount;
        }
        return _transfer(from, to, amount);
    }

    function _transfer(address from, address to, uint256 amount) internal returns (bool) {
        require(balanceOf[from] >= amount, "USDC: insufficient balance");
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        emit Transfer(from, to, amount);
        return true;
    }
}
