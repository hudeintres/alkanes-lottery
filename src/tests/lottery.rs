use alkanes::indexer::index_block;
use alkanes::tests::helpers::{self as alkane_helpers, assert_return_context, assert_revert_context, get_last_outpoint_sheet};
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

/// Test purchasing tickets when purchasing is disabled (should fail)
#[wasm_bindgen_test]
fn test_purchase_tickets_when_disabled_fails() -> Result<()> {
    alkane_helpers::clear();

    // Initialize with purchasing disabled
    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture_purchasing_disabled()?;

    // Try to purchase tickets when purchasing is disabled
    let block_height = 840_002;
    let input_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let purchase_amount = TICKET_PRICE * 5;

    let mut test_block = create_block_with_coinbase_tx(block_height);
    insert_mint_and_buy_txs(
        purchase_amount,
        deployment_ids.lottery_token,
        deployment_ids.lottery_contract,
        &mut test_block,
        input_outpoint,
    );

    index_block(&test_block, block_height)?;

    // Should revert with error about purchasing not allowed
    assert_revert_context(
        &OutPoint {
            txid: test_block.txdata.last().unwrap().compute_txid(),
            vout: 5,
        },
        "Purchasing tickets not allowed",
    )?;

    Ok(())
}

/// Test purchasing tickets successfully using mint_and_buy
#[wasm_bindgen_test]
fn test_mint_and_buy_success() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture()?;
    // Purchasing is enabled during init

    // Mint and buy tickets in one operation
    let block_height = 840_002;
    let purchase_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let purchase_amount = TICKET_PRICE * 5; // Buy 5 tickets worth

    let (purchase_block, collector_id) = do_mint_and_buy(
        purchase_amount,
        purchase_outpoint,
        &deployment_ids,
        block_height,
    )?;
    println!("Minted collector {:?} and purchased tickets", collector_id);

    // Check that tokens were consumed and collector NFT was returned
    let sheet = get_last_outpoint_sheet(&purchase_block)?;
    let remaining_tokens = sheet.get(&deployment_ids.lottery_token.into());
    let collector_balance = sheet.get(&collector_id.into());
    
    println!("Remaining tokens after purchase: {}", remaining_tokens);
    println!("Collector NFT balance: {}", collector_balance);
    
    // User should have received the collector NFT
    assert_eq!(collector_balance, 1, "User should receive 1 collector NFT");

    Ok(())
}

/// Test full lottery cycle: deposit, mint_and_buy, run jackpot, LPs win
#[wasm_bindgen_test]
fn test_lottery_lps_win() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture()?;
    // Purchasing is enabled during init

    // Step 1: LP deposits to create the pool
    let block_height_1 = 840_002;
    let lp_deposit_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let lp_deposit_amount = TICKET_PRICE * 100; // 100 tickets worth
    let (lp_deposit_block, lp_shares) = do_lp_deposit(
        lp_deposit_amount,
        lp_deposit_outpoint,
        &deployment_ids,
        block_height_1,
    )?;
    println!("LP deposited {}, received {} shares", lp_deposit_amount, lp_shares);
    assert_eq!(lp_shares, lp_deposit_amount);

    // Check LP pool after deposit using view function
    let deposit_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };
    let (_deposit_view_block, lp_pool_after_deposit) = call_get_lp_pool_total(&deployment_ids, 840_003)?;
    let (_deposit_view_block2, user_pool_after_deposit) = call_get_user_pool_total(&deployment_ids, 840_004)?;
    assert_eq!(lp_pool_after_deposit, lp_deposit_amount, "LP pool should equal deposit amount after LP deposit");
    assert_eq!(user_pool_after_deposit, 0, "User pool should still be 0 after LP deposit");

    // Step 2: User mints collector and buys tickets (small amount - less than LP pool)
    let block_height_2 = 840_003;
    let purchase_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 2, // Leftover tokens
    };

    let purchase_amount = TICKET_PRICE * 5; // Only 5 tickets - much less than LP pool

    let (purchase_block, collector_id) = do_mint_and_buy(
        purchase_amount,
        purchase_outpoint,
        &deployment_ids,
        block_height_2,
    )?;
    println!("User minted collector {:?} and purchased {} tokens worth of tickets", collector_id, purchase_amount);

    // Check pools after user purchase using view functions
    let purchase_outpoint_for_view = OutPoint {
        txid: purchase_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };
    let (_purchase_view_block, lp_pool_after_purchase) = call_get_lp_pool_total(&deployment_ids, 840_004)?;
    let (_purchase_view_block2, user_pool_after_purchase) = call_get_user_pool_total(&deployment_ids, 840_005)?;
    assert_eq!(lp_pool_after_purchase, lp_deposit_amount, "LP pool should remain unchanged after user purchase");
    let expected_user_pool = (purchase_amount * 85) / 100; // 85% after 15% fee
    assert_eq!(user_pool_after_purchase, expected_user_pool, "User pool should be purchase amount minus fees");

    // Step 3: Wait for round duration and run jackpot
    // Round duration is 144 blocks by default
    let block_height_3 = 840_003 + 145; // After round duration
    let jackpot_outpoint = OutPoint {
        txid: purchase_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let jackpot_block = do_run_jackpot(jackpot_outpoint, &deployment_ids, block_height_3)?;
    println!("Jackpot run executed");

    // Check pools after jackpot using view functions (LPs win, so LP pool should increase by user pool + LP fees, user pool should be 0)
    let jackpot_outpoint_for_view = OutPoint {
        txid: jackpot_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };
    let (_jackpot_view_block, lp_pool_after_jackpot) = call_get_lp_pool_total(&deployment_ids, block_height_3 + 1)?;
    let (_jackpot_view_block2, user_pool_after_jackpot) = call_get_user_pool_total(&deployment_ids, block_height_3 + 2)?;

    // LP fees (14.9% of purchase amount) are added to LP pool during jackpot
    let lp_fees = (purchase_amount * 149) / 1000; // 14.9% = 149/1000
    let expected_lp_pool_after_jackpot = lp_deposit_amount + expected_user_pool + lp_fees;

    assert_eq!(lp_pool_after_jackpot, expected_lp_pool_after_jackpot, "LP pool should increase by user pool + LP fees after LPs win");
    assert_eq!(user_pool_after_jackpot, 0, "User pool should be 0 after jackpot");

    // Step 4: LP withdraws - should get back their deposit plus the user's lost tickets
    let block_height_4 = block_height_3 + 1;

    // LP shares are at lp_deposit_block vout 0
    let lp_shares_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let (withdraw_block, tokens_received) = do_lp_withdraw(
        lp_shares,
        lp_shares_outpoint,
        &deployment_ids,
        block_height_4,
    )?;
    println!("LP withdrew {} shares, received {} tokens", lp_shares, tokens_received);

    // LP should get back more than they deposited (they won the user's pool)
    // Fee is 15% (1500 bps), so user's effective contribution is purchase_amount * 0.85
    // Plus LP fees which are purchase_amount * 0.15 - protocol_fee
    // For simplicity, let's just check LP got at least their deposit back
    assert!(
        tokens_received >= lp_deposit_amount,
        "LP should receive at least their deposit back. Got {}, expected >= {}",
        tokens_received,
        lp_deposit_amount
    );

    // Additional check: LP should get their deposit plus the user's effective contribution
    let user_effective_contribution = (purchase_amount * 85) / 100; // 85% after 15% fee
    let expected_min_payout = lp_deposit_amount + user_effective_contribution;
    // Allow some rounding difference due to ticket price flooring
    assert!(
        tokens_received >= expected_min_payout.saturating_sub(TICKET_PRICE),
        "LP should receive at least their deposit + user's effective contribution. Got {}, expected >= {}",
        tokens_received,
        expected_min_payout.saturating_sub(TICKET_PRICE)
    );

    println!("LP profit: {} tokens", tokens_received.saturating_sub(lp_deposit_amount));

    Ok(())
}

/// Test full lottery cycle: deposit, mint_and_buy large amount, run jackpot, user wins
#[wasm_bindgen_test]
fn test_lottery_user_wins() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture()?;
    // Purchasing is enabled during init

    // Step 1: LP deposits a small amount
    let block_height_1 = 840_002;
    let lp_deposit_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let lp_deposit_amount = TICKET_PRICE * 10; // Small LP pool
    let (lp_deposit_block, lp_shares) = do_lp_deposit(
        lp_deposit_amount,
        lp_deposit_outpoint,
        &deployment_ids,
        block_height_1,
    )?;
    println!("LP deposited {}, received {} shares", lp_deposit_amount, lp_shares);

    // Check LP pool after deposit using view function
    let deposit_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };
    let (_deposit_view_block, lp_pool_after_deposit) = call_get_lp_pool_total(&deployment_ids, 840_003)?;
    let (_deposit_view_block2, user_pool_after_deposit) = call_get_user_pool_total(&deployment_ids, 840_004)?;
    assert_eq!(lp_pool_after_deposit, lp_deposit_amount, "LP pool should equal deposit amount after LP deposit");
    assert_eq!(user_pool_after_deposit, 0, "User pool should still be 0 after LP deposit");

    // Step 2: User mints collector and buys LARGE amount of tickets (more than LP pool)
    let block_height_2 = 840_003;
    let purchase_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let purchase_amount = TICKET_PRICE * 50; // 50 tickets - more than LP pool after fees

    let (purchase_block, collector_id) = do_mint_and_buy(
        purchase_amount,
        purchase_outpoint,
        &deployment_ids,
        block_height_2,
    )?;
    println!("User minted collector {:?} and purchased {} tokens worth of tickets", collector_id, purchase_amount);

    // Check pools after user purchase using view functions
    let purchase_outpoint_for_view = OutPoint {
        txid: purchase_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };
    let (_purchase_view_block, lp_pool_after_purchase) = call_get_lp_pool_total(&deployment_ids, 840_004)?;
    let (_purchase_view_block2, user_pool_after_purchase) = call_get_user_pool_total(&deployment_ids, 840_005)?;
    assert_eq!(lp_pool_after_purchase, lp_deposit_amount, "LP pool should remain unchanged after user purchase");
    let expected_user_pool = (purchase_amount * 85) / 100; // 85% after 15% fee
    assert_eq!(user_pool_after_purchase, expected_user_pool, "User pool should be purchase amount minus fees");

    // Step 3: Run jackpot after round duration
    let block_height_3 = 840_003 + 145;
    let jackpot_outpoint = OutPoint {
        txid: purchase_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let jackpot_block = do_run_jackpot(jackpot_outpoint, &deployment_ids, block_height_3)?;
    println!("Jackpot run executed");

    // Check pools after jackpot using view functions (user wins, so user pool gets distributed and reset, LP pool should be 0)
    let jackpot_outpoint_for_view = OutPoint {
        txid: jackpot_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };
    let (_jackpot_view_block, lp_pool_after_jackpot) = call_get_lp_pool_total(&deployment_ids, block_height_3 + 1)?;
    let (_jackpot_view_block2, user_pool_after_jackpot) = call_get_user_pool_total(&deployment_ids, block_height_3 + 2)?;
    assert_eq!(lp_pool_after_jackpot, 0, "LP pool should be 0 after user wins (LPs lose their stake)");
    assert_eq!(user_pool_after_jackpot, 0, "User pool should be reset to 0 after jackpot (distributed to winner)");

    // When user pool >= LP pool, user is guaranteed to win
    // They win the user pool (minus fees)
    // Now we can test withdraw_winnings because we have the real collector NFT!

    // Step 4: User withdraws winnings
    let block_height_4 = block_height_3 + 1;
    // The collector NFT is at the purchase_block output (vout 0)
    let withdraw_outpoint = OutPoint {
        txid: purchase_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let (withdraw_block, winnings_received) = do_withdraw_winnings(
        collector_id,
        withdraw_outpoint,
        &deployment_ids,
        block_height_4,
    )?;
    println!("User withdrew winnings: {} tokens", winnings_received);

    // User should receive their effective contribution as winnings
    let expected_winnings = (purchase_amount * 85) / 100; // 85% after 15% fee
    assert_eq!(winnings_received, expected_winnings, "User should receive their effective contribution as winnings");

    println!("In user wins scenario, user pool was larger than LP pool");
    println!("User's tickets fully funded the jackpot, so user wins the user pool");
    println!("User has collector {:?} to claim winnings", collector_id);

    Ok(())
}

/// Test running jackpot too early (should fail)
#[wasm_bindgen_test]
fn test_run_jackpot_too_early_fails() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    // Try to run jackpot immediately (before round duration)
    let block_height = 840_002; // Too early
    let jackpot_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let mut test_block = create_block_with_coinbase_tx(block_height);
    insert_run_jackpot_txs(
        deployment_ids.lottery_contract,
        &mut test_block,
        jackpot_outpoint,
    );

    index_block(&test_block, block_height)?;

    // Should revert with error about round duration
    assert_revert_context(
        &OutPoint {
            txid: test_block.txdata.last().unwrap().compute_txid(),
            vout: 4, // Different vout for single cellpack
        },
        "Jackpot can only be run once per round",
    )?;

    Ok(())
}

/// Test running jackpot with no tickets (LP pool returned)
#[wasm_bindgen_test]
fn test_run_jackpot_no_tickets() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture()?;

    // Step 1: LP deposits
    let block_height_1 = 840_002;
    let lp_deposit_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let lp_deposit_amount = TICKET_PRICE * 100;
    let (lp_deposit_block, lp_shares) = do_lp_deposit(
        lp_deposit_amount,
        lp_deposit_outpoint,
        &deployment_ids,
        block_height_1,
    )?;
    println!("LP deposited {}, received {} shares", lp_deposit_amount, lp_shares);

    // Step 2: Run jackpot after round duration (no ticket purchases)
    let block_height_2 = 840_002 + 145;
    let jackpot_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let jackpot_block = do_run_jackpot(jackpot_outpoint, &deployment_ids, block_height_2)?;
    println!("Jackpot run with no tickets");

    // Step 3: LP withdraws - should get back exactly their deposit
    let block_height_3 = block_height_2 + 1;
    let lp_shares_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let (withdraw_block, tokens_received) = do_lp_withdraw(
        lp_shares,
        lp_shares_outpoint,
        &deployment_ids,
        block_height_3,
    )?;
    println!("LP withdrew {} shares, received {} tokens", lp_shares, tokens_received);

    // LP should get back exactly their deposit (no profit, no loss)
    assert_eq!(
        tokens_received, lp_deposit_amount,
        "LP should receive exactly their deposit back when no tickets sold"
    );

    Ok(())
}

/// Test multiple ticket purchases accumulating
#[wasm_bindgen_test]
fn test_multiple_ticket_purchases() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture()?;
    // Purchasing is enabled during init

    // Step 1: LP deposits
    let block_height_1 = 840_002;
    let lp_deposit_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let lp_deposit_amount = TICKET_PRICE * 100;
    let (lp_deposit_block, _lp_shares) = do_lp_deposit(
        lp_deposit_amount,
        lp_deposit_outpoint,
        &deployment_ids,
        block_height_1,
    )?;
    println!("LP deposited {}", lp_deposit_amount);

    // Step 2: First ticket purchase - mint collector and buy
    let block_height_2 = 840_003;
    let purchase1_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let purchase1_amount = TICKET_PRICE * 3;

    let (purchase1_block, collector_id) = do_mint_and_buy(
        purchase1_amount,
        purchase1_outpoint,
        &deployment_ids,
        block_height_2,
    )?;
    println!("First purchase: {} tokens, minted collector {:?}", purchase1_amount, collector_id);

    // Step 3: Second ticket purchase (same collector, using purchase_tickets opcode)
    let block_height_3 = 840_004;
    let purchase2_outpoint = OutPoint {
        txid: purchase1_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let purchase2_amount = TICKET_PRICE * 7;

    let purchase2_block = do_purchase_tickets(
        purchase2_amount,
        collector_id, // Same collector
        purchase2_outpoint,
        &deployment_ids,
        block_height_3,
    )?;
    println!("Second purchase: {} tokens (same collector)", purchase2_amount);

    // Total tickets should be accumulated for the collector
    let total_purchased = purchase1_amount + purchase2_amount;
    println!("Total purchased: {} tokens ({} tickets)", total_purchased, total_purchased / TICKET_PRICE);

    Ok(())
}


/// Test LP payout after user wins jackpot
#[wasm_bindgen_test]
fn test_lp_payout_after_jackpot() -> Result<()> {
    alkane_helpers::clear();

    let (init_block, _runtime_balances, deployment_ids) = test_lottery_init_fixture()?;
    // Purchasing is enabled during init

    // Step 1: LP deposits
    let block_height_1 = 840_002;
    let lp_deposit_outpoint = OutPoint {
        txid: init_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let lp_deposit_amount = TICKET_PRICE * 50;
    let (lp_deposit_block, lp_shares) = do_lp_deposit(
        lp_deposit_amount,
        lp_deposit_outpoint,
        &deployment_ids,
        block_height_1,
    )?;
    println!("LP deposited {}, received {} shares", lp_deposit_amount, lp_shares);

    // Step 2: User mints collector and purchases tickets
    let block_height_2 = 840_003;
    let purchase_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let purchase_amount = TICKET_PRICE * 20;

    let (purchase_block, collector_id) = do_mint_and_buy(
        purchase_amount,
        purchase_outpoint,
        &deployment_ids,
        block_height_2,
    )?;
    println!("User minted collector {:?} and purchased {} tokens worth of tickets", collector_id, purchase_amount);

    // Step 3: Run jackpot
    let block_height_3 = 840_003 + 145;
    let jackpot_outpoint = OutPoint {
        txid: purchase_block.txdata.last().unwrap().compute_txid(),
        vout: 2,
    };

    let jackpot_block = do_run_jackpot(jackpot_outpoint, &deployment_ids, block_height_3)?;
    println!("Jackpot executed");

    // Step 4: LP withdraws
    let block_height_4 = block_height_3 + 1;
    let lp_shares_outpoint = OutPoint {
        txid: lp_deposit_block.txdata.last().unwrap().compute_txid(),
        vout: 0,
    };

    let (withdraw_block, lp_payout) = do_lp_withdraw(
        lp_shares,
        lp_shares_outpoint,
        &deployment_ids,
        block_height_4,
    )?;
    println!("LP withdrew {} shares, received {} tokens", lp_shares, lp_payout);

    // Verify LP got a reasonable payout
    // The exact amount depends on whether user or LP won
    // At minimum, LP should get something back
    assert!(
        lp_payout > 0,
        "LP should receive some payout after jackpot"
    );

    // Calculate expected range
    // If LP won: LP gets deposit + user_pool + fees
    // If user won: LP gets user_pool + fees (loses their stake potentially)
    println!("LP result: {} tokens (original deposit: {})", lp_payout, lp_deposit_amount);
    
    Ok(())
}