use soroban_sdk::{
    assert_with_error,
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    panic_with_error,
    token::TokenClient,
    vec, Address, Env, IntoVal, Map, Symbol, Vec,
};

use crate::{
    dependencies::{BackstopClient, CometClient},
    errors::BackstopBootstrapperError,
    storage,
    types::{Bootstrap, BootstrapStatus},
};

pub fn execute_start_bootstrap(
    e: &Env,
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
) -> Bootstrap {
    assert_with_error!(
        e,
        bootstrap_amount > 0,
        BackstopBootstrapperError::InvalidBootstrapAmount
    );
    assert_with_error!(
        e,
        bootstrap_weight < 1_000_0000 && bootstrap_weight > 0,
        BackstopBootstrapperError::InvalidBootstrapWeight
    );
    assert_with_error!(
        e,
        pair_min >= 0,
        BackstopBootstrapperError::NegativeAmountError
    );
    assert_with_error!(
        e,
        duration > storage::ONE_DAY_LEDGERS,
        BackstopBootstrapperError::DurationTooShort
    );
    assert_with_error!(
        e,
        duration < storage::LEDGER_BUMP_SHARED,
        BackstopBootstrapperError::DurationTooLong
    );

    let bootstrap = Bootstrap {
        bootstrapper: bootstrapper.clone(),
        bootstrap_token,
        pair_token,
        bootstrap_amount,
        pair_min,
        close_ledger: e.ledger().sequence() + duration,
        bootstrap_weight,
        pool_address,
        total_deposits: 0,
        deposits: Map::new(e),
        status: BootstrapStatus::Active as u32,
        backstop_tokens: 0,
        bootstrap_token_index,
        pair_token_index,
    };
    storage::set_bootstrap(
        e,
        bootstrapper.clone(),
        storage::bump_bootstrap_id(e, bootstrapper),
        &bootstrap,
    );
    bootstrap
}

pub fn execute_join(
    e: &Env,
    from: &Address,
    amount: i128,
    bootstrapper: Address,
    bootstrap_id: u32,
) {
    let mut bootstrap: Bootstrap = storage::get_bootstrap(&e, bootstrapper.clone(), bootstrap_id)
        .unwrap_or_else(|| {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapNotFoundError);
        })
        .clone();

    // deposit the pair token into the contract
    TokenClient::new(&e, &bootstrap.pair_token).transfer(
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

    storage::set_bootstrap(&e, bootstrapper, bootstrap_id, &bootstrap);
}

pub fn execute_exit(
    e: &Env,
    from: Address,
    amount: i128,
    bootstrapper: Address,
    bootstrap_id: u32,
) {
    let mut bootstrap: Bootstrap = storage::get_bootstrap(&e, bootstrapper.clone(), bootstrap_id)
        .unwrap_or_else(|| {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapNotFoundError);
        })
        .clone();
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
    TokenClient::new(&e, &bootstrap.pair_token).transfer(
        &e.current_contract_address(),
        &from,
        &amount,
    );

    storage::set_bootstrap(&e, bootstrapper, bootstrap_id, &bootstrap);
}

pub fn execute_close(e: &Env, bootstrap_id: u32, bootstrapper: Address) -> i128 {
    let mut bootstrap: Bootstrap = storage::get_bootstrap(&e, bootstrapper.clone(), bootstrap_id)
        .unwrap_or_else(|| {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapNotFoundError);
        })
        .clone();

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
    if bootstrap.total_deposits < bootstrap.pair_min {
        bootstrap.status = BootstrapStatus::Cancelled as u32;
        storage::set_bootstrap(&e, bootstrapper.clone(), bootstrap_id, &bootstrap);
        return 0;
    }
    let backstop_token = storage::get_backstop_token(e);
    let comet_client = CometClient::new(&e, &backstop_token);
    let backstop_token_balance = comet_client.balance(&e.current_contract_address());
    let bootstrap_token_client = TokenClient::new(&e, &bootstrap.bootstrap_token);
    let bootstrap_token_balance = bootstrap_token_client.balance(&e.current_contract_address());
    let pair_token_client = TokenClient::new(&e, &bootstrap.pair_token);
    let pair_token_balance = pair_token_client.balance(&e.current_contract_address());
    let mut amounts_in = Vec::new(&e);
    amounts_in.insert(
        bootstrap.bootstrap_token_index.clone(),
        bootstrap.bootstrap_amount.clone(),
    );
    amounts_in.insert(
        bootstrap.pair_token_index.clone(),
        bootstrap.total_deposits.clone(),
    );
    let result = comet_client.try_join_pool(&0, &amounts_in, &e.current_contract_address());
    match result {
        Ok(_) => {}
        Err(_) => {
            bootstrap.status = BootstrapStatus::Cancelled as u32;
            storage::set_bootstrap(&e, bootstrapper.clone(), bootstrap_id, &bootstrap);
            return 0;
        }
    }
    let remaining_bootstrap_tokens = bootstrap.bootstrap_amount
        - (bootstrap_token_balance - bootstrap_token_client.balance(&e.current_contract_address()));
    if remaining_bootstrap_tokens > 0 {
        comet_client.dep_tokn_amt_in_get_lp_tokns_out(
            &bootstrap_token_client.address,
            &remaining_bootstrap_tokens,
            &0,
            &e.current_contract_address(),
        );
    }
    let remaining_pair_tokens = bootstrap.total_deposits
        - (pair_token_balance - pair_token_client.balance(&e.current_contract_address()));
    if remaining_pair_tokens > 0 {
        comet_client.dep_tokn_amt_in_get_lp_tokns_out(
            &pair_token_client.address,
            &remaining_pair_tokens,
            &0,
            &e.current_contract_address(),
        );
    }
    bootstrap.backstop_tokens =
        comet_client.balance(&e.current_contract_address()) - backstop_token_balance;
    bootstrap.status = if bootstrap.backstop_tokens == 0 {
        BootstrapStatus::Cancelled as u32
    } else {
        BootstrapStatus::Completed as u32
    };
    storage::set_bootstrap(&e, bootstrapper, bootstrap_id, &bootstrap);

    bootstrap.backstop_tokens.clone()
}

pub fn execute_claim(e: &Env, from: &Address, bootstrap_id: u32, bootstrapper: Address) -> i128 {
    let mut bootstrap = storage::get_bootstrap(&e, bootstrapper.clone(), bootstrap_id)
        .unwrap_or_else(|| {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapNotFoundError);
        })
        .clone();
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
        storage::set_bootstrap(&e, bootstrapper, bootstrap_id, &bootstrap);
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
        TokenClient::new(&e, &bootstrap.bootstrap_token).transfer(
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
        TokenClient::new(&e, &bootstrap.pair_token).transfer(
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
        bootstrap.backstop_tokens * bootstrap.bootstrap_weight as i128 / 1_000_0000
    } else {
        let deposit_amount = bootstrap.deposits.get(from.clone()).unwrap_or_else(|| {
            panic_with_error!(&e, BackstopBootstrapperError::BootstrapAlreadyClaimedError)
        });
        bootstrap.deposits.remove(from.clone());
        deposit_amount * 1_000_0000 / bootstrap.total_deposits * bootstrap.backstop_tokens
            / 1_000_0000
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

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use soroban_sdk::{Env, Vec};

//     #[test]
//     fn test_find_unlock_with_sequence() {
//         let e = Env::default();
//         let sequences = Vec::from_array(&e, [1, 3, 5, 7, 9]);

//         let index = find_unlock_with_sequence(0, &sequences);
//         assert_eq!(index, None);

//         let index = find_unlock_with_sequence(1, &sequences);
//         assert_eq!(index, Some(0));

//         let index = find_unlock_with_sequence(8, &sequences);
//         assert_eq!(index, Some(3));

//         let index = find_unlock_with_sequence(9, &sequences);
//         assert_eq!(index, Some(4));

//         let index = find_unlock_with_sequence(20, &sequences);
//         assert_eq!(index, Some(4));
//     }
// }
