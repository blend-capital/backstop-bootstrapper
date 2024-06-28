#![cfg(test)]

use crate::constants::{MAX_DUST_AMOUNT, SCALAR_7};
use crate::storage::ONE_DAY_LEDGERS;
use crate::testutils::{self, assert_approx_eq_abs, EnvTestUtils};
use crate::types::BootstrapConfig;
use crate::BackstopBootstrapperClient;
use blend_contract_sdk::testutils::BlendFixture;
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::testutils::{Address as _, BytesN as _, MockAuth, MockAuthInvoke};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{vec, Address, BytesN, Env, Error, IntoVal, String};

// @dev: refund is omitted from the happy path test. Test auth.
#[test]
fn test_refund_blnd_bootstrap_after_ledger_and_auth() {
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
        &String::from_str(&e, "test"),
        &BytesN::<32>::random(&e),
        &Address::generate(&e),
        &0,
        &2,
    );
    let bootstrapper = testutils::create_bootstrapper(&e, &blend_fixture);
    let bootstrap_client = BackstopBootstrapperClient::new(&e, &bootstrapper);

    let bootstrap_amount = 1000 * SCALAR_7;
    blnd_client.mock_all_auths().mint(&frodo, &bootstrap_amount);
    let config = BootstrapConfig {
        pair_min: 1 * SCALAR_7,
        close_ledger: e.ledger().sequence() + ONE_DAY_LEDGERS,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 0,
    };
    let id = bootstrap_client.mock_all_auths().bootstrap(&config);
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));

    let join_amount = 25 * SCALAR_7;
    usdc_client.mock_all_auths().mint(&samwise, &join_amount);
    bootstrap_client
        .mock_all_auths()
        .join(&samwise, &id, &join_amount);
    assert_eq!(join_amount, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&samwise));

    // verify refund verifies status
    e.jump(ONE_DAY_LEDGERS + 1);

    let result = bootstrap_client.mock_all_auths().try_refund(&samwise, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(104))));

    // window for close expries
    e.jump(14 * ONE_DAY_LEDGERS);

    let result = bootstrap_client.mock_all_auths().try_close(&id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(104))));

    // refund bootstrapper
    e.set_auths(&[]);
    let refunded = bootstrap_client
        .mock_auths(&[MockAuth {
            address: &frodo,
            invoke: &MockAuthInvoke {
                contract: &bootstrapper,
                fn_name: &"refund",
                args: vec![&e, frodo.into_val(&e), id.into_val(&e)],
                sub_invokes: &[],
            },
        }])
        .refund(&frodo, &id);
    assert_eq!(0, blnd_token.balance(&bootstrapper));
    assert_eq!(bootstrap_amount, blnd_token.balance(&frodo));
    assert_eq!(refunded, bootstrap_amount);

    // refund joiner
    e.set_auths(&[]);
    let refunded = bootstrap_client
        .mock_auths(&[MockAuth {
            address: &samwise,
            invoke: &MockAuthInvoke {
                contract: &bootstrapper,
                fn_name: &"refund",
                args: vec![&e, samwise.into_val(&e), id.into_val(&e)],
                sub_invokes: &[],
            },
        }])
        .refund(&samwise, &id);
    assert_eq!(0, usdc_token.balance(&bootstrapper));
    assert_eq!(join_amount, usdc_token.balance(&samwise));
    assert_eq!(refunded, join_amount);

    // verify bootstrapper and joiner can't refund again
    let result = bootstrap_client.mock_all_auths().try_refund(&frodo, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));

    let result = bootstrap_client.mock_all_auths().try_refund(&samwise, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));
}

#[test]
fn test_refund_pair_after_partial_close() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.set_default_info();
    e.mock_all_auths_allowing_non_root_auth();

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

    let bootstrap_amount = 1000 * SCALAR_7;
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
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));

    let join_amount = 25000000 * SCALAR_7;
    usdc_client.mint(&samwise, &join_amount);
    bootstrap_client.join(&samwise, &id, &join_amount);
    assert_eq!(join_amount, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&samwise));

    // partial close
    e.jump(ONE_DAY_LEDGERS + 1);
    bootstrap_client.close(&id);
    let backstop_tokens = blend_fixture
        .backstop_token
        .balance(&bootstrap_client.address);

    // window for close expries
    e.jump(14 * ONE_DAY_LEDGERS);

    // claim bootstrapper
    let claim_amount = backstop_tokens
        .fixed_mul_floor(800_0000 as i128, SCALAR_7)
        .unwrap();
    let claimed = bootstrap_client.claim(&frodo, &id);
    assert_eq!(claim_amount, claimed);
    assert_approx_eq_abs(
        claim_amount,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &frodo)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // refund joiner
    let usdc_balance = usdc_token.balance(&bootstrapper);
    let refunded = bootstrap_client.refund(&samwise, &id);
    assert_approx_eq_abs(0, usdc_token.balance(&bootstrapper), MAX_DUST_AMOUNT);
    assert_approx_eq_abs(usdc_balance, usdc_token.balance(&samwise), MAX_DUST_AMOUNT);
    assert_approx_eq_abs(refunded, usdc_balance, MAX_DUST_AMOUNT);

    // claim joiner
    let claim_amount = backstop_tokens
        .fixed_mul_floor(200_0000 as i128, SCALAR_7)
        .unwrap();
    let claimed = bootstrap_client.claim(&samwise, &id);
    assert_eq!(claim_amount, claimed);
    assert_approx_eq_abs(
        claim_amount,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &samwise)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // verify bootstrapper refund is 0
    let result = bootstrap_client.refund(&frodo, &id);
    assert_eq!(result, 0);

    let result = bootstrap_client.try_refund(&samwise, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));
}

#[test]
fn test_refund_pair_after_partial_close_multiple_joiners() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.set_default_info();
    e.mock_all_auths_allowing_non_root_auth();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);
    let samwise = Address::generate(&e);
    let pippin = Address::generate(&e);
    let sauron = Address::generate(&e);

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

    let bootstrap_amount = 1000 * SCALAR_7;
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
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));

    let join_amount_samwise = 20000000 * SCALAR_7;
    usdc_client.mint(&samwise, &join_amount_samwise);
    bootstrap_client.join(&samwise, &id, &join_amount_samwise);
    assert_eq!(join_amount_samwise, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&samwise));

    let join_amount_pippin = 5000000 * SCALAR_7;
    usdc_client.mint(&pippin, &join_amount_pippin);
    bootstrap_client.join(&pippin, &id, &join_amount_pippin);
    assert_eq!(
        join_amount_pippin + join_amount_samwise,
        usdc_token.balance(&bootstrapper)
    );
    assert_eq!(0, usdc_token.balance(&pippin));

    let share_samwise = join_amount_samwise
        .fixed_div_floor(join_amount_pippin + join_amount_samwise, SCALAR_7)
        .unwrap();
    let share_pippin = join_amount_pippin
        .fixed_div_floor(join_amount_pippin + join_amount_samwise, SCALAR_7)
        .unwrap();

    // partial close
    e.jump(ONE_DAY_LEDGERS + 1);
    bootstrap_client.close(&id);
    let backstop_tokens = blend_fixture
        .backstop_token
        .balance(&bootstrap_client.address);
    let claim_joiners = backstop_tokens
        .fixed_mul_floor(200_0000 as i128, SCALAR_7)
        .unwrap();
    let refund_joiners = usdc_token.balance(&bootstrapper);

    // window for close expries
    e.jump(14 * ONE_DAY_LEDGERS);

    // refund non-joiner
    let result = bootstrap_client.refund(&sauron, &id);
    assert_eq!(result, 0);
    assert_eq!(0, usdc_token.balance(&sauron));

    // claim pippin
    let claim_amount_pippin = claim_joiners
        .fixed_mul_floor(share_pippin, SCALAR_7)
        .unwrap();
    let claimed_pippin = bootstrap_client.claim(&pippin, &id);
    assert_approx_eq_abs(claim_amount_pippin, claimed_pippin, MAX_DUST_AMOUNT);
    assert_approx_eq_abs(
        claim_amount_pippin,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &pippin)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // claim bootstrapper
    let claim_amount = backstop_tokens
        .fixed_mul_floor(800_0000 as i128, SCALAR_7)
        .unwrap();
    let claimed = bootstrap_client.claim(&frodo, &id);
    assert_approx_eq_abs(claim_amount, claimed, MAX_DUST_AMOUNT);
    assert_approx_eq_abs(
        claim_amount,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &frodo)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // refund samwise
    let refund_samwise = refund_joiners
        .fixed_mul_floor(share_samwise, SCALAR_7)
        .unwrap();
    let refunded_samwise = bootstrap_client.refund(&samwise, &id);
    assert_approx_eq_abs(
        refund_joiners - refunded_samwise,
        usdc_token.balance(&bootstrapper),
        MAX_DUST_AMOUNT,
    );
    assert_approx_eq_abs(
        refund_samwise,
        usdc_token.balance(&samwise),
        MAX_DUST_AMOUNT,
    );
    assert_approx_eq_abs(refund_samwise, refunded_samwise, MAX_DUST_AMOUNT);

    // claim samwise
    let claim_amount_samwise = claim_joiners
        .fixed_mul_floor(share_samwise, SCALAR_7)
        .unwrap();
    let claimed_samwise = bootstrap_client.claim(&samwise, &id);
    assert_approx_eq_abs(claim_amount_samwise, claimed_samwise, MAX_DUST_AMOUNT);
    assert_approx_eq_abs(
        claim_amount_samwise,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &samwise)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // refund pippin
    let refund_pippin = refund_joiners
        .fixed_mul_floor(share_pippin, SCALAR_7)
        .unwrap();
    let refunded_pippin = bootstrap_client.refund(&pippin, &id);
    assert_approx_eq_abs(0, usdc_token.balance(&bootstrapper), MAX_DUST_AMOUNT);
    assert_approx_eq_abs(
        refunded_pippin,
        usdc_token.balance(&pippin),
        MAX_DUST_AMOUNT,
    );
    assert_approx_eq_abs(refund_pippin, refunded_pippin, MAX_DUST_AMOUNT);

    // verify bootstrapper refund is 0
    let result = bootstrap_client.refund(&frodo, &id);
    assert_eq!(result, 0);

    let result = bootstrap_client.try_refund(&samwise, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));

    let result = bootstrap_client.try_refund(&pippin, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));
}

#[test]
fn test_refund_bootstrap_after_partial_close() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.set_default_info();
    e.mock_all_auths_allowing_non_root_auth();

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

    let bootstrap_amount = 10000000 * SCALAR_7;
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
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));

    let join_amount = 25 * SCALAR_7;
    usdc_client.mint(&samwise, &join_amount);
    bootstrap_client.join(&samwise, &id, &join_amount);
    assert_eq!(join_amount, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&samwise));

    // partial close
    e.jump(ONE_DAY_LEDGERS + 1);
    bootstrap_client.close(&id);
    let backstop_tokens = blend_fixture
        .backstop_token
        .balance(&bootstrap_client.address);

    // window for close expries
    e.jump(14 * ONE_DAY_LEDGERS);

    // claim bootstrapper
    let claim_amount = backstop_tokens
        .fixed_mul_floor(800_0000 as i128, SCALAR_7)
        .unwrap();
    let claimed = bootstrap_client.claim(&frodo, &id);
    assert_eq!(claim_amount, claimed);
    assert_approx_eq_abs(
        claim_amount,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &frodo)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // refund bootstrapper
    let blnd_balance = blnd_token.balance(&bootstrapper);
    let refunded = bootstrap_client.refund(&frodo, &id);
    assert_approx_eq_abs(0, blnd_token.balance(&bootstrapper), MAX_DUST_AMOUNT);
    assert_approx_eq_abs(blnd_balance, blnd_token.balance(&frodo), MAX_DUST_AMOUNT);
    assert_approx_eq_abs(refunded, blnd_balance, MAX_DUST_AMOUNT);

    // claim joiner
    let claim_amount = backstop_tokens
        .fixed_mul_floor(200_0000 as i128, SCALAR_7)
        .unwrap();
    let claimed = bootstrap_client.claim(&samwise, &id);
    assert_eq!(claim_amount, claimed);
    assert_approx_eq_abs(
        claim_amount,
        blend_fixture
            .backstop
            .user_balance(&pool_address, &samwise)
            .shares,
        MAX_DUST_AMOUNT,
    );

    // verify bootstrapper and joiner can't refund again
    let result = bootstrap_client.try_refund(&frodo, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));

    let result = bootstrap_client.refund(&samwise, &id);
    assert_eq!(result, 0);
}

#[test]
fn test_refund_usdc_bootstrap_invalid_pair_amount_and_multiple_joiners() {
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
    let bootstrapper = testutils::create_bootstrapper(&e, &blend_fixture);
    let bootstrap_client = BackstopBootstrapperClient::new(&e, &bootstrapper);

    let bootstrap_amount = 50 * SCALAR_7;
    usdc_client.mint(&frodo, &bootstrap_amount);
    let config = BootstrapConfig {
        pair_min: 1000 * SCALAR_7,
        close_ledger: e.ledger().sequence() + ONE_DAY_LEDGERS,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 1,
    };
    let id = bootstrap_client.bootstrap(&config);

    let join_amount_samwise = 225 * SCALAR_7;
    blnd_client.mint(&samwise, &join_amount_samwise);
    bootstrap_client.join(&samwise, &id, &join_amount_samwise);

    let join_amount_pippin = 150 * SCALAR_7;
    blnd_client.mint(&pippin, &join_amount_pippin);
    bootstrap_client.join(&pippin, &id, &join_amount_pippin);

    let join_amount_merry = 450 * SCALAR_7;
    blnd_client.mint(&merry, &join_amount_merry);
    bootstrap_client.join(&merry, &id, &join_amount_merry);

    e.jump(ONE_DAY_LEDGERS + 1);

    // refund all participants
    let refunded_samwise = bootstrap_client.refund(&samwise, &id);
    assert_eq!(join_amount_samwise, blnd_token.balance(&samwise));
    assert_eq!(
        join_amount_pippin + join_amount_merry,
        blnd_token.balance(&bootstrapper)
    );
    assert_eq!(refunded_samwise, join_amount_samwise);

    let refunded_pippin = bootstrap_client.refund(&pippin, &id);
    assert_eq!(join_amount_pippin, blnd_token.balance(&pippin));
    assert_eq!(join_amount_merry, blnd_token.balance(&bootstrapper));
    assert_eq!(refunded_pippin, join_amount_pippin);

    let refunded_merry = bootstrap_client.refund(&merry, &id);
    assert_eq!(join_amount_merry, blnd_token.balance(&merry));
    assert_eq!(0, blnd_token.balance(&bootstrapper));
    assert_eq!(refunded_merry, join_amount_merry);

    // wait a long time and refund bootstrapper
    e.jump(15 * ONE_DAY_LEDGERS);
    let refunded_frodo = bootstrap_client.refund(&frodo, &id);
    assert_eq!(0, usdc_token.balance(&bootstrapper));
    assert_eq!(bootstrap_amount, usdc_token.balance(&frodo));
    assert_eq!(refunded_frodo, bootstrap_amount);
}

#[test]
fn test_refund_twice() {
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
        &String::from_str(&e, "test"),
        &BytesN::<32>::random(&e),
        &Address::generate(&e),
        &0,
        &2,
    );
    let bootstrapper = testutils::create_bootstrapper(&e, &blend_fixture);
    let bootstrap_client = BackstopBootstrapperClient::new(&e, &bootstrapper);

    let bootstrap_amount = 1000 * SCALAR_7;
    blnd_client.mock_all_auths().mint(&frodo, &bootstrap_amount);
    let config = BootstrapConfig {
        pair_min: 100 * SCALAR_7,
        close_ledger: e.ledger().sequence() + ONE_DAY_LEDGERS,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 0,
    };
    let id = bootstrap_client.mock_all_auths().bootstrap(&config);
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));

    let join_amount = 50 * SCALAR_7;
    usdc_client.mock_all_auths().mint(&samwise, &join_amount);
    bootstrap_client
        .mock_all_auths()
        .join(&samwise, &id, &join_amount);
    assert_eq!(join_amount, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&samwise));

    e.jump(ONE_DAY_LEDGERS + 1);
    let refunded = bootstrap_client.mock_all_auths().refund(&frodo, &id);
    assert_eq!(bootstrap_amount, refunded);
    assert_eq!(0, blnd_token.balance(&bootstrapper));
    assert_eq!(bootstrap_amount, blnd_token.balance(&frodo));

    // Mint bootstrapper tokens so a double refund can be attempted
    blnd_client
        .mock_all_auths()
        .mint(&bootstrapper, &bootstrap_amount);
    let result = bootstrap_client.mock_all_auths().try_refund(&frodo, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));

    let refunded = bootstrap_client.mock_all_auths().refund(&samwise, &id);
    assert_eq!(join_amount, refunded);
    assert_eq!(0, usdc_token.balance(&bootstrapper));
    assert_eq!(join_amount, usdc_token.balance(&samwise));

    // Mint bootstrapper tokens so a double refund can be attempted
    usdc_client
        .mock_all_auths()
        .mint(&bootstrapper, &join_amount);
    let result = bootstrap_client.mock_all_auths().try_refund(&samwise, &id);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(108))));
}
