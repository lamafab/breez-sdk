use anyhow::anyhow;
use lightning_invoice::RawInvoice;
use serde_json::json;

use crate::{
    ensure_sdk,
    error::{ReceivePaymentError, SdkError, SdkResult},
    grpc::PaymentInformation,
    invoice::add_lsp_routing_hints,
    parse_invoice, parse_short_channel_id, ChannelState, LNInvoice, LspInformation, NodeState,
    OpeningFeeParams, Peer, ReceivePaymentRequest, ReceivePaymentResponse, RouteHint, RouteHintHop,
    INVOICE_PAYMENT_FEE_EXPIRY_SECONDS,
};

pub struct NodeApiRequestBuilder;

impl NodeApiRequestBuilder {
    pub fn create_invoice() {
        todo!()
    }
}

pub struct PaymentReceiverBuilder;

impl PaymentReceiverBuilder {
    pub fn prepare_invoice(
        req: ReceivePaymentRequest,
        lsp_info: LspInformation,
        node_state: NodeState,
        peers: Option<Vec<Peer>>,
    ) -> Result<(CreateInvoice, LspRoutingHintBuilder), ReceivePaymentError> {
        let expiry = req.expiry.unwrap_or(INVOICE_PAYMENT_FEE_EXPIRY_SECONDS);

        ensure_sdk!(
            req.amount_msat > 0,
            ReceivePaymentError::InvalidAmount {
                err: "Receive amount must be more than 0".into()
            }
        );

        let mut short_channel_id = parse_short_channel_id("1x0x0")?;
        let mut destination_invoice_amount_msat = req.amount_msat;

        let mut channel_opening_fee_params = None;
        let mut channel_fees_msat = None;

        // check if we need to open channel
        let open_channel_needed = node_state.inbound_liquidity_msats < req.amount_msat;
        if open_channel_needed {
            info!("We need to open a channel");

            // we need to open channel so we are calculating the fees for the LSP (coming either from the user, or from the LSP)
            let ofp = match req.opening_fee_params {
                Some(fee_params) => fee_params,
                None => lsp_info.cheapest_open_channel_fee(expiry)?.clone(),
            };

            channel_opening_fee_params = Some(ofp.clone());
            channel_fees_msat = Some(ofp.get_channel_fees_msat_for(req.amount_msat));
            if let Some(channel_fees_msat) = channel_fees_msat {
                info!("zero-conf fee calculation option: lsp fee rate (proportional): {}:  (minimum {}), total fees for channel: {}",
                    ofp.proportional, ofp.min_msat, channel_fees_msat);

                if req.amount_msat < channel_fees_msat + 1000 {
                    return Err(ReceivePaymentError::InvalidAmount {
                        err: format!(
                            "requestPayment: Amount should be more than the minimum fees {channel_fees_msat} msat, but is {} msat",
                            req.amount_msat
                        ),
                    });
                }
                // remove the fees from the amount to get the small amount on the current node invoice.
                destination_invoice_amount_msat = req.amount_msat - channel_fees_msat;
            }
        } else {
            // not opening a channel so we need to get the real channel id into the routing hints
            info!("Finding channel ID for routing hint");
            // TODO:
            for peer in peers.unwrap() {
                if hex::encode(peer.id) == lsp_info.pubkey && !peer.channels.is_empty() {
                    let active_channel = peer
                        .channels
                        .iter()
                        .find(|&c| c.state == ChannelState::Opened)
                        .ok_or_else(|| anyhow!("No open channel found"))?;
                    let hint = active_channel
                        .clone()
                        .alias_remote
                        .unwrap_or(active_channel.clone().short_channel_id);

                    short_channel_id = parse_short_channel_id(&hint)?;
                    info!("Found channel ID: {short_channel_id} {active_channel:?}");
                    break;
                }
            }
        }

        // Prepare the invoice information that must be passed to
        // `NodeAPI::create_invoice`.
        let create_invoice = CreateInvoice {
            amount_msat: destination_invoice_amount_msat,
            description: req.description,
            preimage: req.preimage,
            use_description_hash: req.use_description_hash,
            expiry: Some(expiry),
            cltv: Some(req.cltv.unwrap_or(144)),
        };

        // After `NodeAPI::create_invoice` returns the BOLT11 string, that
        // string must be passed to the next builder.
        let next_builder = LspRoutingHintBuilder {
            req_amount_msat: req.amount_msat,
            lsp_info,
            open_channel_needed,
            short_channel_id,
            destination_invoice_amount_msat,
            channel_opening_fee_params,
            channel_fees_msat,
        };

        // TODO:
        //info!("Invoice created {}", invoice);

        Ok((create_invoice, next_builder))
    }
}

pub struct PostPaymentReceiverContext {
    pub create_invoice: CreateInvoice,
    pub next_builder: LspRoutingHintBuilder,
}

pub struct CreateInvoice {
    pub amount_msat: u64,
    pub description: String,
    pub preimage: Option<Vec<u8>>,
    pub use_description_hash: Option<bool>,
    pub expiry: Option<u32>,
    pub cltv: Option<u32>,
}

pub struct LspRoutingHintBuilder {
    req_amount_msat: u64,
    lsp_info: LspInformation,
    open_channel_needed: bool,
    short_channel_id: u64,
    destination_invoice_amount_msat: u64,
    channel_opening_fee_params: Option<OpeningFeeParams>,
    channel_fees_msat: Option<u64>,
}

impl LspRoutingHintBuilder {
    pub fn check_routing_hint(self, invoice: &str) -> SdkResult<PostLspRoutingHintContext> {
        let parsed_invoice = parse_invoice(invoice)?;

        // TODO: Extra checks between `parsed_invoice` and `self.*`?

        // check if the lsp hint already exists
        info!("Existing routing hints {:?}", parsed_invoice.routing_hints);
        info!("lsp info pubkey = {:?}", self.lsp_info.pubkey.clone());
        let has_lsp_hint = parsed_invoice.routing_hints.iter().any(|h| {
            h.hops
                .iter()
                .any(|h| h.src_node_id == self.lsp_info.pubkey.clone())
        });

        // We only add routing hint if we need to open a channel
        // or if the invoice doesn't have any routing hints that points to the lsp
        let mut lsp_hint: Option<RouteHint> = None;
        if !has_lsp_hint || self.open_channel_needed {
            let lsp_hop = RouteHintHop {
                src_node_id: self.lsp_info.pubkey,
                short_channel_id: self.short_channel_id,
                fees_base_msat: self.lsp_info.base_fee_msat as u32,
                fees_proportional_millionths: (self.lsp_info.fee_rate * 1000000.0) as u32,
                cltv_expiry_delta: self.lsp_info.time_lock_delta as u64,
                htlc_minimum_msat: Some(self.lsp_info.min_htlc_msat as u64),
                htlc_maximum_msat: None,
            };

            info!("Adding LSP hop as routing hint: {:?}", lsp_hop);
            lsp_hint = Some(RouteHint {
                hops: vec![lsp_hop],
            });
        }

        let lsp_id = self.lsp_info.id.clone();
        let lsp_pubkey = self.lsp_info.lsp_pubkey.clone();

        // We only create a new invoice if we need to add the lsp hint or change the amount
        let mut new_invoice_with_hint = None;
        if lsp_hint.is_some() || self.req_amount_msat != self.destination_invoice_amount_msat {
            // create the large amount invoice
            let raw_invoice_with_hint =
                add_lsp_routing_hints(invoice.to_string(), lsp_hint, self.req_amount_msat)?;

            info!("Routing hint added");
            new_invoice_with_hint = Some(raw_invoice_with_hint);
        }

        let next_builder = FinalizedInvoiceBuilder {
            req_amount_msat: self.req_amount_msat,
            lsp_id,
            lsp_pubkey,
            open_channel_needed: self.open_channel_needed,
            destination_invoice_amount_msat: self.destination_invoice_amount_msat,
            channel_opening_fee_params: self.channel_opening_fee_params,
            channel_fees_msat: self.channel_fees_msat,
        };

        Ok(PostLspRoutingHintContext {
            new_invoice_with_hint,
            next_builder,
        })
    }
}

pub struct PostLspRoutingHintContext {
    pub new_invoice_with_hint: Option<RawInvoice>,
    pub next_builder: FinalizedInvoiceBuilder,
}

pub struct FinalizedInvoiceBuilder {
    req_amount_msat: u64,
    lsp_id: String,
    lsp_pubkey: Vec<u8>,
    open_channel_needed: bool,
    channel_opening_fee_params: Option<OpeningFeeParams>,
    destination_invoice_amount_msat: u64,
    // TODO: When is this None?
    channel_fees_msat: Option<u64>,
}

impl FinalizedInvoiceBuilder {
    pub fn finalize(self, invoice: &str) -> Result<FinalizedInvoiceContext, ReceivePaymentError> {
        // TODO: Do extra checks with `new_invoice` generated previously?
        let parsed_invoice = parse_invoice(invoice)?;

        // register the payment at the lsp if needed
        let mut register_payment = None;
        if self.open_channel_needed {
            info!("Registering payment with LSP");

            // TODO: Should this be checked before?
            if self.channel_opening_fee_params.is_none() {
                return Err(ReceivePaymentError::Generic {
                    err: "We need to open a channel, but no channel opening fee params found"
                        .into(),
                });
            }

            register_payment = Some(LspRegisterPaymentParams {
                lsp_id: self.lsp_id,
                lsp_pubkey: self.lsp_pubkey,
                payment_hash: hex::decode(parsed_invoice.payment_hash.clone())
                    .map_err(|e| anyhow!("Failed to decode hex payment hash: {e}"))?,
                payment_secret: parsed_invoice.payment_secret.clone(),
                destination: hex::decode(parsed_invoice.payee_pubkey.clone())
                    .map_err(|e| anyhow!("Failed to decode hex payee pubkey: {e}"))?,
                incoming_amount_msat: self.req_amount_msat as i64,
                outgoing_amount_msat: self.destination_invoice_amount_msat as i64,
                // TODO: Should probably not be Option in the first place.
                opening_fee_params: self.channel_opening_fee_params.clone().unwrap().into(),
            });
        }

        Ok(FinalizedInvoiceContext {
            ln_invoice: parsed_invoice,
            req_amount_msat: self.req_amount_msat,
            opening_fee_params: self.channel_opening_fee_params.unwrap(),
            opening_fee_msat: self.channel_fees_msat,
            register_payment,
        })
    }
}

pub struct FinalizedInvoiceContext {
    // TODO: Needed? Not duplicate value from `ln_invoice`?
    pub req_amount_msat: u64,
    pub ln_invoice: LNInvoice,
    pub opening_fee_params: OpeningFeeParams,
    // TODO: When is this None?
    pub opening_fee_msat: Option<u64>,
    pub register_payment: Option<LspRegisterPaymentParams>,
}

pub struct LspRegisterPaymentParams {
    pub lsp_id: String,
    pub lsp_pubkey: Vec<u8>,
    pub payment_hash: Vec<u8>,
    pub payment_secret: Vec<u8>,
    pub destination: Vec<u8>,
    pub incoming_amount_msat: i64,
    pub outgoing_amount_msat: i64,
    pub opening_fee_params: OpeningFeeParams,
}

impl LspRegisterPaymentParams {
    pub fn into_payment_information(self, api_key_hash: &str) -> PaymentInformation {
        PaymentInformation {
            payment_hash: self.payment_hash,
            payment_secret: self.payment_secret,
            destination: self.destination,
            incoming_amount_msat: self.incoming_amount_msat,
            outgoing_amount_msat: self.outgoing_amount_msat,
            tag: json!({ "apiKeyHash": api_key_hash }).to_string(),
            // TODO: Should this be Option in the first place?
            opening_fee_params: Some(self.opening_fee_params.into()),
        }
    }
}
