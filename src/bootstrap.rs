use soroban_sdk::{contracttype, Env};

use crate::{
    constants::MAX_DUST_AMOUNT,
    storage::{self, ONE_DAY_LEDGERS},
    types::{BootstrapConfig, BootstrapData, BootstrapStatus},
};

#[derive(Clone)]
#[contracttype]
pub struct Bootstrap {
    pub id: u32,
    pub status: BootstrapStatus,
    pub config: BootstrapConfig,
    pub data: BootstrapData,
}

impl Bootstrap {
    /// Load a bootstrap from storage
    ///
    /// ### Arguments
    /// * `id` - The id of the bootstrap
    pub fn load(e: &Env, id: u32) -> Self {
        let config = storage::get_bootstrap_config(e, id);
        let data = storage::get_bootstrap_data(e, id);
        let status: BootstrapStatus;
        if e.ledger().sequence() < config.close_ledger {
            status = BootstrapStatus::Active;
        } else if data.total_pair < config.pair_min {
            status = BootstrapStatus::Cancelled;
        } else if data.pair_amount <= MAX_DUST_AMOUNT
            && data.bootstrap_amount <= MAX_DUST_AMOUNT
            && data.total_backstop_tokens >= MAX_DUST_AMOUNT
        {
            status = BootstrapStatus::Completed;
        } else if config.close_ledger + 14 * ONE_DAY_LEDGERS < e.ledger().sequence() {
            status = BootstrapStatus::Cancelled;
        } else {
            status = BootstrapStatus::Closing;
        }
        Bootstrap {
            id,
            status,
            config,
            data,
        }
    }

    /// Store the bootstrap data to storage
    pub fn store(&self, e: &Env) {
        storage::set_bootstrap_data(e, self.id, &self.data);
    }

    /// Join the bootstrap
    ///
    /// ### Arguments
    /// * `amount` - The amount of the pair token to join with
    pub fn join(&mut self, amount: i128) {
        self.data.pair_amount += amount;
        self.data.total_pair += amount;
    }

    /// Exit the bootstrap
    ///
    /// ### Arguments
    /// * `amount` - The amount of the pair token to exit with
    pub fn exit(&mut self, amount: i128) {
        self.data.pair_amount -= amount;
        self.data.total_pair -= amount;
    }

    /// Spend bootstrap and pair tokens to mint backstop tokens
    ///
    /// ### Arguments
    /// * `bootstrap_amount` - The amount of the bootstrap token to spend
    /// * `pair_amount` - The amount of the pair token to spend
    /// * `backstop_tokens` - The amount of backstop tokens to mint
    pub fn convert(&mut self, bootstrap_amount: i128, pair_amount: i128, backstop_tokens: i128) {
        if bootstrap_amount > 0 {
            self.data.bootstrap_amount -= bootstrap_amount;
        }
        if pair_amount > 0 {
            self.data.pair_amount -= pair_amount;
        }
        self.data.total_backstop_tokens += backstop_tokens;
    }
}
