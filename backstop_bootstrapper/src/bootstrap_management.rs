use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{
    assert_with_error,
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    panic_with_error,
    token::TokenClient,
    unwrap::UnwrapOptimized,
    vec, Address, Env, IntoVal, Symbol, Vec,
};

use crate::{
    constants::{MAX_IN_RATIO, SCALAR_7},
    dependencies::{BackstopClient, CometClient, PoolFactoryClient},
    errors::BackstopBootstrapperError,
    storage,
    types::{Bootstrap, BootstrapStatus},
};

pub fn execute_start_bootstrap(
    e: &Env,
    bootstrapper: Address,
    bootstrap_token_index: u32,
    bootstrap_amount: i128,
    pair_min: i128,
    duration: u32,
    pool_address: Address,
) -> Bootstrap {
    assert_with_error!(
        e,
        bootstrap_amount > 0,
        BackstopBootstrapperError::InvalidBootstrapAmount
    );
    assert_with_error!(
        e,
        pair_min >= 0,
        BackstopBootstrapperError::NegativeAmountError
    );
    assert_with_error!(
        e,
        duration >= 1,
        BackstopBootstrapperError::DurationTooShort
    );
    assert_with_error!(
        e,
        duration < storage::LEDGER_BUMP_SHARED - storage::ONE_DAY_LEDGERS,
        BackstopBootstrapperError::DurationTooLong
    );
    assert_with_error!(
        e,
        PoolFactoryClient::new(&e, &storage::get_pool_factory(e)).is_pool(&pool_address),
        BackstopBootstrapperError::InvalidPoolAddressError
    );
    let bootstrap = Bootstrap::new(
        e,
        bootstrapper.clone(),
        bootstrap_amount,
        pair_min,
        duration,
        pool_address,
        bootstrap_token_index,
    );
    // transfer the bootstrap token to the contract
    TokenClient::new(&e, &bootstrap.bootstrap_token_address).transfer(
        &bootstrapper,
        &e.current_contract_address(),
        &bootstrap_amount,
    );
    bootstrap.store(e, storage::bump_bootstrap_id(e, bootstrapper));
    bootstrap
}

pub fn execute_join(
    e: &Env,
    from: &Address,
    amount: i128,
    bootstrapper: Address,
    bootstrap_id: u32,
) {
    let mut bootstrap = Bootstrap::load(e, bootstrapper, bootstrap_id);
    // joins are disables if the bootstrap has ended
    assert_with_error!(
        e,
        e.ledger().sequence() < bootstrap.close_ledger,
        BackstopBootstrapperError::BootstrapNotActiveError
    );
    // deposit the pair token into the contract
    TokenClient::new(&e, &bootstrap.pair_token_address).transfer(
        &from,
        &e.current_contract_address(),
        &amount,
    );

    // update bootstrap deposits
    bootstrap.total_deposits += amount;
    let current_deposit = bootstrap.deposits.get(from.clone()).unwrap_or_else(|| 0);
    bootstrap
        .deposits
        .set(from.clone(), current_deposit + amount);
    bootstrap.store(e, bootstrap_id);
}

pub fn execute_exit(
    e: &Env,
    from: Address,
    amount: i128,
    bootstrapper: Address,
    bootstrap_id: u32,
) {
    let mut bootstrap = Bootstrap::load(e, bootstrapper, bootstrap_id);
    // exits are disables if the bootstrap has ended
    assert_with_error!(
        e,
        e.ledger().sequence() < bootstrap.close_ledger,
        BackstopBootstrapperError::BootstrapNotActiveError
    );
    // update bootstrap deposits
    let current_deposit = bootstrap.deposits.get(from.clone()).unwrap_or_else(|| 0);
    assert_with_error!(
        e,
        current_deposit >= amount && bootstrap.total_deposits >= amount,
        BackstopBootstrapperError::InsufficientDepositError
    );
    bootstrap.total_deposits -= amount;
    bootstrap
        .deposits
        .set(from.clone(), current_deposit - amount);

    // transfer the pair token back to the user
    TokenClient::new(&e, &bootstrap.pair_token_address).transfer(
        &e.current_contract_address(),
        &from,
        &amount,
    );

    bootstrap.store(e, bootstrap_id);
}

pub fn execute_close(e: &Env, bootstrap_id: u32, bootstrapper: Address) -> i128 {
    let mut bootstrap = Bootstrap::load(e, bootstrapper, bootstrap_id);

    assert_with_error!(
        e,
        bootstrap.status as u32 == BootstrapStatus::Active as u32,
        BackstopBootstrapperError::BootstrapNotActiveError
    );
    assert_with_error!(
        e,
        e.ledger().sequence() >= bootstrap.close_ledger,
        BackstopBootstrapperError::BootstrapNotCompleteError
    );
    if bootstrap.pair_to_deposit.is_none() {
        bootstrap.pair_to_deposit = Some(bootstrap.total_deposits);
        bootstrap.bootstrap_to_deposit = Some(bootstrap.bootstrap_amount);
    }
    // bootstrap must reach the min pair token amount and be closed within a day
    if bootstrap.total_deposits < bootstrap.pair_min
        || e.ledger().sequence() > bootstrap.close_ledger + storage::ONE_DAY_LEDGERS
    {
        bootstrap.status = BootstrapStatus::Cancelled as u32;
        bootstrap.store(e, bootstrap_id);
        return 0;
    }
    let comet_client = CometClient::new(&e, &storage::get_backstop_token(e));
    let bootstrap_token_client = TokenClient::new(&e, &bootstrap.bootstrap_token_address);
    let bootstrap_token_balance = bootstrap_token_client.balance(&e.current_contract_address());
    let pair_token_client = TokenClient::new(&e, &bootstrap.pair_token_address);
    let pair_token_balance = pair_token_client.balance(&e.current_contract_address());
    let mut amounts_in = Vec::new(&e);
    amounts_in.insert(
        bootstrap.bootstrap_token_index.clone(),
        bootstrap.bootstrap_to_deposit.unwrap_optimized().clone(),
    );
    amounts_in.insert(
        bootstrap.pair_token_index.clone(),
        bootstrap.pair_to_deposit.unwrap_optimized().clone(),
    );
    // Get Comet LP token underlying value
    let total_comet_shares = comet_client.get_total_supply();
    let mut comet_bootstrap_token = bootstrap_token_client.balance(&comet_client.address);
    let mut comet_pair_token = pair_token_client.balance(&comet_client.address);

    // underlying per LP token
    let expected_tokens = bootstrap
        .bootstrap_amount
        .fixed_div_floor(comet_bootstrap_token, SCALAR_7)
        .unwrap_optimized()
        .fixed_mul_floor(total_comet_shares, SCALAR_7)
        .unwrap_optimized()
        .min(
            bootstrap
                .total_deposits
                .fixed_div_floor(comet_pair_token, SCALAR_7)
                .unwrap_optimized()
                .fixed_mul_floor(total_comet_shares, SCALAR_7)
                .unwrap_optimized(),
        )
        .fixed_mul_floor(999_0000, SCALAR_7) // we want to leave a little bit of room for rounding
        .unwrap_optimized();

    // handle join_pool
    let approval_ledger = (e.ledger().sequence() / 100000 + 1) * 100000;
    if expected_tokens > 0
        && bootstrap.pair_to_deposit.unwrap_optimized() > 0
        && bootstrap.bootstrap_to_deposit.unwrap_optimized() > 0
    {
        let mut auths = vec![&e];
        for index in 0..amounts_in.len() {
            let amount = amounts_in.get(index).unwrap_optimized();
            let token_address = if index == bootstrap.bootstrap_token_index {
                bootstrap.bootstrap_token_address.clone()
            } else {
                bootstrap.pair_token_address.clone()
            };
            auths.push_back(InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: token_address,
                    fn_name: Symbol::new(&e, "approve"),
                    args: vec![
                        &e,
                        e.current_contract_address().into_val(e),
                        storage::get_backstop_token(&e).into_val(e),
                        amount.into_val(e),
                        approval_ledger.into_val(e),
                    ],
                },
                sub_invocations: vec![&e],
            }));
        }
        e.authorize_as_current_contract(auths);
        comet_client.join_pool(&expected_tokens, &amounts_in, &e.current_contract_address());

        bootstrap.backstop_tokens += expected_tokens;

        let deposited_bootstrap_tokens =
            bootstrap_token_balance - bootstrap_token_client.balance(&e.current_contract_address());
        bootstrap.bootstrap_to_deposit =
            Some(bootstrap.bootstrap_to_deposit.unwrap_optimized() - deposited_bootstrap_tokens);
        comet_bootstrap_token += deposited_bootstrap_tokens;

        let deposited_pair_tokens =
            pair_token_balance - pair_token_client.balance(&e.current_contract_address());
        bootstrap.pair_to_deposit =
            Some(bootstrap.pair_to_deposit.unwrap_optimized() - deposited_pair_tokens);
        comet_pair_token += deposited_pair_tokens;
    }

    // handle single sided bootstrap token deposit
    if bootstrap.bootstrap_to_deposit.unwrap_optimized() > 0 {
        e.authorize_as_current_contract(vec![
            &e,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: bootstrap.bootstrap_token_address.clone(),
                    fn_name: Symbol::new(&e, "approve"),
                    args: vec![
                        &e,
                        e.current_contract_address().into_val(e),
                        storage::get_backstop_token(&e).into_val(e),
                        bootstrap.bootstrap_to_deposit.clone().into_val(e),
                        approval_ledger.into_val(e),
                    ],
                },
                sub_invocations: vec![&e],
            }),
        ]);
        let deposit_amount = bootstrap.bootstrap_to_deposit.unwrap_optimized().min(
            comet_bootstrap_token
                .fixed_mul_floor(MAX_IN_RATIO, SCALAR_7)
                .unwrap_optimized(),
        );
        bootstrap.backstop_tokens += comet_client.dep_tokn_amt_in_get_lp_tokns_out(
            &bootstrap_token_client.address,
            &deposit_amount,
            &0,
            &e.current_contract_address(),
        );
        bootstrap.bootstrap_to_deposit =
            Some(bootstrap.bootstrap_to_deposit.unwrap_optimized() - deposit_amount);
    }
    if bootstrap.pair_to_deposit.unwrap_optimized() > 0 {
        e.authorize_as_current_contract(vec![
            &e,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: bootstrap.pair_token_address.clone(),
                    fn_name: Symbol::new(&e, "approve"),
                    args: vec![
                        &e,
                        e.current_contract_address().into_val(e),
                        storage::get_backstop_token(&e).into_val(e),
                        bootstrap.pair_to_deposit.clone().into_val(e),
                        approval_ledger.into_val(e),
                    ],
                },
                sub_invocations: vec![&e],
            }),
        ]);
        let deposit_amount = bootstrap.pair_to_deposit.unwrap_optimized().min(
            comet_pair_token
                .fixed_mul_floor(MAX_IN_RATIO, SCALAR_7)
                .unwrap_optimized(),
        );
        bootstrap.backstop_tokens += comet_client.dep_tokn_amt_in_get_lp_tokns_out(
            &pair_token_client.address,
            &deposit_amount,
            &0,
            &e.current_contract_address(),
        );
        bootstrap.pair_to_deposit =
            Some(bootstrap.pair_to_deposit.unwrap_optimized() - deposit_amount);
    }
    assert_with_error!(
        e,
        bootstrap.backstop_tokens > 0,
        BackstopBootstrapperError::ReceivedNoBackstopTokens
    );
    if bootstrap.pair_to_deposit.unwrap_optimized() == 0
        && bootstrap.bootstrap_to_deposit.unwrap_optimized() == 0
    {
        bootstrap.status = BootstrapStatus::Completed as u32;
    }
    bootstrap.store(e, bootstrap_id);

    bootstrap.backstop_tokens.clone()
}

pub fn execute_claim(e: &Env, from: &Address, bootstrap_id: u32, bootstrapper: Address) -> i128 {
    let mut bootstrap = Bootstrap::load(e, bootstrapper.clone(), bootstrap_id);
    assert_with_error!(
        e,
        bootstrap.status.clone() as u32 != BootstrapStatus::Active as u32,
        BackstopBootstrapperError::BootstrapNotCompleteError
    );
    let deposit_amount = match BootstrapStatus::from_u32(e, bootstrap.status.clone()) {
        BootstrapStatus::Active => {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapNotCompleteError);
        }
        BootstrapStatus::Cancelled => return_funds(e, &mut bootstrap, from),

        BootstrapStatus::Completed => claim_backstop_tokens(e, &mut bootstrap, from),
    };
    if bootstrap.deposits.len() == 0 && bootstrap.bootstrap_amount == 0 {
        storage::remove_bootstrap(&e, bootstrapper, bootstrap_id);
    } else {
        bootstrap.store(e, bootstrap_id);
    }
    deposit_amount
}

fn return_funds(e: &Env, bootstrap: &mut Bootstrap, from: &Address) -> i128 {
    if bootstrap.bootstrapper == *from {
        assert_with_error!(
            e,
            bootstrap.bootstrap_amount > 0,
            BackstopBootstrapperError::BootstrapAlreadyClaimedError
        );
        TokenClient::new(&e, &bootstrap.bootstrap_token_address).transfer(
            &e.current_contract_address(),
            &from,
            &bootstrap.bootstrap_amount,
        );
        bootstrap.bootstrap_amount = 0;
    } else {
        let deposit_amount = bootstrap.deposits.get(from.clone()).unwrap_or_else(|| {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapAlreadyClaimedError)
        });
        bootstrap.deposits.remove(from.clone());
        TokenClient::new(&e, &bootstrap.pair_token_address).transfer(
            &e.current_contract_address(),
            &from,
            &deposit_amount,
        );
    }
    0
}

fn claim_backstop_tokens(e: &Env, bootstrap: &mut Bootstrap, from: &Address) -> i128 {
    let backstop_address = storage::get_backstop(e);
    let backstop_token_address = storage::get_backstop_token(e);
    let backstop_client = BackstopClient::new(&e, &backstop_address);
    let backstop_token_client = CometClient::new(&e, &backstop_token_address);
    let deposit_amount = if bootstrap.bootstrapper == *from {
        assert_with_error!(
            e,
            bootstrap.bootstrap_amount > 0,
            BackstopBootstrapperError::BootstrapAlreadyClaimedError
        );
        bootstrap.bootstrap_amount = 0;
        bootstrap
            .backstop_tokens
            .fixed_mul_floor(bootstrap.bootstrap_weight as i128, SCALAR_7)
            .unwrap_optimized()
    } else {
        let deposit_amount = bootstrap.deposits.get(from.clone()).unwrap_or_else(|| {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapAlreadyClaimedError)
        });
        //Dev: this contract does not work with bootstrap or backstop tokens with more than 7 decimals
        bootstrap.deposits.remove(from.clone());
        deposit_amount
            .fixed_div_floor(bootstrap.total_deposits, SCALAR_7)
            .unwrap_optimized()
            .fixed_mul_floor(bootstrap.backstop_tokens, SCALAR_7)
            .unwrap_optimized()
            .fixed_mul_floor(SCALAR_7 - (bootstrap.bootstrap_weight as i128), SCALAR_7)
            .unwrap_optimized()
    };
    backstop_token_client.transfer(&e.current_contract_address(), from, &deposit_amount);
    e.authorize_as_current_contract(vec![
        &e,
        InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: backstop_token_address,
                fn_name: Symbol::new(&e, "transfer"),
                args: vec![
                    &e,
                    from.into_val(e),
                    backstop_address.into_val(e),
                    deposit_amount.into_val(e),
                ],
            },
            sub_invocations: Vec::new(&e),
        }),
    ]);
    backstop_client.deposit(from, &bootstrap.pool_address, &deposit_amount)
}

#[cfg(test)]
mod tests {

    use crate::{
        storage::{BootstrapKey, ONE_DAY_LEDGERS},
        testutils::{
            create_backstop, create_blnd_token, create_bootstrapper, create_comet_lp_pool,
            create_emitter, create_mock_pool_factory, create_usdc_token, setup_bootstrapper,
        },
        types::{BootstrapData, TokenInfo},
    };

    use self::storage::LEDGER_BUMP_SHARED;

    use super::*;
    use soroban_sdk::{
        map,
        testutils::{Address as _, Ledger as _, LedgerInfo},
        Address, Env,
    };

    #[test]
    fn test_start_load_bootstrap() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, _) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 100 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        let pair_min = 10 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS + 1;
        e.budget().reset_default();
        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );

            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            let blnd_client = TokenClient::new(&e, &blnd);
            let bootstrap_balance = blnd_client.balance(&bootstrapper);
            let frodo_balance = blnd_client.balance(&frodo);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.bootstrap_amount, bootstrap_amount);
            assert_eq!(bootstrap_data.pair_min, pair_min);
            assert_eq!(bootstrap_data.close_ledger, duration + 1234);
            assert_eq!(bootstrap_data.pool_address, pool_address);
            assert_eq!(bootstrap_data.total_deposits, 0);
            assert_eq!(bootstrap_data.deposits.len(), 0);
            assert_eq!(bootstrap_data.status, 0);
            assert_eq!(bootstrap_data.backstop_tokens, 0);
            assert_eq!(bootstrap_data.bootstrap_token_index, 0);
            assert_eq!(bootstrap_balance, bootstrap_amount);
            assert_eq!(frodo_balance, bootstrap_amount);
        })
    }
    #[test]
    fn test_start_load_bootstrap_3() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 100 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&frodo, &(bootstrap_amount));
        let pair_min = 10 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS + 1;
        e.budget().reset_default();
        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );

            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                1,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            let usdc_client = TokenClient::new(&e, &usdc);
            let bootstrap_balance = usdc_client.balance(&bootstrapper);
            let frodo_balance = usdc_client.balance(&frodo);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 1).unwrap();
            assert_eq!(bootstrap_data.bootstrap_amount, bootstrap_amount);
            assert_eq!(bootstrap_data.pair_min, pair_min);
            assert_eq!(bootstrap_data.close_ledger, duration + 1234);
            assert_eq!(bootstrap_data.pool_address, pool_address);
            assert_eq!(bootstrap_data.total_deposits, 0);
            assert_eq!(bootstrap_data.deposits.len(), 0);
            assert_eq!(bootstrap_data.status, 0);
            assert_eq!(bootstrap_data.backstop_tokens, 0);
            assert_eq!(bootstrap_data.bootstrap_token_index, 1);
            assert_eq!(bootstrap_balance, bootstrap_amount);
            assert_eq!(frodo_balance, 0);
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 2).unwrap();
            assert_eq!(bootstrap_data.bootstrap_amount, bootstrap_amount);
            assert_eq!(bootstrap_data.pair_min, pair_min);
            assert_eq!(bootstrap_data.close_ledger, duration + 1234);
            assert_eq!(bootstrap_data.pool_address, pool_address);
            assert_eq!(bootstrap_data.total_deposits, 0);
            assert_eq!(bootstrap_data.deposits.len(), 0);
            assert_eq!(bootstrap_data.status, 0);
            assert_eq!(bootstrap_data.backstop_tokens, 0);
            assert_eq!(bootstrap_data.bootstrap_token_index, 0);
            assert_eq!(bootstrap_balance, bootstrap_amount);
            assert_eq!(frodo_balance, 0);
        })
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #100)")]
    fn test_start_load_bootstrap_duration_too_small() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, _) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 100 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        let pair_min = 10 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS - 1;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
        })
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #103)")]
    fn test_start_load_bootstrap_not_pool() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, _) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        let comet = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (pool_factory, _) = create_mock_pool_factory(&e);
        let (emitter, _) = create_emitter(&e, &backstop, &comet.0, &blnd);
        let backstop_client: BackstopClient = BackstopClient::new(&e, &backstop);

        backstop_client.initialize(
            &comet.0,
            &emitter,
            &usdc,
            &blnd,
            &pool_factory,
            &map![&e, (pool_address.clone(), 50_000_000 * SCALAR_7)],
        );
        let bootstrap_amount = 100 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        let pair_min = 10 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS - 1;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_is_init(&e);
            storage::set_backstop(&e, backstop.clone());
            storage::set_backstop_token(&e, comet.0);
            storage::set_pool_factory(&e, pool_factory);
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
        })
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #101)")]
    fn test_start_load_bootstrap_duration_too_long() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, _) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 100 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        let pair_min = 10 * SCALAR_7;
        let duration = LEDGER_BUMP_SHARED + 1;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
        })
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #8)")]
    fn test_start_load_bootstrap_negative() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, _) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 100 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        let pair_min = -1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
        })
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #102)")]
    fn test_start_load_bootstrap_invalid_amt() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, _) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 0;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
        })
    }

    #[test]
    fn test_join_bootstrap() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 100 * SCALAR_7;
        let join_2_amount = 200 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.total_deposits, join_amount);
            assert_eq!(
                bootstrap_data.deposits.get(samwise.clone()),
                Some(join_amount)
            );
            let usdc_client = TokenClient::new(&e, &usdc);
            let samwise_balance = usdc_client.balance(&samwise);
            assert_eq!(samwise_balance, 0);
            let bootstrap_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, join_amount);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.total_deposits, join_amount + join_2_amount);
            assert_eq!(
                bootstrap_data.deposits.get(merry.clone()),
                Some(join_2_amount)
            );
            let merry_balance = usdc_client.balance(&merry);
            assert_eq!(merry_balance, 0);
            let bootstrap_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, join_amount + join_2_amount);
        })
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #105)")]
    fn test_join_bootstrap_not_active() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 100 * SCALAR_7;
        let join_2_amount = 200 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + duration,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 2000000,
            });
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
        })
    }

    #[test]
    fn test_exit_bootstrap() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 100 * SCALAR_7;
        let join_2_amount = 200 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(
                bootstrap_data.total_deposits,
                join_amount / 2 + join_2_amount
            );
            assert_eq!(
                bootstrap_data.deposits.get(samwise.clone()),
                Some(join_amount / 2)
            );
            let usdc_client = TokenClient::new(&e, &usdc);
            let samwise_balance = usdc_client.balance(&samwise);
            assert_eq!(samwise_balance, join_amount / 2);
            let bootstrap_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, join_2_amount + join_amount / 2);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(
                bootstrap_data.total_deposits,
                join_amount / 2 + join_2_amount / 2
            );
            assert_eq!(
                bootstrap_data.deposits.get(merry.clone()),
                Some(join_2_amount / 2)
            );
            let merry_balance = usdc_client.balance(&merry);
            assert_eq!(merry_balance, join_2_amount / 2);
            let bootstrap_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, join_amount / 2 + join_2_amount / 2);
        })
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #105)")]
    fn test_exit_bootstrap_not_active() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 100 * SCALAR_7;
        let join_2_amount = 200 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + duration,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 2000000,
            });
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
        })
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #108)")]
    fn test_exit_bootstrap_too_large() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 2000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 100 * SCALAR_7;
        let join_2_amount = 200 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount * 2, frodo.clone(), 0);
        })
    }
    #[test]
    fn test_close_bootstrap() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        let comet_client = setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Completed as u32);

            assert_eq!(bootstrap_data.backstop_tokens, 999998579);
            let bootstrap_balance = comet_client.balance(&bootstrapper);
            let blnd_client = TokenClient::new(&e, &blnd);
            assert_eq!(bootstrap_balance, 999998579);
            let blnd_balance = blnd_client.balance(&bootstrapper);
            assert_eq!(blnd_balance, 0);
            let usdc_client = TokenClient::new(&e, &usdc);
            let usdc_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(usdc_balance, 0);
        })
    }

    #[test]
    fn test_multiple_closes() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        let comet_client = setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 3000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            let single_sided_deposit_amount = (2000 * SCALAR_7).min(
                (19970003002)
                    .fixed_mul_floor(MAX_IN_RATIO, SCALAR_7)
                    .unwrap_optimized(),
            );

            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Active as u32);
            assert_eq!(
                bootstrap_data.bootstrap_to_deposit.unwrap(),
                3000 * SCALAR_7 - single_sided_deposit_amount - 1000 * SCALAR_7
            );
            assert_eq!(bootstrap_data.pair_to_deposit.unwrap(), 0);
            assert_eq!(bootstrap_data.backstop_tokens, 1758463207);
            let bootstrap_balance = comet_client.balance(&bootstrapper);
            let blnd_client = TokenClient::new(&e, &blnd);
            assert_eq!(bootstrap_balance, 1758463207);
            let blnd_balance = blnd_client.balance(&bootstrapper);
            assert_eq!(
                blnd_balance,
                3000 * SCALAR_7 - single_sided_deposit_amount - 1000 * SCALAR_7
            );
            let usdc_client = TokenClient::new(&e, &usdc);
            let usdc_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(usdc_balance, 0);
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Completed as u32);

            assert_eq!(bootstrap_data.backstop_tokens, 2470494556);
            let bootstrap_balance = comet_client.balance(&bootstrapper);
            let blnd_client = TokenClient::new(&e, &blnd);
            assert_eq!(bootstrap_balance, 2470494556);
            let blnd_balance = blnd_client.balance(&bootstrapper);
            assert_eq!(blnd_balance, 0);
            let usdc_client = TokenClient::new(&e, &usdc);
            let usdc_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(usdc_balance, 0);
        })
    }

    #[test]
    fn test_close_bootstrap_canceled_under_min() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        let comet_client = setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 100 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Cancelled as u32);

            assert_eq!(bootstrap_data.backstop_tokens, 0);
            let bootstrap_balance = comet_client.balance(&bootstrapper);
            let blnd_client = TokenClient::new(&e, &blnd);
            assert_eq!(bootstrap_balance, 0);
            let blnd_balance = blnd_client.balance(&bootstrapper);
            assert_eq!(blnd_balance, bootstrap_amount);
            let usdc_client = TokenClient::new(&e, &usdc);
            let usdc_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(usdc_balance, 25 * SCALAR_7);
        })
    }

    #[test]
    fn test_close_bootstrap_canceled_too_long() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        let comet_client = setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS * 3,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Cancelled as u32);

            assert_eq!(bootstrap_data.backstop_tokens, 0);
            let bootstrap_balance = comet_client.balance(&bootstrapper);
            let blnd_client = TokenClient::new(&e, &blnd);
            assert_eq!(bootstrap_balance, 0);
            let blnd_balance = blnd_client.balance(&bootstrapper);
            assert_eq!(blnd_balance, bootstrap_amount);
            let usdc_client = TokenClient::new(&e, &usdc);
            let usdc_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(usdc_balance, 25 * SCALAR_7);
        })
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #106)")]
    fn test_close_bootstrap_not_active_error() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS / 2,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            execute_close(&e, 0, frodo.clone());
        })
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #105)")]
    fn test_close_bootstrap_not_finished_error() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS * 3,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            execute_close(&e, 0, frodo.clone());
        })
    }
    #[test]
    fn test_user_claim() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        let comet_client = setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        let expected_claim_amt = 999998579 / 5 / 5;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let res = execute_claim(&e, &samwise, 0, frodo.clone());
            assert_eq!(res, expected_claim_amt);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Completed as u32);

            assert!(bootstrap_data.deposits.get(samwise.clone()).is_none());

            let bootstrap_balance = comet_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, 999998579 - expected_claim_amt);
        });
    }
    #[test]
    fn test_bootstrapper_claim() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        let comet_client = setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        let expected_claim_amt = 999998579 * 4 / 5;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let res = execute_claim(&e, &frodo, 0, frodo.clone());
            assert_eq!(res, expected_claim_amt);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Completed as u32);

            assert_eq!(bootstrap_data.bootstrap_amount, 0);

            let bootstrap_balance = comet_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, 999998579 - expected_claim_amt);
        });
    }

    #[test]
    fn test_user_claim_canceled() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1000 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let res = execute_claim(&e, &samwise, 0, frodo.clone());
            assert_eq!(res, 0);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Cancelled as u32);

            assert!(bootstrap_data.deposits.get(samwise.clone()).is_none());
            let usdc_client = TokenClient::new(&e, &usdc);
            let bootstrap_balance = usdc_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, 20 * SCALAR_7);
            assert_eq!(usdc_client.balance(&samwise), join_amount);
        });
    }

    #[test]
    fn test_bootstrap_claim_canceled() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1000 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            let res = execute_claim(&e, &frodo, 0, frodo.clone());
            assert_eq!(res, 0);
            let bootstrap_data = storage::get_bootstrap_data(&e, frodo.clone(), 0).unwrap();
            assert_eq!(bootstrap_data.status, BootstrapStatus::Cancelled as u32);

            assert!(bootstrap_data.bootstrap_amount == 0);
            let blnd_client = TokenClient::new(&e, &blnd);
            let bootstrap_balance = blnd_client.balance(&bootstrapper);
            assert_eq!(bootstrap_balance, 0);
            assert_eq!(blnd_client.balance(&frodo), bootstrap_amount * 2);
        });
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #106)")]
    fn test_claim_not_completed() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1000 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            execute_claim(&e, &frodo, 0, frodo.clone());
        });
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #107)")]
    fn test_user_already_claimed_canceled() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1000 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            execute_claim(&e, &samwise, 0, frodo.clone());
            execute_claim(&e, &samwise, 0, frodo.clone());
        });
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #107)")]
    fn test_bootstrap_already_claim_canceled() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1000 * SCALAR_7;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            execute_claim(&e, &frodo, 0, frodo.clone());
            execute_claim(&e, &frodo, 0, frodo.clone());
        });
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #107)")]
    fn test_user_already_claim() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            execute_claim(&e, &samwise, 0, frodo.clone());
            execute_claim(&e, &samwise, 0, frodo.clone());
        });
    }
    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #107)")]
    fn test_bootstrapper_already_claim() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            execute_claim(&e, &frodo, 0, frodo.clone());
            execute_claim(&e, &frodo, 0, frodo.clone());
        });
    }
    #[test]
    fn test_full_claim() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.ledger().set(LedgerInfo {
            timestamp: 600,
            protocol_version: 20,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 2000000,
            max_entry_ttl: 200000000,
        });
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);
        let merry = Address::generate(&e);
        let pool_address = Address::generate(&e);
        let bootstrapper = create_bootstrapper(&e);
        let (backstop, _) = create_backstop(&e);
        let (blnd, blnd_client) = create_blnd_token(&e, &bootstrapper, &bombadil);
        let (usdc, usdc_client) = create_usdc_token(&e, &bootstrapper, &bombadil);
        e.budget().reset_unlimited();
        setup_bootstrapper(
            &e,
            &bootstrapper,
            &pool_address,
            &backstop,
            &bombadil,
            &blnd,
            &usdc,
        );
        let bootstrap_amount = 1000 * SCALAR_7;
        let join_amount = 10 * SCALAR_7;
        let join_2_amount = 40 * SCALAR_7;
        blnd_client.mint(&frodo, &(bootstrap_amount * 2));
        usdc_client.mint(&samwise, &join_amount);
        usdc_client.mint(&merry, &join_2_amount);
        let pair_min = 1;
        let duration = ONE_DAY_LEDGERS;
        e.budget().reset_default();

        e.as_contract(&bootstrapper, || {
            storage::set_comet_token_data(
                &e,
                0,
                TokenInfo {
                    address: blnd.clone(),
                    weight: 800_0000,
                },
            );
            storage::set_comet_token_data(
                &e,
                1,
                TokenInfo {
                    address: usdc.clone(),
                    weight: 200_0000,
                },
            );
            execute_start_bootstrap(
                &e,
                frodo.clone(),
                0,
                bootstrap_amount,
                pair_min,
                duration,
                pool_address.clone(),
            );
            execute_join(&e, &samwise, join_amount, frodo.clone(), 0);
            execute_join(&e, &merry, join_2_amount, frodo.clone(), 0);
            execute_exit(&e, samwise.clone(), join_amount / 2, frodo.clone(), 0);
            execute_exit(&e, merry.clone(), join_2_amount / 2, frodo.clone(), 0);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 20,
                sequence_number: 1234 + ONE_DAY_LEDGERS,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 200000,
                max_entry_ttl: 200000000,
            });
            e.budget().reset_default();
            execute_close(&e, 0, frodo.clone());
            e.budget().reset_unlimited();
            execute_claim(&e, &frodo, 0, frodo.clone());
            execute_claim(&e, &samwise, 0, frodo.clone());
            execute_claim(&e, &merry, 0, frodo.clone());
            let key = BootstrapKey {
                id: 0,
                creator: frodo.clone(),
            };

            let bootstrap_data = e
                .storage()
                .persistent()
                .get::<BootstrapKey, BootstrapData>(&key);
            assert!(bootstrap_data.is_none());
        });
    }
}
