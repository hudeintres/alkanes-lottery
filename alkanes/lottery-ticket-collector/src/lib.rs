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
    response::CallResponse,
    utils::{overflow_error, shift, shift_or_err},
};
use anyhow::{anyhow, Result};
use metashrew_support::compat::to_arraybuffer_layout;

#[derive(MessageDispatch)]
pub enum LotteryTicketMessage {
    #[opcode(0)]
    Initialize { ticket_id: u128 },

    #[opcode(50)]
    Forward,

    #[opcode(99)]
    #[returns(String)]
    GetName,

    #[opcode(100)]
    #[returns(String)]
    GetSymbol,
}

#[derive(Default)]
pub struct LotteryTicket();

impl MintableToken for LotteryTicket {}
impl AlkaneResponder for LotteryTicket {}
impl AuthenticatedResponder for LotteryTicket {}

impl LotteryTicket {
    fn initialize(&self, ticket_id: u128) -> Result<CallResponse> {
        self.observe_initialization()?;
        let context = self.context()?;
        self.set_name_and_symbol_str(
            format!("Lottery Ticket Collector{}", ticket_id),
            format!("LTC{}", ticket_id),
        );
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.alkanes.pay(self.mint(&context, 1)?);

        Ok(response)
    }
    fn forward(&self) -> Result<CallResponse> {
        Ok(CallResponse::forward(&self.context()?.incoming_alkanes))
    }
    fn get_name(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response: CallResponse = CallResponse::forward(&context.incoming_alkanes);
        response.data = self.name().into_bytes().to_vec();
        Ok(response)
    }
    fn get_symbol(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response: CallResponse = CallResponse::forward(&context.incoming_alkanes);
        response.data = self.symbol().into_bytes().to_vec();
        Ok(response)
    }
}

declare_alkane! {
    impl AlkaneResponder for LotteryTicket {
        type Message = LotteryTicketMessage;
    }
}
