use soroban_sdk::{contractclient, Address, Env};

use crate::types::Bootstrap;

#[contractclient(name = "BackstopBootstrapperContract")]
pub trait BackstopBootstrapper {
    /// Initialize the contract with the admin and owner addresses
    ///
    /// # Arguments
    /// * `backstop` - The backstop address
    /// * `backstop_token` - The backstop token address
    /// * `pool_factory_address` - The pool factory address
    fn initialize(
        e: Env,
        backstop: Address,
        backstop_token: Address,
        pool_factory_address: Address,
    );

    /// Add a new unlock time and percentage unlocked if unlock already exists the percentage is updated
    ///
    /// # Arguments
    /// * `bootstrapper` - The address of the bootstrap initiator
    /// * `bootstrap_token` - 0 for BLND, 1 for USDC
    /// * `bootstrap_amount` - The bootstrap token amount
    /// * `pair_min` - The minimum amount of pair token to add
    /// * `duration` - The duration of the bootstrap in blocks
    /// * `pool_address` - The address of the pool whose backstop is being funded
    fn add_bootstrap(
        e: Env,
        boostrapper: Address,
        bootstrap_token_index: u32,
        bootstrap_amount: i128,
        pair_min: i128,
        duration: u32,
        pool_address: Address,
    );

    /// Join a Bootstrap Event with a given amount of pair tokens
    ///
    /// # Arguments
    /// * `from` - The address of the user joining the bootstrap
    /// * `amount` - The amount of tokens to join with
    /// * `bootstrapper` - The address of the bootstrap initiator
    /// * `bootstrap_id` - The id of the bootstrap event
    fn join(e: Env, from: Address, amount: i128, bootstrapper: Address, bootstrap_id: u32);

    /// Exits a Bootstrap Event with a given amount of pair tokens
    ///
    /// # Arguments
    /// * `from` - The address of the user Exiting the bootstrap
    /// * `amount` - The amount of tokens to exit with
    /// * `bootstrapper` - The address of the bootstrap initiator
    /// * `bootstrap_id` - The id of the bootstrap event
    fn exit(e: Env, from: Address, amount: i128, bootstrapper: Address, bootstrap_id: u32);

    /// Close the bootstrap event
    ///
    /// # Arguments
    /// * `from` - The address of the user closing the bootstrap
    /// * `bootstrapper` - The address of the bootstrap initiator
    /// * `bootstrap_id` - The id of the bootstrap event
    fn close_bootstrap(e: Env, from: Address, bootstrapper: Address, bootstrap_id: u32);

    /// Claim and deposit pool tokens into backstop
    ///
    /// # Arguments
    /// * `from` - The address of the user claiming their bootstrap proceeds
    /// * `bootstrapper` - The address of the bootstrap initiator
    /// * `bootstrap_id` - The id of the bootstrap event
    fn claim(e: Env, from: Address, boostrapper: Address, bootstrap_id: u32);

    /// Return bootstrap data
    ///
    /// # Arguments
    /// * `bootstrap_id` - The id of the bootstrap event
    /// * `bootstrapper` - The address of the bootstrap initiator
    fn get_bootstrap(e: Env, bootstrap_id: u32, bootstrapper: Address) -> Bootstrap;
}
