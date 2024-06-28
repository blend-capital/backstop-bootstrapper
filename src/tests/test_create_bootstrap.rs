#![cfg(test)]

use crate::constants::SCALAR_7;
use crate::storage::ONE_DAY_LEDGERS;
use crate::testutils::{self, EnvTestUtils};
use crate::types::BootstrapConfig;
use crate::BackstopBootstrapperClient;
use blend_contract_sdk::testutils::BlendFixture;
use soroban_sdk::testutils::{Address as _, BytesN as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, BytesN, Env, Error, String};

#[test]
fn test_bootstrap_uses_next_id() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);

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

    // create BLND bootstrap
    let blnd_amount = 1000 * SCALAR_7;
    blnd_client.mint(&frodo, &blnd_amount);
    let config_1 = BootstrapConfig {
        pair_min: 10 * SCALAR_7,
        close_ledger: e.ledger().sequence() + ONE_DAY_LEDGERS + 10,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: blnd_amount,
        token_index: 0,
    };
    let id_1 = bootstrap_client.bootstrap(&config_1);

    // create USDC bootstrap
    let usdc_amount = 10 * SCALAR_7;
    usdc_client.mint(&frodo, &usdc_amount);
    let config_2 = BootstrapConfig {
        pair_min: 500 * SCALAR_7,
        close_ledger: e.ledger().sequence() + ONE_DAY_LEDGERS + 5,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: usdc_amount,
        token_index: 1,
    };
    let id_2 = bootstrap_client.bootstrap(&config_2);

    assert_eq!(blnd_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(0, blnd_token.balance(&frodo));
    assert_eq!(usdc_amount, usdc_token.balance(&bootstrapper));
    assert_eq!(0, usdc_token.balance(&frodo));
    let bootstrap_1 = bootstrap_client.get_bootstrap(&id_1);
    assert_eq!(id_1, bootstrap_1.id);
    assert_eq!(config_1.amount, bootstrap_1.config.amount);
    assert_eq!(config_1.bootstrapper, bootstrap_1.config.bootstrapper);
    assert_eq!(config_1.close_ledger, bootstrap_1.config.close_ledger);
    assert_eq!(config_1.pair_min, bootstrap_1.config.pair_min);
    assert_eq!(config_1.pool, bootstrap_1.config.pool);
    assert_eq!(config_1.token_index, bootstrap_1.config.token_index);
    let bootstrap_2 = bootstrap_client.get_bootstrap(&id_2);
    assert_eq!(id_2, bootstrap_2.id);
    assert_eq!(config_2.amount, bootstrap_2.config.amount);
    assert_eq!(config_2.bootstrapper, bootstrap_2.config.bootstrapper);
    assert_eq!(config_2.close_ledger, bootstrap_2.config.close_ledger);
    assert_eq!(config_2.pair_min, bootstrap_2.config.pair_min);
    assert_eq!(config_2.pool, bootstrap_2.config.pool);
    assert_eq!(config_2.token_index, bootstrap_2.config.token_index);
}

#[test]
fn test_bootstrap_validates_config() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);

    let blnd = e.register_stellar_asset_contract(bombadil.clone());
    let usdc = e.register_stellar_asset_contract(bombadil.clone());
    let blnd_client = StellarAssetClient::new(&e, &blnd);

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

    // base config
    let blnd_amount = 1000 * SCALAR_7;
    blnd_client.mint(&frodo, &blnd_amount);
    let config = BootstrapConfig {
        pair_min: 10 * SCALAR_7,
        close_ledger: e.ledger().sequence() + ONE_DAY_LEDGERS + 10,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: blnd_amount,
        token_index: 0,
    };

    // pair_min
    let mut config_pair_min = config.clone();
    config_pair_min.pair_min = -1;
    let result = bootstrap_client.try_bootstrap(&config_pair_min);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(8))));

    // close ledger
    let mut config_close_short = config.clone();
    config_close_short.close_ledger = e.ledger().sequence() + ONE_DAY_LEDGERS - 1;
    let result = bootstrap_client.try_bootstrap(&config_close_short);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(100))));

    let mut config_close_long = config.clone();
    config_close_long.close_ledger = e.ledger().sequence() + 14 * ONE_DAY_LEDGERS + 1;
    let result = bootstrap_client.try_bootstrap(&config_close_long);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(100))));

    let mut config_close_pre_ledger = config.clone();
    config_close_pre_ledger.close_ledger = e.ledger().sequence() - 1;
    let result = bootstrap_client.try_bootstrap(&config_close_long);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(100))));

    // pool
    let mut config_pool = config.clone();
    config_pool.pool = Address::generate(&e);
    let result = bootstrap_client.try_bootstrap(&config_pool);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(103))));

    // token index
    let mut config_token = config.clone();
    config_token.token_index = 2;
    let result = bootstrap_client.try_bootstrap(&config_token);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(101))));

    // verify original config works
    let id = bootstrap_client.bootstrap(&config);
    assert_eq!(id, 0);
}
