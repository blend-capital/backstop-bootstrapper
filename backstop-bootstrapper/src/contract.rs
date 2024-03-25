use crate::{
    backstop_bootstrapper::BackstopBootstrapper, bootstrap_management,
    errors::BackstopBootstrapperError, storage,
};
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env};

#[contract]
pub struct BackstopBootstrapperContract;

#[contractimpl]
impl BackstopBootstrapper for BackstopBootstrapperContract {
    fn initialize(e: Env, backstop: Address, backstop_token: Address) {
        if storage::get_is_init(&e) {
            panic_with_error!(&e, BackstopBootstrapperError::AlreadyInitializedError);
        }
        storage::set_is_init(&e);
        storage::set_backstop(&e, backstop);
        storage::set_backstop_token(&e, backstop_token);
    }

    fn add_bootstrap(
        e: Env,
        bootstrapper: Address,
        bootstrap_token: Address,
        pair_token: Address,
        bootstrap_amount: i128,
        pair_min: i128,
        duration: u32,
        bootstrap_weight: u64,
        pool_address: Address,
        bootstrap_token_index: u32,
        pair_token_index: u32,
    ) {
        bootstrapper.require_auth();
        bootstrap_management::execute_start_bootstrap(
            &e,
            bootstrapper,
            bootstrap_token,
            pair_token,
            bootstrap_amount,
            pair_min,
            duration,
            bootstrap_weight,
            pool_address,
            bootstrap_token_index,
            pair_token_index,
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

    fn close_bootstrap(e: Env, bootstrapper: Address, bootstrap_id: u32) {
        bootstrap_management::execute_close(&e, bootstrap_id, bootstrapper);
    }
    fn claim(e: Env, from: Address, bootstrapper: Address, bootstrap_id: u32) {
        from.require_auth();
        bootstrap_management::execute_claim(&e, &from, bootstrap_id, bootstrapper);
    }
}
