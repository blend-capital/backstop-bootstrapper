use crate::{
    bootstrap::Bootstrap,
    comet_utils,
    constants::{MAX_DUST_AMOUNT, SCALAR_7},
    dependencies::comet::Client as CometClient,
    errors::BackstopBootstrapperError,
    storage,
    types::{BootstrapConfig, BootstrapData, BootstrapStatus, TokenInfo},
};

use blend_contract_sdk::{backstop, pool_factory};
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{
    assert_with_error,
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contractimpl, panic_with_error,
    token::TokenClient,
    unwrap::UnwrapOptimized,
    vec, Address, Env, IntoVal, Symbol, Vec,
};

#[contract]
pub struct BackstopBootstrapper;

#[contractimpl]
impl BackstopBootstrapper {
    /// Initialize the contract
    ///
    /// ### Arguments
    /// * `backstop` - The backstop address
    /// * `backstop_token` - The backstop token address
    /// * `pool_factory_address` - The pool factory address
    ///
    /// ### Panics
    /// * `AlreadyInitializedError` - If the contract has already been initialized
    pub fn initialize(
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
        let mut token_data: Vec<TokenInfo> = Vec::new(&e);
        for address in tokens.iter() {
            let weight = backstop_token.get_normalized_weight(&address);
            token_data.push_back(TokenInfo { address, weight });
        }
        storage::set_comet_token_data(&e, &token_data);
        storage::set_next_id(&e, 0);
    }

    //********** Read-Only ***********//

    /// Fetch data for a bootstrap
    ///
    /// ### Arguments
    /// * `id` - The id of the bootstrap
    pub fn get_bootstrap(e: Env, id: u32) -> Bootstrap {
        Bootstrap::load(&e, id)
    }

    /// Fetch the next bootstrap's ID. The previous (and most recently created) bootsrap's ID will
    /// be this value decremented by 1.
    pub fn get_next_id(e: Env) -> u32 {
        storage::get_next_id(&e)
    }

    /// Fetch a deposit for a user in a bootstrap
    ///
    /// ### Arguments
    /// * `id` - The id of the bootstrap
    /// * `user` - The address of the user
    pub fn get_deposit(e: Env, id: u32, user: Address) -> i128 {
        storage::get_deposit(&e, id, &user)
    }

    //********** Read-Write ***********//

    /// Add a new bootstrap
    ///
    /// Returns the ID of the bootstrap
    ///
    /// ### Arguments
    /// * `config` - The configuration for the bootstrap
    pub fn bootstrap(e: Env, config: BootstrapConfig) -> u32 {
        config.bootstrapper.require_auth();
        assert_with_error!(
            e,
            config.token_index == 0 || config.token_index == 1,
            BackstopBootstrapperError::InvalidBootstrapToken
        );
        assert_with_error!(
            e,
            config.amount > 0,
            BackstopBootstrapperError::InvalidBootstrapAmount
        );
        assert_with_error!(
            e,
            config.pair_min >= 0,
            BackstopBootstrapperError::NegativeAmountError
        );
        let duration = config.close_ledger.saturating_sub(e.ledger().sequence());
        assert_with_error!(
            e,
            duration >= storage::ONE_DAY_LEDGERS && duration <= 14 * storage::ONE_DAY_LEDGERS,
            BackstopBootstrapperError::InvalidCloseLedger
        );
        assert_with_error!(
            e,
            pool_factory::Client::new(&e, &storage::get_pool_factory(&e)).is_pool(&config.pool),
            BackstopBootstrapperError::InvalidPoolAddressError
        );

        // transfer the bootstrapped tokens into the contract and create the bootstrap
        let id = storage::get_next_id(&e);
        let token_info = storage::get_comet_token_data(&e).get_unchecked(config.token_index);
        TokenClient::new(&e, &token_info.address).transfer(
            &config.bootstrapper,
            &e.current_contract_address(),
            &config.amount,
        );
        storage::set_bootstrap_config(&e, id, &config);
        storage::set_bootstrap_data(
            &e,
            id,
            &BootstrapData {
                bootstrap_amount: config.amount,
                pair_amount: 0,
                total_backstop_tokens: 0,
                total_pair: 0,
            },
        );
        storage::set_next_id(&e, id + 1);

        e.events().publish(
            (Symbol::new(&e, "bootstrap"), config.bootstrapper, id),
            (config.token_index, config.amount, config.close_ledger),
        );
        id
    }

    /// Join a bootstrap by depositing a given amount of pair tokens
    ///
    /// Returns the total amount of pair tokens deposited by `from` in this bootstrap
    ///
    /// ### Arguments
    /// * `from` - The address of the user joining the bootstrap
    /// * `id` - The bootstrap id to join
    /// * `amount` - The amount of tokens to join with
    pub fn join(e: Env, from: Address, id: u32, amount: i128) -> i128 {
        from.require_auth();
        let mut bootstrap = Bootstrap::load(&e, id);
        assert_with_error!(
            e,
            bootstrap.status == BootstrapStatus::Active,
            BackstopBootstrapperError::InvalidBootstrapStatus
        );

        let pair_token =
            storage::get_comet_token_data(&e).get_unchecked(bootstrap.config.token_index ^ 1);
        TokenClient::new(&e, &pair_token.address).transfer(
            &from,
            &e.current_contract_address(),
            &amount,
        );

        bootstrap.join(amount);
        bootstrap.store(&e);
        let mut deposit = storage::get_deposit(&e, id, &from);
        deposit += amount;
        storage::set_deposit(&e, id, &from, deposit);
        deposit
    }

    /// Exits a bootstrap by withdrawing a given amount of pair tokens
    ///
    /// Returns the remaining amount of pair tokens deposited by `from` in this bootstrap
    ///
    /// ### Arguments
    /// * `from` - The address of the user joining the bootstrap
    /// * `id` - The bootstrap id to join
    /// * `amount` - The amount of tokens to join with
    pub fn exit(e: Env, from: Address, id: u32, amount: i128) -> i128 {
        from.require_auth();
        assert_with_error!(
            e,
            amount >= 0,
            BackstopBootstrapperError::NegativeAmountError
        );
        let mut bootstrap = Bootstrap::load(&e, id);
        assert_with_error!(
            e,
            bootstrap.status == BootstrapStatus::Active,
            BackstopBootstrapperError::InvalidBootstrapStatus
        );

        let pair_token =
            storage::get_comet_token_data(&e).get_unchecked(bootstrap.config.token_index ^ 1);
        let mut deposit = storage::get_deposit(&e, id, &from);
        deposit -= amount;
        bootstrap.exit(amount);
        assert_with_error!(
            e,
            deposit >= 0 && bootstrap.data.pair_amount >= 0 && bootstrap.data.total_pair >= 0,
            BackstopBootstrapperError::InsufficientDepositError
        );
        TokenClient::new(&e, &pair_token.address).transfer(
            &e.current_contract_address(),
            &from,
            &amount,
        );
        bootstrap.store(&e);
        storage::set_deposit(&e, id, &from, deposit);
        deposit
    }

    /// Close the bootstrap by depositing bootstrapping tokens into the comet
    ///
    /// ### Arguments
    /// * `id` - The id of the bootstrap
    pub fn close(e: Env, id: u32) -> i128 {
        let mut bootstrap = Bootstrap::load(&e, id);
        assert_with_error!(
            e,
            bootstrap.status == BootstrapStatus::Closing,
            BackstopBootstrapperError::InvalidBootstrapStatus
        );

        let comet_client = CometClient::new(&e, &storage::get_backstop_token(&e));
        let comet_tokens = storage::get_comet_token_data(&e);
        let bootstrap_info = comet_tokens.get_unchecked(bootstrap.config.token_index);
        let pair_info = comet_tokens.get_unchecked(bootstrap.config.token_index ^ 1);
        let bootstrap_token_client = TokenClient::new(&e, &bootstrap_info.address);
        let pair_token_client = TokenClient::new(&e, &pair_info.address);

        // get contract starting balances
        let bootstrap_token_balance = bootstrap_token_client.balance(&e.current_contract_address());
        let pair_token_balance = pair_token_client.balance(&e.current_contract_address());

        // Get Comet LP token underlying value
        let total_comet_shares = comet_client.get_total_supply();
        let mut comet_bootstrap_token = bootstrap_token_client.balance(&comet_client.address);
        let mut comet_pair_token = pair_token_client.balance(&comet_client.address);

        if bootstrap.data.bootstrap_amount > MAX_DUST_AMOUNT
            && bootstrap.data.pair_amount > MAX_DUST_AMOUNT
        {
            let (dep_bootstrap, dep_pair, minted_backstop) = comet_utils::join_pool(
                &e,
                &comet_client,
                &comet_tokens,
                &bootstrap,
                bootstrap_token_balance,
                pair_token_balance,
                comet_bootstrap_token,
                comet_pair_token,
                total_comet_shares,
            );
            bootstrap.convert(dep_bootstrap, dep_pair, minted_backstop);
            comet_bootstrap_token += dep_bootstrap;
            comet_pair_token += dep_pair;
        }

        // handle single sided bootstrap token deposit
        if bootstrap.data.bootstrap_amount > MAX_DUST_AMOUNT {
            let (dep_bootstrap, minted_backstop) = comet_utils::single_sided_join(
                &e,
                &comet_client,
                &bootstrap_info.address,
                bootstrap.data.bootstrap_amount,
                comet_bootstrap_token,
            );
            bootstrap.convert(dep_bootstrap, 0, minted_backstop);
        }

        if bootstrap.data.pair_amount > 0 {
            let (dep_pair, minted_backstop) = comet_utils::single_sided_join(
                &e,
                &comet_client,
                &pair_info.address,
                bootstrap.data.pair_amount,
                comet_pair_token,
            );
            bootstrap.convert(0, dep_pair, minted_backstop);
        }

        assert_with_error!(
            e,
            bootstrap.data.total_backstop_tokens > 0,
            BackstopBootstrapperError::ReceivedNoBackstopTokens
        );
        bootstrap.store(&e);
        e.events().publish(
            (Symbol::new(&e, "bootstrap_close"), bootstrap.id),
            bootstrap.data.total_backstop_tokens,
        );
        bootstrap.data.total_backstop_tokens
    }

    /// Claim and deposit pool tokens into backstop
    ///
    /// Returns the amount of backstop shares minted
    ///
    /// ### Arguments
    /// * `from` - The address of the user claiming their bootstrap proceeds
    /// * `id` - The address of the bootstrap initiator
    pub fn claim(e: Env, from: Address, id: u32) -> i128 {
        from.require_auth();
        let bootstrap = Bootstrap::load(&e, id);
        assert_with_error!(
            e,
            bootstrap.status == BootstrapStatus::Completed,
            BackstopBootstrapperError::InvalidBootstrapStatus
        );
        let backstop_address = storage::get_backstop(&e);
        let backstop_token_address = storage::get_backstop_token(&e);
        let backstop_client = backstop::Client::new(&e, &backstop_address);
        let backstop_token_client = CometClient::new(&e, &backstop_token_address);
        let backstop_tokens: i128;
        if bootstrap.config.bootstrapper == from {
            assert_with_error!(
                e,
                !storage::get_claimed(&e, bootstrap.id),
                BackstopBootstrapperError::AlreadyClaimedError
            );
            let bootstrap_info =
                storage::get_comet_token_data(&e).get_unchecked(bootstrap.config.token_index);
            backstop_tokens = bootstrap
                .data
                .total_backstop_tokens
                .fixed_mul_floor(bootstrap_info.weight as i128, SCALAR_7)
                .unwrap_optimized();
        } else {
            let deposit_amount = storage::get_deposit(&e, bootstrap.id, &from);
            assert_with_error!(
                e,
                deposit_amount > 0,
                BackstopBootstrapperError::AlreadyClaimedError
            );
            storage::set_deposit(&e, bootstrap.id, &from, 0);
            let pair_info =
                storage::get_comet_token_data(&e).get_unchecked(bootstrap.config.token_index ^ 1);
            backstop_tokens = deposit_amount
                .fixed_div_floor(bootstrap.data.total_pair, SCALAR_7)
                .unwrap_optimized()
                .fixed_mul_floor(bootstrap.data.total_backstop_tokens, SCALAR_7)
                .unwrap_optimized()
                .fixed_mul_floor(pair_info.weight as i128, SCALAR_7)
                .unwrap_optimized();
        };
        backstop_token_client.transfer(&e.current_contract_address(), &from, &backstop_tokens);
        e.authorize_as_current_contract(vec![
            &e,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: backstop_token_address,
                    fn_name: Symbol::new(&e, "transfer"),
                    args: vec![
                        &e,
                        from.into_val(&e),
                        backstop_address.into_val(&e),
                        backstop_tokens.into_val(&e),
                    ],
                },
                sub_invocations: Vec::new(&e),
            }),
        ]);
        backstop_client.deposit(&from, &bootstrap.config.pool, &backstop_tokens)
    }

    /// Refund funds from a cancelled bootstrap
    ///
    /// Returns the amount of funds returned
    ///
    /// ### Arguments
    /// * `from` - The address of the user claiming their bootstrap proceeds
    /// * `id` - The address of the bootstrap initiator
    pub fn refund(e: Env, from: Address, id: u32) -> i128 {
        from.require_auth();
        let mut bootstrap = Bootstrap::load(&e, id);
        assert_with_error!(
            e,
            bootstrap.status == BootstrapStatus::Cancelled,
            BackstopBootstrapperError::InvalidBootstrapStatus
        );
        let amount_refunded: i128;
        if bootstrap.config.bootstrapper == from {
            assert_with_error!(
                e,
                bootstrap.data.bootstrap_amount > 0,
                BackstopBootstrapperError::AlreadyClaimedError
            );
            let bootstrap_info =
                storage::get_comet_token_data(&e).get_unchecked(bootstrap.config.token_index);
            amount_refunded = bootstrap.data.bootstrap_amount;
            bootstrap.data.bootstrap_amount = 0;
            TokenClient::new(&e, &bootstrap_info.address).transfer(
                &e.current_contract_address(),
                &from,
                &amount_refunded,
            );
        } else {
            let deposit_amount = storage::get_deposit(&e, bootstrap.id, &from);
            assert_with_error!(
                e,
                deposit_amount > 0,
                BackstopBootstrapperError::AlreadyClaimedError
            );
            storage::set_deposit(&e, bootstrap.id, &from, 0);

            amount_refunded = deposit_amount;
            bootstrap.data.pair_amount -= amount_refunded;
            let pair_info =
                storage::get_comet_token_data(&e).get_unchecked(bootstrap.config.token_index ^ 1);
            TokenClient::new(&e, &pair_info.address).transfer(
                &e.current_contract_address(),
                &from,
                &amount_refunded,
            );
        }
        bootstrap.store(&e);
        amount_refunded
    }
}
