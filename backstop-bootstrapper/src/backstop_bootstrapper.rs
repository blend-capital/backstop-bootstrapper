use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "BackstopBootstrapperContract")]
pub trait BackstopBootstrapper {
    /// Initialize the contract with the admin and owner addresses
    ///
    /// # Arguments
    /// * `backstop` - The backstop address
    /// * `backstop_token` - The backstop token address
    fn initialize(e: Env, backstop: Address, backstop_token: Address);

    /// Add a new unlock time and percentage unlocked if unlock already exists the percentage is updated
    ///
    /// # Arguments
    /// * `bootstrapper` - The address of the bootstrap initiator
    /// * `bootstrap_token` - The address of the token that needs pairing
    /// * `pair_token` - The address of the token to pair with
    /// * `bootstrap_amount` - The bootstrap token amount
    /// * `pair_min` - The minimum amount of pair token to add
    /// * `duration` - The duration of the bootstrap in blocks
    /// * `bootstrap_weight` - The weight of the bootstrap
    /// * `pool_address` - The address of the pool whose backstop is being funded
    /// * `index` - The index of the bootstrap event
    fn add_bootstrap(
        e: Env,
        boostrapper: Address,
        bootstrap_token: Address,
        pair_token: Address,
        bootstrap_amount: i128,
        pair_min: i128,
        duration: u32,
        bootstrap_weight: u64,
        pool_address: Address,
        bootstrap_token_index: u32,
        pair_token_index: u32,
    );

    /// Join a Bootstrap Event with a given amount of pair tokens
    ///
    /// # Arguments
    /// * `from` - The address of the user joining the bootstrap
    /// * `amount` - The amount of tokens to join with
    fn join(e: Env, from: Address, amount: i128, bootstrapper: Address, bootstrap_id: u32);

    /// Exits a Bootstrap Event with a given amount of pair tokens
    ///
    /// # Arguments
    /// * `from` - The address of the user Exiting the bootstrap
    /// * `amount` - The amount of tokens to exit with
    fn exit(e: Env, from: Address, amount: i128, bootstrapper: Address, bootstrap_id: u32);

    /// Close the bootstrap event
    ///
    fn close_bootstrap(e: Env, bootstrapper: Address, bootstrap_id: u32);

    /// Claim and deposit pool tokens into backstop
    ///
    fn claim(e: Env, from: Address, boostrapper: Address, bootstrap_id: u32);
}
