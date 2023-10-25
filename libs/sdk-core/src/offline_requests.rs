use crate::{LspInformation, NodeState, INVOICE_PAYMENT_FEE_EXPIRY_SECONDS, ensure_sdk, ReceivePaymentRequest, error::{SdkError, SdkResult}, parse_short_channel_id, ChannelState, Peer};

pub struct NodeApiRequestBuilder;

impl NodeApiRequestBuilder {
	pub fn create_invoice() {
		todo!()
	}
}

pub struct PaymentReceiverBuilder;

impl PaymentReceiverBuilder {
	pub fn lsp_information(req: ReceivePaymentRequest,lsp_info: LspInformation, node_state: NodeState, peers: Option<Vec<Peer>>) -> SdkResult<()>{
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

		todo!()
	}
}
