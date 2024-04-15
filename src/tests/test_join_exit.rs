#![cfg(test)]

use crate::constants::SCALAR_7;
use crate::storage::ONE_DAY_LEDGERS;
use crate::testutils::{self, EnvTestUtils};
use crate::types::BootstrapConfig;
use crate::BackstopBootstrapperClient;
use blend_contract_sdk::testutils::BlendFixture;
use soroban_sdk::testutils::{Address as _, BytesN as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, BytesN, Env, Error, Symbol};

#[test]
fn test_join_exit() {
    let e = Env::default();
    e.budget().reset_unlimited();
    e.mock_all_auths();
    e.set_default_info();

    let bombadil = Address::generate(&e);
    let frodo = Address::generate(&e);
    let samwise = Address::generate(&e);
    let pippin = Address::generate(&e);

    let blnd = e.register_stellar_asset_contract(bombadil.clone());
    let usdc = e.register_stellar_asset_contract(bombadil.clone());
    let blnd_client = StellarAssetClient::new(&e, &blnd);
    let blnd_token = TokenClient::new(&e, &blnd);
    let usdc_client = StellarAssetClient::new(&e, &usdc);
    let usdc_token = TokenClient::new(&e, &usdc);

    let blend_fixture = BlendFixture::deploy(&e, &bombadil, &blnd, &usdc);
    let pool_address = blend_fixture.pool_factory.deploy(
        &bombadil,
        &Symbol::new(&e, "test"),
        &BytesN::<32>::random(&e),
        &Address::generate(&e),
        &0,
        &2,
    );

    let bootstrapper = testutils::create_bootstrapper(&e, &blend_fixture);
    let bootstrap_client = BackstopBootstrapperClient::new(&e, &bootstrapper);

    // create bootstrap
    let initial_balance = 2 * 1000 * SCALAR_7;
    let bootstrap_amount = 1000 * SCALAR_7;
    blnd_client.mint(&frodo, &initial_balance);

    let pair_min = 10 * SCALAR_7;
    let duration = 2 * ONE_DAY_LEDGERS;
    let config = BootstrapConfig {
        pair_min,
        close_ledger: e.ledger().sequence() + duration,
        bootstrapper: frodo.clone(),
        pool: pool_address.clone(),
        amount: bootstrap_amount,
        token_index: 0,
    };
    let id = bootstrap_client.bootstrap(&config);
    assert_eq!(bootstrap_amount, blnd_token.balance(&bootstrapper));
    assert_eq!(
        initial_balance - bootstrap_amount,
        blnd_token.balance(&frodo)
    );

    // bootstrap active
    let init_bal_samwise = 1000 * SCALAR_7;
    let init_bal_pippin = 500 * SCALAR_7;
    let join_amount = 100 * SCALAR_7;
    usdc_client.mint(&samwise, &init_bal_samwise);
    usdc_client.mint(&pippin, &init_bal_pippin);
    bootstrap_client.join(&samwise, &id, &join_amount);
    bootstrap_client.join(&pippin, &id, &join_amount);
    assert_eq!(join_amount * 2, usdc_token.balance(&bootstrapper));
    assert_eq!(init_bal_samwise - join_amount, usdc_token.balance(&samwise));
    assert_eq!(init_bal_pippin - join_amount, usdc_token.balance(&pippin));

    e.jump(duration / 2);

    let exit_amount = 75 * SCALAR_7;
    bootstrap_client.exit(&samwise, &id, &exit_amount);
    assert_eq!(
        join_amount * 2 - exit_amount,
        usdc_token.balance(&bootstrapper)
    );
    assert_eq!(
        init_bal_samwise - join_amount + exit_amount,
        usdc_token.balance(&samwise)
    );

    let result = bootstrap_client.try_exit(&pippin, &id, &(join_amount + 1));
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(106))));

    let result = bootstrap_client.try_exit(&samwise, &id, &(-1));
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(8))));

    let result = bootstrap_client.try_join(&pippin, &id, &(-1));
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(8))));

    let join_2_amount = 15 * SCALAR_7;
    let exit_2_amount = 10 * SCALAR_7;
    bootstrap_client.join(&samwise, &id, &join_2_amount);
    bootstrap_client.exit(&pippin, &id, &exit_2_amount);
    let total_deposit = join_amount * 2 + join_2_amount - exit_amount - exit_2_amount;
    assert_eq!(total_deposit, usdc_token.balance(&bootstrapper));
    assert_eq!(
        init_bal_samwise - join_amount + exit_amount - join_2_amount,
        usdc_token.balance(&samwise)
    );
    assert_eq!(
        init_bal_pippin - join_amount + exit_2_amount,
        usdc_token.balance(&pippin)
    );

    // verify bootstrapper data
    let bootstrap = bootstrap_client.get_bootstrap(&id);
    assert_eq!(bootstrap.data.bootstrap_amount, config.amount);
    assert_eq!(bootstrap.data.total_pair, total_deposit);
    assert_eq!(bootstrap.data.pair_amount, total_deposit);

    // move bootstrap out of active and try and join / exit
    e.jump(duration / 2);

    let result = bootstrap_client.try_exit(&samwise, &id, &1);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(104))));

    let result = bootstrap_client.try_join(&pippin, &id, &1);
    assert_eq!(result.err(), Some(Ok(Error::from_contract_error(104))));
}
