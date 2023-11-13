tonic::include_proto!("airgap");

mod temp {
	use crate::{SyncResponse, NodeState};
	use crate::airgap::primitives as proto;

	impl From<proto::SyncResponse> for SyncResponse {
		fn from(proto: proto::SyncResponse) -> Self {
			let ns = proto.node_state.unwrap();

			let x = SyncResponse {
				node_state: NodeState {
					id: ns.id,
					block_height: ns.block_height,
					channels_balance_msat: ns.channels_balance_msat,
					onchain_balance_msat: ns.onchain_balance_msat,
					utxos: vec![], 
					max_payable_msat: ns.max_payable_msat,
					max_receivable_msat: ns.max_receivable_msat, 
					max_single_payment_amount_msat: ns.max_single_payment_amount_msat, 
					max_chan_reserve_msats: ns.max_chan_reserve_msats, 
					connected_peers: ns.connected_peers, 
					inbound_liquidity_msats: ns.inbound_liquidity_msats,
				},
				payments: vec![],
				channels: vec![],
			};

			todo!()
		}
	}
}