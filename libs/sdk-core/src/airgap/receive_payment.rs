use crate::{
    ensure_sdk, error::ReceivePaymentError, invoice::add_lsp_routing_hints, parse_invoice,
    parse_short_channel_id, ChannelState, LspInformation, OpeningFeeParams, Peer,
    ReceivePaymentRequest, RouteHint, RouteHintHop,
};
use anyhow::anyhow;
use gl_client::pb::cln;
use lightning_invoice::RawInvoice;

pub const INVOICE_PAYMENT_FEE_EXPIRY_SECONDS: u32 = 60 * 60; // 60 minutes

pub struct PreparedInvoiceContext {
    pub lsp_id: String,
    pub lsp_pubkey: String,
    pub short_channel_id: u64,
    pub destination_invoice_amount_msat: u64,
    pub channel_opening_fee_params: Option<OpeningFeeParams>,
    pub open_channel_needed: bool,
    pub channel_fees_msat: Option<u64>,
}

pub fn prepare_invoice(
    req: ReceivePaymentRequest,
    lsp_info: LspInformation,
    // TODO: This should take `Vec<Peer>`.
    node_peers: cln::ListpeersResponse,
    node_state_inbound_liquidity_msats: u64,
) -> Result<PreparedInvoiceContext, ReceivePaymentError> {
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
    let open_channel_needed = node_state_inbound_liquidity_msats < req.amount_msat;
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
                return Err(
                        ReceivePaymentError::InvalidAmount{err: format!(
                           "Amount should be more than the minimum fees {channel_fees_msat} msat, but is {} msat",
                            req.amount_msat
                        )}
                    );
            }
            // remove the fees from the amount to get the small amount on the current node invoice.
            destination_invoice_amount_msat = req.amount_msat - channel_fees_msat;
        }
    } else {
        // not opening a channel so we need to get the real channel id into the routing hints
        info!("Finding channel ID for routing hint");
        // TODO: Remove cloned
        let peer_models: Vec<Peer> = node_peers.peers.into_iter().map(|p| p.into()).collect();
        for peer in peer_models {
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

    Ok(PreparedInvoiceContext {
        lsp_id: lsp_info.id,
        lsp_pubkey: lsp_info.pubkey,
        short_channel_id,
        destination_invoice_amount_msat,
        channel_opening_fee_params,
        open_channel_needed,
        channel_fees_msat,
    })
}

pub struct PaymentInfo {
    pub payment_hash: Vec<u8>,
    pub payment_secret: Vec<u8>,
    pub destination: Vec<u8>,
    pub incoming_amount_msat: i64,
    pub outgoing_amount_msat: i64,
    pub opening_fee_params: Option<OpeningFeeParams>,
}

pub fn check_lsp_hints(
    invoice: String,
    req: &ReceivePaymentRequest,
    ctx: &PreparedInvoiceContext,
    lsp_info: &LspInformation,
) -> Result<Option<RawInvoice>, ReceivePaymentError> {
    let parsed_invoice = parse_invoice(&invoice)?;

    // check if the lsp hint already exists
    info!("Existing routing hints {:?}", parsed_invoice.routing_hints);
    info!("lsp info pubkey = {:?}", lsp_info.pubkey.clone());
    let has_lsp_hint = parsed_invoice.routing_hints.iter().any(|h| {
        h.hops
            .iter()
            .any(|h| h.src_node_id == lsp_info.pubkey.clone())
    });

    // We only add routing hint if we need to open a channel
    // or if the invoice doesn't have any routing hints that points to the lsp
    let mut lsp_hint: Option<RouteHint> = None;
    if !has_lsp_hint || ctx.open_channel_needed {
        let lsp_hop = RouteHintHop {
            src_node_id: lsp_info.pubkey.clone(),
            short_channel_id: ctx.short_channel_id,
            fees_base_msat: lsp_info.base_fee_msat as u32,
            fees_proportional_millionths: (lsp_info.fee_rate * 1000000.0) as u32,
            cltv_expiry_delta: lsp_info.time_lock_delta as u64,
            htlc_minimum_msat: Some(lsp_info.min_htlc_msat as u64),
            htlc_maximum_msat: None,
        };

        info!("Adding LSP hop as routing hint: {:?}", lsp_hop);
        lsp_hint = Some(RouteHint {
            hops: vec![lsp_hop],
        });
    }

    // We only create a new invoice if we need to add the lsp hint or change the amount
    if lsp_hint.is_some() || req.amount_msat != ctx.destination_invoice_amount_msat {
        // create the large amount invoice
        Ok(Some(add_lsp_routing_hints(
            invoice.clone(),
            !ctx.open_channel_needed,
            lsp_hint,
            req.amount_msat,
        )?))
    } else {
        Ok(None)
    }
}

pub fn check_payment_registration(
    invoice: &str,
    req: &ReceivePaymentRequest,
    ctx: &PreparedInvoiceContext,
) -> Result<Option<PaymentInfo>, ReceivePaymentError> {
    let parsed_invoice = parse_invoice(invoice)?;

    // register the payment at the lsp if needed
    if ctx.open_channel_needed {
        info!("Registering payment with LSP");

        if ctx.channel_opening_fee_params.is_none() {
            return Err(ReceivePaymentError::Generic {
                err: "We need to open a channel, but no channel opening fee params found".into(),
            });
        }

        Ok(Some(PaymentInfo {
            payment_hash: hex::decode(parsed_invoice.payment_hash.clone())
                .map_err(|e| anyhow!("Failed to decode hex payment hash: {e}"))?,
            payment_secret: parsed_invoice.payment_secret.clone(),
            destination: hex::decode(parsed_invoice.payee_pubkey.clone())
                .map_err(|e| anyhow!("Failed to decode hex payee pubkey: {e}"))?,
            incoming_amount_msat: req.amount_msat as i64,
            outgoing_amount_msat: ctx.destination_invoice_amount_msat as i64,
            opening_fee_params: ctx.channel_opening_fee_params.clone().map(Into::into),
        }))
    } else {
        Ok(None)
    }
}
