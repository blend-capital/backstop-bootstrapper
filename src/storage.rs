use soroban_sdk::{contracttype, unwrap::UnwrapOptimized, Address, Env, Symbol, Vec};

use crate::types::{BootstrapConfig, BootstrapData, DepositData, TokenInfo};

//********** Storage Keys **********//

const BACKSTOP_KEY: &str = "Bstop";
const POOL_FACTORY_KEY: &str = "PoolFact";
const BACKSTOP_TOKEN_KEY: &str = "BstopTkn";
const COMET_KEY: &str = "Comet";
const IS_INIT_KEY: &str = "IsInit";
const NEXT_ID_KEY: &str = "NextId";

#[derive(Clone)]
#[contracttype]
pub struct DepositKey {
    id: u32,
    user: Address,
}

#[derive(Clone)]
#[contracttype]
pub enum BootstrapKey {
    Config(u32),
    Data(u32),
    Claim(u32),
    Refund(u32),
    Deposit(DepositKey),
}

//********** Storage Utils **********//

pub const ONE_DAY_LEDGERS: u32 = 17280; // assumes 5 seconds per ledger on average

const LEDGER_BUMP_SHARED: u32 = 31 * ONE_DAY_LEDGERS;
const LEDGER_THRESHOLD_SHARED: u32 = LEDGER_BUMP_SHARED - ONE_DAY_LEDGERS;

const LEDGER_BUMP_USER: u32 = 120 * ONE_DAY_LEDGERS;
const LEDGER_THRESHOLD_USER: u32 = LEDGER_BUMP_USER - 20 * ONE_DAY_LEDGERS;

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

/// Get comet token data
pub fn get_comet_token_data(e: &Env) -> Vec<TokenInfo> {
    e.storage()
        .instance()
        .get::<Symbol, Vec<TokenInfo>>(&Symbol::new(e, COMET_KEY))
        .unwrap_optimized()
}

/// Set comet token data
pub fn set_comet_token_data(e: &Env, data: &Vec<TokenInfo>) {
    e.storage()
        .instance()
        .set::<Symbol, Vec<TokenInfo>>(&Symbol::new(e, COMET_KEY), &data);
}

/********** Persistent **********/

/// Get the next ID for a bootstrap
pub fn get_next_id(e: &Env) -> u32 {
    let key = Symbol::new(e, NEXT_ID_KEY);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    e.storage()
        .persistent()
        .get::<Symbol, u32>(&key)
        .unwrap_optimized()
}

/// Set the backstop address
pub fn set_next_id(e: &Env, next_id: u32) {
    let key = Symbol::new(e, NEXT_ID_KEY);
    e.storage().persistent().set::<Symbol, u32>(&key, &next_id);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
}

/// Get a bootstrap
pub fn get_bootstrap_config(e: &Env, id: u32) -> BootstrapConfig {
    let key = BootstrapKey::Config(id);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    e.storage()
        .persistent()
        .get::<BootstrapKey, BootstrapConfig>(&key)
        .unwrap_optimized()
}

/// Set the mapping of sequence to unlock percentage
pub fn set_bootstrap_config(e: &Env, id: u32, bootstrap_config: &BootstrapConfig) {
    let key = BootstrapKey::Config(id);
    e.storage()
        .persistent()
        .set::<BootstrapKey, BootstrapConfig>(&key, &bootstrap_config);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
}

/// Get the data for a bootstrap
pub fn get_bootstrap_data(e: &Env, id: u32) -> BootstrapData {
    let key = BootstrapKey::Data(id);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    e.storage()
        .persistent()
        .get::<BootstrapKey, BootstrapData>(&key)
        .unwrap_optimized()
}

/// Set the data for a bootsrap
pub fn set_bootstrap_data(e: &Env, id: u32, bootstrap_data: &BootstrapData) {
    let key = BootstrapKey::Data(id);
    e.storage()
        .persistent()
        .set::<BootstrapKey, BootstrapData>(&key, &bootstrap_data);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
}

/// Get a boostrap deposit for a user
pub fn get_deposit(e: &Env, id: u32, user: &Address) -> DepositData {
    let key = BootstrapKey::Deposit(DepositKey {
        id,
        user: user.clone(),
    });
    let result = e
        .storage()
        .persistent()
        .get::<BootstrapKey, DepositData>(&key);
    match result {
        Some(data) => {
            e.storage()
                .persistent()
                .extend_ttl(&key, LEDGER_THRESHOLD_USER, LEDGER_BUMP_USER);
            data
        }
        None => DepositData::default(),
    }
}

/// Set a boostrap deposit for a user
pub fn set_deposit(e: &Env, id: u32, user: &Address, data: DepositData) {
    let key = BootstrapKey::Deposit(DepositKey {
        id,
        user: user.clone(),
    });
    e.storage()
        .persistent()
        .set::<BootstrapKey, DepositData>(&key, &data);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_USER, LEDGER_BUMP_USER);
}

/// Get if the bootstrapper claimed their backstop token balance
pub fn get_claimed(e: &Env, id: u32) -> bool {
    let key = BootstrapKey::Claim(id);
    e.storage().persistent().has::<BootstrapKey>(&key)
}

/// Set if the bootstrapped claimed their backstop token balance
pub fn set_claimed(e: &Env, id: u32) {
    let key = BootstrapKey::Claim(id);
    e.storage()
        .persistent()
        .set::<BootstrapKey, bool>(&key, &true);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_USER, LEDGER_BUMP_USER);
}

/// Get if the bootstrapper was refunded their bootstrap token balance
pub fn get_refunded(e: &Env, id: u32) -> bool {
    let key = BootstrapKey::Refund(id);
    e.storage().persistent().has::<BootstrapKey>(&key)
}

/// Set if the bootstrapped refunded their bootstrap token balance
pub fn set_refunded(e: &Env, id: u32) {
    let key = BootstrapKey::Refund(id);
    e.storage()
        .persistent()
        .set::<BootstrapKey, bool>(&key, &true);
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_USER, LEDGER_BUMP_USER);
}
