# Blend Backstop Bootstrapper

This repository contains an auction smart contract that helps bootstrap Blend Protocol backstops by providing a mechanism for user's with a large amount of either BLND or USDC but an insufficient amount of the other asset to create bootstrapping events which allow other users to pair the other asset with the user's asset to mint the required BLND:USDC LP tokens and deposit them into the backstop

## Documentation

To learn more about the Blend Protocol, visit the docs:

- [Blend Docs](https://docs.blend.capital/)

### Bootstrapping

Blend Bootstraps are carried out over a series of steps:

1. A user creates a bootstrapping event by calling the `add_bootstrap` function. This function takes the following parameters:

- boostrapper: The address of the bootstrap initiator.
- bootstrap_token: The index of the token in the comet pool that you want to bootstrap (0 for BLND 1 for USDC).
- bootstrap_amount: The bootstrap token amount.
- pair_min: The minimum amount of pair token to add.
- duration: The duration of the bootstrap in blocks.
- pool_address: The address of the pool whose backstop is being funded.

There are a few things to consider when creating your bootstrap event:

- Pair min is the minimum amount of pair tokens that you're willing to pair your bootstrap tokens with. Setting this too low will result in you receiving fewer LP tokens as you'll realize more slippage when the tokens are deposited into the comet pool. Setting it too high will make it harder to fill your bootstrap event. You should consider the current balance of bootstrap and pair tokens in the pool, and how much larger you're making them pool by adding your tokens when setting this field.
- Duration is the number of blocks that the bootstrap event will be open for. This is important as the longer the duration, the more time there is for other users to pair their tokens with yours. Setting this too low might result in you being unable to fill your bootstrap event.
- Pool address is the address of the pool that you're bootstrapping. When you claim the tokens from a successful bootstrap event the LP tokens will be deposited into this pool's backstop. So make sure you're bootstrapping a pool that both you, and potential participants are interested in insuring.

2. User's can now join and exit the bootstrap event by calling the `join` and `exit` functions. The important parameter for these functions is the `amount` parameter which is the amount of pair tokens the user deposits or withdraws from the bootstrap event.

User's joining and exiting the bootstrap event influences the number of LP tokens that are minted and deposited into the backstop. You could think of it as a user agreeing to "buy" or "sell" deposited LP tokens, with the price being determined by the ratio of the bootstrap tokens to the pair tokens in the pool.

Once the bootstrap duration has expired users can no longer join or exit the bootstrap event.

3. Once the bootstrap event has ended, anyone can call the `close_bootstrap` function to finalize the bootstrap. Then, if the `pair_min` was met, all tokens are deposited into the comet pool. If the `pair_min` was not met, the bootstrap is marked as cancelled and the bootstrapper and participants can retrieve their tokens by calling `claim`.

It's important to note that multiple `close_bootstrap` calls may be required in order to fully finalize the bootstrap. This is because comet does not allow single sided deposits larger than 50% of the pool's token balance. If a bootstrap is too unbalanced it will deposit up to this limit, and the someone will need to call `close_bootstrap` again to deposit the remaining tokens.

4. After the bootstrap has been finalized, the bootstrapper and participants can call the `claim` function to retrieve their tokens. In the case of a successful bootstrap the claimed comet LP tokens will be deposited into the specified pool's backstop. In the case of a cancelled bootstrap the originally deposited tokens will be returned to the bootstrapper and participants.

## Audits

No audits are planned at this time.

## Getting Started

Build the contracts with:

```
make
```

Run all unit tests and the integration test suite with:

```
make test
```

## Deployment

The `make` command creates an optimized and un-optimized set of WASM contracts. It's recommended to use the optimized version if deploying to a network.

These can be found at the path:

```
target/wasm32-unknown-unknown/optimized
```

For help with deployment to a network, please visit the [Blend Utils](https://github.com/blend-capital/blend-utils) repo.

## Contributing

Notes for contributors:

- Under no circumstances should the "overflow-checks" flag be removed otherwise contract math will become unsafe

## Community Links

A set of links for various things in the community. Please submit a pull request if you would like a link included.

- [Blend Discord](https://discord.com/invite/a6CDBQQcjW)
