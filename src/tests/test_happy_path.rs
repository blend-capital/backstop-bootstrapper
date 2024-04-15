#![cfg(test)]

use crate::constants::{MAX_DUST_AMOUNT, SCALAR_7};
use crate::storage::ONE_DAY_LEDGERS;
use crate::testutils::{
    self, assert_approx_eq_abs, assert_approx_eq_rel, est_close_mint, EnvTestUtils,
};
use crate::types::BootstrapConfig;
use crate::BackstopBootstrapperClient;
use blend_contract_sdk::testutils::BlendFixture;
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::testutils::{Address as _, BytesN as _, Events, MockAuth, MockAuthInvoke};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{vec, Address, BytesN, Env, IntoVal, Symbol, Val};

#[test]
fn test_happy_path() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);
    let samwise = Address::generate(&e);

    let blnd = e.register_stellar_asset_contract(bombadil.clone());
    let usdc = e.register_stellar_asset_contract(bombadil.clone());
    let blnd_client = StellarAssetClient::new(&e, &blnd);
    let blnd_token = TokenClient::new(&e, &blnd);
    let usdc_client = StellarAssetClient::new(&e, &usdc);
    let usdc_token = TokenClient::new(&e, &usdc);

    let blend_fixture = BlendFixture::deploy(&e, &bombadil, &blnd, &usdc);
    let pool_address = blend_fixture.pool_factory.mock_all_auths().deploy(
        &bombadil,
        &Symbol::new(&e, "test"),
        &BytesN::<32>::random(&e),
        &Address::generate(&e),
        &0,
        &2,
    );
    let comet_shares = blend_fixture.backstop_token.get_total_supply();
    let comet_blnd = blnd_token.balance(&blend_fixture.backstop_token.address);
    let comet_usdc = usdc_token.balance(&blend_fixture.backstop_token.address);

    let bootstrapper = testutils::create_bootstrapper(&e, &blend_fixture);
    let bootstrap_client = BackstopBootstrapperClient::new(&e, &bootstrapper);

    // create bootstrap
    let initial_balance = 2 * 1000 * SCALAR_7;
    let bootstrap_amount = 1000 * SCALAR_7;
    blnd_client.mock_all_auths().mint(&frodo, &initial_balance);

    let pair_min = 10 * SCALAR_7;
    let duration = ONE_DAY_LEDGERS + 1;
    let config = BootstrapConfig {
        pair_min,
        close_ledger: e.ledger().sequence() + duration,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 0,
    };
    e.set_auths(&[]);
    let id = bootstrap_client
        .mock_auths(&[MockAuth {
            address: &frodo,
            invoke: &MockAuthInvoke {
                contract: &bootstrapper,
                fn_name: &"bootstrap",
                args: vec![&e, config.into_val(&e)],
                sub_invokes: &[MockAuthInvoke {
                    contract: &blnd,
                    fn_name: &"transfer",
                    args: vec![
                        &e,
                        frodo.into_val(&e),
                        bootstrapper.into_val(&e),
                        bootstrap_amount.into_val(&e),
                    ],
                    sub_invokes: &[],
                }],
            },
        }])
        .bootstrap(&config);
    let event = vec![&e, e.events().all().last_unchecked()];
    let event_data: soroban_sdk::Vec<Val> = vec![
        &e,
        config.token_index.into_val(&e),
        config.amount.into_val(&e),
        config.close_ledger.into_val(&e),
    ];
    assert_eq!(
        event,
        vec![
            &e,
            (
                bootstrapper.clone(),
                (Symbol::new(&e, "bootstrap"), frodo.clone(), id.clone()).into_val(&e),
                event_data.into_val(&e)
            )
        ]
    );
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(
        initial_balance - bootstrap_amount,
        blnd_token.balance(&frodo)
    );

    // join bootstrap
    let join_amount = 50 * SCALAR_7;
    usdc_client.mock_all_auths().mint(&samwise, &join_amount);
    e.set_auths(&[]);
    bootstrap_client
        .mock_auths(&[MockAuth {
            address: &samwise,
            invoke: &MockAuthInvoke {
                contract: &bootstrapper,
                fn_name: &"join",
                args: vec![
                    &e,
                    samwise.into_val(&e),
                    id.into_val(&e),
                    join_amount.into_val(&e),
                ],
                sub_invokes: &[MockAuthInvoke {
                    contract: &usdc,
                    fn_name: &"transfer",
                    args: vec![
                        &e,
                        samwise.into_val(&e),
                        bootstrapper.into_val(&e),
                        join_amount.into_val(&e),
                    ],
                    sub_invokes: &[],
                }],
            },
        }])
        .join(&samwise, &id, &join_amount);
    assert_eq!(join_amount, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&samwise));

    // exit bootstrap
    let exit_amount = 20 * SCALAR_7;
    e.set_auths(&[]);
    bootstrap_client
        .mock_auths(&[MockAuth {
            address: &samwise,
            invoke: &MockAuthInvoke {
                contract: &bootstrapper,
                fn_name: &"exit",
                args: vec![
                    &e,
                    samwise.into_val(&e),
                    id.into_val(&e),
                    exit_amount.into_val(&e),
                ],
                sub_invokes: &[],
            },
        }])
        .exit(&samwise, &id, &exit_amount);
    assert_eq!(join_amount - exit_amount, usdc_token.balance(&bootstrapper));
    assert_eq!(exit_amount, usdc_token.balance(&samwise));

    // close bootstrap
    let est_backstop_tokens = est_close_mint(
        bootstrap_amount,
        join_amount - exit_amount,
        comet_blnd,
        comet_usdc,
        comet_shares,
    );
    e.jump(duration + 1);
    e.set_auths(&[]);
    // no auths required by caller
    let backstop_tokens = bootstrap_client.close(&id);
    let event = vec![&e, e.events().all().last_unchecked()];
    assert_eq!(
        event,
        vec![
            &e,
            (
                bootstrapper.clone(),
                (Symbol::new(&e, "bootstrap_close"), id.clone()).into_val(&e),
                backstop_tokens.into_val(&e)
            )
        ]
    );
    assert_approx_eq_abs(0, blnd_token.balance(&bootstrapper), MAX_DUST_AMOUNT);
    assert_approx_eq_abs(0, usdc_token.balance(&bootstrapper), MAX_DUST_AMOUNT);
    assert_eq!(
        backstop_tokens,
        blend_fixture.backstop_token.balance(&bootstrapper)
    );
    // at most 3% slippage on close
    assert_approx_eq_rel(est_backstop_tokens, backstop_tokens, 0_0300000);

    // claim (backstop tokens are 1-1 with backstop shares)
    let est_frodo = backstop_tokens
        .fixed_mul_floor(0_8000000, SCALAR_7)
        .unwrap();
    e.set_auths(&[]);

    bootstrap_client
        .mock_auths(&[MockAuth {
            address: &frodo,
            invoke: &MockAuthInvoke {
                contract: &bootstrapper,
                fn_name: &"claim",
                args: vec![&e, frodo.into_val(&e), id.into_val(&e)],
                sub_invokes: &[MockAuthInvoke {
                    contract: &blend_fixture.backstop.address,
                    fn_name: &"deposit",
                    args: vec![
                        &e,
                        frodo.into_val(&e),
                        config.pool.into_val(&e),
                        est_frodo.into_val(&e),
                    ],
                    sub_invokes: &[MockAuthInvoke {
                        contract: &blend_fixture.backstop_token.address,
                        fn_name: &"transfer",
                        args: vec![
                            &e,
                            frodo.into_val(&e),
                            blend_fixture.backstop.address.into_val(&e),
                            est_frodo.into_val(&e),
                        ],
                        sub_invokes: &[],
                    }],
                }],
            },
        }])
        .claim(&frodo, &id);
    assert_approx_eq_abs(
        est_frodo,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &frodo)
            .shares,
        MAX_DUST_AMOUNT,
    );

    let est_samwise = backstop_tokens
        .fixed_mul_floor(0_2000000, SCALAR_7)
        .unwrap();
    e.set_auths(&[]);
    bootstrap_client
        .mock_auths(&[MockAuth {
            address: &samwise,
            invoke: &MockAuthInvoke {
                contract: &bootstrapper,
                fn_name: &"claim",
                args: vec![&e, samwise.into_val(&e), id.into_val(&e)],
                sub_invokes: &[MockAuthInvoke {
                    contract: &blend_fixture.backstop.address,
                    fn_name: &"deposit",
                    args: vec![
                        &e,
                        samwise.into_val(&e),
                        config.pool.into_val(&e),
                        est_samwise.into_val(&e),
                    ],
                    sub_invokes: &[MockAuthInvoke {
                        contract: &blend_fixture.backstop_token.address,
                        fn_name: &"transfer",
                        args: vec![
                            &e,
                            samwise.into_val(&e),
                            blend_fixture.backstop.address.into_val(&e),
                            est_samwise.into_val(&e),
                        ],
                        sub_invokes: &[],
                    }],
                }],
            },
        }])
        .claim(&samwise, &id);
    assert_approx_eq_abs(
        est_samwise,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &samwise)
            .shares,
        MAX_DUST_AMOUNT,
    );
    assert_approx_eq_abs(
        0,
        blend_fixture.backstop_token.balance(&bootstrapper),
        MAX_DUST_AMOUNT,
    );
}
