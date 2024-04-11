use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    token::TokenClient,
    unwrap::UnwrapOptimized,
    vec, Address, Env, IntoVal, Symbol, Vec,
};

use crate::{
    bootstrap::Bootstrap,
    constants::{MAX_IN_RATIO, SCALAR_7},
    dependencies::comet,
    storage,
    types::TokenInfo,
};

/// Execute join pool against comet
///
/// Returns (amount of bootstrap deposited, amount of pair tokens deposited, amount of shares minted)
///
/// ### Arguments
/// * `e` - The environment
/// * `comet_client` - The comet client
/// * `tokens` - The comet tokens
/// * `bootstrap` - The bootstrap (modified in place)
/// * `bootstrap_bal` - The current contract balance of bootstrap tokens
/// * `pair_bal` - The current contract balance of pair tokens
/// * `comet_bootstrap_bal` - The current contract balance of comet bootstrap tokens (modified in place)
/// * `comet_pair_bal` - The current contract balance of comet pair tokens (modified in place)
/// * `comet_shares` - The current contract balance of comet shares (modified in place)
pub fn join_pool(
    e: &Env,
    comet_client: &comet::Client,
    tokens: &Vec<TokenInfo>,
    bootstrap: &Bootstrap,
    bootstrap_bal: i128,
    pair_bal: i128,
    comet_bootstrap_bal: i128,
    comet_pair_bal: i128,
    comet_shares: i128,
) -> (i128, i128, i128) {
    let bootstrap_info = tokens.get_unchecked(bootstrap.config.token_index);
    let pair_info = tokens.get_unchecked(bootstrap.config.token_index ^ 1);

    // underlying per LP token
    let expected_tokens = bootstrap
        .data
        .bootstrap_amount
        .fixed_div_floor(comet_bootstrap_bal, SCALAR_7)
        .unwrap_optimized()
        .fixed_mul_floor(comet_shares, SCALAR_7)
        .unwrap_optimized()
        .min(
            bootstrap
                .data
                .pair_amount
                .fixed_div_floor(comet_pair_bal, SCALAR_7)
                .unwrap_optimized()
                .fixed_mul_floor(comet_shares, SCALAR_7)
                .unwrap_optimized(),
        )
        .fixed_mul_floor(0_9999000, SCALAR_7) // we want to leave a little bit of room for rounding
        .unwrap_optimized();

    // handle join_pool
    let approval_ledger = (e.ledger().sequence() / 100000 + 1) * 100000;
    if expected_tokens > 0 {
        let mut auths = vec![&e];
        let mut amounts_in = vec![&e];
        for index in 0..2 {
            let (address, amount) = if index == bootstrap.config.token_index {
                amounts_in.push_back(bootstrap.data.bootstrap_amount);
                (
                    bootstrap_info.address.clone(),
                    bootstrap.data.bootstrap_amount,
                )
            } else {
                amounts_in.push_back(bootstrap.data.pair_amount);
                (pair_info.address.clone(), bootstrap.data.pair_amount)
            };
            auths.push_back(InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: address,
                    fn_name: Symbol::new(&e, "approve"),
                    args: vec![
                        &e,
                        e.current_contract_address().into_val(e),
                        storage::get_backstop_token(&e).into_val(e),
                        amount.into_val(e),
                        approval_ledger.into_val(e),
                    ],
                },
                sub_invocations: vec![e],
            }));
        }
        e.authorize_as_current_contract(auths);
        comet_client.join_pool(&expected_tokens, &amounts_in, &e.current_contract_address());

        let deposited_bootstrap_tokens = bootstrap_bal
            - TokenClient::new(e, &bootstrap_info.address).balance(&e.current_contract_address());
        let deposited_pair_tokens = pair_bal
            - TokenClient::new(e, &pair_info.address).balance(&e.current_contract_address());
        (
            deposited_bootstrap_tokens,
            deposited_pair_tokens,
            expected_tokens,
        )
    } else {
        (0, 0, 0)
    }
}

/// Execute single sided deposit of the pair token against comet
///    
/// Returns (amount of tokens deposited, amount of shares minted)
///
/// ### Arguments
/// * `e` - The environment
/// * `comet_client` - The comet client
/// * `token` - The address of the token to deposit
/// * `amount` - The amount of tokens to deposit
/// * `comet_bal` - The current contract balance of comet tokens
pub fn single_sided_join(
    e: &Env,
    comet_client: &comet::Client,
    token: &Address,
    amount: i128,
    comet_bal: i128,
) -> (i128, i128) {
    let deposit_amount = amount.min(
        comet_bal
            .fixed_mul_floor(MAX_IN_RATIO, SCALAR_7)
            .unwrap_optimized(),
    );

    let approval_ledger = (e.ledger().sequence() / 100000 + 1) * 100000;
    e.authorize_as_current_contract(vec![
        &e,
        InvokerContractAuthEntry::Contract(SubContractInvocation {
            context: ContractContext {
                contract: token.clone(),
                fn_name: Symbol::new(&e, "approve"),
                args: vec![
                    &e,
                    e.current_contract_address().into_val(e),
                    storage::get_backstop_token(e).into_val(e),
                    amount.into_val(e),
                    approval_ledger.into_val(e),
                ],
            },
            sub_invocations: vec![&e],
        }),
    ]);
    let tokens_minted = comet_client.dep_tokn_amt_in_get_lp_tokns_out(
        &token,
        &deposit_amount,
        &0,
        &e.current_contract_address(),
    );
    (deposit_amount, tokens_minted)
}
