use gl_client::pb::cln::{self, ListpeersPeers, listinvoices_invoices::ListinvoicesInvoicesStatus};
use cln::listpeers_peers_channels::ListpeersPeersChannelsState as ChannelState;

use crate::{NodeState, Channel, node_api::NodeResult, UnspentTransactionOutput, Payment, SyncResponse};

// TODO: Import from `crate::greenlight` instead?
const MAX_PAYMENT_AMOUNT_MSAT: u64 = 4294967000;
const MAX_INBOUND_LIQUIDITY_MSAT: u64 = 4000000000;

/// 
/// * node_info: response returned by `/cln.Node/Getinfo`.
/// * node_funds: response returned by `/cln.Node/ListFunds`.
/// * node_closed_channels: response returned by `/cln.Node/ListClosedChannels`.
/// * node_peers: reponse returned by `/cln.Node/ListPeers`.
pub fn pull_changed(
	node_info: cln::GetinfoResponse,
	node_funds: cln::ListfundsResponse,
	node_closed_channels: cln::ListclosedchannelsResponse,
	node_peers: cln::ListpeersResponse,
	node_invoices: cln::ListinvoicesResponse,
	since_timestamp: u64,
	node_payments: cln::ListpaysResponse,
) -> NodeResult<SyncResponse> {
	// TODO: Greenlight::fech_channels_and_balances does some extra procesing with the `balance_changed` flag.

	let all_channels: Vec<cln::ListpeersPeersChannels> = node_peers
		.peers
		.iter()
		.cloned()
		.map(|peer| peer.channels)
		.flatten()
		.collect();

	let opened_channels: Vec<cln::ListpeersPeersChannels> = all_channels
		.iter()
		.filter(|channel| channel.state() == ChannelState::ChanneldNormal)
		.cloned()
		.collect();

	let forgotten_closed_channels: Vec<Channel> = node_closed_channels
		.closedchannels
		.into_iter()
		.filter(|closed| {
			all_channels
				.iter()
				.all(|any| any.funding_txid.as_ref() != Some(&closed.funding_txid))
		})
		.map(TryInto::try_into)
		.collect::<NodeResult<_>>()?;

	let all_channel_models: Vec<Channel> = all_channels
		.iter()
		.cloned()
		.map(Channel::from)
		.chain(forgotten_closed_channels.into_iter())
		.collect();

	// Process invoices
	let received_transactions: Vec<Payment> = node_invoices
		.invoices
		.into_iter()
		.filter(|invoice| {
			invoice.paid_at.unwrap_or_default() > since_timestamp
				&& invoice.status() == ListinvoicesInvoicesStatus::Paid
		})
		.map(TryInto::try_into)
		.collect::<NodeResult<_>>()?;

	let outbound_transactions: Vec<Payment> = node_payments
		.pays
		.into_iter()
		.filter(|pays| pays.created_at > since_timestamp)
		.map(TryInto::try_into)
		.collect::<NodeResult<_>>()?;

	// All transactions (received & outbound)
	let transactions: Vec<Payment> = received_transactions
		.into_iter()
		.chain(outbound_transactions.into_iter())
		.collect();

	// Calculate channel balance
	let channels_balance: u64 = opened_channels
		.iter()
		.map(|channel| Channel::from(channel.clone()))
		.map(|channel| channel.spendable_msat)
		.sum();

	// Calculate onchain balance
	let onchain_balance = node_funds
		.outputs
		.iter()
		.fold(0, |total, outputs| {
			if outputs.reserved {
				total
			} else {
				total + outputs.amount_msat.as_ref().map(|amount| amount.msat).unwrap_or_default()
			}
		});

	// List of UTXOs
	let utxos: Vec<UnspentTransactionOutput> = node_funds
		.outputs
		.iter()
		.map(|output| UnspentTransactionOutput {
			txid: output.txid.clone(),
			outnum: output.output,
			amount_millisatoshi: output.amount_msat.as_ref().map(|amount| amount.msat).unwrap_or_default(),
			address: output.address.clone().unwrap_or_default(),
			reserved: output.reserved,
		})
		.collect();

	// List of connected peers
	let connected_peers: Vec<String> = node_peers
		.peers
		.iter()
		.filter(|peer| peer.connected)
		.map(|peer| hex::encode(&peer.id))
		.collect();

	// Calculate payment limits and inbound liquidity
	let mut max_payable = 0;
	let mut max_receivable_single_channel = 0;
	for channel in &opened_channels {
		max_payable += channel
			.spendable_msat
			.as_ref()
			.map(|amount| amount.msat)
			.unwrap_or_default();

		let receivable_amount = channel
			.receivable_msat
			.as_ref()
			.map(|amount| amount.msat)
			.unwrap_or_default();

		max_receivable_single_channel = max_receivable_single_channel.max(receivable_amount);
	}

	let max_allowed_to_receive_msats = MAX_INBOUND_LIQUIDITY_MSAT.saturating_sub(channels_balance);
	let max_allowed_reserve_msats = channels_balance - max_payable.min(channels_balance);

	let node_pubkey = hex::encode(node_info.id);

	let node_state = NodeState {
		id: node_pubkey,
		block_height: node_info.blockheight,
		channels_balance_msat: channels_balance,
		onchain_balance_msat: onchain_balance,
		utxos,
		max_payable_msat: max_payable,
		max_receivable_msat: max_allowed_to_receive_msats,
		max_single_payment_amount_msat: MAX_PAYMENT_AMOUNT_MSAT,
		max_chan_reserve_msats: max_allowed_reserve_msats,
		connected_peers: connected_peers,
		inbound_liquidity_msats: max_receivable_single_channel,
	};

	Ok(SyncResponse {
		node_state,
		payments: transactions,
		channels: all_channel_models,
	})
}