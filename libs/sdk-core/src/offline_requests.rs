use lightning_invoice::RawInvoice;

use crate::{LspInformation, NodeState, INVOICE_PAYMENT_FEE_EXPIRY_SECONDS, ensure_sdk, ReceivePaymentRequest, error::{SdkError, SdkResult}, parse_short_channel_id, ChannelState, Peer, parse_invoice, RouteHint, RouteHintHop, invoice::add_lsp_routing_hints};

pub struct NodeApiRequestBuilder;

impl NodeApiRequestBuilder {
	pub fn create_invoice() {
		todo!()
	}
}

pub struct PaymentReceiverBuilder;

impl PaymentReceiverBuilder {
	pub fn lsp_information(req: ReceivePaymentRequest, lsp_info: LspInformation, node_state: NodeState, peers: Option<Vec<Peer>>) -> SdkResult<()>{
        let expiry = req.expiry.unwrap_or(INVOICE_PAYMENT_FEE_EXPIRY_SECONDS);

        ensure_sdk!(
            req.amount_msat > 0,
            SdkError::ReceivePaymentFailed {
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
                    return Err(SdkError::ReceivePaymentFailed {
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
                        .ok_or_else(|| SdkError::ReceivePaymentFailed {
                            err: "No open channel found".into(),
                        })?;
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

		/*
        info!("Creating invoice on NodeAPI");
        let invoice = &self
            .node_api
            .create_invoice(
                destination_invoice_amount_msat,
                req.description,
                req.preimage,
                req.use_description_hash,
                Some(expiry),
                Some(req.cltv.unwrap_or(144)),
            )
            .await?;
        info!("Invoice created {}", invoice);

        let mut parsed_invoice = parse_invoice(invoice)?;
		*/

		todo!()
	}
}

pub struct PostInvoiceBuilder {
	req: ReceivePaymentRequest,
	lsp_info: LspInformation,
	open_channel_needed: bool,
	short_channel_id: u64,
	destination_invoice_amount_msat: u64,
	invoice: String,
}

impl PostInvoiceBuilder {
	pub fn invoice(self, invoice: &str) -> SdkResult<UpdatedInvoiceContext> {
        let mut parsed_invoice = parse_invoice(invoice)?;

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

        // We only create a new invoice if we need to add the lsp hint or change the amount
		let mut new_invoice = None;
        if lsp_hint.is_some() || self.req.amount_msat != self.destination_invoice_amount_msat {
            // create the large amount invoice
            let raw_invoice_with_hint =
                add_lsp_routing_hints(self.invoice.clone(), lsp_hint, self.req.amount_msat)?;

            info!("Routing hint added");
			new_invoice = Some(raw_invoice_with_hint);
			// TODO:
            //info!("Signed invoice with hint = {}", signed_invoice_with_hint);
        }

		Ok(UpdatedInvoiceContext {
			new_invoice,
		})
	}
}

pub struct UpdatedInvoiceContext {
	// We only create a new invoice if we need to add the lsp hint or change the amount
	// TODO: Expand.
	new_invoice: Option<RawInvoice>,
}
