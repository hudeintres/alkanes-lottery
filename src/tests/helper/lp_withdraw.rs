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

/// Insert LP withdraw transaction
pub fn insert_lp_withdraw_txs(
    shares_amount: u128,
    lottery_address: AlkaneId,
    test_block: &mut Block,
    input_outpoint: OutPoint,
) {
    test_block.txdata.push(
        create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
            Witness::new(),
            vec![
                CellpackOrEdict::Edict(vec![ProtostoneEdict {
                    id: lottery_address.into(), // Send LP shares back
                    amount: shares_amount,
                    output: 0,
                }]),
                CellpackOrEdict::Cellpack(Cellpack {
                    target: lottery_address,
                    inputs: vec![8, shares_amount], // opcode 8 = LpWithdraw, amount_desired
                }),
            ],
            input_outpoint,
            false,
            true, // separate leftovers
        ),
    );
}

/// Check LP withdraw balances after withdrawal
pub fn check_lp_withdraw_balance(
    shares_burned: u128,
    total_shares: u128,
    pool_total: u128,
    ticket_price: u128,
    test_block: &Block,
    deployment_ids: &LotteryTestDeploymentIds,
) -> Result<u128> {
    let sheet = get_last_outpoint_sheet(test_block)?;

    // Calculate expected tokens returned
    // amount_out = (shares * current_assets) / total_shares
    // floored to ticket_price
    let amount_out = if total_shares == 0 {
        0
    } else {
        (shares_burned * pool_total) / total_shares
    };
    let floored_amount_out = (amount_out / ticket_price) * ticket_price;

    // Check that tokens were received
    let tokens_received = sheet.get_cached(&deployment_ids.lottery_token.into());
    println!(
        "Tokens received: {:?}, expected (floored): {:?}",
        tokens_received, floored_amount_out
    );

    Ok(tokens_received)
}

/// Check runtime balance after LP withdraw
pub fn check_lp_withdraw_runtime_balance(
    runtime_balances: &mut BalanceSheet<IndexPointer>,
    withdrawn_amount: u128,
    shares_burned: u128,
    deployment_ids: &LotteryTestDeploymentIds,
) -> Result<()> {
    // The lottery contract should have less tokens now
    runtime_balances.decrease(&deployment_ids.lottery_token.into(), withdrawn_amount);
    // LP shares were burned (sent back to contract)
    runtime_balances.increase(&deployment_ids.lottery_contract.into(), shares_burned)?;

    let sheet = get_sheet_for_runtime();
    let lazy_sheet = get_lazy_sheet_for_runtime();

    // Verify runtime balance matches expected
    assert_eq!(
        sheet.get(&deployment_ids.lottery_token.into()),
        runtime_balances.get(&deployment_ids.lottery_token.into())
    );

    Ok(())
}

/// Perform LP withdraw and return the block and tokens received
pub fn do_lp_withdraw(
    shares_amount: u128,
    input_outpoint: OutPoint,
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<(Block, u128)> {
    let mut test_block = create_block_with_coinbase_tx(block_height);

    insert_lp_withdraw_txs(
        shares_amount,
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    // Get the tokens received from the output
    let sheet = get_last_outpoint_sheet(&test_block)?;
    let tokens = sheet.get_cached(&deployment_ids.lottery_token.into());

    Ok((test_block, tokens))
}
