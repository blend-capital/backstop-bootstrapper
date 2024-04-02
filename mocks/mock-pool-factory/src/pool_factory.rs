use crate::{
    storage::{self, PoolInitMeta},
    PoolFactoryError,
};
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, BytesN, Env, Symbol};

#[contract]
pub struct MockPoolFactory;

pub trait MockPoolFactoryTrait {
    /// Setup the pool factory
    ///
    /// ### Arguments
    /// * `pool_init_meta` - The pool initialization metadata
    fn initialize(e: Env, pool_init_meta: PoolInitMeta);

    /// Deploys and initializes a lending pool
    ///
    /// # Arguments
    /// * `admin` - The admin address for the pool
    /// * `name` - The name of the pool
    /// * `oracle` - The oracle address for the pool
    /// * `backstop_take_rate` - The backstop take rate for the pool (7 decimals)
    fn deploy(
        e: Env,
        admin: Address,
        name: Symbol,
        salt: BytesN<32>,
        oracle: Address,
        backstop_take_rate: u32,
        max_positions: u32,
    ) -> Address;

    /// Checks if contract address was deployed by the factory
    ///
    /// Returns true if pool was deployed by factory and false otherwise
    ///
    /// # Arguments
    /// * 'pool_address' - The contract address to be checked
    fn is_pool(e: Env, pool_address: Address) -> bool;

    /// Mock Only: Set a pool_address as having been deployed by the pool factory
    ///
    /// ### Arguments
    /// * `pool_address` - The pool address to set
    fn set_pool(e: Env, pool_address: Address);
}

#[contractimpl]
impl MockPoolFactoryTrait for MockPoolFactory {
    fn initialize(e: Env, pool_init_meta: PoolInitMeta) {
        if storage::has_pool_init_meta(&e) {
            panic_with_error!(&e, PoolFactoryError::AlreadyInitialized);
        }
        storage::set_pool_init_meta(&e, &pool_init_meta);
    }

    fn deploy(
        _e: Env,
        _admin: Address,
        _name: Symbol,
        _salt: BytesN<32>,
        _oracle: Address,
        _backstop_take_rate: u32,
        _max_positions: u32,
    ) -> Address {
        panic!("Not implemented")
    }

    fn is_pool(e: Env, pool_address: Address) -> bool {
        storage::is_deployed(&e, &pool_address)
    }

    fn set_pool(e: Env, pool_address: Address) {
        storage::set_deployed(&e, &pool_address);
    }
}
