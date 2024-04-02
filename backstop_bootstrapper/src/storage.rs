use soroban_sdk::{contracttype, unwrap::UnwrapOptimized, Address, Env, Symbol};

use crate::types::{BootstrapData, TokenInfo};

const BACKSTOP_KEY: &str = "Backstop";
const POOL_FACTORY_KEY: &str = "PoolFactory";
const BACKSTOP_TOKEN_KEY: &str = "Owner";
const IS_INIT_KEY: &str = "IsInit";

pub const ONE_DAY_LEDGERS: u32 = 17280; // assumes 5 seconds per ledger on average
const LEDGER_THRESHOLD_SHARED: u32 = 14 * ONE_DAY_LEDGERS;
pub const LEDGER_BUMP_SHARED: u32 = 15 * ONE_DAY_LEDGERS;

#[derive(Clone)]
#[contracttype]
pub struct BootstrapKey {
    pub id: u32,
    pub creator: Address,
}

#[derive(Clone)]
#[contracttype]
pub enum CometData {
    Token(u32),
}
//********** Storage Utils **********//

/// Bump the instance lifetime by the defined amount
pub fn extend_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
}

/********** Instance **********/

/// Check if the contract has been initialized
pub fn get_is_init(e: &Env) -> bool {
    e.storage().instance().has(&Symbol::new(e, IS_INIT_KEY))
}

/// Set the contract as initialized
pub fn set_is_init(e: &Env) {
    e.storage()
        .instance()
        .set::<Symbol, bool>(&Symbol::new(e, IS_INIT_KEY), &true);
}

/// Get the backstop address
pub fn get_backstop(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, BACKSTOP_KEY))
        .unwrap_optimized()
}

/// Set the backstop address
pub fn set_backstop(e: &Env, backstop: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, BACKSTOP_KEY), &backstop);
}

/// Get the pool factory address
pub fn get_pool_factory(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, POOL_FACTORY_KEY))
        .unwrap_optimized()
}

/// Set the backstop address
pub fn set_pool_factory(e: &Env, pool_factory: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, POOL_FACTORY_KEY), &pool_factory);
}

/// Get the backstop token address
pub fn get_backstop_token(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, BACKSTOP_TOKEN_KEY))
        .unwrap_optimized()
}

/// Set the comet address
pub fn set_backstop_token(e: &Env, backstop_token: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, BACKSTOP_TOKEN_KEY), &backstop_token);
}

// Set comet token data
pub fn set_comet_token_data(e: &Env, index: u32, data: TokenInfo) {
    let key = CometData::Token(index);
    e.storage()
        .instance()
        .set::<CometData, TokenInfo>(&key, &data);
}

// Get comet token data
pub fn get_comet_token_data(e: &Env, index: u32) -> Option<TokenInfo> {
    let key = CometData::Token(index);
    e.storage().instance().get::<CometData, TokenInfo>(&key)
}

/********** Persistent **********/

/// Get a bootstrap
pub fn get_bootstrap_data(e: &Env, creator: Address, id: u32) -> Option<BootstrapData> {
    let key = BootstrapKey { id, creator };
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    e.storage()
        .persistent()
        .get::<BootstrapKey, BootstrapData>(&key)
}

/// Set the mapping of sequence to unlock percentage
pub fn set_bootstrap_data(e: &Env, creator: Address, id: u32, bootstrap_data: &BootstrapData) {
    let key = BootstrapKey { id, creator };
    e.storage()
        .persistent()
        .set::<BootstrapKey, BootstrapData>(&key, &bootstrap_data);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
}

/// Remove a bootstrap
pub fn remove_bootstrap(e: &Env, creator: Address, id: u32) {
    let key = BootstrapKey { id, creator };
    e.storage().persistent().remove::<BootstrapKey>(&key);
}

pub fn bump_bootstrap_id(e: &Env, creator: Address) -> u32 {
    let id = match e.storage().persistent().get::<Address, u32>(&creator) {
        Some(id) => id + 1,
        None => 0,
    };
    e.storage().persistent().set::<Address, u32>(&creator, &id);
    e.storage()
        .persistent()
        .extend_ttl(&creator, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    id
}
