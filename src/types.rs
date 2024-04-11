use soroban_sdk::{contracttype, Address};

#[derive(Clone, Copy, PartialEq)]
#[repr(u32)]
#[contracttype]
pub enum BootstrapStatus {
    Active = 0,
    Closing = 1,
    Completed = 2,
    Cancelled = 3,
}

#[derive(Clone)]
#[contracttype]
pub struct TokenInfo {
    pub address: Address,
    pub weight: i128,
}

#[derive(Clone)]
#[contracttype]
pub struct BootstrapConfig {
    /// The address creating the bootstrap
    pub bootstrapper: Address,
    /// The address of the pool to bootstrap
    pub pool: Address,
    /// The amount of the bootstrap token to bootstrap
    pub amount: i128,
    /// The minimum amount of the pair token to bootstrap
    pub pair_min: i128,
    /// The index of the comet underlying token being bootstrapped
    pub token_index: u32,
    /// The ledger number at which the bootstrap will close
    pub close_ledger: u32,
}

#[derive(Clone)]
#[contracttype]
pub struct BootstrapData {
    /// The total number of pair tokens deposited for this bootstrap
    pub total_pair: i128,
    // The total of backstop tokens minted for this bootstrap
    pub total_backstop_tokens: i128,
    /// The amount of the boostrapped token held by the contract for this boostrap
    pub bootstrap_amount: i128,
    /// The amount of pair tokens held by the contract for this bootstrap
    pub pair_amount: i128,
}
