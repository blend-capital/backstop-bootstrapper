// use soroban_sdk::{Address, Env};
// use team_vesting_lockup::{StandardLockupContract, StandardLockupContractClient};

// mod backstop_bootstrapper_wasm {
//     soroban_sdk::contractimport!(
//         file = "../target/wasm32-unknown-unknown/optimized/backstop_bootstrapper.wasm"
//     );
// }

// /// Create a vesting lockup contract with the wasm contract
// ///
// /// ### Arguments
// /// * `admin` - The address of the admin
// /// * `owner` - The address of the owner
// pub fn create_vesting_lockup<'a>(
//     e: &Env,
//     admin: &Address,
//     owner: &Address,
// ) -> (Address, StandardLockupContractClient<'a>) {
//     let vesting_lockup_address = e.register_contract(None, StandardLockupContract {});
//     let vesting_lockup_client: StandardLockupContractClient<'a> =
//         StandardLockupContractClient::new(&e, &vesting_lockup_address);
//     vesting_lockup_client.initialize(admin, owner);
//     (vesting_lockup_address, vesting_lockup_client)
// }

// /// Create a vesting lockup contract with the wasm contract
// ///
// /// ### Arguments
// /// * `admin` - The address of the admin
// /// * `owner` - The address of the owner
// pub fn create_vesting_lockup_wasm<'a>(
//     e: &Env,
//     admin: &Address,
//     owner: &Address,
// ) -> (Address, StandardLockupContractClient<'a>) {
//     let vesting_lockup_address = e.register_contract_wasm(None, vesting_lockup_wasm::WASM);
//     let vesting_lockup_client: StandardLockupContractClient<'a> =
//         StandardLockupContractClient::new(&e, &vesting_lockup_address);
//     vesting_lockup_client.initialize(admin, owner);
//     (vesting_lockup_address, vesting_lockup_client)
// }
