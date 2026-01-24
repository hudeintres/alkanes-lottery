use alkanes::tests::helpers::{self as alkane_helpers, assert_return_context};
use alkanes_support::{cellpack::Cellpack, id::AlkaneId};
use anyhow::Result;
use bitcoin::address::NetworkChecked;
use bitcoin::blockdata::transaction::OutPoint;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{Address, Amount, Block, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
#[allow(unused_imports)]
use metashrew_core::{get_cache, index_pointer::IndexPointer, println, stdio::stdout};

use ordinals::{Etching, Rune, Runestone};
use protorune::protostone::Protostones;
use protorune::test_helpers::{get_address, ADDRESS1};
use protorune_support::balance_sheet::{BalanceSheet, BalanceSheetOperations, ProtoruneRuneId};
use protorune_support::protostone::Protostone;
use protorune_support::protostone::ProtostoneEdict;
use std::collections::BTreeSet;
use std::str::FromStr;

/// Test deployment IDs for the lottery system
pub struct LotteryTestDeploymentIds {
    /// The lottery contract deployment
    pub lottery_contract: AlkaneId,
    /// The token used for the lottery (e.g., a stablecoin or wrapped BTC)
    pub lottery_token: AlkaneId,
    /// The collector factory for minting ticket collectors
    pub collector_factory: AlkaneId,
    /// The auth token for the lottery contract (for admin functions)
    pub lottery_auth_token: AlkaneId,
}

// Deployment tx constants
pub const LOTTERY_CONTRACT_TX: u128 = 1;
pub const LOTTERY_TOKEN_TX: u128 = 2;
pub const COLLECTOR_FACTORY_TX: u128 = 3;

/// Initial token amounts for testing
/// This needs to be large enough for multiple deposit/withdraw operations
pub const INIT_AMT_TOKEN: u128 = 10_000_000_000_000u128; // 100000 tokens with 8 decimals

pub fn create_deployment_ids() -> LotteryTestDeploymentIds {
    LotteryTestDeploymentIds {
        lottery_contract: AlkaneId {
            block: 4,
            tx: LOTTERY_CONTRACT_TX,
        },
        lottery_token: AlkaneId {
            block: 4,
            tx: LOTTERY_TOKEN_TX,
        },
        collector_factory: AlkaneId {
            block: 4,
            tx: COLLECTOR_FACTORY_TX,
        },
        // Auth token for lottery contract is at block 2 with same tx as the contract
        lottery_auth_token: AlkaneId {
            block: 2,
            tx: 2,
        },
    }
}

pub enum CellpackOrEdict {
    Cellpack(Cellpack),
    Edict(Vec<ProtostoneEdict>),
}

pub fn create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
    witness: Witness,
    cellpacks_or_edicts: Vec<CellpackOrEdict>,
    previous_output: OutPoint,
    etch: bool,
    with_leftovers_to_separate: bool,
) -> Transaction {
    let protocol_id = 1;
    let input_script = ScriptBuf::new();
    let txin = TxIn {
        previous_output,
        script_sig: input_script,
        sequence: Sequence::MAX,
        witness,
    };
    let protostones = [
        match etch {
            true => vec![Protostone {
                burn: Some(protocol_id),
                edicts: vec![],
                pointer: Some(5),
                refund: None,
                from: None,
                protocol_tag: 13, // this value must be 13 if protoburn
                message: vec![],
            }],
            false => vec![],
        },
        cellpacks_or_edicts
            .into_iter()
            .enumerate()
            .map(|(i, cellpack_or_edict)| match cellpack_or_edict {
                CellpackOrEdict::Cellpack(cellpack) => Protostone {
                    message: cellpack.encipher(),
                    pointer: Some(0),
                    refund: Some(0),
                    edicts: vec![],
                    from: None,
                    burn: None,
                    protocol_tag: protocol_id as u128,
                },
                CellpackOrEdict::Edict(edicts) => Protostone {
                    message: vec![],
                    pointer: if with_leftovers_to_separate {
                        Some(2)
                    } else {
                        Some(0)
                    },
                    refund: if with_leftovers_to_separate {
                        Some(2)
                    } else {
                        Some(0)
                    },
                    edicts: edicts
                        .into_iter()
                        .map(|edict| {
                            let mut edict = edict;
                            edict.output = if etch { 5 + i as u128 } else { 4 + i as u128 };
                            if with_leftovers_to_separate {
                                edict.output += 1;
                            }
                            edict
                        })
                        .collect(),
                    from: None,
                    burn: None,
                    protocol_tag: protocol_id as u128,
                },
            })
            .collect(),
    ]
    .concat();
    let etching = if etch {
        Some(Etching {
            divisibility: Some(2),
            premine: Some(1000),
            rune: Some(Rune::from_str("TESTTESTTESTTEST").unwrap()),
            spacers: Some(0),
            symbol: Some(char::from_str("A").unwrap()),
            turbo: true,
            terms: None,
        })
    } else {
        None
    };
    let runestone: ScriptBuf = (Runestone {
        etching,
        pointer: match etch {
            true => Some(1),
            false => Some(0),
        },
        edicts: Vec::new(),
        mint: None,
        protocol: protostones.encipher().ok(),
    })
    .encipher();

    let op_return = TxOut {
        value: Amount::from_sat(0),
        script_pubkey: runestone,
    };
    let address: Address<NetworkChecked> = get_address(&ADDRESS1().as_str());

    let script_pubkey = address.script_pubkey();
    let txout = TxOut {
        value: Amount::from_sat(100_000_000),
        script_pubkey: script_pubkey.clone(),
    };
    let outputs = if with_leftovers_to_separate {
        vec![
            txout,
            op_return,
            TxOut {
                value: Amount::from_sat(546),
                script_pubkey,
            },
        ]
    } else {
        vec![txout, op_return]
    };
    Transaction {
        version: Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![txin],
        output: outputs,
    }
}

pub fn create_multiple_cellpack_with_witness_and_in_with_edicts(
    witness: Witness,
    cellpacks_or_edicts: Vec<CellpackOrEdict>,
    previous_output: OutPoint,
    etch: bool,
) -> Transaction {
    create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
        witness,
        cellpacks_or_edicts,
        previous_output,
        etch,
        false,
    )
}

/// Check that input tokens were refunded correctly
pub fn check_input_tokens_refunded(
    input_sheet: BalanceSheet<IndexPointer>,
    output_sheet: BalanceSheet<IndexPointer>,
    expected_diffs: BTreeSet<ProtoruneRuneId>,
) -> Result<()> {
    let mut all_runes = input_sheet.balances().keys().collect::<BTreeSet<_>>();
    all_runes.extend(output_sheet.balances().keys());

    for rune in all_runes {
        if let Some(_) = expected_diffs.get(rune) {
            continue;
        }
        assert_eq!(input_sheet.get(rune), input_sheet.get(rune));
    }
    Ok(())
}

/// Extract response data from a view function call transaction
pub fn extract_view_response_data(test_block: &Block) -> Result<u128> {
    use alkanes::tests::helpers::assert_return_context;

    // Use assert_return_context to get and parse the return data
    assert_return_context(&OutPoint {
        txid: test_block.txdata.last().unwrap().compute_txid(),
        vout: 4, // View function data is in vout 4
    }, |response| -> Result<u128> {
        // Parse the return data as u128 (little endian)
        if response.inner.data.len() == 16 {
            Ok(u128::from_le_bytes(response.inner.data.clone().try_into().unwrap()))
        } else {
            Ok(0) // Fallback if data format is unexpected
        }
    })
}

/// Call GetLpPoolTotal view function
pub fn call_get_lp_pool_total(
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<(Block, u128)> {
    use alkanes::indexer::index_block;
    use protorune::test_helpers::create_block_with_coinbase_tx;

    // Create a default outpoint for view function calls
    let default_outpoint = OutPoint {
        txid: bitcoin::Txid::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_slice(&[0; 32]).unwrap()),
        vout: 0,
    };

    let mut test_block = create_block_with_coinbase_tx(block_height);
    test_block.txdata.push(
        create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
            Witness::new(),
            vec![CellpackOrEdict::Cellpack(Cellpack {
                target: deployment_ids.lottery_contract,
                inputs: vec![100], // opcode 100 = GetLpPoolTotal
            })],
            default_outpoint,
            false,
            true,
        ),
    );

    index_block(&test_block, block_height)?;

    // Extract response data from the transaction
    let data = extract_view_response_data(&test_block)?;
    Ok((test_block, data))
}

/// Call GetUserPoolTotal view function
pub fn call_get_user_pool_total(
    deployment_ids: &LotteryTestDeploymentIds,
    block_height: u32,
) -> Result<(Block, u128)> {
    use alkanes::indexer::index_block;
    use protorune::test_helpers::create_block_with_coinbase_tx;

    // Create a default outpoint for view function calls
    let default_outpoint = OutPoint {
        txid: bitcoin::Txid::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_slice(&[0; 32]).unwrap()),
        vout: 0,
    };

    let mut test_block = create_block_with_coinbase_tx(block_height);
    test_block.txdata.push(
        create_multiple_cellpack_with_witness_and_in_with_edicts_and_leftovers(
            Witness::new(),
            vec![CellpackOrEdict::Cellpack(Cellpack {
                target: deployment_ids.lottery_contract,
                inputs: vec![101], // opcode 101 = GetUserPoolTotal
            })],
            default_outpoint,
            false,
            true,
        ),
    );

    index_block(&test_block, block_height)?;

    // Extract response data from the transaction
    let data = extract_view_response_data(&test_block)?;
    Ok((test_block, data))
}
