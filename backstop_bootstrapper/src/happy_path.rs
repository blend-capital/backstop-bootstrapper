#![cfg(test)]
use std::println;

use crate::constants::SCALAR_7;
use crate::storage::ONE_DAY_LEDGERS;
use crate::testutils;
use backstop_bootstrapper_wasm::{Client as BootstrapClient, WASM as BootstrapWasm};
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use soroban_sdk::{Address, Env};
mod backstop_bootstrapper_wasm {
    soroban_sdk::contractimport!(
        file = "../target/wasm32-unknown-unknown/optimized/backstop_bootstrapper.wasm"
    );
}

/// Test user exposed functions on the backstop for basic functionality, auth, and events.
/// Does not test internal state management of the backstop, only external effects.
#[test]
fn test_bootstrapper() {
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
    let bootstrapper = e.register_contract_wasm(None, BootstrapWasm);

    let (backstop, _) = testutils::create_backstop(&e);
    let (blnd, blnd_client) = testutils::create_blnd_token(&e, &bootstrapper, &bombadil);
    let (usdc, usdc_client) = testutils::create_usdc_token(&e, &bootstrapper, &bombadil);
    e.budget().reset_unlimited();
    let (backstop_token, _) = testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
    let pool_factory =
        testutils::setup_backstop(&e, &pool_address, &backstop, &backstop_token, &usdc, &blnd);
    let bootstrap_client = BootstrapClient::new(&e, &bootstrapper);
    // init
    bootstrap_client.initialize(&backstop, &backstop_token, &pool_factory);
    // create bootstrap
    let bootstrap_amount = 100 * SCALAR_7;
    blnd_client.mint(&frodo, &(bootstrap_amount * 2));
    let pair_min = 10 * SCALAR_7;
    let duration = ONE_DAY_LEDGERS + 1;
    bootstrap_client.add_bootstrap(
        &frodo,
        &0,
        &bootstrap_amount,
        &pair_min,
        &duration,
        &pool_address,
    );
    // join bootstrap
    let samwise = Address::generate(&e);
    let join_amount = 50 * SCALAR_7;
    usdc_client.mint(&samwise, &join_amount);
    bootstrap_client.join(&samwise, &join_amount, &frodo, &0);
    // exit bootstrap
    let exit_amount = 25 * SCALAR_7;
    bootstrap_client.exit(&samwise, &exit_amount, &frodo, &0);
    // close bootstrap
    e.ledger().set(LedgerInfo {
        timestamp: 600,
        protocol_version: 20,
        sequence_number: 1234 + duration + 1,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 10,
        min_persistent_entry_ttl: 10,
        max_entry_ttl: 2000000,
    });
    println!("closing bootstrap");
    println!("usdc address: {:?}", usdc);
    println!("blnd address: {:?}", blnd);
    bootstrap_client.close_bootstrap(&samwise, &frodo, &0);
    bootstrap_client.close_bootstrap(&samwise, &frodo, &0);

    // claim
    bootstrap_client.claim(&frodo, &frodo, &0);
    bootstrap_client.claim(&samwise, &frodo, &0);
}
