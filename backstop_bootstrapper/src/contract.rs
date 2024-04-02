use core::u32;

use crate::{
    backstop_bootstrapper::BackstopBootstrapper, bootstrap_management, dependencies::CometClient,
    errors::BackstopBootstrapperError, storage, types::TokenInfo,
};
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env};

#[contract]
pub struct BackstopBootstrapperContract;

#[contractimpl]
impl BackstopBootstrapper for BackstopBootstrapperContract {
    fn initialize(
        e: Env,
        backstop: Address,
        backstop_token: Address,
        pool_factory_address: Address,
    ) {
        if storage::get_is_init(&e) {
            panic_with_error!(&e, BackstopBootstrapperError::AlreadyInitializedError);
        }
        storage::set_is_init(&e);
        storage::set_backstop(&e, backstop);
        storage::set_backstop_token(&e, backstop_token.clone());
        storage::set_pool_factory(&e, pool_factory_address);
        let backstop_token = CometClient::new(&e, &backstop_token);
        let tokens = backstop_token.get_tokens();
        for (i, address) in tokens.iter().enumerate() {
            let weight = backstop_token.get_normalized_weight(&address);
            storage::set_comet_token_data(&e, i as u32, TokenInfo { address, weight });
        }
    }

    fn add_bootstrap(
        e: Env,
        bootstrapper: Address,
        bootstrap_token_index: u32,
        bootstrap_amount: i128,
        pair_min: i128,
        duration: u32,
        pool_address: Address,
    ) {
        bootstrapper.require_auth();
        bootstrap_management::execute_start_bootstrap(
            &e,
            bootstrapper,
            bootstrap_token_index,
            bootstrap_amount,
            pair_min,
            duration,
            pool_address,
        );
    }

    fn join(e: Env, from: Address, amount: i128, bootstrapper: Address, bootstrap_id: u32) {
        from.require_auth();
        bootstrap_management::execute_join(&e, &from, amount, bootstrapper, bootstrap_id)
    }

    fn exit(e: Env, from: Address, amount: i128, bootstrapper: Address, bootstrap_id: u32) {
        from.require_auth();
        bootstrap_management::execute_exit(&e, from, amount, bootstrapper, bootstrap_id);
    }

    fn close_bootstrap(e: Env, from: Address, bootstrapper: Address, bootstrap_id: u32) {
        from.require_auth();
        bootstrap_management::execute_close(&e, bootstrap_id, bootstrapper);
    }
    fn claim(e: Env, from: Address, bootstrapper: Address, bootstrap_id: u32) {
        from.require_auth();
        bootstrap_management::execute_claim(&e, &from, bootstrap_id, bootstrapper);
    }
}
