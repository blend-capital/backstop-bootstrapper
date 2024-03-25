use soroban_sdk::{contracttype, panic_with_error, Address, Env, Map};

use crate::errors::BackstopBootstrapperError;

#[derive(Clone)]
#[repr(u32)]
pub enum BootstrapStatus {
    Active = 0,
    Completed = 1,
    Cancelled = 2,
}

impl BootstrapStatus {
    pub fn from_u32(e: &Env, value: u32) -> Self {
        match value {
            0 => BootstrapStatus::Active,
            1 => BootstrapStatus::Completed,
            2 => BootstrapStatus::Cancelled,
            _ => panic_with_error!(e, BackstopBootstrapperError::BadRequest),
        }
    }
}

#[derive(Clone)]
#[contracttype]
pub struct Bootstrap {
    pub bootstrapper: Address,
    pub bootstrap_token: Address,
    pub pair_token: Address,
    pub bootstrap_amount: i128,
    pub pair_min: i128,
    pub close_ledger: u32,
    pub bootstrap_weight: u64, //should be 7 decimals
    pub pool_address: Address,
    pub total_deposits: i128,
    pub deposits: Map<Address, i128>,
    pub status: u32,
    pub backstop_tokens: i128,
    pub bootstrap_token_index: u32,
    pub pair_token_index: u32,
}
