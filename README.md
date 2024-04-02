# Blend Backstop Bootstrapper

This repository contains an auction smart contract that helps bootstrap Blend Protocol backstops by providing a mechanism for user's with a large amount of either BLND or USDC but an insufficient amount of the other asset to create bootstrapping events which allow other users to pair the other asset with the user's asset to mint the required BLND:USDC LP tokens and deposit them into the backstop

## Documentation

To learn more about the Blend Protocol, visit the docs:

- [Blend Docs](https://docs.blend.capital/)

### Bootstrapping

Blend Bootstraps are carried out over a series of steps:

1. A user creates a bootstrapping event by calling the `add_bootstrap` function. This function takes the following parameters:

- boostrapper: The address of the bootstrap initiator.
- bootstrap_token: The address of the token that the bootstrapper will be depositing so that it can be paired with the other backstop liquidity pool token. (this will either be BLND or USDC)
- pair_token: The address of the token to pair with. (this will either be BLND or USDC)
- bootstrap_amount: The bootstrap token amount.
- pair_min: The minimum amount of pair token to add.
- duration: The duration of the bootstrap in blocks.
- bootstrap_weight: The weight of the bootstrap.
- pool_address: The address of the pool whose backstop is being funded.
- bootstrap_token_index: The index of the bootstrap token.
- pair_token_index: The index of the pair token.

## Audits

No audits have been conducted for the protocol at this time. Results will be included here at the conclusion of an audit.

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
