// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test, console} from "forge-std/Test.sol";
import {CoreWriter} from "../src/CoreWriter.sol";
import {ICoreWriter} from "../src/interfaces/ICoreWriter.sol";

/// @dev Mock USDC token for testing
contract MockUSDC {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        require(balanceOf[msg.sender] >= amount, "Insufficient balance");
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(balanceOf[from] >= amount, "Insufficient balance");
        require(allowance[from][msg.sender] >= amount, "Insufficient allowance");
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        allowance[from][msg.sender] -= amount;
        return true;
    }
}

contract CoreWriterTest is Test {
    CoreWriter public coreWriter;
    MockUSDC public usdc;

    address public alice = makeAddr("alice");
    address public bob = makeAddr("bob");

    uint256 constant INITIAL_BALANCE = 100_000e6; // 100,000 USDC

    event ActionQueued(
        bytes32 indexed actionId,
        address indexed sender,
        uint8 actionType,
        bytes data
    );

    event DepositToCore(address indexed user, uint256 amount);

    function setUp() public {
        // Deploy mock USDC
        usdc = new MockUSDC();

        // Deploy CoreWriter
        coreWriter = new CoreWriter(address(usdc));

        // Fund test accounts
        usdc.mint(alice, INITIAL_BALANCE);
        usdc.mint(bob, INITIAL_BALANCE);

        // Approve CoreWriter to spend USDC
        vm.prank(alice);
        usdc.approve(address(coreWriter), type(uint256).max);
        vm.prank(bob);
        usdc.approve(address(coreWriter), type(uint256).max);
    }

    // ============ Constructor Tests ============

    function test_constructor() public view {
        assertEq(coreWriter.usdcToken(), address(usdc));
        assertEq(coreWriter.getUsdcToken(), address(usdc));
    }

    // ============ Place Order Tests ============

    function test_placeOrder_limit() public {
        vm.prank(alice);

        ICoreWriter.OrderParams memory params = ICoreWriter.OrderParams({
            marketId: 0,
            isBuy: true,
            price: 65000e8,
            size: 1e8,
            orderType: ICoreWriter.OrderType.Limit,
            reduceOnly: false
        });

        bytes32 actionId = coreWriter.placeOrder(params);

        // Check action was recorded
        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(actionId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Pending));
        assertTrue(coreWriter.isPending(actionId));
    }

    function test_placeOrder_emitsEvent() public {
        vm.prank(alice);

        ICoreWriter.OrderParams memory params = ICoreWriter.OrderParams({
            marketId: 0,
            isBuy: true,
            price: 65000e8,
            size: 1e8,
            orderType: ICoreWriter.OrderType.Limit,
            reduceOnly: false
        });

        vm.expectEmit(false, true, false, false);
        emit ActionQueued(bytes32(0), alice, uint8(ICoreWriter.ActionType.PlaceOrder), "");

        coreWriter.placeOrder(params);
    }

    function test_placeOrder_revert_zeroSize() public {
        vm.prank(alice);

        ICoreWriter.OrderParams memory params = ICoreWriter.OrderParams({
            marketId: 0,
            isBuy: true,
            price: 65000e8,
            size: 0,
            orderType: ICoreWriter.OrderType.Limit,
            reduceOnly: false
        });

        vm.expectRevert("CoreWriter: size must be positive");
        coreWriter.placeOrder(params);
    }

    function test_placeOrder_revert_zeroPriceNonMarket() public {
        vm.prank(alice);

        ICoreWriter.OrderParams memory params = ICoreWriter.OrderParams({
            marketId: 0,
            isBuy: true,
            price: 0,
            size: 1e8,
            orderType: ICoreWriter.OrderType.Limit,
            reduceOnly: false
        });

        vm.expectRevert("CoreWriter: invalid price");
        coreWriter.placeOrder(params);
    }

    function test_placeOrder_marketOrder_zeroPriceAllowed() public {
        vm.prank(alice);

        ICoreWriter.OrderParams memory params = ICoreWriter.OrderParams({
            marketId: 0,
            isBuy: true,
            price: 0,
            size: 1e8,
            orderType: ICoreWriter.OrderType.Market,
            reduceOnly: false
        });

        bytes32 actionId = coreWriter.placeOrder(params);
        assertTrue(coreWriter.isPending(actionId));
    }

    // ============ Cancel Order Tests ============

    function test_cancelOrder() public {
        vm.prank(alice);

        bytes32 actionId = coreWriter.cancelOrder(0, 12345);

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(actionId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Pending));
    }

    function test_cancelOrder_revert_zeroOrderId() public {
        vm.prank(alice);

        vm.expectRevert("CoreWriter: invalid order ID");
        coreWriter.cancelOrder(0, 0);
    }

    // ============ Cancel All Orders Tests ============

    function test_cancelAllOrders() public {
        vm.prank(alice);

        bytes32 actionId = coreWriter.cancelAllOrders(0);

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(actionId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Pending));
    }

    // ============ Update Leverage Tests ============

    function test_updateLeverage() public {
        vm.prank(alice);

        bytes32 actionId = coreWriter.updateLeverage(0, 10);

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(actionId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Pending));
    }

    function test_updateLeverage_revert_tooLow() public {
        vm.prank(alice);

        vm.expectRevert("CoreWriter: invalid leverage");
        coreWriter.updateLeverage(0, 0);
    }

    function test_updateLeverage_revert_tooHigh() public {
        vm.prank(alice);

        vm.expectRevert("CoreWriter: invalid leverage");
        coreWriter.updateLeverage(0, 101);
    }

    // ============ Deposit Tests ============

    function test_depositToCore() public {
        uint256 depositAmount = 10_000e6;

        vm.prank(alice);

        vm.expectEmit(true, false, false, true);
        emit DepositToCore(alice, depositAmount);

        bytes32 actionId = coreWriter.depositToCore(depositAmount);

        // Check USDC was transferred
        assertEq(usdc.balanceOf(alice), INITIAL_BALANCE - depositAmount);
        assertEq(usdc.balanceOf(address(coreWriter)), depositAmount);

        // Check action is pending
        assertTrue(coreWriter.isPending(actionId));
    }

    function test_depositToCore_revert_zeroAmount() public {
        vm.prank(alice);

        vm.expectRevert("CoreWriter: amount must be positive");
        coreWriter.depositToCore(0);
    }

    // ============ Withdraw Tests ============

    function test_withdrawToEvm() public {
        vm.prank(alice);

        bytes32 actionId = coreWriter.withdrawToEvm(5_000e6);

        assertTrue(coreWriter.isPending(actionId));
    }

    function test_withdrawToEvm_revert_zeroAmount() public {
        vm.prank(alice);

        vm.expectRevert("CoreWriter: amount must be positive");
        coreWriter.withdrawToEvm(0);
    }

    // ============ Action Result Tests ============

    function test_setActionResult_success() public {
        vm.prank(alice);
        bytes32 actionId = coreWriter.cancelAllOrders(0);

        // Simulate node calling _setActionResult
        bytes memory resultData = abi.encode(uint256(5)); // 5 orders cancelled
        coreWriter._setActionResult(actionId, true, resultData);

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(actionId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Executed));
        assertEq(result.result, resultData);
        assertFalse(coreWriter.isPending(actionId));
    }

    function test_setActionResult_failure() public {
        vm.prank(alice);
        bytes32 actionId = coreWriter.cancelOrder(0, 99999);

        // Simulate node calling _setActionResult with failure
        bytes memory resultData = abi.encode("Order not found");
        coreWriter._setActionResult(actionId, false, resultData);

        ICoreWriter.ActionResult memory result = coreWriter.getActionStatus(actionId);
        assertEq(uint8(result.status), uint8(ICoreWriter.ActionStatus.Failed));
    }

    // ============ Complete Withdraw Tests ============

    function test_completeWithdraw() public {
        // First deposit some USDC to the contract
        vm.prank(alice);
        coreWriter.depositToCore(10_000e6);

        uint256 bobBalanceBefore = usdc.balanceOf(bob);

        // Simulate node completing withdrawal
        coreWriter._completeWithdraw(bob, 5_000e6);

        assertEq(usdc.balanceOf(bob), bobBalanceBefore + 5_000e6);
    }

    // ============ Fuzz Tests ============

    function testFuzz_placeOrder(uint256 price, uint256 size, bool isBuy) public {
        vm.assume(price > 0 && price < type(uint128).max);
        vm.assume(size > 0 && size < type(uint128).max);

        vm.prank(alice);

        ICoreWriter.OrderParams memory params = ICoreWriter.OrderParams({
            marketId: 0,
            isBuy: isBuy,
            price: price,
            size: size,
            orderType: ICoreWriter.OrderType.Limit,
            reduceOnly: false
        });

        bytes32 actionId = coreWriter.placeOrder(params);
        assertTrue(coreWriter.isPending(actionId));
    }

    function testFuzz_depositToCore(uint256 amount) public {
        vm.assume(amount > 0 && amount <= INITIAL_BALANCE);

        vm.prank(alice);
        bytes32 actionId = coreWriter.depositToCore(amount);

        assertTrue(coreWriter.isPending(actionId));
        assertEq(usdc.balanceOf(address(coreWriter)), amount);
    }
}
