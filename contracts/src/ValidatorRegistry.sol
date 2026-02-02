// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title ValidatorRegistry
 * @notice On-chain registry for HyperCore validators with permissioned/permissionless modes
 *
 * This contract manages the validator set for the HyperCore chain:
 * - Validators register by staking USDC
 * - Minimum stake required to become a validator
 * - Unbonding period before stake can be withdrawn
 * - Voting power proportional to stake (capped)
 *
 * The consensus layer reads this contract's state to determine the
 * active validator set. Changes take effect at epoch boundaries.
 *
 * ## Validator Modes
 *
 * The contract supports two modes:
 *
 * 1. **Permissioned Mode (default)**: Only pre-approved addresses can register
 *    as validators. This is similar to how Hyperliquid operates initially.
 *    - Admin approves validators via `approveValidator()`
 *    - Approved validators can then `register()` with stake
 *    - Useful for initial network bootstrapping and security
 *
 * 2. **Permissionless Mode**: Anyone meeting stake requirements can register.
 *    - Enable via `setPermissionedMode(false)`
 *    - Economic security comes from stake at risk
 *    - Recommended once network is stable and battle-tested
 *
 * ## Integration with CometBFT
 *
 * The ABCI application reads the validator set from this contract at:
 * 1. InitChain - Initial validator set from genesis
 * 2. EndBlock - Check for validator updates each epoch
 *
 * Validator updates are returned to CometBFT which handles the consensus
 * layer validator set management.
 *
 * ## Validator Lifecycle
 *
 * 1. (Permissioned) Admin calls `approveValidator(address)`
 * 2. Validator calls `register(pubKey, moniker)` with required stake
 * 3. Validator is activated at next epoch boundary
 * 4. Validator can `increaseStake()` to gain more voting power
 * 5. Validator calls `startUnbonding()` to exit
 * 6. After unbonding period, `completeUnbonding()` to withdraw stake
 */

import "./interfaces/IERC20.sol";

contract ValidatorRegistry {
    // =========================================================================
    // Types
    // =========================================================================

    struct Validator {
        address operator;        // Address that controls the validator
        bytes pubKey;           // Ed25519 public key for consensus (32 bytes)
        uint256 stake;          // Amount of USDC staked
        uint256 votingPower;    // Computed voting power (may be capped)
        bool active;            // Whether validator is in active set
        uint256 activeSince;    // Block number when activated
        uint256 unbondingStart; // Block number when unbonding started (0 if not unbonding)
        string moniker;         // Human-readable name
    }

    struct ValidatorUpdate {
        bytes pubKey;
        int64 power;  // Negative means remove
    }

    // =========================================================================
    // State
    // =========================================================================

    /// @notice USDC token used for staking
    IERC20 public immutable stakingToken;

    /// @notice Minimum stake required to become a validator
    uint256 public minStake;

    /// @notice Maximum stake that counts toward voting power (soft cap)
    uint256 public maxEffectiveStake;

    /// @notice Maximum number of active validators
    uint256 public maxValidators;

    /// @notice Unbonding period in blocks before stake can be withdrawn
    uint256 public unbondingPeriod;

    /// @notice Current epoch number (increments when validator set changes)
    uint256 public currentEpoch;

    /// @notice Block number of last epoch transition
    uint256 public lastEpochBlock;

    /// @notice Blocks between epoch transitions
    uint256 public epochLength;

    /// @notice Total staked amount across all validators
    uint256 public totalStaked;

    /// @notice Validator data by operator address
    mapping(address => Validator) public validators;

    /// @notice Mapping from pubKey hash to operator address
    mapping(bytes32 => address) public pubKeyToOperator;

    /// @notice List of all validator operators (active and inactive)
    address[] public validatorList;

    /// @notice Number of active validators
    uint256 public activeValidatorCount;

    /// @notice Pending validator updates for next epoch
    ValidatorUpdate[] public pendingUpdates;

    /// @notice Contract admin (for initial setup, can be renounced)
    address public admin;

    /// @notice Whether the contract is initialized
    bool public initialized;

    /// @notice Whether the registry is in permissioned mode
    /// When true, only approved addresses can register as validators
    /// When false, anyone meeting stake requirements can register
    bool public permissionedMode;

    /// @notice Approved validators (only used in permissioned mode)
    mapping(address => bool) public approvedValidators;

    // =========================================================================
    // Events
    // =========================================================================

    event ValidatorRegistered(
        address indexed operator,
        bytes pubKey,
        uint256 stake,
        string moniker
    );

    event ValidatorStakeIncreased(
        address indexed operator,
        uint256 amount,
        uint256 newStake
    );

    event ValidatorUnbondingStarted(
        address indexed operator,
        uint256 unbondingBlock
    );

    event ValidatorExited(
        address indexed operator,
        uint256 stake
    );

    event ValidatorSlashed(
        address indexed operator,
        uint256 amount,
        string reason
    );

    event EpochTransition(
        uint256 indexed epoch,
        uint256 activeValidators,
        uint256 totalStake
    );

    event ValidatorSetUpdated(
        uint256 indexed epoch,
        uint256 addedCount,
        uint256 removedCount
    );

    event ValidatorApproved(address indexed operator);

    event ValidatorRevoked(address indexed operator);

    event PermissionedModeChanged(bool permissioned);

    // =========================================================================
    // Modifiers
    // =========================================================================

    modifier onlyAdmin() {
        require(msg.sender == admin, "Not admin");
        _;
    }

    modifier onlyInitialized() {
        require(initialized, "Not initialized");
        _;
    }

    // =========================================================================
    // Constructor
    // =========================================================================

    constructor(address _stakingToken) {
        stakingToken = IERC20(_stakingToken);
        admin = msg.sender;
    }

    // =========================================================================
    // Admin Functions
    // =========================================================================

    /**
     * @notice Initialize the registry with parameters
     * @param _minStake Minimum stake to become a validator
     * @param _maxEffectiveStake Maximum stake counted for voting power
     * @param _maxValidators Maximum number of active validators
     * @param _unbondingPeriod Blocks before withdrawn stake is available
     * @param _epochLength Blocks between epoch transitions
     */
    function initialize(
        uint256 _minStake,
        uint256 _maxEffectiveStake,
        uint256 _maxValidators,
        uint256 _unbondingPeriod,
        uint256 _epochLength
    ) external onlyAdmin {
        require(!initialized, "Already initialized");

        minStake = _minStake;
        maxEffectiveStake = _maxEffectiveStake;
        maxValidators = _maxValidators;
        unbondingPeriod = _unbondingPeriod;
        epochLength = _epochLength;
        lastEpochBlock = block.number;
        currentEpoch = 1;
        permissionedMode = true; // Start in permissioned mode by default
        initialized = true;
    }

    /**
     * @notice Approve a validator address (only in permissioned mode)
     * @param operator Address to approve
     */
    function approveValidator(address operator) external onlyAdmin {
        approvedValidators[operator] = true;
        emit ValidatorApproved(operator);
    }

    /**
     * @notice Revoke validator approval (only in permissioned mode)
     * @param operator Address to revoke
     */
    function revokeValidator(address operator) external onlyAdmin {
        approvedValidators[operator] = false;
        emit ValidatorRevoked(operator);
    }

    /**
     * @notice Set permissioned mode
     * @param _permissioned True for permissioned, false for permissionless
     */
    function setPermissionedMode(bool _permissioned) external onlyAdmin {
        permissionedMode = _permissioned;
        emit PermissionedModeChanged(_permissioned);
    }

    /**
     * @notice Add genesis validators (called during chain initialization)
     * @param operators Array of operator addresses
     * @param pubKeys Array of Ed25519 public keys
     * @param stakes Array of initial stakes
     * @param monikers Array of validator names
     */
    function addGenesisValidators(
        address[] calldata operators,
        bytes[] calldata pubKeys,
        uint256[] calldata stakes,
        string[] calldata monikers
    ) external onlyAdmin {
        require(operators.length == pubKeys.length, "Length mismatch");
        require(operators.length == stakes.length, "Length mismatch");
        require(operators.length == monikers.length, "Length mismatch");

        for (uint256 i = 0; i < operators.length; i++) {
            _registerValidator(operators[i], pubKeys[i], stakes[i], monikers[i]);
            validators[operators[i]].active = true;
            validators[operators[i]].activeSince = block.number;
            activeValidatorCount++;
        }
    }

    /**
     * @notice Update parameters (governance)
     */
    function updateParams(
        uint256 _minStake,
        uint256 _maxEffectiveStake,
        uint256 _maxValidators,
        uint256 _unbondingPeriod
    ) external onlyAdmin {
        minStake = _minStake;
        maxEffectiveStake = _maxEffectiveStake;
        maxValidators = _maxValidators;
        unbondingPeriod = _unbondingPeriod;
    }

    /**
     * @notice Renounce admin role (make contract immutable)
     */
    function renounceAdmin() external onlyAdmin {
        admin = address(0);
    }

    // =========================================================================
    // Validator Functions
    // =========================================================================

    /**
     * @notice Register as a new validator
     * @param pubKey Ed25519 public key (32 bytes)
     * @param moniker Human-readable name
     */
    function register(bytes calldata pubKey, string calldata moniker) external onlyInitialized {
        require(pubKey.length == 32, "Invalid pubKey length");
        require(validators[msg.sender].operator == address(0), "Already registered");
        require(pubKeyToOperator[keccak256(pubKey)] == address(0), "PubKey already used");

        // In permissioned mode, only approved validators can register
        if (permissionedMode) {
            require(approvedValidators[msg.sender], "Not approved in permissioned mode");
        }

        // Transfer initial stake
        uint256 stake = minStake;
        require(stakingToken.transferFrom(msg.sender, address(this), stake), "Transfer failed");

        _registerValidator(msg.sender, pubKey, stake, moniker);

        // Queue for activation at next epoch
        pendingUpdates.push(ValidatorUpdate({
            pubKey: pubKey,
            power: int64(uint64(_computeVotingPower(stake)))
        }));

        emit ValidatorRegistered(msg.sender, pubKey, stake, moniker);
    }

    /**
     * @notice Increase stake (increases voting power up to cap)
     * @param amount Amount of USDC to add
     */
    function increaseStake(uint256 amount) external onlyInitialized {
        Validator storage v = validators[msg.sender];
        require(v.operator != address(0), "Not registered");
        require(v.unbondingStart == 0, "Unbonding");

        require(stakingToken.transferFrom(msg.sender, address(this), amount), "Transfer failed");

        v.stake += amount;
        totalStaked += amount;

        uint256 newPower = _computeVotingPower(v.stake);
        if (newPower != v.votingPower) {
            v.votingPower = newPower;
            pendingUpdates.push(ValidatorUpdate({
                pubKey: v.pubKey,
                power: int64(uint64(newPower))
            }));
        }

        emit ValidatorStakeIncreased(msg.sender, amount, v.stake);
    }

    /**
     * @notice Start unbonding (validator will be removed from active set)
     */
    function startUnbonding() external onlyInitialized {
        Validator storage v = validators[msg.sender];
        require(v.operator != address(0), "Not registered");
        require(v.unbondingStart == 0, "Already unbonding");

        v.unbondingStart = block.number;

        // Queue removal at next epoch
        pendingUpdates.push(ValidatorUpdate({
            pubKey: v.pubKey,
            power: 0  // Power of 0 means removal
        }));

        emit ValidatorUnbondingStarted(msg.sender, block.number);
    }

    /**
     * @notice Complete unbonding and withdraw stake
     */
    function completeUnbonding() external onlyInitialized {
        Validator storage v = validators[msg.sender];
        require(v.operator != address(0), "Not registered");
        require(v.unbondingStart != 0, "Not unbonding");
        require(block.number >= v.unbondingStart + unbondingPeriod, "Unbonding period not complete");

        uint256 stake = v.stake;
        totalStaked -= stake;

        // Clear validator data
        bytes32 pubKeyHash = keccak256(v.pubKey);
        delete pubKeyToOperator[pubKeyHash];
        delete validators[msg.sender];

        if (v.active) {
            activeValidatorCount--;
        }

        // Transfer stake back
        require(stakingToken.transfer(msg.sender, stake), "Transfer failed");

        emit ValidatorExited(msg.sender, stake);
    }

    // =========================================================================
    // Epoch Management (called by consensus layer)
    // =========================================================================

    /**
     * @notice Process epoch transition
     * @dev Called by ABCI app at epoch boundaries
     * @return updates Array of validator updates to apply
     */
    function processEpoch() external onlyInitialized returns (ValidatorUpdate[] memory updates) {
        require(block.number >= lastEpochBlock + epochLength, "Epoch not complete");

        updates = new ValidatorUpdate[](pendingUpdates.length);
        uint256 added = 0;
        uint256 removed = 0;

        for (uint256 i = 0; i < pendingUpdates.length; i++) {
            updates[i] = pendingUpdates[i];

            // Update active status
            address operator = pubKeyToOperator[keccak256(pendingUpdates[i].pubKey)];
            if (operator != address(0)) {
                Validator storage v = validators[operator];
                if (pendingUpdates[i].power > 0) {
                    if (!v.active) {
                        v.active = true;
                        v.activeSince = block.number;
                        activeValidatorCount++;
                        added++;
                    }
                } else {
                    if (v.active) {
                        v.active = false;
                        activeValidatorCount--;
                        removed++;
                    }
                }
            }
        }

        // Clear pending updates
        delete pendingUpdates;

        // Update epoch tracking
        currentEpoch++;
        lastEpochBlock = block.number;

        emit EpochTransition(currentEpoch, activeValidatorCount, totalStaked);
        emit ValidatorSetUpdated(currentEpoch, added, removed);

        return updates;
    }

    /**
     * @notice Check if epoch transition is pending
     */
    function isEpochTransitionPending() external view returns (bool) {
        return block.number >= lastEpochBlock + epochLength && pendingUpdates.length > 0;
    }

    // =========================================================================
    // Slashing (called by consensus layer on misbehavior)
    // =========================================================================

    /**
     * @notice Slash a validator's stake
     * @param operator Address of validator to slash
     * @param amount Amount to slash
     * @param reason Reason for slashing
     */
    function slash(address operator, uint256 amount, string calldata reason) external onlyAdmin {
        Validator storage v = validators[operator];
        require(v.operator != address(0), "Not registered");

        uint256 slashAmount = amount > v.stake ? v.stake : amount;
        v.stake -= slashAmount;
        totalStaked -= slashAmount;

        // Update voting power
        v.votingPower = _computeVotingPower(v.stake);

        // If stake falls below minimum, start unbonding
        if (v.stake < minStake && v.unbondingStart == 0) {
            v.unbondingStart = block.number;
            pendingUpdates.push(ValidatorUpdate({
                pubKey: v.pubKey,
                power: 0
            }));
        }

        // Slashed tokens go to a burn address or treasury
        // For simplicity, we just reduce total supply (burn)
        // In production, send to treasury or insurance fund

        emit ValidatorSlashed(operator, slashAmount, reason);
    }

    // =========================================================================
    // View Functions
    // =========================================================================

    /**
     * @notice Get active validators for consensus
     * @return operators Array of operator addresses
     * @return pubKeys Array of public keys
     * @return powers Array of voting powers
     */
    function getActiveValidators() external view returns (
        address[] memory operators,
        bytes[] memory pubKeys,
        uint256[] memory powers
    ) {
        operators = new address[](activeValidatorCount);
        pubKeys = new bytes[](activeValidatorCount);
        powers = new uint256[](activeValidatorCount);

        uint256 index = 0;
        for (uint256 i = 0; i < validatorList.length && index < activeValidatorCount; i++) {
            Validator storage v = validators[validatorList[i]];
            if (v.active) {
                operators[index] = v.operator;
                pubKeys[index] = v.pubKey;
                powers[index] = v.votingPower;
                index++;
            }
        }
    }

    /**
     * @notice Get validator info
     */
    function getValidator(address operator) external view returns (
        bytes memory pubKey,
        uint256 stake,
        uint256 votingPower,
        bool active,
        uint256 activeSince,
        uint256 unbondingStart,
        string memory moniker
    ) {
        Validator storage v = validators[operator];
        return (
            v.pubKey,
            v.stake,
            v.votingPower,
            v.active,
            v.activeSince,
            v.unbondingStart,
            v.moniker
        );
    }

    /**
     * @notice Get total validator count (active and inactive)
     */
    function getTotalValidatorCount() external view returns (uint256) {
        return validatorList.length;
    }

    /**
     * @notice Get pending updates count
     */
    function getPendingUpdatesCount() external view returns (uint256) {
        return pendingUpdates.length;
    }

    /**
     * @notice Check if an address can register as validator
     * @param operator Address to check
     * @return canRegister True if the address can register
     * @return reason Reason if cannot register
     */
    function canRegister(address operator) external view returns (bool canRegister, string memory reason) {
        if (!initialized) {
            return (false, "Not initialized");
        }
        if (validators[operator].operator != address(0)) {
            return (false, "Already registered");
        }
        if (permissionedMode && !approvedValidators[operator]) {
            return (false, "Not approved in permissioned mode");
        }
        if (activeValidatorCount >= maxValidators) {
            return (false, "Max validators reached");
        }
        return (true, "");
    }

    /**
     * @notice Check if registry is in permissioned mode
     */
    function isPermissioned() external view returns (bool) {
        return permissionedMode;
    }

    // =========================================================================
    // Internal Functions
    // =========================================================================

    function _registerValidator(
        address operator,
        bytes memory pubKey,
        uint256 stake,
        string memory moniker
    ) internal {
        validators[operator] = Validator({
            operator: operator,
            pubKey: pubKey,
            stake: stake,
            votingPower: _computeVotingPower(stake),
            active: false,
            activeSince: 0,
            unbondingStart: 0,
            moniker: moniker
        });

        pubKeyToOperator[keccak256(pubKey)] = operator;
        validatorList.push(operator);
        totalStaked += stake;
    }

    function _computeVotingPower(uint256 stake) internal view returns (uint256) {
        // Voting power is stake, capped at maxEffectiveStake
        // This prevents a single validator from having too much power
        if (stake > maxEffectiveStake) {
            return maxEffectiveStake;
        }
        return stake;
    }
}
