use crate::constants::{
    build_block_context, build_invoke_transaction, TEST_ACCOUNT_CONTRACT_ADDRESS,
};
use crate::state::DictStateReader;
use crate::{cheatcodes::EnhancedHintError, CheatnetState};
use anyhow::{Context, Result};
use blockifier::abi::abi_utils::selector_from_name;
use blockifier::execution::execution_utils::felt_to_stark_felt;

use blockifier::execution::entry_point::CallInfo;
use blockifier::state::cached_state::CachedState;
use blockifier::state::state_api::StateReader;
use blockifier::transaction::account_transaction::AccountTransaction;
use blockifier::transaction::transactions::{ExecutableTransaction, InvokeTransaction};
use cairo_felt::Felt252;

use starknet_api::core::{ClassHash, ContractAddress, EntryPointSelector, PatriciaKey};
use starknet_api::hash::{StarkFelt, StarkHash};
use starknet_api::transaction::{
    Calldata, ContractAddressSalt, InvokeTransactionV1, TransactionHash,
};
use starknet_api::{patricia_key, stark_felt};

use super::CheatcodeError;
use crate::conversions::felt_from_short_string;
use crate::panic_data::try_extract_panic_data;

impl CheatnetState {
    pub fn deploy(
        &mut self,
        class_hash: &ClassHash,
        calldata: &[Felt252],
    ) -> Result<ContractAddress, CheatcodeError> {
        // Deploy a contract using syscall deploy.
        let account_address = ContractAddress(patricia_key!(TEST_ACCOUNT_CONTRACT_ADDRESS));
        let block_context = build_block_context();
        let entry_point_selector = selector_from_name("deploy_contract");
        let salt = self.get_salt();
        self.increment_deploy_salt_base();

        let blockifier_state: &mut CachedState<DictStateReader> = &mut self.blockifier_state;

        let contract_class = blockifier_state
            .get_compiled_contract_class(class_hash)
            .map_err::<EnhancedHintError, _>(From::from)?;
        if contract_class.constructor_selector().is_none() && !calldata.is_empty() {
            return Err(CheatcodeError::Recoverable(vec![felt_from_short_string(
                "No constructor in contract",
            )]));
        }

        let execute_calldata = create_execute_calldata(
            calldata,
            class_hash,
            &account_address,
            &entry_point_selector,
            &salt,
        );

        let nonce = blockifier_state
            .get_nonce_at(account_address)
            .context("Failed to get nonce")
            .map_err::<EnhancedHintError, _>(From::from)?;
        let tx = build_invoke_transaction(execute_calldata, account_address);
        let tx = InvokeTransactionV1 { nonce, ..tx };
        let account_tx = AccountTransaction::Invoke(InvokeTransaction {
            tx: starknet_api::transaction::InvokeTransaction::V1(tx),
            tx_hash: TransactionHash::default(), // TODO(#358): Check if this is legit
        });

        let tx_info = account_tx
            .execute(blockifier_state, &block_context, true, true)
            .unwrap_or_else(|e| panic!("Unparseable transaction error: {e:?}"));

        if let Some(CallInfo { execution, .. }) = tx_info.execute_call_info {
            let contract_address = execution
                .retdata
                .0
                .get(0)
                .expect("Failed to get contract_address from return_data");

            let contract_address = ContractAddress::try_from(*contract_address)
                .expect("Failed to cast contract address into the right struct");

            return Ok(contract_address);
        }

        let revert_error = tx_info
            .revert_error
            .expect("Unparseable tx info, {tx_info:?}");
        let extracted_panic_data = try_extract_panic_data(&revert_error)
            .expect("Unparseable error message, {revert_error}");

        Err(CheatcodeError::Recoverable(extracted_panic_data))
    }
}

fn create_execute_calldata(
    calldata: &[Felt252],
    class_hash: &ClassHash,
    account_address: &ContractAddress,
    entry_point_selector: &EntryPointSelector,
    salt: &ContractAddressSalt,
) -> Calldata {
    let calldata_len = u128::try_from(calldata.len()).unwrap();
    let mut execute_calldata = vec![
        *account_address.0.key(),      // Contract address.
        entry_point_selector.0,        // EP selector.
        stark_felt!(calldata_len + 3), // Calldata length.
        class_hash.0,                  // Calldata: class_hash.
        salt.0,                        // Contract_address_salt.
        stark_felt!(calldata_len),     // Constructor calldata length.
    ];
    let mut calldata: Vec<StarkFelt> = calldata.iter().map(felt_to_stark_felt).collect();
    execute_calldata.append(&mut calldata);
    Calldata(execute_calldata.into())
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn execute_calldata() {
        let calldata = create_execute_calldata(
            &[Felt252::from(100), Felt252::from(200)],
            &ClassHash(StarkFelt::from(123_u32)),
            &ContractAddress::try_from(StarkFelt::from(111_u32)).unwrap(),
            &EntryPointSelector(StarkFelt::from(222_u32)),
            &ContractAddressSalt(StarkFelt::from(333_u32)),
        );
        assert_eq!(
            calldata,
            Calldata(Arc::new(vec![
                StarkFelt::from(111_u32),
                StarkFelt::from(222_u32),
                StarkFelt::from(5_u32),
                StarkFelt::from(123_u32),
                StarkFelt::from(333_u32),
                StarkFelt::from(2_u32),
                StarkFelt::from(100_u32),
                StarkFelt::from(200_u32),
            ]))
        );
    }

    #[test]
    fn execute_calldata_no_entrypoint_calldata() {
        let calldata = create_execute_calldata(
            &[],
            &ClassHash(StarkFelt::from(123_u32)),
            &ContractAddress::try_from(StarkFelt::from(111_u32)).unwrap(),
            &EntryPointSelector(StarkFelt::from(222_u32)),
            &ContractAddressSalt(StarkFelt::from(333_u32)),
        );
        assert_eq!(
            calldata,
            Calldata(Arc::new(vec![
                StarkFelt::from(111_u32),
                StarkFelt::from(222_u32),
                StarkFelt::from(3_u32),
                StarkFelt::from(123_u32),
                StarkFelt::from(333_u32),
                StarkFelt::from(0_u32),
            ]))
        );
    }
}
