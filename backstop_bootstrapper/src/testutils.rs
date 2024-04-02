#![cfg(test)]

use crate::{
    constants::SCALAR_7,
    dependencies::{BackstopClient, BackstopContract, CometClient, CometContract},
    storage,
    types::TokenInfo,
    BackstopBootstrapperContract,
};
use soroban_sdk::{
    map, testutils::Address as _, token::StellarAssetClient, token::TokenClient, vec, Address, Env,
};

use mock_pool_factory::{MockPoolFactory, MockPoolFactoryClient};

pub(crate) fn create_bootstrapper(e: &Env) -> Address {
    e.register_contract(None, BackstopBootstrapperContract {})
}

pub(crate) fn setup_bootstrapper<'a>(
    e: &Env,
    bootstrapper_address: &Address,
    pool_address: &Address,
    backstop: &Address,
    admin: &Address,
    blnd: &Address,
    usdc: &Address,
) -> CometClient<'a> {
    let comet = create_comet_lp_pool(e, admin, &blnd, &usdc);
    setup_backstop(e, pool_address, &backstop, &comet.0, &usdc, &blnd);
    e.as_contract(bootstrapper_address, || {
        storage::set_is_init(e);
        storage::set_backstop(e, backstop.clone());
        storage::set_backstop_token(e, comet.0);
    });
    comet.1
}

//************************************************
//           External Contract Helpers
//************************************************

// ***** Token *****

pub(crate) fn create_token_contract<'a>(
    e: &Env,
    admin: &Address,
) -> (Address, StellarAssetClient<'a>) {
    let contract_address = e.register_stellar_asset_contract(admin.clone());
    let client = StellarAssetClient::new(e, &contract_address);
    (contract_address, client)
}

pub(crate) fn create_blnd_token<'a>(
    e: &Env,
    bootstrapper_address: &Address,
    admin: &Address,
) -> (Address, StellarAssetClient<'a>) {
    let (contract_address, client) = create_token_contract(e, admin);
    e.as_contract(bootstrapper_address, || {
        storage::set_comet_token_data(
            e,
            0,
            TokenInfo {
                address: contract_address.clone(),
                weight: 800_0000,
            },
        );
    });
    (contract_address, client)
}

pub(crate) fn create_usdc_token<'a>(
    e: &Env,
    bootstrapper_address: &Address,
    admin: &Address,
) -> (Address, StellarAssetClient<'a>) {
    let (contract_address, client) = create_token_contract(e, admin);

    e.as_contract(bootstrapper_address, || {
        storage::set_comet_token_data(
            e,
            0,
            TokenInfo {
                address: contract_address.clone(),
                weight: 200_0000,
            },
        );
    });
    (contract_address, client)
}

//***** Pool Factory ******

pub(crate) fn create_mock_pool_factory(e: &Env) -> (Address, MockPoolFactoryClient) {
    let contract_address = e.register_contract(None, MockPoolFactory {});
    (
        contract_address.clone(),
        MockPoolFactoryClient::new(e, &contract_address),
    )
}

//***** Backstop ******

mod emitter {
    soroban_sdk::contractimport!(
        file = "../../blend-contracts/target/wasm32-unknown-unknown/release/emitter.wasm"
    );
}

pub(crate) fn create_emitter<'a>(
    e: &Env,
    backstop_id: &Address,
    backstop_token: &Address,
    blnd_token: &Address,
) -> (Address, emitter::Client<'a>) {
    let contract_address = e.register_contract_wasm(None, emitter::WASM);
    let client = emitter::Client::new(e, &contract_address);
    client.initialize(blnd_token, backstop_id, backstop_token);
    (contract_address.clone(), client)
}

pub(crate) fn create_backstop(e: &Env) -> (Address, BackstopClient) {
    let contract_address = e.register_contract_wasm(&Address::generate(&e), BackstopContract);
    (
        contract_address.clone(),
        BackstopClient::new(e, &contract_address),
    )
}

pub(crate) fn setup_backstop(
    e: &Env,
    pool_address: &Address,
    backstop_address: &Address,
    backstop_token: &Address,
    usdc_token: &Address,
    blnd_token: &Address,
) {
    let (pool_factory, mock_pool_factory_client) = create_mock_pool_factory(e);
    mock_pool_factory_client.set_pool(pool_address);
    let (emitter, _) = create_emitter(e, backstop_address, backstop_token, blnd_token);
    let backstop_client: BackstopClient = BackstopClient::new(e, backstop_address);

    backstop_client.initialize(
        backstop_token,
        &emitter,
        usdc_token,
        blnd_token,
        &pool_factory,
        &map![e, (pool_address.clone(), 50_000_000 * SCALAR_7)],
    );
}

/// Deploy a test Comet LP pool of 80% BLND / 20% USDC and set it as the backstop token.
///
/// Initializes the pool with the following settings:
/// - Swap fee: 0.3%
/// - BLND: 1,000
/// - USDC: 25
/// - Shares: 100
pub(crate) fn create_comet_lp_pool<'a>(
    e: &Env,
    admin: &Address,
    blnd_token: &Address,
    usdc_token: &Address,
) -> (Address, CometClient<'a>) {
    let contract_address = Address::generate(e);
    e.register_contract_wasm(&contract_address, CometContract);
    let client = CometClient::new(e, &contract_address);

    let blnd_client = StellarAssetClient::new(e, blnd_token);
    let usdc_client = StellarAssetClient::new(e, usdc_token);
    blnd_client.mint(&admin, &2_000_0000000);
    usdc_client.mint(&admin, &2_000_0000000);
    let exp_ledger = e.ledger().sequence() + 100000;
    let blnd_client = TokenClient::new(e, blnd_token);
    blnd_client.approve(&admin, &contract_address, &2_000_0000000, &exp_ledger);
    let usdc_client = TokenClient::new(e, usdc_token);
    usdc_client.approve(&admin, &contract_address, &2_000_0000000, &exp_ledger);

    client.init(&Address::generate(e), &admin);
    client.bundle_bind(
        &vec![e, blnd_token.clone(), usdc_token.clone()],
        &vec![e, 1_000_0000000, 25_0000000],
        &vec![e, 8_0000000, 2_0000000],
    );

    client.set_swap_fee(&0_0030000, &admin);
    client.set_public_swap(&admin, &true);
    client.finalize();

    (contract_address, client)
}
