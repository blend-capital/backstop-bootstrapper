use soroban_sdk::{contracttype, Address, Env, IntoVal, Symbol, TryFromVal, Val};

use crate::types::Bootstrap;

const BACKSTOP_KEY: &str = "Backstop";
const BACKSTOP_TOKEN_KEY: &str = "Owner";
const IS_INIT_KEY: &str = "IsInit";

pub const ONE_DAY_LEDGERS: u32 = 17280; // assumes 5 seconds per ledger on average
const LEDGER_THRESHOLD_SHARED: u32 = 14 * ONE_DAY_LEDGERS;
pub const LEDGER_BUMP_SHARED: u32 = 15 * ONE_DAY_LEDGERS;

#[derive(Clone)]
#[contracttype]
pub struct BootstrapKey {
    id: u32,
    creator: Address,
}
//********** Storage Utils **********//

/// Bump the instance lifetime by the defined amount
pub fn extend_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
}

/// Fetch an entry in persistent storage that has a default value if it doesn't exist
fn get_persistent_default<K: IntoVal<Env, Val>, V: TryFromVal<Env, Val>>(
    e: &Env,
    key: &K,
    default: V,
    bump_threshold: u32,
    bump_amount: u32,
) -> V {
    if let Some(result) = e.storage().persistent().get::<K, V>(key) {
        e.storage()
            .persistent()
            .extend_ttl(key, bump_threshold, bump_amount);
        result
    } else {
        default
    }
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
        .unwrap()
}

/// Set the backstop address
pub fn set_backstop(e: &Env, admin: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, BACKSTOP_KEY), &admin);
}

/// Get the backstop token address
pub fn get_backstop_token(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<Symbol, Address>(&Symbol::new(e, BACKSTOP_TOKEN_KEY))
        .unwrap()
}

/// Set the comet address
pub fn set_backstop_token(e: &Env, admin: Address) {
    e.storage()
        .instance()
        .set::<Symbol, Address>(&Symbol::new(e, BACKSTOP_TOKEN_KEY), &admin);
}

/********** Persistent **********/

/// Get a bootstrap
pub fn get_bootstrap(e: &Env, creator: Address, id: u32) -> Option<Bootstrap> {
    let key = BootstrapKey { id, creator };
    e.storage()
        .persistent()
        .extend_ttl(&key, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    e.storage()
        .persistent()
        .get::<BootstrapKey, Bootstrap>(&key)
}

/// Set the mapping of sequence to unlock percentage
pub fn set_bootstrap(e: &Env, creator: Address, id: u32, bootstrap: &Bootstrap) {
    let key = BootstrapKey { id, creator };
    e.storage()
        .persistent()
        .set::<BootstrapKey, Bootstrap>(&key, &bootstrap);
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
    let id =
        get_persistent_default(e, &creator, 0, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED) + 1;
    e.storage().persistent().set::<Address, u32>(&creator, &id);
    e.storage()
        .persistent()
        .extend_ttl(&creator, LEDGER_THRESHOLD_SHARED, LEDGER_BUMP_SHARED);
    id
}
