use alkanes_runtime::{
    auth::AuthenticatedResponder, declare_alkane, message::MessageDispatch,
    runtime::AlkaneResponder,
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
    id::AlkaneId,
    parcel::{AlkaneTransfer, AlkaneTransferParcel},
    response::CallResponse,
};
use anyhow::{anyhow, Result};
use bitcoin::hashes::Hash;
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use oylswap_library::U256;

#[derive(MessageDispatch)]
pub enum LotteryContractMessage {
    #[opcode(0)]
    Initialize {
        token_id: AlkaneId,
        ticket_price: u128, // price of single ticket in token
        collector_factory_id: AlkaneId,
        enable_purchasing: u128, // 0 = false, non-zero = true
    },
    #[opcode(1)]
    LpDeposit { amount_desired: u128 },
    #[opcode(2)]
    MintAndBuy,
    #[opcode(3)]
    PurchaseTickets {
        collector_id: AlkaneId,
    },
    #[opcode(4)]
    RunJackpot,
    #[opcode(5)]
    WithdrawWinnings {
        collector_id: AlkaneId,
    },
    #[opcode(7)]
    WithdrawProtocolFees,
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

    #[opcode(50)]
    Forward,

    #[opcode(99)]
    #[returns(String)]
    GetName,
    #[opcode(100)]
    #[returns(u128)]
    GetLpPoolTotal,
    #[opcode(101)]
    #[returns(u128)]
    GetUserPoolTotal,
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

    storage_variable!(num_unique_ticket_collectors_for_round: u128);
    // Maps collector_id -> tickets_bps for the current round
    mapping_variable!(tickets_for_collector: (AlkaneId, u128));
    // Maps index -> collector_id for iteration (stores block and tx as two u128s)
    mapping_variable!(collector_at_index_block: (u128, u128));
    mapping_variable!(collector_at_index_tx: (u128, u128));

    storage_variable!(collector_factory_id: AlkaneId);
    storage_variable!(next_ticket_collector_id: u128);

    mapping_variable!(reward_claimable_for_collector: (AlkaneId, u128));

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

    fn initialize(
        &self,
        token_id: AlkaneId,
        ticket_price: u128,
        collector_factory_id: AlkaneId,
        enable_purchasing: u128,
    ) -> Result<CallResponse> {
        self.observe_initialization()?;
        let context = self.context()?;
        self.set_token(token_id.clone());
        self.set_name_and_symbol_str("Lottery LP".to_string(), "LTY LP".to_string());

        self.set_ticket_price(ticket_price);
        self.set_collector_factory_id(collector_factory_id.clone());
        self.set_next_ticket_collector_id(0u128);
        self.set_num_unique_ticket_collectors_for_round(0u128);
        self.set_ticket_count_total_bps(0u128);

        // Initialize default values
        self.set_fee_bps(1500u128); // 15%
        self.set_protocol_fee_bps(10u128); // 0.1%
        self.set_round_duration_in_blocks(144u128); // 1 day
        self.set_lp_limit(100u128);
        self.set_user_limit(1500u128);
        self.set_allow_purchasing(enable_purchasing); // Set based on parameter
        self.set_last_jackpot_end_block(self.height() as u128);

        let min_lp_deposit = ticket_price * 100;
        self.set_min_lp_deposit(min_lp_deposit);
        self.set_lp_pool_cap(min_lp_deposit * 1000);

        // Build response with auth token deployed to caller
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        
        // Deploy auth token (1 unit) to the caller for admin operations
        response.alkanes.0.push(self.deploy_auth_token(1)?);

        Ok(response)
    }

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

    /// LP Deposit function - deposit tokens in exchange for LP shares
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

    /// Helper function to process ticket purchase logic
    /// Returns the amount used for tickets (for refund calculation)
    fn process_ticket_purchase(&self, collector_id: AlkaneId, incoming_balance: u128) -> Result<u128> {
        let ticket_price = self.ticket_price();
        let fee_bps = self.fee_bps();

        // Calculate ticket count and used amount
        let ticket_count = incoming_balance / ticket_price;
        if ticket_count == 0 {
            return Err(anyhow!("Insufficient amount for minimum ticket purchase"));
        }

        let used_amount = ticket_count * ticket_price;
        let tickets_purchased_bps = ticket_count * (10000 - fee_bps);

        // Calculate fees
        let all_fee_amount = (used_amount * fee_bps) / 10000;
        let protocol_fee_bps = self.protocol_fee_bps();
        let protocol_fee = (used_amount * protocol_fee_bps) / 10000;
        let lp_fee_amount = all_fee_amount - protocol_fee;

        // Update fee totals
        let all_fees = self.all_fees_total();
        self.set_all_fees_total(all_fees + all_fee_amount);

        let lp_fees = self.lp_fees_total();
        self.set_lp_fees_total(lp_fees + lp_fee_amount);

        let protocol_claimable = self.protocol_fee_claimable();
        self.set_protocol_fee_claimable(protocol_claimable + protocol_fee);

        // Update user pool
        let user_pool = self.user_pool_total();
        self.set_user_pool_total(user_pool + used_amount - all_fee_amount);

        // Update ticket count for this collector
        let current_tickets = self.tickets_for_collector(collector_id.clone());
        if current_tickets == 0 {
            // New collector for this round, add to index list
            let num_collectors = self.num_unique_ticket_collectors_for_round();
            self.set_collector_at_index_block(num_collectors, collector_id.block);
            self.set_collector_at_index_tx(num_collectors, collector_id.tx);
            self.set_num_unique_ticket_collectors_for_round(num_collectors + 1);
        }
        self.set_tickets_for_collector(collector_id, current_tickets + tickets_purchased_bps);

        // Update total ticket count
        let total_tickets = self.ticket_count_total_bps();
        self.set_ticket_count_total_bps(total_tickets + tickets_purchased_bps);

        Ok(used_amount)
    }

    /// Mint a new ticket collector NFT and buy tickets in one operation
    /// Calls the collector factory to deploy a new collector NFT, then purchases tickets for it
    fn mint_and_buy(&self) -> Result<CallResponse> {
        if self.allow_purchasing() == 0 {
            return Err(anyhow!("Purchasing tickets not allowed"));
        }

        if self.jackpot_lock() != 0 {
            return Err(anyhow!("Jackpot is currently running!"));
        }

        let context = self.context()?;
        let lottery_token = self.token()?;

        // Calculate incoming token balance
        let incoming_balance: u128 = context
            .incoming_alkanes
            .0
            .iter()
            .filter(|t| t.id == lottery_token)
            .map(|t| t.value)
            .sum();

        if incoming_balance == 0 {
            return Err(anyhow!("No tokens received for ticket purchase"));
        }

        // --- MINT COLLECTOR ---
        let collector_factory_id = self.collector_factory_id()?;
        
        // Get next collector id and increment
        let next_id = self.next_ticket_collector_id();
        self.set_next_ticket_collector_id(next_id + 1);
        
        // Determine the target block based on collector_factory_id.block
        // Block 5 if factory is at block 2 (standard factory)
        // Block 6 if factory is at block 4 (proxy factory)
        let target_block = if collector_factory_id.block == 2 {
            5
        } else if collector_factory_id.block == 4 {
            6
        } else {
            return Err(anyhow!("Invalid collector factory block: {}", collector_factory_id.block));
        };
        
        // Call the factory to mint a new collector
        let mint_result = self.call(
            &Cellpack {
                target: AlkaneId {
                    block: target_block,
                    tx: collector_factory_id.tx,
                },
                inputs: vec![0, next_id], // Initialize opcode with ticket_id
            },
            &AlkaneTransferParcel::default(),
            self.fuel(),
        )?;

        // Extract the collector_id from mint response
        let collector_id = if let Some(first_alkane) = mint_result.alkanes.0.first() {
            first_alkane.id.clone()
        } else {
            return Err(anyhow!("No collector NFT returned from mint"));
        };

        // --- BUY TICKETS FOR THE NEW COLLECTOR ---
        let used_amount = self.process_ticket_purchase(collector_id, incoming_balance)?;

        // Build response - return collector NFT and refund remainder
        let mut response = mint_result;
        
        let remainder = incoming_balance - used_amount;
        if remainder > 0 {
            response.alkanes.pay(AlkaneTransfer {
                id: lottery_token,
                value: remainder,
            });
        }
        
        // Return any non-lottery-token alkanes
        for transfer in context.incoming_alkanes.0.iter() {
            if transfer.id != lottery_token {
                response.alkanes.pay(transfer.clone());
            }
        }

        Ok(response)
    }

    /// Purchase tickets for a specific collector
    fn purchase_tickets(&self, collector_id: AlkaneId) -> Result<CallResponse> {
        if self.allow_purchasing() == 0 {
            return Err(anyhow!("Purchasing tickets not allowed"));
        }

        if self.jackpot_lock() != 0 {
            return Err(anyhow!("Jackpot is currently running!"));
        }

        let context = self.context()?;
        let lottery_token = self.token()?;

        // Calculate incoming token balance
        let incoming_balance: u128 = context
            .incoming_alkanes
            .0
            .iter()
            .filter(|t| t.id == lottery_token)
            .map(|t| t.value)
            .sum();

        if incoming_balance == 0 {
            return Err(anyhow!("No tokens received for ticket purchase"));
        }

        // Process the ticket purchase
        let used_amount = self.process_ticket_purchase(collector_id, incoming_balance)?;

        // Refund remainder
        let remainder = incoming_balance - used_amount;
        let mut response = CallResponse::default();
        if remainder > 0 {
            response.alkanes.pay(AlkaneTransfer {
                id: lottery_token,
                value: remainder,
            });
        }
        
        // Return any non-token alkanes
        for transfer in context.incoming_alkanes.0.iter() {
            if transfer.id != lottery_token {
                response.alkanes.pay(transfer.clone());
            }
        }

        Ok(response)
    }

    /// Get block hash as random entropy source
    fn block_hash_to_u128(&self) -> Result<u128> {
        let block_header = self.block_header()?;
        let block_hash = block_header.block_hash();
        let hash_bytes = block_hash.to_byte_array();
        Ok(u128::from_le_bytes([
            hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
            hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
            hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
            hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
        ]))
    }

    fn get_winning_ticket(&self, max: u128) -> Result<u128> {
        let random_value = self.block_hash_to_u128()?;
        Ok((random_value % max) + 1)
    }

    /// Find winner collector from ticket position
    fn find_winner_collector(&self, winning_ticket: u128) -> Result<AlkaneId> {
        let num_collectors = self.num_unique_ticket_collectors_for_round();
        let mut cumulative_tickets_bps = 0u128;
        
        for i in 0..num_collectors {
            let collector_id = self.get_collector_at_index(i)?;
            let tickets = self.tickets_for_collector(collector_id.clone());
            cumulative_tickets_bps += tickets;
            
            if winning_ticket <= cumulative_tickets_bps {
                return Ok(collector_id);
            }
        }
        
        // Fallback - return first collector if no winner found
        if num_collectors > 0 {
            return self.get_collector_at_index(0);
        }
        Err(anyhow!("No winner found - no collectors in round"))
    }

    /// Get collector at index (helper for iteration)
    fn get_collector_at_index(&self, index: u128) -> Result<AlkaneId> {
        let block = self.collector_at_index_block(index);
        let tx = self.collector_at_index_tx(index);
        if block == 0 && tx == 0 {
            return Err(anyhow!("Collector not found at index"));
        }
        Ok(AlkaneId::new(block, tx))
    }

    /// Clear collectors for round (called after jackpot)
    fn clear_round_collectors(&self) {
        let num_collectors = self.num_unique_ticket_collectors_for_round();
        for i in 0..num_collectors {
            if let Ok(collector_id) = self.get_collector_at_index(i) {
                self.set_tickets_for_collector(collector_id, 0u128);
            }
            self.set_collector_at_index_block(i, 0u128);
            self.set_collector_at_index_tx(i, 0u128);
        }
    }

    /// Run the jackpot lottery
    fn run_jackpot(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let current_height = self.height() as u128;
        let last_jackpot_end = self.last_jackpot_end_block();
        let round_duration = self.round_duration_in_blocks();

        if current_height < last_jackpot_end + round_duration {
            return Err(anyhow!("Jackpot can only be run once per round"));
        }

        if self.jackpot_lock() != 0 {
            return Err(anyhow!("Jackpot is currently running!"));
        }

        // Acquire jackpot lock
        self.set_jackpot_lock(1u128);

        // Update last jackpot end block
        self.set_last_jackpot_end_block(current_height);

        let ticket_count_total_bps = self.ticket_count_total_bps();
        let lp_pool_total = self.lp_pool_total();
        let user_pool_total = self.user_pool_total();
        let ticket_price = self.ticket_price();

        // No tickets bought - return LP pool to LPs
        if ticket_count_total_bps == 0 {
            // LP fees stay with LPs, just reset
            self.set_jackpot_lock(0u128);
            return Ok(CallResponse::forward(&context.incoming_alkanes));
        }

        // Distribute LP fees to LP pool
        let lp_fees = self.lp_fees_total();
        if lp_pool_total > 0 && lp_fees > 0 {
            self.set_lp_pool_total(lp_pool_total + lp_fees);
            self.set_lp_fees_total(0);
        }

        let new_lp_pool_total = self.lp_pool_total();

        // Determine winner
        if user_pool_total >= new_lp_pool_total {
            // Jackpot fully funded by users - winner gets user pool, LPs lose their stake
            let winning_ticket = self.get_winning_ticket(ticket_count_total_bps)?;
            if let Ok(winner_collector) = self.find_winner_collector(winning_ticket) {
                let current_claimable = self.reward_claimable_for_collector(winner_collector.clone());
                self.set_reward_claimable_for_collector(winner_collector.clone(), current_claimable + user_pool_total);
                self.set_last_winner_id(winner_collector);
            }
            // LPs lose their stake when jackpot is fully funded by users
            self.set_lp_pool_total(0);
        } else {
            // Jackpot partially funded by LPs
            let total_tickets = (new_lp_pool_total * 10000) / ticket_price;
            let winning_ticket = self.get_winning_ticket(total_tickets)?;

            if winning_ticket <= ticket_count_total_bps {
                // User wins - gets LP pool
                if let Ok(winner_collector) = self.find_winner_collector(winning_ticket) {
                    let current_claimable = self.reward_claimable_for_collector(winner_collector.clone());
                    self.set_reward_claimable_for_collector(winner_collector.clone(), current_claimable + new_lp_pool_total);
                    self.set_last_winner_id(winner_collector);

                    // LPs get user pool (LP pool becomes user pool)
                    self.set_lp_pool_total(user_pool_total);
                }
            } else {
                // LPs win - get both pools
                self.set_lp_pool_total(new_lp_pool_total + user_pool_total);
            }
        }

        // Reset for next round
        self.clear_round_collectors();
        self.set_user_pool_total(0);
        self.set_ticket_count_total_bps(0);
        self.set_all_fees_total(0);
        self.set_num_unique_ticket_collectors_for_round(0);

        // Release lock
        self.set_jackpot_lock(0u128);

        Ok(CallResponse::forward(&context.incoming_alkanes))
    }

    /// Withdraw winnings for a collector
    fn withdraw_winnings(&self, collector_id: AlkaneId) -> Result<CallResponse> {
        let context = self.context()?;
        let lottery_token = self.token()?;

        // Check that the caller has the collector NFT
        let collector_balance: u128 = context
            .incoming_alkanes
            .0
            .iter()
            .filter(|t| t.id == collector_id)
            .map(|t| t.value)
            .sum();

        if collector_balance == 0 {
            return Err(anyhow!("Must send collector NFT to prove ownership"));
        }

        let claimable = self.reward_claimable_for_collector(collector_id.clone());
        if claimable == 0 {
            // Return the collector NFT
            let response = CallResponse::forward(&context.incoming_alkanes);
            return Ok(response);
        }

        // Reset claimable before transfer
        self.set_reward_claimable_for_collector(collector_id, 0u128);

        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.alkanes.pay(AlkaneTransfer {
            id: lottery_token,
            value: claimable,
        });

        Ok(response)
    }

    /// Withdraw protocol fees (admin only)
    fn withdraw_protocol_fees(&self) -> Result<CallResponse> {
        self.only_owner()?;

        let protocol_fee_claimable = self.protocol_fee_claimable();
        if protocol_fee_claimable == 0 {
            return Err(anyhow!("No protocol fees to withdraw"));
        }

        let context = self.context()?;
        let lottery_token = self.token()?;

        self.set_protocol_fee_claimable(0);

        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.alkanes.pay(AlkaneTransfer {
            id: lottery_token,
            value: protocol_fee_claimable,
        });

        Ok(response)
    }

    fn get_name(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response: CallResponse = CallResponse::forward(&context.incoming_alkanes);
        response.data = self.name().into_bytes().to_vec();
        Ok(response)
    }

    fn get_lp_pool_total(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response: CallResponse = CallResponse::forward(&context.incoming_alkanes);
        let lp_pool_total = self.lp_pool_total();
        response.data = lp_pool_total.to_le_bytes().to_vec();
        Ok(response)
    }

    fn get_user_pool_total(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response: CallResponse = CallResponse::forward(&context.incoming_alkanes);
        let user_pool_total = self.user_pool_total();
        response.data = user_pool_total.to_le_bytes().to_vec();
        Ok(response)
    }

    fn forward_incoming(&self) -> Result<CallResponse> {
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }

    fn forward(&self) -> Result<CallResponse> {
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }
}

declare_alkane! {
    impl AlkaneResponder for LotteryContract {
        type Message = LotteryContractMessage;
    }
}
