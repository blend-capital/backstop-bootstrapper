#![cfg(test)]

use crate::constants::{MAX_DUST_AMOUNT, SCALAR_7};
use crate::storage::ONE_DAY_LEDGERS;
use crate::testutils::{
    self, assert_approx_eq_abs, assert_approx_eq_rel, est_close_mint, EnvTestUtils,
};
use crate::types::BootstrapConfig;
use crate::BackstopBootstrapperClient;
use blend_contract_sdk::testutils::BlendFixture;
use soroban_sdk::testutils::{Address as _, BytesN as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, BytesN, Env, String};

#[test]
fn test_frontrunning_not_effective() {
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
    // comet setup
    // -> 2m BLND
    // -> 50k USDC
    // -> 200k shares
    // -> (10 BLND, 0.25 USDC per share)
    let comet_shares = blend_fixture.backstop_token.get_total_supply();
    let comet_blnd = blnd_token.balance(&blend_fixture.backstop_token.address);
    let comet_usdc = usdc_token.balance(&blend_fixture.backstop_token.address);

    let bootstrapper = testutils::create_bootstrapper(&e, &blend_fixture);
    let bootstrap_client = BackstopBootstrapperClient::new(&e, &bootstrapper);

    // create bootstrap
    // blnd -> 50k
    // uscd -> 1k
    // approx 10k extra blend
    let bootstrap_amount = 50000 * SCALAR_7;
    blnd_client.mint(&frodo, &bootstrap_amount);

    let config = BootstrapConfig {
        pair_min: 1 * SCALAR_7,
        close_ledger: e.ledger().sequence() + ONE_DAY_LEDGERS,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 0,
    };
    let id = bootstrap_client.bootstrap(&config);

    // join bootstrap
    let join_amount = 1000 * SCALAR_7;
    usdc_client.mint(&samwise, &join_amount);
    bootstrap_client.join(&samwise, &id, &join_amount);

    // close bootstrap
    e.jump(ONE_DAY_LEDGERS + 1);
    let est_backstop_tokens = est_close_mint(
        bootstrap_amount,
        join_amount,
        comet_blnd,
        comet_usdc,
        comet_shares,
    );
    //frontrun close
    usdc_client.mint(&frodo, &(50000 * &SCALAR_7));
    blend_fixture.backstop_token.swap_exact_amount_in(
        &usdc,
        &(16666 * &SCALAR_7),
        &blnd,
        &0,
        &(333000 * SCALAR_7),
        &frodo,
    );
    blend_fixture.backstop_token.swap_exact_amount_in(
        &usdc,
        &(16666 * &SCALAR_7),
        &blnd,
        &0,
        &(333000 * SCALAR_7),
        &frodo,
    );
    blend_fixture.backstop_token.swap_exact_amount_in(
        &usdc,
        &(16666 * &SCALAR_7),
        &blnd,
        &0,
        &(333000 * SCALAR_7),
        &frodo,
    );
    let backstop_tokens = bootstrap_client.close(&id);
    assert_approx_eq_abs(0, blnd_token.balance(&bootstrapper), MAX_DUST_AMOUNT);
    assert_approx_eq_abs(0, usdc_token.balance(&bootstrapper), MAX_DUST_AMOUNT);
    assert_eq!(
        backstop_tokens,
        blend_fixture.backstop_token.balance(&bootstrapper)
    );
    // at most 8% slippage on close
    assert_approx_eq_rel(est_backstop_tokens, backstop_tokens, 0_0800000);
}
