use soroban_sdk::{contracttype, panic_with_error, Address, Env, Map};

use crate::{errors::BackstopBootstrapperError, storage};

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
pub struct TokenInfo {
    pub address: Address,
    pub weight: i128,
}
#[derive(Clone)]
#[contracttype]
pub struct BootstrapData {
    pub bootstrapper: Address,
    pub bootstrap_amount: i128,
    pub pair_min: i128,
    pub close_ledger: u32,
    pub pool_address: Address,
    pub total_deposits: i128,
    pub deposits: Map<Address, i128>,
    pub status: u32,
    pub backstop_tokens: i128,
    pub bootstrap_token_index: u32,
    pub pair_to_deposit: Option<i128>,
    pub bootstrap_to_deposit: Option<i128>,
}

#[derive(Clone)]
pub struct Bootstrap {
    pub bootstrapper: Address,
    pub bootstrap_amount: i128,
    pub pair_min: i128,
    pub close_ledger: u32,
    pub pool_address: Address,
    pub total_deposits: i128,
    pub deposits: Map<Address, i128>,
    pub status: u32,
    pub backstop_tokens: i128,
    pub bootstrap_token_index: u32,
    pub pair_token_index: u32,
    pub bootstrap_weight: u64,
    pub bootstrap_token_address: Address,
    pub pair_token_address: Address,
    pub pair_to_deposit: Option<i128>,
    pub bootstrap_to_deposit: Option<i128>,
}

impl Bootstrap {
    pub fn new(
        e: &Env,
        bootstrapper: Address,
        bootstrap_amount: i128,
        pair_min: i128,
        duration: u32,
        pool_address: Address,
        bootstrap_token_index: u32,
    ) -> Self {
        let bootstrap_token_info = storage::get_comet_token_data(e, bootstrap_token_index)
            .unwrap_or_else(|| {
                panic_with_error!(&e, BackstopBootstrapperError::BadRequest);
            });
        // we assume the comet pool only has 2 assets
        let pair_token_index = if bootstrap_token_index == 0 { 1 } else { 0 };
        let pair_token_info = storage::get_comet_token_data(e, pair_token_index)
            .unwrap_or_else(|| panic_with_error!(&e, BackstopBootstrapperError::BadRequest));
        Bootstrap {
            bootstrapper,
            bootstrap_amount,
            pair_min,
            close_ledger: e.ledger().sequence() + duration,
            pool_address,
            total_deposits: 0,
            deposits: Map::new(&e),
            status: 0,
            backstop_tokens: 0,
            bootstrap_token_index,
            pair_token_index,
            bootstrap_weight: bootstrap_token_info.weight as u64,
            bootstrap_token_address: bootstrap_token_info.address,
            pair_token_address: pair_token_info.address,
            pair_to_deposit: None,
            bootstrap_to_deposit: None,
        }
    }
    pub fn load(e: &Env, bootstrapper: Address, bootstrap_id: u32) -> Self {
        let bootstrap_data = storage::get_bootstrap_data(&e, bootstrapper.clone(), bootstrap_id)
            .unwrap_or_else(|| {
                panic_with_error!(&e, BackstopBootstrapperError::BootstrapNotFoundError);
            })
            .clone();
        let pair_token_index = if bootstrap_data.bootstrap_token_index == 0 {
            1
        } else {
            0
        };
        let pair_token_info = storage::get_comet_token_data(e, pair_token_index)
            .unwrap_or_else(|| panic_with_error!(&e, BackstopBootstrapperError::BadRequest));
        let bootstrap_token_info =
            storage::get_comet_token_data(e, bootstrap_data.bootstrap_token_index)
                .unwrap_or_else(|| panic_with_error!(&e, BackstopBootstrapperError::BadRequest));
        Bootstrap {
            bootstrapper,
            bootstrap_amount: bootstrap_data.bootstrap_amount,
            pair_min: bootstrap_data.pair_min,
            close_ledger: bootstrap_data.close_ledger,
            pool_address: bootstrap_data.pool_address,
            total_deposits: bootstrap_data.total_deposits,
            deposits: bootstrap_data.deposits,
            status: bootstrap_data.status,
            backstop_tokens: bootstrap_data.backstop_tokens,
            bootstrap_token_index: bootstrap_data.bootstrap_token_index,
            pair_token_index,
            bootstrap_weight: bootstrap_token_info.weight as u64,
            bootstrap_token_address: bootstrap_token_info.address,
            pair_token_address: pair_token_info.address,
            pair_to_deposit: bootstrap_data.pair_to_deposit,
            bootstrap_to_deposit: bootstrap_data.bootstrap_to_deposit,
        }
    }
    pub fn store(&self, e: &Env, bootstrap_id: u32) {
        let bootstrap_data = BootstrapData {
            bootstrapper: self.bootstrapper.clone(),
            bootstrap_amount: self.bootstrap_amount,
            pair_min: self.pair_min,
            close_ledger: self.close_ledger,
            pool_address: self.pool_address.clone(),
            total_deposits: self.total_deposits,
            deposits: self.deposits.clone(),
            status: self.status,
            backstop_tokens: self.backstop_tokens,
            bootstrap_token_index: self.bootstrap_token_index,
            pair_to_deposit: self.pair_to_deposit,
            bootstrap_to_deposit: self.bootstrap_to_deposit,
        };
        storage::set_bootstrap_data(&e, self.bootstrapper.clone(), bootstrap_id, &bootstrap_data);
    }
}

#[cfg(test)]
mod tests {

    use crate::BackstopBootstrapperContract;

    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _, LedgerInfo},
        Address, Env,
    };

    #[test]
    fn test_bootstrap_impl() {
        let e = Env::default();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let this_contract = e.register_contract(None, BackstopBootstrapperContract {});
        let bootstrapper = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrap_token_index = 0;
        let bootstrap_token_info = TokenInfo {
            address: Address::generate(&e),
            weight: 10,
        };
        let pair_token_info = TokenInfo {
            address: Address::generate(&e),
            weight: 90,
        };
        e.as_contract(&this_contract, || {
            storage::set_comet_token_data(&e, 0, bootstrap_token_info.clone());
            storage::set_comet_token_data(&e, 1, pair_token_info.clone());
            let bootstrap = Bootstrap::new(
                &e,
                bootstrapper.clone(),
                100,
                10,
                100,
                pool_address.clone(),
                bootstrap_token_index,
            );
            bootstrap.store(&e, 0);
            let mut loaded_bootstrap = Bootstrap::load(&e, bootstrapper.clone(), 0);
            assert_eq!(loaded_bootstrap.bootstrapper, bootstrapper);
            assert_eq!(loaded_bootstrap.bootstrap_amount, 100);
            assert_eq!(loaded_bootstrap.pair_min, 10);
            assert_eq!(loaded_bootstrap.close_ledger, 100 + e.ledger().sequence());
            assert_eq!(loaded_bootstrap.pool_address, pool_address);
            assert_eq!(loaded_bootstrap.total_deposits, 0);
            assert_eq!(loaded_bootstrap.deposits.len(), 0);
            assert_eq!(loaded_bootstrap.status, 0);
            assert_eq!(loaded_bootstrap.backstop_tokens, 0);
            assert_eq!(
                loaded_bootstrap.bootstrap_token_index,
                bootstrap_token_index
            );
            assert_eq!(loaded_bootstrap.pair_token_index, 1);
            assert_eq!(loaded_bootstrap.bootstrap_weight, 10);
            assert_eq!(
                loaded_bootstrap.bootstrap_token_address,
                bootstrap_token_info.address
            );
            assert_eq!(loaded_bootstrap.pair_token_address, pair_token_info.address);
            let user_address = Address::generate(&e);
            loaded_bootstrap.status = 1;
            loaded_bootstrap.deposits.set(user_address.clone(), 100);
            loaded_bootstrap.store(&e, 0);
            let loaded_bootstrap = Bootstrap::load(&e, bootstrapper.clone(), 0);
            assert_eq!(loaded_bootstrap.status, 1);
            assert_eq!(loaded_bootstrap.deposits.get(user_address), Some(100));
        })
    }
}
