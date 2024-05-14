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
use soroban_sdk::testutils::{Address as _, BytesN as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, BytesN, Env, Error, String};

#[test]
fn test_claim_multiple_joiners() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);
    let samwise = Address::generate(&e);
    let pippin = Address::generate(&e);
    let merry = Address::generate(&e);

    let blnd = e.register_stellar_asset_contract(bombadil.clone());
    let usdc = e.register_stellar_asset_contract(bombadil.clone());
    let blnd_client = StellarAssetClient::new(&e, &blnd);
    let blnd_token = TokenClient::new(&e, &blnd);
    let usdc_client = StellarAssetClient::new(&e, &usdc);
    let usdc_token = TokenClient::new(&e, &usdc);

    let blend_fixture = BlendFixture::deploy(&e, &bombadil, &blnd, &usdc);
    let pool_address = blend_fixture.pool_factory.deploy(
        &bombadil,
        &String::from_str(&e, "test"),
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
    let bootstrap_amount = 100_000 * SCALAR_7;
    blnd_client.mint(&frodo, &bootstrap_amount);
    let config = BootstrapConfig {
        pair_min: 2000 * SCALAR_7,
        close_ledger: e.ledger().sequence() + 3 * ONE_DAY_LEDGERS,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 0,
    };
    let id = bootstrap_client.bootstrap(&config);
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));

    // join samwise 60% of total
    let join_amount_samwise = 1500 * SCALAR_7;
    usdc_client.mint(&samwise, &join_amount_samwise);
    bootstrap_client.join(&samwise, &id, &join_amount_samwise);
    assert_eq!(join_amount_samwise, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&samwise));

    // join pippin 30% of total
    let join_amount_pippin = 750 * SCALAR_7;
    usdc_client.mint(&pippin, &join_amount_pippin);
    bootstrap_client.join(&pippin, &id, &join_amount_pippin);
    assert_eq!(
        join_amount_samwise + join_amount_pippin,
        usdc_token.balance(&bootstrapper)
    );
    assert_eq!(0, usdc_token.balance(&pippin));

    // join merry 10% of total
    let join_amount_merry = 250 * SCALAR_7;
    usdc_client.mint(&merry, &join_amount_merry);
    bootstrap_client.join(&merry, &id, &join_amount_merry);
    assert_eq!(
        join_amount_samwise + join_amount_pippin + join_amount_merry,
        usdc_token.balance(&bootstrapper)
    );
    assert_eq!(0, usdc_token.balance(&merry));

    // close bootstrap
    let est_backstop_tokens = est_close_mint(
        bootstrap_amount,
        join_amount_samwise + join_amount_pippin + join_amount_merry,
        comet_blnd,
        comet_usdc,
        comet_shares,
    );
    e.jump(3 * ONE_DAY_LEDGERS + 1);
    let backstop_tokens = bootstrap_client.close(&id);
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
    bootstrap_client.claim(&frodo, &id);
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
        .unwrap()
        .fixed_mul_floor(0_6000000, SCALAR_7)
        .unwrap();
    bootstrap_client.claim(&samwise, &id);
    assert_approx_eq_abs(
        est_samwise,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &samwise)
            .shares,
        MAX_DUST_AMOUNT,
    );

    let est_pippin = backstop_tokens
        .fixed_mul_floor(0_2000000, SCALAR_7)
        .unwrap()
        .fixed_mul_floor(0_3000000, SCALAR_7)
        .unwrap();
    bootstrap_client.claim(&pippin, &id);
    assert_approx_eq_abs(
        est_pippin,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &pippin)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // wait a long time to ensure merry can still claim
    e.jump(15 * ONE_DAY_LEDGERS);
    let est_merry = backstop_tokens
        .fixed_mul_floor(0_2000000, SCALAR_7)
        .unwrap()
        .fixed_mul_floor(0_1000000, SCALAR_7)
        .unwrap();
    bootstrap_client.claim(&merry, &id);
    assert_approx_eq_abs(
        est_merry,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &merry)
            .shares,
        MAX_DUST_AMOUNT,
    );

    assert_approx_eq_abs(
        0,
        blend_fixture.backstop_token.balance(&bootstrapper),
        MAX_DUST_AMOUNT,
    );
}

#[test]
fn test_claim_twice() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.mock_all_auths();
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
    let pool_address = blend_fixture.pool_factory.deploy(
        &bombadil,
        &String::from_str(&e, "test"),
        &BytesN::<32>::random(&e),
        &Address::generate(&e),
        &0,
        &2,
    );

    let bootstrapper = testutils::create_bootstrapper(&e, &blend_fixture);
    let bootstrap_client = BackstopBootstrapperClient::new(&e, &bootstrapper);

    // create bootstrap
    let bootstrap_amount = 100_000 * SCALAR_7;
    blnd_client.mint(&frodo, &bootstrap_amount);
    let config = BootstrapConfig {
        pair_min: 2000 * SCALAR_7,
        close_ledger: e.ledger().sequence() + 3 * ONE_DAY_LEDGERS,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 0,
    };
    let id = bootstrap_client.bootstrap(&config);
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));

    // join samwise 60% of total
    let join_amount_samwise = 2000 * SCALAR_7;
    usdc_client.mint(&samwise, &join_amount_samwise);
    bootstrap_client.join(&samwise, &id, &join_amount_samwise);

    e.jump(3 * ONE_DAY_LEDGERS + 1);
    let backstop_tokens = bootstrap_client.close(&id);

    let est_frodo = backstop_tokens
        .fixed_mul_floor(0_8000000, SCALAR_7)
        .unwrap();
    bootstrap_client.claim(&frodo, &id);
    assert_approx_eq_abs(
        est_frodo,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &frodo)
            .shares,
        MAX_DUST_AMOUNT,
    );

    let result = bootstrap_client.try_claim(&frodo, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(105))));

    let est_samwise = backstop_tokens
        .fixed_mul_floor(0_2000000, SCALAR_7)
        .unwrap()
        .fixed_mul_floor(1_0000000, SCALAR_7)
        .unwrap();
    bootstrap_client.claim(&samwise, &id);
    assert_approx_eq_abs(
        est_samwise,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &samwise)
            .shares,
        MAX_DUST_AMOUNT,
    );
    let result = bootstrap_client.try_claim(&samwise, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(105))));
}
