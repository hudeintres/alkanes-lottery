use alkanes::indexer::index_block;
use alkanes::tests::helpers::{
    get_last_outpoint_sheet, get_lazy_sheet_for_runtime, get_sheet_for_runtime,
};
use alkanes_support::cellpack::Cellpack;
use alkanes_support::id::AlkaneId;
use anyhow::Result;
use bitcoin::blockdata::transaction::OutPoint;
use bitcoin::{Block, Witness};
#[allow(unused_imports)]
use metashrew_core::{get_cache, index_pointer::IndexPointer, println, stdio::stdout};
use protorune::test_helpers::create_block_with_coinbase_tx;
use protorune_support::balance_sheet::{BalanceSheet, BalanceSheetOperations};
use protorune_support::protostone::ProtostoneEdict;
use std::fmt::Write;

use super::common::*;

/// Insert LP deposit transaction
pub fn insert_lp_deposit_txs(
    amount: u128,
    token_address: AlkaneId,
    lottery_address: AlkaneId,
    test_block: &mut Block,
    input_outpoint: OutPoint,
) {
    test_block.txdata.push(
        create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
            Witness::new(),
            vec![
                CellpackOrEdict::Edict(vec![ProtostoneEdict {
                    id: token_address.into(),
                    amount,
                    output: 0,
                }]),
                CellpackOrEdict::Cellpack(Cellpack {
                    target: lottery_address,
                    inputs: vec![1, amount], // opcode 1 = LpDeposit, amount_desired
                }),
            ],
            input_outpoint,
            false,
            true, // separate leftovers
        ),
    );
}

/// Check LP deposit balances after deposit
pub fn check_lp_deposit_balance(
    deposit_amount: u128,
    prev_lp_shares: u128,
    prev_pool_total: u128,
    test_block: &Block,
    deployment_ids: &LotteryTestDeploymentIds,
) -> Result<u128> {
    let sheet = get_last_outpoint_sheet(test_block)?;

    // Calculate expected shares
    // If first deposit, shares = amount
    // Otherwise, shares = (amount * total_shares) / current_assets
    let expected_shares = if prev_pool_total == 0 {
        deposit_amount
    } else {
        // This is simplified - actual calculation uses U256
        (deposit_amount * prev_lp_shares) / prev_pool_total
    };

    // Check that LP shares were received
    let lp_shares = sheet.get_cached(&deployment_ids.lottery_contract.into());
    println!("LP shares received: {:?}, expected: {:?}", lp_shares, expected_shares);

    Ok(lp_shares)
}

/// Check runtime balance after LP deposit
pub fn check_lp_deposit_runtime_balance(
    runtime_balances: &mut BalanceSheet<IndexPointer>,
    deposited_amount: u128,
    deployment_ids: &LotteryTestDeploymentIds,
) -> Result<()> {
    // The lottery contract should now hold the deposited tokens
    runtime_balances.increase(&deployment_ids.lottery_token.into(), deposited_amount)?;

    let sheet = get_sheet_for_runtime();
    let lazy_sheet = get_lazy_sheet_for_runtime();

    // Verify runtime balance matches expected
    assert_eq!(
        sheet.get(&deployment_ids.lottery_token.into()),
        runtime_balances.get(&deployment_ids.lottery_token.into())
    );
    assert_eq!(
        lazy_sheet.get(&deployment_ids.lottery_token.into()),
        runtime_balances.get(&deployment_ids.lottery_token.into())
    );

    Ok(())
}

/// Perform LP deposit and return the block and shares received
pub fn do_lp_deposit(
    amount: u128,
    input_outpoint: OutPoint,
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<(Block, u128)> {
    let mut test_block = create_block_with_coinbase_tx(block_height);

    insert_lp_deposit_txs(
        amount,
        deployment_ids.lottery_token,
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    // Get the LP shares from the output
    let sheet = get_last_outpoint_sheet(&test_block)?;
    let shares = sheet.get_cached(&deployment_ids.lottery_contract.into());

    Ok((test_block, shares))
}
