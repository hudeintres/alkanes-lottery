use alkanes::indexer::index_block;
use alkanes::tests::helpers::{self as alkane_helpers, assert_revert_context, get_last_outpoint_sheet};
use alkanes_support::cellpack::Cellpack;
use alkanes_support::id::AlkaneId;
use anyhow::Result;
use bitcoin::blockdata::transaction::OutPoint;
use bitcoin::Witness;
#[allow(unused_imports)]
use metashrew_core::{get_cache, index_pointer::IndexPointer, println, stdio::stdout};
use protorune::test_helpers::create_block_with_coinbase_tx;
use protorune_support::balance_sheet::{BalanceSheet, BalanceSheetOperations};
use std::fmt::Write;
use wasm_bindgen_test::wasm_bindgen_test;

use crate::tests::helper::*;

/// Test single LP deposit
#[wasm_bindgen_test]
fn test_lp_single_deposit() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, mut runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    let block_height = 840_001;
    let input_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    // Deposit 1000 tokens (must be multiple of ticket price)
    let deposit_amount = TICKET_PRICE * 10; // 10 tickets worth

    let (deposit_block, shares_received) = do_lp_deposit(
        deposit_amount,
        input_outpoint,
        &deployment_ids,
        block_height,
    )?;

    // First deposit: shares should equal the deposit amount
    println!("Shares received from first deposit: {:?}", shares_received);
    assert_eq!(shares_received, deposit_amount, "First depositor should receive shares equal to deposit");

    Ok(())
}

/// Test multiple LP deposits from different users
#[wasm_bindgen_test]
fn test_lp_multiple_deposits() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, mut runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    // First LP deposit
    let block_height_1 = 840_001;
    let input_outpoint_1 = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let deposit_amount_1 = TICKET_PRICE * 10; // 1000 tokens
    let (deposit_block_1, shares_1) = do_lp_deposit(
        deposit_amount_1,
        input_outpoint_1,
        &deployment_ids,
        block_height_1,
    )?;

    println!("LP1 deposited {:?}, received {:?} shares", deposit_amount_1, shares_1);
    assert_eq!(shares_1, deposit_amount_1, "First LP should receive shares equal to deposit");

    // Second LP deposit
    let block_height_2 = 840_002;
    let input_outpoint_2 = OutPoint {
        txid: deposit_block_1.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let deposit_amount_2 = TICKET_PRICE * 20; // 2000 tokens
    let (deposit_block_2, shares_2) = do_lp_deposit(
        deposit_amount_2,
        input_outpoint_2,
        &deployment_ids,
        block_height_2,
    )?;

    // Second depositor: shares = (deposit * total_shares) / pool_total
    // shares_2 = (2000 * 1000) / 1000 = 2000
    let expected_shares_2 = (deposit_amount_2 * shares_1) / deposit_amount_1;
    println!("LP2 deposited {:?}, received {:?} shares (expected {:?})",
             deposit_amount_2, shares_2, expected_shares_2);
    assert_eq!(shares_2, expected_shares_2, "Second LP should receive proportional shares");

    // Third LP deposit
    let block_height_3 = 840_003;
    let input_outpoint_3 = OutPoint {
        txid: deposit_block_2.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let deposit_amount_3 = TICKET_PRICE * 5; // 500 tokens
    let (deposit_block_3, shares_3) = do_lp_deposit(
        deposit_amount_3,
        input_outpoint_3,
        &deployment_ids,
        block_height_3,
    )?;

    // Third depositor: shares = (deposit * total_shares) / pool_total
    let total_shares_before_3 = shares_1 + shares_2;
    let pool_total_before_3 = deposit_amount_1 + deposit_amount_2;
    let expected_shares_3 = (deposit_amount_3 * total_shares_before_3) / pool_total_before_3;
    println!("LP3 deposited {:?}, received {:?} shares (expected {:?})",
             deposit_amount_3, shares_3, expected_shares_3);
    assert_eq!(shares_3, expected_shares_3, "Third LP should receive proportional shares");

    Ok(())
}

/// Test LP deposit and then full withdrawal to get initial deposit back
#[wasm_bindgen_test]
fn test_lp_deposit_and_full_withdraw() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, mut runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    // Deposit
    let block_height_1 = 840_001;
    let input_outpoint_1 = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let deposit_amount = TICKET_PRICE * 10;
    let (deposit_block, shares_received) = do_lp_deposit(
        deposit_amount,
        input_outpoint_1,
        &deployment_ids,
        block_height_1,
    )?;

    println!("Deposited {:?}, received {:?} shares", deposit_amount, shares_received);

    // Withdraw all shares
    let block_height_2 = 840_002;
    let input_outpoint_2 = OutPoint {
        txid: deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let (withdraw_block, tokens_received) = do_lp_withdraw(
        shares_received,
        input_outpoint_2,
        &deployment_ids,
        block_height_2,
    )?;

    println!("Withdrew {:?} shares, received {:?} tokens", shares_received, tokens_received);

    // Should get back the full deposit (floored to ticket price)
    let expected_tokens = (deposit_amount / TICKET_PRICE) * TICKET_PRICE;
    assert_eq!(tokens_received, expected_tokens, "Should receive full deposit back on withdrawal");

    Ok(())
}

/// Test multiple LPs deposit and withdraw to verify they get their proportional share back
#[wasm_bindgen_test]
fn test_multiple_lps_deposit_and_withdraw() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, mut runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    // LP1 deposits
    let block_height_1 = 840_001;
    let input_outpoint_1 = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let deposit_amount_1 = TICKET_PRICE * 10;
    let (deposit_block_1, shares_1) = do_lp_deposit(
        deposit_amount_1,
        input_outpoint_1,
        &deployment_ids,
        block_height_1,
    )?;
    println!("LP1: deposited {:?}, received {:?} shares", deposit_amount_1, shares_1);

    // LP2 deposits
    let block_height_2 = 840_002;
    let input_outpoint_2 = OutPoint {
        txid: deposit_block_1.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let deposit_amount_2 = TICKET_PRICE * 20;
    let (deposit_block_2, shares_2) = do_lp_deposit(
        deposit_amount_2,
        input_outpoint_2,
        &deployment_ids,
        block_height_2,
    )?;
    println!("LP2: deposited {:?}, received {:?} shares", deposit_amount_2, shares_2);

    // Total pool state
    let total_shares = shares_1 + shares_2;
    let total_pool = deposit_amount_1 + deposit_amount_2;
    println!("Total pool: {:?}, Total shares: {:?}", total_pool, total_shares);

    // LP1 withdraws all their shares
    let block_height_3 = 840_003;
    let input_outpoint_3 = OutPoint {
        txid: deposit_block_2.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let (withdraw_block_1, tokens_1) = do_lp_withdraw(
        shares_1,
        input_outpoint_3,
        &deployment_ids,
        block_height_3,
    )?;

    // LP1 should get back their proportional share
    // tokens = (shares * pool_total) / total_shares
    let expected_tokens_1 = (shares_1 * total_pool) / total_shares;
    let floored_expected_1 = (expected_tokens_1 / TICKET_PRICE) * TICKET_PRICE;
    println!("LP1: withdrew {:?} shares, received {:?} tokens (expected {:?})",
             shares_1, tokens_1, floored_expected_1);
    assert_eq!(tokens_1, floored_expected_1, "LP1 should receive proportional tokens");

    // LP2 withdraws all their shares
    let block_height_4 = 840_004;
    let input_outpoint_4 = OutPoint {
        txid: withdraw_block_1.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    // After LP1 withdrawal, pool state changes
    let remaining_pool = total_pool - tokens_1;
    let remaining_shares = total_shares - shares_1;

    let (withdraw_block_2, tokens_2) = do_lp_withdraw(
        shares_2,
        input_outpoint_4,
        &deployment_ids,
        block_height_4,
    )?;

    // LP2 should get back their proportional share of remaining pool
    let expected_tokens_2 = (shares_2 * remaining_pool) / remaining_shares;
    let floored_expected_2 = (expected_tokens_2 / TICKET_PRICE) * TICKET_PRICE;
    println!("LP2: withdrew {:?} shares, received {:?} tokens (expected {:?})",
             shares_2, tokens_2, floored_expected_2);
    assert_eq!(tokens_2, floored_expected_2, "LP2 should receive proportional tokens");

    // Verify both LPs got back approximately their initial deposits
    // (may have small rounding differences due to flooring)
    let total_withdrawn = tokens_1 + tokens_2;
    println!("Total deposited: {:?}, Total withdrawn: {:?}", total_pool, total_withdrawn);
    assert!(
        total_withdrawn <= total_pool,
        "Total withdrawn should not exceed total deposited"
    );

    Ok(())
}

/// Test partial withdrawal
#[wasm_bindgen_test]
fn test_lp_partial_withdraw() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, mut runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    // Deposit
    let block_height_1 = 840_001;
    let input_outpoint_1 = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let deposit_amount = TICKET_PRICE * 10;
    let (deposit_block, shares_received) = do_lp_deposit(
        deposit_amount,
        input_outpoint_1,
        &deployment_ids,
        block_height_1,
    )?;

    println!("Deposited {:?}, received {:?} shares", deposit_amount, shares_received);

    // Withdraw half the shares
    let block_height_2 = 840_002;
    let input_outpoint_2 = OutPoint {
        txid: deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let shares_to_withdraw = shares_received / 2;
    let (withdraw_block_1, tokens_1) = do_lp_withdraw(
        shares_to_withdraw,
        input_outpoint_2,
        &deployment_ids,
        block_height_2,
    )?;

    // Should get back half the deposit (floored)
    let expected_tokens = (deposit_amount / 2 / TICKET_PRICE) * TICKET_PRICE;
    println!("Withdrew {:?} shares, received {:?} tokens (expected {:?})",
             shares_to_withdraw, tokens_1, expected_tokens);
    assert_eq!(tokens_1, expected_tokens, "Should receive half deposit on partial withdrawal");

    // Withdraw remaining shares
    let block_height_3 = 840_003;
    let input_outpoint_3 = OutPoint {
        txid: withdraw_block_1.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let remaining_shares = shares_received - shares_to_withdraw;
    let (withdraw_block_2, tokens_2) = do_lp_withdraw(
        remaining_shares,
        input_outpoint_3,
        &deployment_ids,
        block_height_3,
    )?;

    println!("Withdrew remaining {:?} shares, received {:?} tokens", remaining_shares, tokens_2);

    // Total withdrawn should equal initial deposit (minus any rounding)
    let total_withdrawn = tokens_1 + tokens_2;
    assert!(
        total_withdrawn <= deposit_amount,
        "Total withdrawn should not exceed deposit"
    );

    Ok(())
}

/// Test that deposit below ticket price fails
#[wasm_bindgen_test]
fn test_lp_deposit_below_ticket_price_fails() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, mut runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    let block_height = 840_001;
    let input_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    // Try to deposit less than ticket price
    let deposit_amount = TICKET_PRICE / 2;

    let mut test_block = create_block_with_coinbase_tx(block_height);
    insert_lp_deposit_txs(
        deposit_amount,
        deployment_ids.lottery_token,
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    // Should revert with error about deposit being too small
    assert_revert_context(
        &OutPoint {
            txid: test_block.txdata.last().unwrap().compute_txid(),
            vout: 0,
        },
        "Invalid deposit amount",
    )?;

    Ok(())
}

/// Test that deposit exceeding pool cap fails
#[wasm_bindgen_test]
fn test_lp_deposit_exceeds_cap_fails() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, mut runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    let block_height = 840_001;
    let input_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    // Try to deposit more than the pool cap
    // Default pool cap is ticket_price * 100 * 1000 = 10,000,000 tickets worth
    let deposit_amount = TICKET_PRICE * 100 * 1000 + TICKET_PRICE;

    let mut test_block = create_block_with_coinbase_tx(block_height);
    insert_lp_deposit_txs(
        deposit_amount,
        deployment_ids.lottery_token,
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    // Should revert with error about exceeding cap
    assert_revert_context(
        &OutPoint {
            txid: test_block.txdata.last().unwrap().compute_txid(),
            vout: 0,
        },
        "Deposit exceeds LP pool cap",
    )?;

    Ok(())
}
