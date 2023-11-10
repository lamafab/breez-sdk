use gl_client::pb::cln::{self, ListpeersPeers};
use cln::listpeers_peers_channels::ListpeersPeersChannelsState as ChannelState;

use crate::{NodeState, Channel, node_api::NodeResult};

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
) -> NodeResult<NodeState> {

	let connected_peers: Vec<String> = node_peers
		.peers
		.iter()
		.filter(|peer| peer.connected)
		.map(|peer| hex::encode(peer.id))
		.collect();

	let all_channels: Vec<cln::ListpeersPeersChannels> = node_peers
		.peers
		.iter()
		.map(|peer| peer.channels)
		.flatten()
		.collect();

	let opened_channels: Vec<cln::ListpeersPeersChannels> = all_channels
		.iter()
		.filter(|channel| channel.state() == ChannelState::ChanneldNormal)
		.collect();

	let channels_balance: u64 = opened_channels
		.iter()
		.map(|channel| Channel::from(channel))
		.map(|channel| channel.spendable_msat)
		.sum();

	let forgotten_closed_channels: NodeResult<Vec<Channel>> = node_closed_channels
		.closedchannels
		.iter()
		.filter(|closed| {
			all_channels
				.iter()
				.all(|any| any.funding_txid != Some(closed.funding_txid))
		})
		.map(|closed| Channel::try_from)
		.collect()?;

	let onchain_balance = node_funds
		.outputs
		.iter()
		.fold(0, |total, outputs| {
			if outputs.reserved {
				total
			} else {
				total + outputs.amount_msat.unwrap_or_default().msat
			}
		});

	let node_pubkey = hex::encode(node_info.id);

	let node_state = NodeState {
		id: node_pubkey,
		block_height: node_info.blockheight,
		channels_balance_msat: channels_balance,
		onchain_balance_msat: onchain_balance,
		utxos: Vec<UnspentTransactionOutput>,
		max_payable_msat: u64,
		max_receivable_msat: u64,
		max_single_payment_amount_msat: u64,
		max_chan_reserve_msats: u64,
		connected_peers: Vec<String>,
		inbound_liquidity_msats: u64,
	};

	todo!()
}