use alkanes_runtime::{
    auth::AuthenticatedResponder, declare_alkane, message::MessageDispatch,
    runtime::AlkaneResponder, storage::StoragePointer,
};

use alkanes_macros::{mapping_variable, storage_variable};
#[allow(unused_imports)]
use alkanes_runtime::{
    println,
    stdio::{stdout, Write},
};
use alkanes_std_factory_support::MintableToken;
use alkanes_support::{
    cellpack::Cellpack,
    checked_expr,
    context::Context,
    id::AlkaneId,
    parcel::{AlkaneTransfer, AlkaneTransferParcel},
    response::CallResponse,
    utils::{overflow_error, shift, shift_or_err},
};
use anyhow::{anyhow, Result};
use bitcoin::{Block, BlockHash};
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::{index_pointer::KeyValuePointer, utils::consume_u128};
use oylswap_library::U256;
use protorune_support::balance_sheet::{BalanceSheetOperations, CachedBalanceSheet};
use protorune_support::utils::consensus_decode;
use std::{cmp::min, sync::Arc};

#[derive(MessageDispatch)]
pub enum LotteryContractMessage {
    #[opcode(0)]
    Initialize {
        token_id: AlkaneId,
        ticket_price: u128, // price of single ticket in token
    },
    #[opcode(1)]
    LpDeposit { amount_desired: u128 },
    // #[opcode(3)]
    // PurchaseTickets,
    // #[opcode(4)]
    // RunJackpot,
    // #[opcode(5)]
    // WithdrawWinnings,
    // #[opcode(7)]
    // WithdrawProtocolFees,
    #[opcode(8)]
    LpWithdraw { amount_desired: u128 },
    // Admin functions
    #[opcode(20)]
    AdminSetTicketPrice { new_price: u128 },
    #[opcode(21)]
    AdminSetRoundDurationInBlocks { duration: u128 },
    #[opcode(23)]
    AdminSetFeeBps { bps: u128 },
    #[opcode(24)]
    AdminSetLpPoolCap { cap: u128 },
    #[opcode(25)]
    AdminSetProtocolFeeBps { bps: u128 },
    #[opcode(27)]
    AdminForceReleaseJackpotLock,
    #[opcode(29)]
    AdminSetLpLimit { limit: u128 },
    #[opcode(30)]
    AdminSetUserLimit { limit: u128 },
    #[opcode(31)]
    AdminSetMinLpDeposit { min_deposit: u128 },
    #[opcode(32)]
    AdminSetAllowPurchasing { allow: u128 },

    #[opcode(99)]
    #[returns(String)]
    GetName,
}

#[derive(Default)]
pub struct LotteryContract();

impl MintableToken for LotteryContract {}
impl AlkaneResponder for LotteryContract {}
impl AuthenticatedResponder for LotteryContract {}

impl LotteryContract {
    storage_variable!(token: AlkaneId);
    storage_variable!(fee_bps: u128);
    storage_variable!(protocol_fee_bps: u128);
    storage_variable!(ticket_price: u128);
    storage_variable!(round_duration_in_blocks: u128);
    storage_variable!(last_jackpot_end_block: u128);
    storage_variable!(lp_pool_total: u128);
    storage_variable!(lp_pool_cap: u128);
    storage_variable!(user_pool_total: u128);
    storage_variable!(ticket_count_total_bps: u128);
    storage_variable!(last_winner_id: AlkaneId);
    storage_variable!(jackpot_lock: u128);
    storage_variable!(all_fees_total: u128);
    storage_variable!(lp_fees_total: u128);
    storage_variable!(protocol_fee_claimable: u128);
    storage_variable!(lp_limit: u128);
    storage_variable!(min_lp_deposit: u128);
    storage_variable!(user_limit: u128);
    storage_variable!(allow_purchasing: u128);

    // Helper functions for User storage
    // mapping_variable!(User, "/user/");

    // Helper functions for active user addresses array
    // fn active_user_addresses_pointer(&self) -> StoragePointer {
    //     StoragePointer::from_keyword("/active_user_addresses")
    // }

    // fn get_active_user_addresses(&self) -> Vec<AlkaneId> {
    //     let ptr = self.active_user_addresses_pointer();
    //     let bytes = ptr.get();
    //     if bytes.is_empty() {
    //         return Vec::new();
    //     }
    //     // Deserialize Vec<AlkaneId>
    //     let mut addresses = Vec::new();
    //     let mut offset = 0;
    //     while offset + 32 <= bytes.len() {
    //         let block = u128::from_le_bytes(
    //             bytes[offset..offset + 16].try_into().unwrap_or([0u8; 16])
    //         );
    //         offset += 16;
    //         let tx = u128::from_le_bytes(
    //             bytes[offset..offset + 16].try_into().unwrap_or([0u8; 16])
    //         );
    //         offset += 16;
    //         addresses.push(AlkaneId::new(block, tx));
    //     }
    //     addresses
    // }

    // fn set_active_user_addresses(&self, addresses: &Vec<AlkaneId>) {
    //     let mut bytes = Vec::new();
    //     for addr in addresses {
    //         bytes.extend_from_slice(&addr.block.to_le_bytes());
    //         bytes.extend_from_slice(&addr.tx.to_le_bytes());
    //     }
    //     self.active_user_addresses_pointer().set(Arc::new(bytes));
    // }

    // // Helper functions for active LP addresses array
    // fn active_lp_addresses_pointer(&self) -> StoragePointer {
    //     StoragePointer::from_keyword("/active_lp_addresses")
    // }

    // fn get_active_lp_addresses(&self) -> Vec<AlkaneId> {
    //     let ptr = self.active_lp_addresses_pointer();
    //     let bytes = ptr.get();
    //     if bytes.is_empty() {
    //         return Vec::new();
    //     }
    //     let mut addresses = Vec::new();
    //     let mut offset = 0;
    //     while offset + 32 <= bytes.len() {
    //         let block = u128::from_le_bytes(
    //             bytes[offset..offset + 16].try_into().unwrap_or([0u8; 16])
    //         );
    //         offset += 16;
    //         let tx = u128::from_le_bytes(
    //             bytes[offset..offset + 16].try_into().unwrap_or([0u8; 16])
    //         );
    //         offset += 16;
    //         addresses.push(AlkaneId::new(block, tx));
    //     }
    //     addresses
    // }

    // fn set_active_lp_addresses(&self, addresses: &Vec<AlkaneId>) {
    //     let mut bytes = Vec::new();
    //     for addr in addresses {
    //         bytes.extend_from_slice(&addr.block.to_le_bytes());
    //         bytes.extend_from_slice(&addr.tx.to_le_bytes());
    //     }
    //     self.active_lp_addresses_pointer().set(Arc::new(bytes));
    // }

    // // Helper to get block timestamp
    // fn block_timestamp(&self) -> Result<u32> {
    //     Ok(self.block_header()?.time)
    // }

    fn _refund_and_check_inputs(
        &self,
        desired_input_token: AlkaneId,
        desired_input_amount: u128,
    ) -> Result<CallResponse> {
        let context = self.context()?;
        let mut token_received: u128 = 0;
        let mut ret = CallResponse::default();
        for alkane_transfer in context.incoming_alkanes.0.clone() {
            if alkane_transfer.id != desired_input_token {
                ret.alkanes.pay(alkane_transfer);
            } else {
                token_received += alkane_transfer.value;
            }
        }
        if desired_input_amount > token_received {
            return Err(anyhow!(format!(
                "desired amount ({}) is greater than amount input ({})",
                desired_input_amount, token_received
            )));
        }
        ret.alkanes.pay(AlkaneTransfer {
            id: desired_input_token,
            value: token_received - desired_input_amount,
        });
        Ok(ret)
    }

    fn initialize(&self, token_id: AlkaneId, ticket_price: u128) -> Result<CallResponse> {
        self.observe_initialization()?;
        let context = self.context()?;
        self.set_token(token_id.clone());
        self.set_name_and_symbol_str("Lottery LP".to_string(), "LTY LP".to_string());

        self.set_ticket_price(ticket_price);

        // Initialize default values
        self.set_fee_bps(1500u128); // 15%
        self.set_protocol_fee_bps(10u128); // 0.1%
        self.set_round_duration_in_blocks(144u128); // 1 day
        self.set_lp_limit(100u128);
        self.set_user_limit(1500u128);
        self.set_allow_purchasing(0u128); // false
        self.set_last_jackpot_end_block(self.height() as u128);

        let min_lp_deposit = ticket_price * 100;
        self.set_min_lp_deposit(min_lp_deposit);
        self.set_lp_pool_cap(min_lp_deposit * 1000);

        Ok(CallResponse::forward(&context.incoming_alkanes))
    }

    // Helper functions for fee calculations
    // fn calculate_fees(&self, used_amount: u128, referrer: Option<&AlkaneId>) -> (u128, u128, u128) {
    //     let fee_bps = self.fee_bps_pointer().get_value::<u128>();
    //     let referral_fee_bps = self.referral_fee_bps_pointer().get_value::<u128>();
    //     let all_fee_amount = (used_amount * fee_bps) / 10000;
    //     let referral_fee_amount = if referrer.is_some() {
    //         (used_amount * referral_fee_bps) / 10000
    //     } else {
    //         0
    //     };
    //     let lp_fee_amount = all_fee_amount - referral_fee_amount;
    //     (all_fee_amount, referral_fee_amount, lp_fee_amount)
    // }

    // fn update_fee_totals(&self, all_fee_amount: u128, referral_fee_amount: u128, lp_fee_amount: u128, referrer: Option<&AlkaneId>) {
    //     let all_fees = self.all_fees_total_pointer().get_value::<u128>();
    //     self.all_fees_total_pointer().set_value(all_fees + all_fee_amount);

    //     if let Some(ref_addr) = referrer {
    //         let current = self.get_referral_fees_claimable(ref_addr);
    //         self.set_referral_fees_claimable(ref_addr, current + referral_fee_amount);
    //         let total = self.referral_fees_total_pointer().get_value::<u128>();
    //         self.referral_fees_total_pointer().set_value(total + referral_fee_amount);
    //     }

    //     let lp_fees = self.lp_fees_total_pointer().get_value::<u128>();
    //     self.lp_fees_total_pointer().set_value(lp_fees + lp_fee_amount);
    // }

    // fn process_ticket_purchase(&self, actual_received: u128, user_address: &AlkaneId) -> Result<(u128, u128)> {
    //     let ticket_price = self.ticket_price();
    //     let ticket_count = actual_received / ticket_price;
    //     if ticket_count == 0 {
    //         return Err(anyhow!("Insufficient amount for minimum ticket purchase"));
    //     }

    //     let used_amount = ticket_count * ticket_price;
    //     let fee_bps = self.fee_bps_pointer().get_value::<u128>();
    //     let tickets_purchased_bps = ticket_count * (10000 - fee_bps);

    //     let mut user = self.get_user(user_address);
    //     if !user.active {
    //         let active_users = self.get_active_user_addresses();
    //         let user_limit = self.user_limit_pointer().get_value::<u128>();
    //         if active_users.len() as u128 >= user_limit {
    //             return Err(anyhow!("Max user limit reached"));
    //         }
    //         user.active = true;
    //         let mut new_active = active_users;
    //         new_active.push(user_address.clone());
    //         self.set_active_user_addresses(&new_active);
    //     }

    //     user.tickets_purchased_total_bps += tickets_purchased_bps;
    //     self.set_user(user_address, user);

    //     let ticket_count_total = self.ticket_count_total_bps_pointer().get_value::<u128>();
    //     self.ticket_count_total_bps_pointer().set_value(ticket_count_total + tickets_purchased_bps);

    //     Ok((tickets_purchased_bps, used_amount))
    // }

    fn lottery_assets_before_transfer(&self) -> Result<u128> {
        let context = self.context()?;
        let lottery_token = self.token()?;
        let current_assets = self.balance(&context.myself, &lottery_token);
        let incoming_balance: u128 = context
            .incoming_alkanes
            .0
            .iter()
            .filter(|t| t.id == lottery_token)
            .map(|t| t.value)
            .sum();
        Ok(current_assets - incoming_balance)
    }

    // // LP Deposit function
    fn lp_deposit(&self, amount_desired: u128) -> Result<CallResponse> {
        if self.jackpot_lock() != 0 {
            return Err(anyhow!("Jackpot is currently running!"));
        }

        let context = self.context()?;
        let lottery_token = self.token()?;

        // Calculate actual received amount
        let incoming_balance: u128 = context
            .incoming_alkanes
            .0
            .iter()
            .filter(|t| t.id == lottery_token)
            .map(|t| t.value)
            .sum();
        let actual_received = incoming_balance;

        if actual_received == 0 || actual_received > amount_desired {
            return Err(anyhow!(
                "Invalid deposit amount, must be greater than amount_desired and nonzero"
            ));
        }
        let true_amount_desired = if amount_desired == 0 {
            actual_received
        } else {
            amount_desired
        };

        let ticket_price = self.ticket_price();
        let floored_value = (true_amount_desired / ticket_price) * ticket_price;

        if floored_value == 0 {
            return Err(anyhow!(
                "Invalid deposit amount, must be greater than ticket price"
            ));
        }

        let lp_pool_total = self.lp_pool_total();
        let lp_pool_cap = self.lp_pool_cap();
        if lp_pool_total + floored_value > lp_pool_cap {
            return Err(anyhow!("Deposit exceeds LP pool cap"));
        }

        let total_shares = self.total_supply();
        let current_assets = lp_pool_total;
        let shares: u128 = if current_assets == 0 {
            floored_value
        } else {
            (U256::from(floored_value) * U256::from(total_shares) / U256::from(current_assets))
                .try_into()?
        };

        self.set_lp_pool_total(current_assets + floored_value);

        let shares_transfer = self.mint(&context, shares)?;

        let mut response = self._refund_and_check_inputs(lottery_token, floored_value)?;
        response.alkanes.pay(shares_transfer);
        Ok(response)
    }

    // fn purchase_tickets(&self) -> Result<CallResponse> {
    //     if self.allow_purchasing_pointer().get_value::<u128>() == 0 {
    //         return Err(anyhow!("Purchasing tickets not allowed"));
    //     }

    //     if self.jackpot_lock_pointer().get_value::<u128>() != 0 {
    //         return Err(anyhow!("Jackpot is currently running!"));
    //     }

    //     let context = self.context()?;
    //     if let Some(ref ref_addr) = referrer {
    //         if *ref_addr == context.caller {
    //             return Err(anyhow!("Cannot refer yourself"));
    //         }
    //     }

    //     let token_id = self.token_id()?;
    //     let balance_before = self.get_token_balance(&context.myself, &token_id);
    //     let incoming_balance: u128 = context.incoming_alkanes.0.iter()
    //         .filter(|t| t.id == token_id)
    //         .map(|t| t.value)
    //         .sum();
    //     let balance_after = balance_before + incoming_balance;
    //     let actual_received = balance_after - balance_before;

    //     if actual_received == 0 {
    //         return Err(anyhow!("Invalid purchase amount, must be positive"));
    //     }

    //     let user_address = match recipient {
    //         Some(addr) if addr != context.caller => addr,
    //         _ => context.caller.clone(),
    //     };

    //     let (tickets_purchased_bps, used_amount) = self.process_ticket_purchase(actual_received, &user_address)?;

    //     let (all_fee_amount, referral_fee_amount, lp_fee_amount) = self.calculate_fees(used_amount, referrer.as_ref());
    //     self.update_fee_totals(all_fee_amount, referral_fee_amount, lp_fee_amount, referrer.as_ref());

    //     let user_pool_total = self.user_pool_total_pointer().get_value::<u128>();
    //     self.user_pool_total_pointer().set_value(user_pool_total + used_amount - all_fee_amount);

    //     let remainder = actual_received - used_amount;
    //     let mut response = CallResponse::forward(&context.incoming_alkanes);
    //     if remainder > 0 {
    //         response.alkanes.pay(AlkaneTransfer {
    //             id: token_id,
    //             value: remainder,
    //         });
    //     }
    //     Ok(response)
    // }

    // // Convert block hash to u128 for modulo operations
    // fn block_hash_to_u128(&self) -> Result<u128> {
    //     let block_header = self.block_header()?;
    //     let block_hash = block_header.block_hash();
    //     // Take first 16 bytes of block hash as u128
    //     // BlockHash in bitcoin crate can be converted to byte array
    //     let hash_bytes = block_hash.as_byte_array();
    //     Ok(u128::from_le_bytes([
    //         hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
    //         hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
    //         hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
    //         hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
    //     ]))
    // }

    // fn get_winning_ticket(&self, max: u128) -> Result<u128> {
    //     let random_value = self.block_hash_to_u128()?;
    //     Ok((random_value % max) + 1)
    // }

    // fn find_winner_from_users(&self, winning_ticket: u128) -> AlkaneId {
    //     let active_users = self.get_active_user_addresses();
    //     let mut cumulative_tickets_bps = 0u128;
    //     for user_address in active_users {
    //         let user = self.get_user(&user_address);
    //         cumulative_tickets_bps += user.tickets_purchased_total_bps;
    //         if winning_ticket <= cumulative_tickets_bps {
    //             return user_address;
    //         }
    //     }
    //     // No winner found, return fallback winner
    //     let ptr = self.fallback_winner_pointer().get();
    //     if ptr.len() >= 32 {
    //         let block = u128::from_le_bytes(ptr[0..16].try_into().unwrap_or([0u8; 16]));
    //         let tx = u128::from_le_bytes(ptr[16..32].try_into().unwrap_or([0u8; 16]));
    //         AlkaneId::new(block, tx)
    //     } else {
    //         self.context().unwrap().myself
    //     }
    // }

    // fn distribute_lp_fees_to_lps(&self) {
    //     let lp_pool_total = self.lp_pool_total_pointer().get_value::<u128>();
    //     let lp_fees_total = self.lp_fees_total_pointer().get_value::<u128>();

    //     if lp_pool_total == 0 {
    //         // If no LPs have staked, distribute LP fees to the user pool
    //         let user_pool = self.user_pool_total_pointer().get_value::<u128>();
    //         self.user_pool_total_pointer().set_value(user_pool + lp_fees_total);
    //         self.lp_fees_total_pointer().set_value(0);
    //         return;
    //     }

    //     // Check protocol fee
    //     let protocol_fee_address_ptr = self.protocol_fee_address_pointer().get();
    //     let protocol_fee_threshold = self.protocol_fee_threshold_pointer().get_value::<u128>();
    //     let mut lp_fees_remaining = lp_fees_total;

    //     if !protocol_fee_address_ptr.is_empty() && lp_fees_total >= protocol_fee_threshold {
    //         let protocol_fee = lp_fees_total / 10;
    //         lp_fees_remaining -= protocol_fee;
    //         let current = self.protocol_fee_claimable_pointer().get_value::<u128>();
    //         self.protocol_fee_claimable_pointer().set_value(current + protocol_fee);
    //     }

    //     let token_decimals = self.token_decimals_pointer().get_value::<u128>();
    //     let decimals_multiplier = 10u128.pow(token_decimals as u32);
    //     let mut total_distributed = 0u128;

    //     let active_lps = self.get_active_lp_addresses();
    //     for lp_address in active_lps {
    //         let mut lp = self.get_lp(&lp_address);
    //         if lp.active {
    //             // Calculate proportion of lp.stake to lpPoolTotal
    //             let lp_fees_share = ((lp_fees_remaining * decimals_multiplier * lp.stake) / lp_pool_total) / decimals_multiplier;
    //             lp.principal += lp_fees_share;
    //             total_distributed += lp_fees_share;
    //             self.set_lp(&lp_address, lp);
    //         }
    //     }

    //     self.lp_fees_total_pointer().set_value(lp_fees_remaining - total_distributed);
    // }

    // fn distribute_user_pool_to_lps(&self) {
    //     let lp_pool_total = self.lp_pool_total_pointer().get_value::<u128>();
    //     if lp_pool_total == 0 {
    //         return;
    //     }

    //     let user_pool_total = self.user_pool_total_pointer().get_value::<u128>();
    //     let token_decimals = self.token_decimals_pointer().get_value::<u128>();
    //     let decimals_multiplier = 10u128.pow(token_decimals as u32);

    //     let active_lps = self.get_active_lp_addresses();
    //     for lp_address in active_lps {
    //         let mut lp = self.get_lp(&lp_address);
    //         if lp.active {
    //             let user_pool_share = ((user_pool_total * decimals_multiplier * lp.stake) / lp_pool_total) / decimals_multiplier;
    //             lp.principal += user_pool_share;
    //             self.set_lp(&lp_address, lp);
    //         }
    //     }
    // }

    // fn return_lp_pool_back_to_lps(&self) {
    //     let active_lps = self.get_active_lp_addresses();
    //     for lp_address in active_lps {
    //         let mut lp = self.get_lp(&lp_address);
    //         if lp.active {
    //             lp.principal += lp.stake;
    //             lp.stake = 0;
    //             self.set_lp(&lp_address, lp);
    //         }
    //     }
    // }

    // fn stake_lps(&self) {
    //     let active_lps = self.get_active_lp_addresses();
    //     let mut lp_pool_total = 0u128;

    //     for lp_address in active_lps.clone() {
    //         let mut lp = self.get_lp(&lp_address);
    //         if lp.active {
    //             let principal = lp.principal;
    //             let stake = (principal * lp.risk_percentage) / 100;
    //             lp.stake = stake;
    //             lp_pool_total += stake;
    //             lp.principal = principal - stake;
    //             self.set_lp(&lp_address, lp);
    //         }
    //     }

    //     self.lp_pool_total_pointer().set_value(lp_pool_total);
    // }

    // fn clear_user_ticket_purchases(&self) {
    //     let active_users = self.get_active_user_addresses();
    //     for user_address in &active_users {
    //         let mut user = self.get_user(user_address);
    //         user.tickets_purchased_total_bps = 0;
    //         user.active = false;
    //         self.set_user(user_address, user);
    //     }
    //     self.set_active_user_addresses(&Vec::new());
    // }

    // fn determine_winner_and_adjust_stakes(&self) -> Result<()> {
    //     let current_time = self.block_timestamp()? as u128;
    //     self.last_jackpot_end_time_pointer().set_value(current_time);

    //     let ticket_count_total_bps = self.ticket_count_total_bps_pointer().get_value::<u128>();

    //     // No tickets bought
    //     if ticket_count_total_bps == 0 {
    //         self.return_lp_pool_back_to_lps();
    //         self.lp_pool_total_pointer().set_value(0);
    //         self.stake_lps();
    //         return Ok(());
    //     }

    //     // Distribute LP fees to LP's
    //     self.distribute_lp_fees_to_lps();

    //     let user_pool_total = self.user_pool_total_pointer().get_value::<u128>();
    //     let lp_pool_total = self.lp_pool_total_pointer().get_value::<u128>();
    //     let ticket_price = self.ticket_price();

    //     if user_pool_total >= lp_pool_total {
    //         // Jackpot is fully funded by users
    //         let winning_ticket = self.get_winning_ticket(ticket_count_total_bps)?;
    //         let winner_address = self.find_winner_from_users(winning_ticket);
    //         self.last_winner_address_pointer().set(Arc::new(winner_address.into()));

    //         let win_amount = user_pool_total;
    //         let mut winner = self.get_user(&winner_address);
    //         winner.winnings_claimable += win_amount;
    //         self.set_user(&winner_address, winner);

    //         self.return_lp_pool_back_to_lps();
    //     } else {
    //         // Jackpot is partially funded by LP's
    //         let total_tickets = (lp_pool_total * 10000) / ticket_price;
    //         let winning_ticket = self.get_winning_ticket(total_tickets)?;

    //         if winning_ticket <= ticket_count_total_bps {
    //             // Won by a user
    //             let winner_address = self.find_winner_from_users(winning_ticket);
    //             self.last_winner_address_pointer().set(Arc::new(winner_address.into()));

    //             let win_amount = lp_pool_total;
    //             let mut winner = self.get_user(&winner_address);
    //             winner.winnings_claimable += win_amount;
    //             self.set_user(&winner_address, winner);

    //             self.distribute_user_pool_to_lps();
    //         } else {
    //             // Won by LP's
    //             self.last_winner_address_pointer().set(Arc::new(AlkaneId::default().into()));
    //             self.distribute_user_pool_to_lps();
    //             self.return_lp_pool_back_to_lps();
    //         }
    //     }

    //     // Reset for next round
    //     self.clear_user_ticket_purchases();
    //     self.user_pool_total_pointer().set_value(0);
    //     self.lp_pool_total_pointer().set_value(0);
    //     self.ticket_count_total_bps_pointer().set_value(0);
    //     self.all_fees_total_pointer().set_value(0);
    //     self.referral_fees_total_pointer().set_value(0);

    //     // Stake the LP's
    //     self.stake_lps();

    //     Ok(())
    // }

    // fn run_jackpot(&self) -> Result<CallResponse> {
    //     let current_time = self.block_timestamp()? as u128;
    //     let last_jackpot_end_time = self.last_jackpot_end_time_pointer().get_value::<u128>();
    //     let round_duration = self.round_duration_in_seconds_pointer().get_value::<u128>();

    //     if current_time < last_jackpot_end_time + round_duration {
    //         return Err(anyhow!("Jackpot can only be run once per round"));
    //     }

    //     if self.jackpot_lock_pointer().get_value::<u128>() != 0 {
    //         return Err(anyhow!("Jackpot is currently running!"));
    //     }

    //     // Acquire jackpot lock
    //     self.jackpot_lock_pointer().set_value(1u128);

    //     // Use block hash as entropy source (instead of Pyth oracle)
    //     self.determine_winner_and_adjust_stakes()?;

    //     // Release jackpot lock
    //     self.jackpot_lock_pointer().set_value(0u128);

    //     Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    // }

    // fn withdraw_winnings(&self) -> Result<CallResponse> {
    //     let context = self.context()?;
    //     let mut user = self.get_user(&context.caller);

    //     if user.winnings_claimable == 0 {
    //         return Err(anyhow!("No winnings to withdraw"));
    //     }

    //     let transfer_amount = user.winnings_claimable;
    //     user.winnings_claimable = 0;
    //     self.set_user(&context.caller, user);

    //     let token_id = self.token_id()?;
    //     self.transfer_tokens(&context.caller, &token_id, transfer_amount)
    // }

    // fn withdraw_referral_fees(&self) -> Result<CallResponse> {
    //     let context = self.context()?;
    //     let claimable = self.get_referral_fees_claimable(&context.caller);

    //     if claimable == 0 {
    //         return Err(anyhow!("No referral fees to withdraw"));
    //     }

    //     self.set_referral_fees_claimable(&context.caller, 0);

    //     let token_id = self.token_id()?;
    //     self.transfer_tokens(&context.caller, &token_id, claimable)
    // }

    // fn withdraw_protocol_fees(&self) -> Result<CallResponse> {
    //     self.only_owner()?;

    //     let protocol_fee_claimable = self.protocol_fee_claimable_pointer().get_value::<u128>();
    //     if protocol_fee_claimable == 0 {
    //         return Err(anyhow!("No protocol fees to withdraw"));
    //     }

    //     self.protocol_fee_claimable_pointer().set_value(0);

    //     let protocol_fee_address_ptr = self.protocol_fee_address_pointer().get();
    //     if protocol_fee_address_ptr.is_empty() {
    //         return Err(anyhow!("Protocol fee address not set"));
    //     }

    //     let block = u128::from_le_bytes(protocol_fee_address_ptr[0..16].try_into().unwrap_or([0u8; 16]));
    //     let tx = u128::from_le_bytes(protocol_fee_address_ptr[16..32].try_into().unwrap_or([0u8; 16]));
    //     let protocol_fee_address = AlkaneId::new(block, tx);

    //     let token_id = self.token_id()?;
    //     self.transfer_tokens(&protocol_fee_address, &token_id, protocol_fee_claimable)
    // }

    fn lp_withdraw(&self, amount_desired: u128) -> Result<CallResponse> {
        let context = self.context()?;
        let token_id = self.token()?;
        let vault_token = context.myself.clone();
        let ticket_price = self.ticket_price();

        let incoming_shares: u128 = context
            .incoming_alkanes
            .0
            .iter()
            .filter(|t| t.id == vault_token)
            .map(|t| t.value)
            .sum();

        if incoming_shares == 0 || incoming_shares > amount_desired {
            return Err(anyhow!("No shares provided for withdrawal, or amount_desired is greater than incoming shares"));
        }

        let true_amount_desired = if amount_desired == 0 {
            incoming_shares
        } else {
            amount_desired
        };

        if self.jackpot_lock() != 0 {
            return Err(anyhow!("Jackpot is currently running!"));
        }

        let total_shares = self.total_supply();
        if total_shares < true_amount_desired {
            return Err(anyhow!("Insufficient shares"));
        }

        let current_assets = self.lp_pool_total();
        let amount_out = if total_shares == 0 {
            0
        } else {
            (U256::from(true_amount_desired) * U256::from(current_assets)
                / U256::from(total_shares))
            .try_into()?
        };

        let floored_amount_out = (amount_out / ticket_price) * ticket_price;

        if floored_amount_out > current_assets {
            return Err(anyhow!("Insufficient assets"));
        }

        self.decrease_total_supply(true_amount_desired)?;
        self.set_lp_pool_total(current_assets - floored_amount_out);

        let mut response = self._refund_and_check_inputs(vault_token, true_amount_desired)?;
        response.alkanes.pay(AlkaneTransfer {
            id: token_id,
            value: floored_amount_out,
        });
        Ok(response)
    }

    // Admin functions
    fn admin_set_ticket_price(&self, new_price: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_ticket_price(new_price);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_round_duration_in_blocks(&self, duration: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_round_duration_in_blocks(duration);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_fee_bps(&self, bps: u128) -> Result<CallResponse> {
        self.only_owner()?;
        if bps > 8000 {
            return Err(anyhow!("Fee bps should not exceed 8000"));
        }
        self.set_fee_bps(bps);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_protocol_fee_bps(&self, bps: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_protocol_fee_bps(bps);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_lp_pool_cap(&self, cap: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_lp_pool_cap(cap);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_force_release_jackpot_lock(&self) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_jackpot_lock(0u128);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_lp_limit(&self, limit: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_lp_limit(limit);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_user_limit(&self, limit: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_user_limit(limit);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_min_lp_deposit(&self, min_deposit: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_min_lp_deposit(min_deposit);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn admin_set_allow_purchasing(&self, allow: u128) -> Result<CallResponse> {
        self.only_owner()?;
        self.set_allow_purchasing(allow);
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn get_name(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response: CallResponse = CallResponse::forward(&context.incoming_alkanes);
        response.data = self.name().into_bytes().to_vec();
        Ok(response)
    }

    fn forward_incoming(&self) -> Result<CallResponse> {
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }
}

declare_alkane! {
    impl AlkaneResponder for LotteryContract {
        type Message = LotteryContractMessage;
    }
}
