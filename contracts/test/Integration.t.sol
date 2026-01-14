// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test, console} from "forge-std/Test.sol";
import {CoreWriter} from "../src/CoreWriter.sol";
import {ICoreWriter} from "../src/interfaces/ICoreWriter.sol";
import {VaultExample} from "../src/examples/VaultExample.sol";
import {HyperCore} from "../src/HyperCore.sol";
import {IPositionPrecompile} from "../src/interfaces/IPrecompiles.sol";
import {IAccountPrecompile} from "../src/interfaces/IPrecompiles.sol";
import {IMarketPrecompile} from "../src/interfaces/IPrecompiles.sol";
import {IFundingPrecompile} from "../src/interfaces/IPrecompiles.sol";
import {IOrderBookPrecompile} from "../src/interfaces/IPrecompiles.sol";

/// @dev Mock USDC token for integration tests
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

/// @title IntegrationTest
/// @notice End-to-end integration tests for HyperCore contracts
contract IntegrationTest is Test {
    // ============ Contracts ============
    CoreWriter public coreWriter;
    MockUSDC public usdc;
    VaultExample public vault;

    // ============ Test Accounts ============
    address public alice;
    uint256 public aliceKey;
    address public bob;
    uint256 public bobKey;
    address public charlie;
    uint256 public charlieKey;
    address public vaultManager;
    uint256 public vaultManagerKey;

    // ============ Constants ============
    uint256 constant INITIAL_BALANCE = 1_000_000e6; // 1M USDC each
    uint256 constant BTC_PRICE = 65_000e8; // $65,000 BTC
    uint256 constant ETH_PRICE = 3_500e8; // $3,500 ETH
    uint8 constant BTC_MARKET = 0;
    uint8 constant ETH_MARKET = 1;

    // ============ Events ============
    event ActionQueued(
        bytes32 indexed actionId,
        address indexed sender,
        uint8 actionType,
        bytes data
    );
    event DepositToCore(address indexed user, uint256 amount);
    event WithdrawToEvm(address indexed recipient, uint256 amount);
    event ActionExecuted(bytes32 indexed actionId, bool success, bytes result);
    event PositionOpened(uint8 indexed marketId, bool isBuy, uint256 size, uint256 price);
    event PositionClosed(uint8 indexed marketId, uint256 size);

    // ============ Setup ============

    function setUp() public {
        // Create test accounts with known private keys
        (alice, aliceKey) = makeAddrAndKey("alice");
        (bob, bobKey) = makeAddrAndKey("bob");
        (charlie, charlieKey) = makeAddrAndKey("charlie");
        (vaultManager, vaultManagerKey) = makeAddrAndKey("vaultManager");

        // Deploy contracts
        usdc = new MockUSDC();
        coreWriter = new CoreWriter(address(usdc));

        // Deploy vault with vaultManager as manager
        vm.prank(vaultManager);
        vault = new VaultExample(address(coreWriter));

        // Fund test accounts
        usdc.mint(alice, INITIAL_BALANCE);
        usdc.mint(bob, INITIAL_BALANCE);
        usdc.mint(charlie, INITIAL_BALANCE);
        usdc.mint(vaultManager, INITIAL_BALANCE);
        usdc.mint(address(vault), INITIAL_BALANCE); // Pre-fund vault for testing

        // Approve CoreWriter for all accounts
        vm.prank(alice);
        usdc.approve(address(coreWriter), type(uint256).max);
        vm.prank(bob);
        usdc.approve(address(coreWriter), type(uint256).max);
        vm.prank(charlie);
        usdc.approve(address(coreWriter), type(uint256).max);
        vm.prank(vaultManager);
        usdc.approve(address(coreWriter), type(uint256).max);
        vm.prank(address(vault));
        usdc.approve(address(coreWriter), type(uint256).max);

        // Setup mock precompiles
        _setupMockPrecompiles();
    }

    function _setupMockPrecompiles() internal {
        // Mock Position Precompile
        address positionPrecompile = 0x0000000000000000000000000000000000000800;
        vm.etch(positionPrecompile, hex"00"); // Deploy minimal code

        // Mock Account Precompile
        address accountPrecompile = 0x0000000000000000000000000000000000000801;
        vm.etch(accountPrecompile, hex"00");

        // Mock Market Precompile
        address marketPrecompile = 0x0000000000000000000000000000000000000802;
        vm.etch(marketPrecompile, hex"00");

        // Mock Funding Precompile
        address fundingPrecompile = 0x0000000000000000000000000000000000000804;
        vm.etch(fundingPrecompile, hex"00");

        // Mock OrderBook Precompile
        address orderbookPrecompile = 0x0000000000000000000000000000000000000805;
        vm.etch(orderbookPrecompile, hex"00");
    }

    // ============ CoreWriter Integration Tests ============

    /// @notice Test complete deposit -> trade -> withdraw flow
    function test_fullTradingFlow() public {
        uint256 depositAmount = 100_000e6; // 100k USDC

        // Step 1: Alice deposits to Core
        vm.prank(alice);
        bytes32 depositId = coreWriter.depositToCore(depositAmount);
        assertTrue(coreWriter.isPending(depositId));
        assertEq(usdc.balanceOf(address(coreWriter)), depositAmount);
        assertEq(usdc.balanceOf(alice), INITIAL_BALANCE - depositAmount);

        // Step 2: Simulate node confirming deposit
        coreWriter._setActionResult(depositId, true, abi.encode(depositAmount));
        assertFalse(coreWriter.isPending(depositId));
        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(depositId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Executed));

        // Step 3: Alice places a limit buy order for BTC
        vm.prank(alice);
        bytes32 orderId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE,
                size: 1e8, // 1 BTC
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );
        assertTrue(coreWriter.isPending(orderId));

        // Step 4: Simulate node executing order (filled)
        bytes memory fillResult = abi.encode(
            uint256(1e8), // fillSize
            uint256(BTC_PRICE), // fillPrice
            uint256(65e6) // fee in USDC (0.1% of $65k)
        );
        coreWriter._setActionResult(orderId, true, fillResult);
        assertFalse(coreWriter.isPending(orderId));

        // Step 5: Alice requests withdrawal
        vm.prank(alice);
        bytes32 withdrawId = coreWriter.withdrawToEvm(50_000e6);
        assertTrue(coreWriter.isPending(withdrawId));

        // Step 6: Simulate node completing withdrawal
        uint256 aliceBalanceBefore = usdc.balanceOf(alice);
        coreWriter._completeWithdraw(alice, 50_000e6);
        assertEq(usdc.balanceOf(alice), aliceBalanceBefore + 50_000e6);
    }

    /// @notice Test multiple users trading concurrently
    function test_multiUserTrading() public {
        uint256 depositAmount = 100_000e6;

        // Alice and Bob both deposit
        vm.prank(alice);
        bytes32 aliceDeposit = coreWriter.depositToCore(depositAmount);

        vm.prank(bob);
        bytes32 bobDeposit = coreWriter.depositToCore(depositAmount);

        // Both deposits should be pending
        assertTrue(coreWriter.isPending(aliceDeposit));
        assertTrue(coreWriter.isPending(bobDeposit));
        assertEq(usdc.balanceOf(address(coreWriter)), depositAmount * 2);

        // Confirm both deposits
        coreWriter._setActionResult(aliceDeposit, true, abi.encode(depositAmount));
        coreWriter._setActionResult(bobDeposit, true, abi.encode(depositAmount));

        // Alice places buy order, Bob places sell order (potential match)
        vm.prank(alice);
        bytes32 aliceOrder = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        vm.prank(bob);
        bytes32 bobOrder = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: false,
                price: BTC_PRICE,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        // Both orders pending
        assertTrue(coreWriter.isPending(aliceOrder));
        assertTrue(coreWriter.isPending(bobOrder));

        // Simulate match execution
        bytes memory matchResult = abi.encode(1e8, BTC_PRICE, 65e6);
        coreWriter._setActionResult(aliceOrder, true, matchResult);
        coreWriter._setActionResult(bobOrder, true, matchResult);

        // Verify executed
        assertFalse(coreWriter.isPending(aliceOrder));
        assertFalse(coreWriter.isPending(bobOrder));
    }

    /// @notice Test order cancellation flow
    function test_orderCancellation() public {
        // Deposit first
        vm.prank(alice);
        bytes32 depositId = coreWriter.depositToCore(100_000e6);
        coreWriter._setActionResult(depositId, true, abi.encode(100_000e6));

        // Place order (we don't use the actionId here, just demonstrating order flow)
        vm.prank(alice);
        coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        // For cancel, we'd need the actual order ID from the engine
        // Using a mock order ID here
        uint256 mockEngineOrderId = 12345;

        vm.prank(alice);
        bytes32 cancelId = coreWriter.cancelOrder(BTC_MARKET, mockEngineOrderId);
        assertTrue(coreWriter.isPending(cancelId));

        // Simulate successful cancellation
        coreWriter._setActionResult(cancelId, true, abi.encode(mockEngineOrderId));
        assertFalse(coreWriter.isPending(cancelId));

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(cancelId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Executed));
    }

    /// @notice Test cancel all orders
    function test_cancelAllOrders() public {
        // Deposit
        vm.prank(alice);
        coreWriter.depositToCore(100_000e6);

        // Place multiple orders
        vm.startPrank(alice);
        coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE - 1000e8,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );
        coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE - 2000e8,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        // Cancel all
        bytes32 cancelAllId = coreWriter.cancelAllOrders(BTC_MARKET);
        vm.stopPrank();

        assertTrue(coreWriter.isPending(cancelAllId));

        // Simulate cancellation of 2 orders
        coreWriter._setActionResult(cancelAllId, true, abi.encode(uint256(2)));

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(cancelAllId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Executed));

        uint256 cancelledCount = abi.decode(result.result, (uint256));
        assertEq(cancelledCount, 2);
    }

    /// @notice Test leverage update
    function test_leverageUpdate() public {
        vm.prank(alice);
        bytes32 actionId = coreWriter.updateLeverage(BTC_MARKET, 20);
        assertTrue(coreWriter.isPending(actionId));

        coreWriter._setActionResult(actionId, true, abi.encode(uint8(20)));
        assertFalse(coreWriter.isPending(actionId));
    }

    /// @notice Test market order
    function test_marketOrder() public {
        vm.prank(alice);
        coreWriter.depositToCore(100_000e6);

        // Place market order (price=0 allowed)
        vm.prank(alice);
        bytes32 orderId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: 0, // Market order
                size: 1e8,
                orderType: ICoreWriter.OrderType.Market,
                reduceOnly: false
            })
        );
        assertTrue(coreWriter.isPending(orderId));
    }

    /// @notice Test IOC (Immediate-or-Cancel) order
    function test_iocOrder() public {
        vm.prank(alice);
        coreWriter.depositToCore(100_000e6);

        vm.prank(alice);
        bytes32 orderId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: ETH_MARKET,
                isBuy: true,
                price: ETH_PRICE,
                size: 10e8, // 10 ETH
                orderType: ICoreWriter.OrderType.Ioc,
                reduceOnly: false
            })
        );
        assertTrue(coreWriter.isPending(orderId));

        // IOC partially filled
        bytes memory partialFill = abi.encode(
            uint256(5e8), // Only 5 ETH filled
            ETH_PRICE,
            uint256(175e5) // 0.1% fee on $17.5k
        );
        coreWriter._setActionResult(orderId, true, partialFill);
    }

    /// @notice Test reduce-only order
    function test_reduceOnlyOrder() public {
        vm.prank(alice);
        coreWriter.depositToCore(100_000e6);

        // First open a position
        vm.prank(alice);
        bytes32 openId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );
        coreWriter._setActionResult(openId, true, abi.encode(1e8, BTC_PRICE, 65e6));

        // Now place reduce-only order to close
        vm.prank(alice);
        bytes32 closeId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: false,
                price: BTC_PRICE + 1000e8, // Sell higher
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: true
            })
        );
        assertTrue(coreWriter.isPending(closeId));
    }

    // ============ VaultExample Integration Tests ============

    /// @notice Test VaultExample deployment and manager setup
    function test_vaultDeployment() public view {
        assertEq(vault.manager(), vaultManager);
        assertEq(address(vault.coreWriter()), address(coreWriter));
        assertEq(vault.targetLeverage(), 10);
        assertEq(vault.maxPositionValue(), 1_000_000e6);
    }

    /// @notice Test vault manager controls
    function test_vaultManagerControls() public {
        // Update max position value
        vm.prank(vaultManager);
        vault.setMaxPositionValue(5_000_000e6);
        assertEq(vault.maxPositionValue(), 5_000_000e6);

        // Transfer manager role
        vm.prank(vaultManager);
        vault.setManager(charlie);
        assertEq(vault.manager(), charlie);

        // Old manager should not have access
        vm.prank(vaultManager);
        vm.expectRevert("Not manager");
        vault.setMaxPositionValue(1_000_000e6);

        // New manager should have access
        vm.prank(charlie);
        vault.setMaxPositionValue(2_000_000e6);
        assertEq(vault.maxPositionValue(), 2_000_000e6);
    }

    /// @notice Test vault cannot be controlled by non-manager
    function test_vaultOnlyManager() public {
        vm.prank(alice);
        vm.expectRevert("Not manager");
        vault.setMaxPositionValue(0);

        vm.prank(bob);
        vm.expectRevert("Not manager");
        vault.setManager(bob);
    }

    // ============ Error Handling Tests ============

    /// @notice Test failed action handling
    function test_failedAction() public {
        vm.prank(alice);
        bytes32 orderId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        // Simulate order rejection (insufficient margin)
        coreWriter._setActionResult(orderId, false, abi.encode("Insufficient margin"));

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(orderId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Failed));
    }

    /// @notice Test withdrawal failure handling
    function test_failedWithdrawal() public {
        vm.prank(alice);
        bytes32 withdrawId = coreWriter.withdrawToEvm(1_000_000e6);

        // Simulate rejection (insufficient Core balance)
        coreWriter._setActionResult(withdrawId, false, abi.encode("Insufficient balance"));

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(withdrawId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Failed));
    }

    /// @notice Test deposit transfer failure
    function test_depositInsufficientBalance() public {
        // Remove alice's balance
        vm.prank(alice);
        usdc.transfer(address(0xdead), INITIAL_BALANCE);
        assertEq(usdc.balanceOf(alice), 0);

        vm.prank(alice);
        vm.expectRevert("CoreWriter: transfer failed");
        coreWriter.depositToCore(1000e6);
    }

    /// @notice Test deposit without approval
    function test_depositWithoutApproval() public {
        // Create new user without approval
        address newUser = makeAddr("newUser");
        usdc.mint(newUser, 100_000e6);

        vm.prank(newUser);
        vm.expectRevert("CoreWriter: transfer failed");
        coreWriter.depositToCore(1000e6);
    }

    // ============ Edge Case Tests ============

    /// @notice Test minimum order size
    function test_minimumOrderSize() public {
        vm.prank(alice);
        bytes32 orderId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE,
                size: 1, // 0.00000001 BTC
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );
        assertTrue(coreWriter.isPending(orderId));
    }

    /// @notice Test maximum price
    function test_maxPrice() public {
        vm.prank(alice);
        bytes32 orderId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: type(uint128).max, // Very large price
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );
        assertTrue(coreWriter.isPending(orderId));
    }

    /// @notice Test all markets
    function test_allMarkets() public {
        vm.startPrank(alice);

        // BTC market
        bytes32 btcOrder = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: 0,
                isBuy: true,
                price: BTC_PRICE,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        // ETH market
        bytes32 ethOrder = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: 1,
                isBuy: true,
                price: ETH_PRICE,
                size: 10e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        vm.stopPrank();

        assertTrue(coreWriter.isPending(btcOrder));
        assertTrue(coreWriter.isPending(ethOrder));
    }

    // ============ Gas Optimization Tests ============

    /// @notice Measure gas for order placement
    function test_gasPlaceOrder() public {
        vm.prank(alice);

        uint256 gasBefore = gasleft();
        coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: BTC_MARKET,
                isBuy: true,
                price: BTC_PRICE,
                size: 1e8,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );
        uint256 gasUsed = gasBefore - gasleft();

        console.log("Gas used for placeOrder:", gasUsed);
        assertTrue(gasUsed < 100_000, "Order placement should be gas efficient");
    }

    /// @notice Measure gas for deposit
    function test_gasDeposit() public {
        vm.prank(alice);

        uint256 gasBefore = gasleft();
        coreWriter.depositToCore(100_000e6);
        uint256 gasUsed = gasBefore - gasleft();

        console.log("Gas used for deposit:", gasUsed);
        assertTrue(gasUsed < 100_000, "Deposit should be gas efficient");
    }

    // ============ Stress Tests ============

    /// @notice Test many orders from single user
    function test_manyOrdersSingleUser() public {
        vm.startPrank(alice);

        bytes32[] memory orderIds = new bytes32[](10);
        for (uint256 i = 0; i < 10; i++) {
            orderIds[i] = coreWriter.placeOrder(
                ICoreWriter.OrderParams({
                    marketId: BTC_MARKET,
                    isBuy: i % 2 == 0,
                    price: BTC_PRICE + (i * 100e8),
                    size: 1e8,
                    orderType: ICoreWriter.OrderType.Limit,
                    reduceOnly: false
                })
            );
        }

        vm.stopPrank();

        // All orders should be pending
        for (uint256 i = 0; i < 10; i++) {
            assertTrue(coreWriter.isPending(orderIds[i]));
        }
    }

    /// @notice Test orders from many users
    function test_ordersFromManyUsers() public {
        // Create 5 users and have them each place orders
        for (uint256 i = 0; i < 5; i++) {
            address user = makeAddr(string(abi.encodePacked("user", i)));
            usdc.mint(user, 100_000e6);

            vm.prank(user);
            usdc.approve(address(coreWriter), type(uint256).max);

            vm.prank(user);
            bytes32 orderId = coreWriter.placeOrder(
                ICoreWriter.OrderParams({
                    marketId: BTC_MARKET,
                    isBuy: i % 2 == 0,
                    price: BTC_PRICE,
                    size: 1e8,
                    orderType: ICoreWriter.OrderType.Limit,
                    reduceOnly: false
                })
            );
            assertTrue(coreWriter.isPending(orderId));
        }
    }

    // ============ Fuzz Tests ============

    /// @notice Fuzz test deposit amounts
    function testFuzz_deposit(uint256 amount) public {
        vm.assume(amount > 0 && amount <= INITIAL_BALANCE);

        vm.prank(alice);
        bytes32 depositId = coreWriter.depositToCore(amount);

        assertTrue(coreWriter.isPending(depositId));
        assertEq(usdc.balanceOf(address(coreWriter)), amount);
    }

    /// @notice Fuzz test order parameters
    function testFuzz_orderParams(
        uint8 marketId,
        bool isBuy,
        uint128 price,
        uint128 size
    ) public {
        vm.assume(price > 0);
        vm.assume(size > 0);

        vm.prank(alice);
        bytes32 orderId = coreWriter.placeOrder(
            ICoreWriter.OrderParams({
                marketId: marketId,
                isBuy: isBuy,
                price: price,
                size: size,
                orderType: ICoreWriter.OrderType.Limit,
                reduceOnly: false
            })
        );

        assertTrue(coreWriter.isPending(orderId));
    }

    /// @notice Fuzz test leverage values
    function testFuzz_leverage(uint8 leverage) public {
        vm.assume(leverage >= 1 && leverage <= 100);

        vm.prank(alice);
        bytes32 actionId = coreWriter.updateLeverage(BTC_MARKET, leverage);
        assertTrue(coreWriter.isPending(actionId));
    }

    // ============ Event Tests ============

    /// @notice Test ActionQueued event contains correct data
    function test_actionQueuedEvent() public {
        ICoreWriter.OrderParams memory params = ICoreWriter.OrderParams({
            marketId: BTC_MARKET,
            isBuy: true,
            price: BTC_PRICE,
            size: 1e8,
            orderType: ICoreWriter.OrderType.Limit,
            reduceOnly: false
        });

        bytes memory expectedData = abi.encode(
            params.marketId,
            params.isBuy,
            params.price,
            params.size,
            uint8(params.orderType),
            params.reduceOnly
        );

        vm.prank(alice);
        vm.expectEmit(false, true, false, true);
        emit ActionQueued(bytes32(0), alice, uint8(ICoreWriter.ActionType.PlaceOrder), expectedData);

        coreWriter.placeOrder(params);
    }

    /// @notice Test DepositToCore event
    function test_depositEvent() public {
        uint256 amount = 50_000e6;

        vm.prank(alice);
        vm.expectEmit(true, false, false, true);
        emit DepositToCore(alice, amount);

        coreWriter.depositToCore(amount);
    }

    /// @notice Test WithdrawToEvm event on completion
    function test_withdrawEvent() public {
        uint256 amount = 25_000e6;

        // First deposit so coreWriter has USDC to withdraw
        vm.prank(alice);
        coreWriter.depositToCore(amount);

        vm.expectEmit(true, false, false, true);
        emit WithdrawToEvm(bob, amount);

        coreWriter._completeWithdraw(bob, amount);
    }
}
