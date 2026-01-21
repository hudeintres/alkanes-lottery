use alkanes::indexer::index_block;
use alkanes::tests::helpers::{
    get_last_outpoint_sheet, get_lazy_sheet_for_runtime, get_sheet_for_runtime,
};
use alkanes_support::cellpack::Cellpack;
use alkanes_support::id::AlkaneId;
use anyhow::{anyhow, Result};
use bitcoin::blockdata::transaction::OutPoint;
use bitcoin::{Block, Witness};
#[allow(unused_imports)]
use metashrew_core::{get_cache, index_pointer::IndexPointer, println, stdio::stdout};
use protorune::test_helpers::create_block_with_coinbase_tx;
use protorune_support::balance_sheet::{BalanceSheet, BalanceSheetOperations};
use protorune_support::protostone::ProtostoneEdict;
use std::fmt::Write;

use super::common::*;

/// Insert ticket purchase transaction
pub fn insert_purchase_tickets_txs(
    amount: u128,
    collector_id: AlkaneId,
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
                    inputs: vec![
                        3, // opcode 3 = PurchaseTickets
                        collector_id.block,
                        collector_id.tx,
                    ],
                }),
            ],
            input_outpoint,
            false,
            true, // separate leftovers
        ),
    );
}

/// Perform ticket purchase and return the block
pub fn do_purchase_tickets(
    amount: u128,
    collector_id: AlkaneId,
    input_outpoint: OutPoint,
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<Block> {
    let mut test_block = create_block_with_coinbase_tx(block_height);

    insert_purchase_tickets_txs(
        amount,
        collector_id,
        deployment_ids.lottery_token,
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    Ok(test_block)
}

/// Insert run jackpot transaction
pub fn insert_run_jackpot_txs(
    lottery_address: AlkaneId,
    test_block: &mut Block,
    input_outpoint: OutPoint,
) {
    test_block.txdata.push(
        create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
            Witness::new(),
            vec![
                CellpackOrEdict::Cellpack(Cellpack {
                    target: lottery_address,
                    inputs: vec![4], // opcode 4 = RunJackpot
                }),
            ],
            input_outpoint,
            false,
            true,
        ),
    );
}

/// Run the jackpot and return the block
pub fn do_run_jackpot(
    input_outpoint: OutPoint,
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<Block> {
    let mut test_block = create_block_with_coinbase_tx(block_height);

    insert_run_jackpot_txs(
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    Ok(test_block)
}

/// Insert withdraw winnings transaction
/// Must send the collector NFT to prove ownership
pub fn insert_withdraw_winnings_txs(
    collector_id: AlkaneId,
    lottery_address: AlkaneId,
    test_block: &mut Block,
    input_outpoint: OutPoint,
) {
    test_block.txdata.push(
        create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
            Witness::new(),
            vec![
                CellpackOrEdict::Edict(vec![ProtostoneEdict {
                    id: collector_id.into(), // Send collector NFT
                    amount: 1,
                    output: 0,
                }]),
                CellpackOrEdict::Cellpack(Cellpack {
                    target: lottery_address,
                    inputs: vec![
                        5, // opcode 5 = WithdrawWinnings
                        collector_id.block,
                        collector_id.tx,
                    ],
                }),
            ],
            input_outpoint,
            false,
            true,
        ),
    );
}

/// Withdraw winnings and return the block and tokens received
pub fn do_withdraw_winnings(
    collector_id: AlkaneId,
    input_outpoint: OutPoint,
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<(Block, u128)> {
    let mut test_block = create_block_with_coinbase_tx(block_height);

    insert_withdraw_winnings_txs(
        collector_id,
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

/// Insert admin enable purchasing transaction
/// Requires sending the auth token to prove ownership
pub fn insert_admin_enable_purchasing_txs(
    lottery_address: AlkaneId,
    auth_token_id: AlkaneId,
    test_block: &mut Block,
    input_outpoint: OutPoint,
) {
    test_block.txdata.push(
        create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
            Witness::new(),
            vec![
                // Send auth token to prove ownership
                CellpackOrEdict::Edict(vec![ProtostoneEdict {
                    id: auth_token_id.into(),
                    amount: 1,
                    output: 0,
                }]),
                CellpackOrEdict::Cellpack(Cellpack {
                    target: lottery_address,
                    inputs: vec![32, 1], // opcode 32 = AdminSetAllowPurchasing, 1 = true
                }),
            ],
            input_outpoint,
            false,
            true,
        ),
    );
}

/// Enable purchasing and return the block
pub fn do_enable_purchasing(
    input_outpoint: OutPoint,
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<Block> {
    let mut test_block = create_block_with_coinbase_tx(block_height);

    insert_admin_enable_purchasing_txs(
        deployment_ids.lottery_contract,
        deployment_ids.lottery_auth_token,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    Ok(test_block)
}

/// Insert mint and buy transaction (mints collector + buys tickets in one operation)
pub fn insert_mint_and_buy_txs(
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
                    inputs: vec![2], // opcode 2 = MintAndBuy
                }),
            ],
            input_outpoint,
            false,
            true,
        ),
    );
}

/// Mint a new collector and buy tickets in one operation
/// Returns the block and the collector ID that was minted
pub fn do_mint_and_buy(
    amount: u128,
    input_outpoint: OutPoint,
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<(Block, AlkaneId)> {
    let mut test_block = create_block_with_coinbase_tx(block_height);

    insert_mint_and_buy_txs(
        amount,
        deployment_ids.lottery_token,
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    // Get the collector ID from the output balance sheet
    // The collector NFT is returned at output 0
    let sheet = get_last_outpoint_sheet(&test_block)?;
    
    // Debug: print what's in the balance sheet
    println!("Balance sheet after mint_and_buy: {:?}", sheet);
    
    // Find the collector NFT in the balance sheet
    // When calling block 6 (factory deployment) for a factory at block 4,
    // new instances are deployed at block 2 (the "instanced" block)
    let collector_id = sheet
        .cached
        .balances
        .iter()
        .find(|(id, balance)| {
            // Collector is at block 2 (factory instances) and not the lottery token or contract
            id.block == 2 && **balance > 0
        })
        .map(|(id, _)| AlkaneId::new(id.block, id.tx))
        .ok_or_else(|| anyhow!("No collector NFT found in output. Balance sheet: {:?}", sheet))?;

    Ok((test_block, collector_id))
}