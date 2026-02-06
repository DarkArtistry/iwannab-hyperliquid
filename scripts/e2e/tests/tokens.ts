/**
 * Token Standards Tests - ERC20, ERC721, and ERC1155
 *
 * Tests for deploying and interacting with standard token contracts on the EVM.
 */

import {
  CONFIG,
  TEST_ACCOUNTS,
  runTest,
  logSection,
  log,
  logProgress,
  TestContext,
} from '../lib/index.js';

import {
  createPublicClient,
  createWalletClient,
  http,
  encodeAbiParameters,
} from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { foundry } from 'viem/chains';

export async function runTokenStandardsTests(ctx: TestContext): Promise<void> {
  logSection('9. Token Standards Tests');
  log('');
  log('  Testing ERC20, ERC721, and ERC1155 token deployments and interactions');
  log('');

  const publicClient = createPublicClient({
    chain: { ...foundry, id: CONFIG.CHAIN_ID },
    transport: http(CONFIG.EVM_RPC_URL),
  });

  const account = privateKeyToAccount(TEST_ACCOUNTS.ALICE.privateKey);
  const walletClient = createWalletClient({
    account,
    chain: { ...foundry, id: CONFIG.CHAIN_ID },
    transport: http(CONFIG.EVM_RPC_URL),
  });

  // =========================================================================
  // ERC20 CONTRACT BYTECODE
  // =========================================================================
  //
  // Minimal ERC20 implementation with:
  //   - Constructor: (string name, string symbol, uint8 decimals, uint256 supply)
  //   - name(), symbol(), decimals(), totalSupply() - metadata getters
  //   - balanceOf(address) - get token balance
  //   - transfer(address to, uint256 amount) - transfer tokens
  //   - approve(address spender, uint256 amount) - approve spending
  //
  // Constructor encodes 4 parameters, so we append them to the bytecode.
  // =========================================================================
  const ERC20_BYTECODE =
    '0x608060405234801561000f575f5ffd5b506040516109ab3803806109ab83398101604081905261002e91610115565b5f610039858261021c565b506001610046848261021c565b506002805460ff191660ff93909316929092179091556003819055335f90815260046020526040902055506102d69050565b634e487b7160e01b5f52604160045260245ffd5b5f82601f83011261009b575f5ffd5b81516001600160401b038111156100b4576100b4610078565b604051601f8201601f19908116603f011681016001600160401b03811182821017156100e2576100e2610078565b6040528181528382016020018510156100f9575f5ffd5b8160208501602083015e5f918101602001919091529392505050565b5f5f5f5f60808587031215610128575f5ffd5b84516001600160401b0381111561013d575f5ffd5b6101498782880161008c565b602087015190955090506001600160401b03811115610166575f5ffd5b6101728782880161008c565b935050604085015160ff81168114610188575f5ffd5b6060959095015193969295505050565b600181811c908216806101ac57607f821691505b6020821081036101ca57634e487b7160e01b5f52602260045260245ffd5b50919050565b601f82111561021757805f5260205f20601f840160051c810160208510156101f55750805b601f840160051c820191505b81811015610214575f8155600101610201565b50505b505050565b81516001600160401b0381111561023557610235610078565b610249816102438454610198565b846101d0565b6020601f82116001811461027b575f83156102645750848201515b5f19600385901b1c1916600184901b178455610214565b5f84815260208120601f198516915b828110156102aa578785015182556020948501946001909201910161028a565b50848210156102c757868401515f19600387901b60f8161c191681555b50505050600190811b01905550565b6106c8806102e35f395ff3fe608060405234801561000f575f5ffd5b5060043610610090575f3560e01c8063313ce56711610063578063313ce567146100ff57806370a082311461011e57806395d89b411461013d578063a9059cbb14610145578063dd62ed3e14610158575f5ffd5b806306fdde0314610094578063095ea7b3146100b257806318160ddd146100d557806323b872dd146100ec575b5f5ffd5b61009c610182565b6040516100a9919061051d565b60405180910390f35b6100c56100c036600461056d565b61020d565b60405190151581526020016100a9565b6100de60035481565b6040519081526020016100a9565b6100c56100fa366004610595565b610279565b60025461010c9060ff1681565b60405160ff90911681526020016100a9565b6100de61012c3660046105cf565b60046020525f908152604090205481565b61009c61042f565b6100c561015336600461056d565b61043c565b6100de6101663660046105ef565b600560209081525f928352604080842090915290825290205481565b5f805461018e90610620565b80601f01602080910402602001604051908101604052809291908181526020018280546101ba90610620565b80156102055780601f106101dc57610100808354040283529160200191610205565b820191905f5260205f20905b8154815290600101906020018083116101e857829003601f168201915b505050505081565b335f8181526005602090815260408083206001600160a01b038716808552925280832085905551919290917f8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925906102679086815260200190565b60405180910390a35060015b92915050565b6001600160a01b0383165f908152600460205260408120548211156102dc5760405162461bcd60e51b8152602060048201526014602482015273496e73756666696369656e742062616c616e636560601b60448201526064015b60405180910390fd5b6001600160a01b0384165f9081526005602090815260408083203384529091529020548211156103475760405162461bcd60e51b8152602060048201526016602482015275496e73756666696369656e7420616c6c6f77616e636560501b60448201526064016102d3565b6001600160a01b0384165f908152600460205260408120805484929061036e90849061066c565b90915550506001600160a01b0383165f908152600460205260408120805484929061039a90849061067f565b90915550506001600160a01b0384165f908152600560209081526040808320338452909152812080548492906103d190849061066c565b92505081905550826001600160a01b0316846001600160a01b03167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8460405161041d91815260200190565b60405180910390a35060019392505050565b6001805461018e90610620565b335f908152600460205260408120548211156104915760405162461bcd60e51b8152602060048201526014602482015273496e73756666696369656e742062616c616e636560601b60448201526064016102d3565b335f90815260046020526040812080548492906104af90849061066c565b90915550506001600160a01b0383165f90815260046020526040812080548492906104db90849061067f565b90915550506040518281526001600160a01b0384169033907fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef90602001610267565b602081525f82518060208401528060208501604085015e5f604082850101526040601f19601f83011684010191505092915050565b80356001600160a01b0381168114610568575f5ffd5b919050565b5f5f6040838503121561057e575f5ffd5b61058783610552565b946020939093013593505050565b5f5f5f606084860312156105a7575f5ffd5b6105b084610552565b92506105be60208501610552565b929592945050506040919091013590565b5f602082840312156105df575f5ffd5b6105e882610552565b9392505050565b5f5f60408385031215610600575f5ffd5b61060983610552565b915061061760208401610552565b90509250929050565b600181811c9082168061063457607f821691505b60208210810361065257634e487b7160e01b5f52602260045260245ffd5b50919050565b634e487b7160e01b5f52601160045260245ffd5b8181038181111561027357610273610658565b808201808211156102735761027361065856fea264697066735822122064975e691fcc9f1366dd82129c79b3cc5e839a683eec4825fc85c64445e9117464736f6c634300081d0033';

  // ERC721 bytecode compiled with solc 0.8.29
  const ERC721_BYTECODE =
    '0x608060405234801561000f575f5ffd5b50604051610a83380380610a8383398101604081905261002e916100eb565b5f61003983826101d4565b50600161004682826101d4565b50505061028e565b634e487b7160e01b5f52604160045260245ffd5b5f82601f830112610071575f5ffd5b81516001600160401b0381111561008a5761008a61004e565b604051601f8201601f19908116603f011681016001600160401b03811182821017156100b8576100b861004e565b6040528181528382016020018510156100cf575f5ffd5b8160208501602083015e5f918101602001919091529392505050565b5f5f604083850312156100fc575f5ffd5b82516001600160401b03811115610111575f5ffd5b61011d85828601610062565b602085015190935090506001600160401b0381111561013a575f5ffd5b61014685828601610062565b9150509250929050565b600181811c9082168061016457607f821691505b60208210810361018257634e487b7160e01b5f52602260045260245ffd5b50919050565b601f8211156101cf57805f5260205f20601f840160051c810160208510156101ad5750805b601f840160051c820191505b818110156101cc575f81556001016101b9565b50505b505050565b81516001600160401b038111156101ed576101ed61004e565b610201816101fb8454610150565b84610188565b6020601f821160018114610233575f831561021c5750848201515b5f19600385901b1c1916600184901b1784556101cc565b5f84815260208120601f198516915b828110156102625787850151825560209485019460019092019101610242565b508482101561027f57868401515f19600387901b60f8161c191681555b50505050600190811b01905550565b6107e88061029b5f395ff3fe608060405234801561000f575f5ffd5b506004361061009b575f3560e01c80636a627842116100635780636a6278421461014d57806370a082311461016e57806395d89b411461018d578063a22cb46514610195578063e985e9c5146101a8575f5ffd5b806306fdde031461009f578063081812fc146100bd578063095ea7b3146100fd57806323b872dd146101125780636352211e14610125575b5f5ffd5b6100a76101e5565b6040516100b491906105e6565b60405180910390f35b6100e56100cb36600461061b565b60046020525f90815260409020546001600160a01b031681565b6040516001600160a01b0390911681526020016100b4565b61011061010b36600461064d565b610270565b005b610110610120366004610675565b61031e565b6100e561013336600461061b565b60026020525f90815260409020546001600160a01b031681565b61016061015b3660046106af565b6104d1565b6040519081526020016100b4565b61016061017c3660046106af565b60036020525f908152604090205481565b6100a761056e565b6101106101a33660046106cf565b61057b565b6101d56101b6366004610708565b600560209081525f928352604080842090915290825290205460ff1681565b60405190151581526020016100b4565b5f80546101f190610739565b80601f016020809104026020016040519081016040528092919081815260200182805461021d90610739565b80156102685780601f1061023f57610100808354040283529160200191610268565b820191905f5260205f20905b81548152906001019060200180831161024b57829003601f168201915b505050505081565b5f818152600260205260409020546001600160a01b031633146102c65760405162461bcd60e51b81526020600482015260096024820152682737ba1037bbb732b960b91b60448201526064015b60405180910390fd5b5f8181526004602052604080822080546001600160a01b0319166001600160a01b0386169081179091559051839233917f8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b9259190a45050565b5f818152600260205260409020546001600160a01b038481169116146103725760405162461bcd60e51b81526020600482015260096024820152682737ba1037bbb732b960b91b60448201526064016102bd565b336001600160a01b038416148061039e57505f818152600460205260409020546001600160a01b031633145b806103cb57506001600160a01b0383165f90815260056020908152604080832033845290915290205460ff165b6104085760405162461bcd60e51b815260206004820152600e60248201526d139bdd08185d5d1a1bdc9a5e995960921b60448201526064016102bd565b5f81815260026020908152604080832080546001600160a01b0319166001600160a01b0387811691909117909155861683526003909152812080549161044d83610785565b90915550506001600160a01b0382165f9081526003602052604081208054916104758361079a565b90915550505f8181526004602052604080822080546001600160a01b03191690555182916001600160a01b0385811692908716917fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef91a4505050565b600680545f91829190826104e48361079a565b909155505f81815260026020908152604080832080546001600160a01b0319166001600160a01b03891690811790915583526003909152812080549293509061052c8361079a565b909155505060405181906001600160a01b038516905f907fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef908290a492915050565b600180546101f190610739565b335f8181526005602090815260408083206001600160a01b03871680855290835292819020805460ff191686151590811790915590519081529192917f17307eab39ab6107e8899845ad3d59bd9653f200f220920489ca2b5937696c31910160405180910390a35050565b602081525f82518060208401528060208501604085015e5f604082850101526040601f19601f83011684010191505092915050565b5f6020828403121561062b575f5ffd5b5035919050565b80356001600160a01b0381168114610648575f5ffd5b919050565b5f5f6040838503121561065e575f5ffd5b61066783610632565b946020939093013593505050565b5f5f5f60608486031215610687575f5ffd5b61069084610632565b925061069e60208501610632565b929592945050506040919091013590565b5f602082840312156106bf575f5ffd5b6106c882610632565b9392505050565b5f5f604083850312156106e0575f5ffd5b6106e983610632565b9150602083013580151581146106fd575f5ffd5b809150509250929050565b5f5f60408385031215610719575f5ffd5b61072283610632565b915061073060208401610632565b90509250929050565b600181811c9082168061074d57607f821691505b60208210810361076b57634e487b7160e01b5f52602260045260245ffd5b50919050565b634e487b7160e01b5f52601160045260245ffd5b5f8161079357610793610771565b505f190190565b5f600182016107ab576107ab610771565b506001019056fea2646970667358221220623ee4f26fce173811beba104ecf1a004fe5c8531c1e2506c0d6931fb9ba94f864736f6c634300081d0033';

  // ERC1155 bytecode compiled with solc 0.8.29
  const ERC1155_BYTECODE =
    '0x6080604052348015600e575f5ffd5b5061089c8061001c5f395ff3fe608060405234801561000f575f5ffd5b5060043610610060575f3560e01c8063156e29f6146100645780633656eec2146100795780634e1273f4146100b3578063a22cb465146100d3578063e985e9c5146100e6578063f242432a14610123575b5f5ffd5b6100776100723660046104bf565b610136565b005b6100a06100873660046104ef565b5f60208181529281526040808220909352908152205481565b6040519081526020015b60405180910390f35b6100c66100c13660046105eb565b6101b7565b6040516100aa91906106ae565b6100776100e13660046106f0565b61029e565b6101136100f4366004610729565b600160209081525f928352604080842090915290825290205460ff1681565b60405190151581526020016100aa565b610077610131366004610751565b610309565b5f828152602081815260408083206001600160a01b03871684529091528120805483929061016590849061082c565b909155505060408051838152602081018390526001600160a01b038516915f9133917fc3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62910160405180910390a4505050565b60605f835167ffffffffffffffff8111156101d4576101d4610519565b6040519080825280602002602001820160405280156101fd578160200160208202803683370190505b5090505f5b8451811015610294575f5f85838151811061021f5761021f61083f565b602002602001015181526020019081526020015f205f8683815181106102475761024761083f565b60200260200101516001600160a01b03166001600160a01b031681526020019081526020015f20548282815181106102815761028161083f565b6020908102919091010152600101610202565b5090505b92915050565b335f8181526001602090815260408083206001600160a01b03871680855290835292819020805460ff191686151590811790915590519081529192917f17307eab39ab6107e8899845ad3d59bd9653f200f220920489ca2b5937696c31910160405180910390a35050565b336001600160a01b038616148061034257506001600160a01b0385165f90815260016020908152604080832033845290915290205460ff165b6103845760405162461bcd60e51b815260206004820152600e60248201526d139bdd08185d5d1a1bdc9a5e995960921b60448201526064015b60405180910390fd5b5f838152602081815260408083206001600160a01b03891684529091529020548211156103ea5760405162461bcd60e51b8152602060048201526014602482015273496e73756666696369656e742062616c616e636560601b604482015260640161037b565b5f838152602081815260408083206001600160a01b038916845290915281208054849290610419908490610853565b90915550505f838152602081815260408083206001600160a01b03881684529091528120805484929061044d90849061082c565b909155505060408051848152602081018490526001600160a01b03808716929088169133917fc3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62910160405180910390a45050505050565b80356001600160a01b03811681146104ba575f5ffd5b919050565b5f5f5f606084860312156104d1575f5ffd5b6104da846104a4565b95602085013595506040909401359392505050565b5f5f60408385031215610500575f5ffd5b82359150610510602084016104a4565b90509250929050565b634e487b7160e01b5f52604160045260245ffd5b604051601f8201601f1916810167ffffffffffffffff8111828210171561055657610556610519565b604052919050565b5f67ffffffffffffffff82111561057757610577610519565b5060051b60200190565b5f82601f830112610590575f5ffd5b81356105a361059e8261055e565b61052d565b8082825260208201915060208360051b8601019250858311156105c4575f5ffd5b602085015b838110156105e15780358352602092830192016105c9565b5095945050505050565b5f5f604083850312156105fc575f5ffd5b823567ffffffffffffffff811115610612575f5ffd5b8301601f81018513610622575f5ffd5b803561063061059e8261055e565b8082825260208201915060208360051b850101925087831115610651575f5ffd5b6020840193505b8284101561067a57610669846104a4565b825260209384019390910190610658565b9450505050602083013567ffffffffffffffff811115610698575f5ffd5b6106a485828601610581565b9150509250929050565b602080825282518282018190525f918401906040840190835b818110156106e55783518352602093840193909201916001016106c7565b509095945050505050565b5f5f60408385031215610701575f5ffd5b61070a836104a4565b91506020830135801515811461071e575f5ffd5b809150509250929050565b5f5f6040838503121561073a575f5ffd5b610743836104a4565b9150610510602084016104a4565b5f5f5f5f5f60a08688031215610765575f5ffd5b61076e866104a4565b945061077c602087016104a4565b93506040860135925060608601359150608086013567ffffffffffffffff8111156107a5575f5ffd5b8601601f810188136107b5575f5ffd5b803567ffffffffffffffff8111156107cf576107cf610519565b6107e2601f8201601f191660200161052d565b8181528960208385010111156107f6575f5ffd5b816020840160208301375f602083830101528093505050509295509295909350565b634e487b7160e01b5f52601160045260245ffd5b8082018082111561029857610298610818565b634e487b7160e01b5f52603260045260245ffd5b818103818111156102985761029861081856fea264697066735822122074a41f87bd4b3857dcfa9d80d955064326778d15dc94fb1b655d95e8758e32ee64736f6c634300081d0033';

  // ABIs for token contracts
  const ERC20_ABI = [
    { inputs: [], name: 'name', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'symbol', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'decimals', outputs: [{ type: 'uint8' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'totalSupply', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'account', type: 'address' }], name: 'balanceOf', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'to', type: 'address' }, { name: 'amount', type: 'uint256' }], name: 'transfer', outputs: [{ type: 'bool' }], stateMutability: 'nonpayable', type: 'function' },
    { inputs: [{ name: 'spender', type: 'address' }, { name: 'amount', type: 'uint256' }], name: 'approve', outputs: [{ type: 'bool' }], stateMutability: 'nonpayable', type: 'function' },
    { anonymous: false, inputs: [{ indexed: true, name: 'from', type: 'address' }, { indexed: true, name: 'to', type: 'address' }, { indexed: false, name: 'value', type: 'uint256' }], name: 'Transfer', type: 'event' },
    { anonymous: false, inputs: [{ indexed: true, name: 'owner', type: 'address' }, { indexed: true, name: 'spender', type: 'address' }, { indexed: false, name: 'value', type: 'uint256' }], name: 'Approval', type: 'event' },
  ] as const;

  const ERC721_ABI = [
    { inputs: [], name: 'name', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [], name: 'symbol', outputs: [{ type: 'string' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'account', type: 'address' }], name: 'balanceOf', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'to', type: 'address' }], name: 'mint', outputs: [{ type: 'uint256' }], stateMutability: 'nonpayable', type: 'function' },
    { inputs: [{ name: 'tokenId', type: 'uint256' }], name: 'ownerOf', outputs: [{ type: 'address' }], stateMutability: 'view', type: 'function' },
    { anonymous: false, inputs: [{ indexed: true, name: 'from', type: 'address' }, { indexed: true, name: 'to', type: 'address' }, { indexed: true, name: 'tokenId', type: 'uint256' }], name: 'Transfer', type: 'event' },
  ] as const;

  const ERC1155_ABI = [
    { inputs: [{ name: 'id', type: 'uint256' }, { name: 'account', type: 'address' }], name: 'balanceOf', outputs: [{ type: 'uint256' }], stateMutability: 'view', type: 'function' },
    { inputs: [{ name: 'to', type: 'address' }, { name: 'id', type: 'uint256' }, { name: 'amount', type: 'uint256' }], name: 'mint', outputs: [], stateMutability: 'nonpayable', type: 'function' },
  ] as const;

  let erc20Address: `0x${string}` | null = null;
  let erc721Address: `0x${string}` | null = null;
  let erc1155Address: `0x${string}` | null = null;

  // ERC20 Tests
  await runTest(ctx, 'Deploy ERC20 token', 'tokens', 'Deploy a minimal ERC20 token contract', async () => {
    logProgress('Deploying TestToken ERC20...');

    // Encode constructor arguments: name, symbol, decimals, initialSupply
    const args = encodeAbiParameters(
      [{ type: 'string' }, { type: 'string' }, { type: 'uint8' }, { type: 'uint256' }],
      ['Test Token', 'TEST', 18, 1000000000000000000000000n] // 1 million tokens
    );

    const deployData = (ERC20_BYTECODE + args.slice(2)) as `0x${string}`;

    const hash = await walletClient.sendTransaction({
      data: deployData,
    });
    logProgress(`Deploy tx: ${hash}`);

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('ERC20 deployment failed');
    if (!receipt.contractAddress) throw new Error('No contract address');

    erc20Address = receipt.contractAddress;
    logProgress(`ERC20 deployed at: ${erc20Address}`);
  });

  await runTest(ctx, 'Verify ERC20 token metadata', 'tokens', 'Read name, symbol, decimals from ERC20', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress('Reading token metadata...');
    const name = await publicClient.readContract({ address: erc20Address, abi: ERC20_ABI, functionName: 'name' });
    const symbol = await publicClient.readContract({ address: erc20Address, abi: ERC20_ABI, functionName: 'symbol' });
    const decimals = await publicClient.readContract({ address: erc20Address, abi: ERC20_ABI, functionName: 'decimals' });

    logProgress(`Token: ${name} (${symbol}), ${decimals} decimals`);
    if (name !== 'Test Token') throw new Error(`Expected name "Test Token", got "${name}"`);
    if (symbol !== 'TEST') throw new Error(`Expected symbol "TEST", got "${symbol}"`);
    if (decimals !== 18) throw new Error(`Expected 18 decimals, got ${decimals}`);
  });

  await runTest(ctx, 'Check ERC20 balance', 'tokens', 'Verify deployer received initial supply', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress('Checking deployer balance...');
    const balance = await publicClient.readContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'balanceOf',
      args: [TEST_ACCOUNTS.ALICE.address],
    });

    logProgress(`Alice balance: ${balance}`);
    if (balance !== 1000000000000000000000000n) throw new Error('Incorrect initial balance');
  });

  await runTest(ctx, 'ERC20 transfer', 'tokens', 'Transfer tokens between accounts', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress('Transferring 1000 tokens to Bob...');
    const hash = await walletClient.writeContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'transfer',
      args: [TEST_ACCOUNTS.BOB.address, 1000000000000000000000n],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Transfer failed');

    const bobBalance = await publicClient.readContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'balanceOf',
      args: [TEST_ACCOUNTS.BOB.address],
    });

    logProgress(`Bob balance: ${bobBalance}`);
    if (bobBalance !== 1000000000000000000000n) throw new Error('Transfer amount mismatch');

    // Verify Alice's balance decreased (catch mint-without-burn bugs)
    const aliceBalance = await publicClient.readContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'balanceOf',
      args: [TEST_ACCOUNTS.ALICE.address],
    });
    const expectedAlice = 1000000000000000000000000n - 1000000000000000000000n;
    logProgress(`Alice balance after transfer: ${aliceBalance}`);
    if (aliceBalance !== expectedAlice) throw new Error(`Alice balance should be ${expectedAlice}, got ${aliceBalance}`);
  });

  // EVM Event Log Tests
  const TRANSFER_TOPIC = '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef';

  await runTest(ctx, 'ERC20 Transfer event in receipt', 'tokens', 'Verify transfer receipt contains Transfer event log', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress('Performing transfer for event verification...');
    const hash = await walletClient.writeContract({
      address: erc20Address,
      abi: ERC20_ABI,
      functionName: 'transfer',
      args: [TEST_ACCOUNTS.BOB.address, 500000000000000000000n],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Transfer failed');

    logProgress(`Receipt has ${receipt.logs.length} log(s)`);
    if (receipt.logs.length === 0) throw new Error('Receipt has no logs - EVM log storage may be broken');

    // Find Transfer event by topic
    const transferLog = receipt.logs.find(
      (log) => log.topics[0]?.toLowerCase() === TRANSFER_TOPIC.toLowerCase()
    );
    if (!transferLog) throw new Error(`No Transfer event found in logs (topics: ${receipt.logs.map(l => l.topics[0]).join(', ')})`);

    // Verify address matches contract
    if (transferLog.address.toLowerCase() !== erc20Address.toLowerCase()) {
      throw new Error(`Log address ${transferLog.address} != contract ${erc20Address}`);
    }

    // Verify indexed from/to topics (padded to 32 bytes)
    const alicePadded = '0x000000000000000000000000' + TEST_ACCOUNTS.ALICE.address.slice(2).toLowerCase();
    const bobPadded = '0x000000000000000000000000' + TEST_ACCOUNTS.BOB.address.slice(2).toLowerCase();

    if (transferLog.topics[1]?.toLowerCase() !== alicePadded) {
      throw new Error(`From topic ${transferLog.topics[1]} != expected ${alicePadded}`);
    }
    if (transferLog.topics[2]?.toLowerCase() !== bobPadded) {
      throw new Error(`To topic ${transferLog.topics[2]} != expected ${bobPadded}`);
    }

    logProgress(`Transfer event verified: from=${transferLog.topics[1]?.slice(0, 20)}..., to=${transferLog.topics[2]?.slice(0, 20)}...`);
  });

  await runTest(ctx, 'eth_getLogs returns ERC20 events', 'tokens', 'Query logs by contract address and verify structure', async () => {
    if (!erc20Address) throw new Error('ERC20 not deployed');

    logProgress(`Querying logs for ERC20 contract ${erc20Address}...`);
    const logs = await publicClient.getLogs({
      address: erc20Address,
    });

    logProgress(`Found ${logs.length} log(s) for ERC20 contract`);
    if (logs.length === 0) throw new Error('eth_getLogs returned no events for ERC20 contract');

    // Verify at least one Transfer event
    const transferLogs = logs.filter(
      (log) => log.topics[0]?.toLowerCase() === TRANSFER_TOPIC.toLowerCase()
    );
    if (transferLogs.length === 0) throw new Error('No Transfer events found in eth_getLogs results');

    // Verify log structure
    for (const log of transferLogs) {
      if (!log.address) throw new Error('Log missing address field');
      if (!log.topics || log.topics.length === 0) throw new Error('Log missing topics');
      if (log.data === undefined) throw new Error('Log missing data field');
      if (log.blockNumber === undefined) throw new Error('Log missing blockNumber');
      if (!log.transactionHash) throw new Error('Log missing transactionHash');
    }

    logProgress(`Verified ${transferLogs.length} Transfer event(s) with complete structure`);
  });

  // ERC721 Tests
  await runTest(ctx, 'Deploy ERC721 NFT', 'tokens', 'Deploy a minimal ERC721 NFT contract', async () => {
    logProgress('Deploying TestNFT ERC721...');

    const args = encodeAbiParameters(
      [{ type: 'string' }, { type: 'string' }],
      ['Test NFT', 'TNFT']
    );

    const deployData = (ERC721_BYTECODE + args.slice(2)) as `0x${string}`;

    const hash = await walletClient.sendTransaction({
      data: deployData,
    });
    logProgress(`Deploy tx: ${hash}`);

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('ERC721 deployment failed');
    if (!receipt.contractAddress) throw new Error('No contract address');

    erc721Address = receipt.contractAddress;
    logProgress(`ERC721 deployed at: ${erc721Address}`);
  });

  await runTest(ctx, 'Mint ERC721 NFT', 'tokens', 'Mint a new NFT to Alice', async () => {
    if (!erc721Address) throw new Error('ERC721 not deployed');

    logProgress('Minting NFT to Alice...');
    const hash = await walletClient.writeContract({
      address: erc721Address,
      abi: ERC721_ABI,
      functionName: 'mint',
      args: [TEST_ACCOUNTS.ALICE.address],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Mint failed');

    const balance = await publicClient.readContract({
      address: erc721Address,
      abi: ERC721_ABI,
      functionName: 'balanceOf',
      args: [TEST_ACCOUNTS.ALICE.address],
    });

    logProgress(`Alice NFT balance: ${balance}`);
    if (balance !== 1n) throw new Error(`Expected exactly 1 NFT, got ${balance}`);

    // Verify ownership of the minted token (tokenId 0 is the first minted)
    const owner = await publicClient.readContract({
      address: erc721Address,
      abi: ERC721_ABI,
      functionName: 'ownerOf',
      args: [0n],
    });
    logProgress(`Token 0 owner: ${owner}`);
    if (owner.toLowerCase() !== TEST_ACCOUNTS.ALICE.address.toLowerCase()) {
      throw new Error(`Expected owner ${TEST_ACCOUNTS.ALICE.address}, got ${owner}`);
    }
  });

  await runTest(ctx, 'ERC721 Transfer event in receipt', 'tokens', 'Verify mint receipt contains Transfer event from 0x0', async () => {
    if (!erc721Address) throw new Error('ERC721 not deployed');

    logProgress('Minting another NFT for event verification...');
    const hash = await walletClient.writeContract({
      address: erc721Address,
      abi: ERC721_ABI,
      functionName: 'mint',
      args: [TEST_ACCOUNTS.ALICE.address],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Mint failed');

    logProgress(`Mint receipt has ${receipt.logs.length} log(s)`);
    if (receipt.logs.length === 0) throw new Error('Mint receipt has no logs');

    // Find Transfer event
    const transferLog = receipt.logs.find(
      (log) => log.topics[0]?.toLowerCase() === TRANSFER_TOPIC.toLowerCase()
    );
    if (!transferLog) throw new Error('No Transfer event found in mint receipt');

    // Verify from is zero address (mint)
    const zeroPadded = '0x0000000000000000000000000000000000000000000000000000000000000000';
    if (transferLog.topics[1]?.toLowerCase() !== zeroPadded) {
      throw new Error(`Mint Transfer 'from' should be zero address, got ${transferLog.topics[1]}`);
    }

    // Verify to is Alice
    const alicePadded = '0x000000000000000000000000' + TEST_ACCOUNTS.ALICE.address.slice(2).toLowerCase();
    if (transferLog.topics[2]?.toLowerCase() !== alicePadded) {
      throw new Error(`Mint Transfer 'to' should be Alice, got ${transferLog.topics[2]}`);
    }

    logProgress(`ERC721 mint Transfer event verified: from=0x0, to=Alice`);
  });

  // ERC1155 Tests
  await runTest(ctx, 'Deploy ERC1155 multi-token', 'tokens', 'Deploy a minimal ERC1155 multi-token contract', async () => {
    logProgress('Deploying TestMultiToken ERC1155...');

    const hash = await walletClient.sendTransaction({
      data: ERC1155_BYTECODE as `0x${string}`,
    });
    logProgress(`Deploy tx: ${hash}`);

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('ERC1155 deployment failed');
    if (!receipt.contractAddress) throw new Error('No contract address');

    erc1155Address = receipt.contractAddress;
    logProgress(`ERC1155 deployed at: ${erc1155Address}`);
  });

  await runTest(ctx, 'Mint ERC1155 tokens', 'tokens', 'Mint fungible tokens with ERC1155', async () => {
    if (!erc1155Address) throw new Error('ERC1155 not deployed');

    logProgress('Minting token ID 1 (100 units) to Alice...');
    const hash = await walletClient.writeContract({
      address: erc1155Address,
      abi: ERC1155_ABI,
      functionName: 'mint',
      args: [TEST_ACCOUNTS.ALICE.address, 1n, 100n],
    });

    const receipt = await publicClient.waitForTransactionReceipt({ hash });
    if (receipt.status !== 'success') throw new Error('Mint failed');

    const balance = await publicClient.readContract({
      address: erc1155Address,
      abi: ERC1155_ABI,
      functionName: 'balanceOf',
      args: [1n, TEST_ACCOUNTS.ALICE.address],
    });

    logProgress(`Alice ERC1155 balance (ID 1): ${balance}`);
    if (balance !== 100n) throw new Error('ERC1155 mint amount mismatch');
  });
}
