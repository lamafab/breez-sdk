use bitcoin::Network;
use gl_client::tls;
use gl_client::signer::Signer;
use gl_client::pb::scheduler::{ChallengeResponse, StartupMessage};

pub struct RegistrationRequest {
	pub bip32_key: Vec<u8>,
	pub challenge: Vec<u8>,
	pub signer_proto: String,
	pub init_msg: Vec<u8>,
	pub device_csr: String,
	pub signature: Vec<u8>,
	pub startup_msgs: Vec<StartupMessage>,
}

impl RegistrationRequest {
	pub fn into_pb_registration_request(
		self,
		node_id: Vec<u8>,
		network: Network,
		invite_code: String,
	) -> gl_client::pb::scheduler::RegistrationRequest {
		gl_client::pb::scheduler::RegistrationRequest {
			node_id,
			bip32_key: self.bip32_key,
			network: network.to_string(),
			challenge: self.challenge,
			signer_proto: self.signer_proto,
			init_msg: self.init_msg,
			signature: self.signature,
			csr: self.device_csr.into_bytes(),
			invite_code,
			startupmsgs:self.startup_msgs 
		}
	}
	pub fn into_pb_recovery_request(
		self,
		node_id: Vec<u8>,
	) -> gl_client::pb::scheduler::RecoveryRequest {
		gl_client::pb::scheduler::RecoveryRequest {
			node_id,
			challenge: self.challenge,
			signature: self.signature,
			csr: self.device_csr.into_bytes(),
		}
	}
}

/// Signs the [`ChallengeResponse`](gl_client::pb::scheduler::ChallengeResponse)
/// returned by the Greenlight RPC. The return value should then be converted
/// into a
/// [`RegistrationRequest`](gl_client::pb::scheduler::RegistrationRequest) or
/// [`RecoveryRequest`](gl_client::pb::scheduler::RecoveryRequest), depending on
/// intended usecase.
pub fn sign_challenge_response(
	signer: Signer,
	is_recovery: bool,
	challenge: Vec<u8>,
	node_id: &[u8],
) -> RegistrationRequest {
	let bip32_key = signer.bip32_ext_key();
	let signer_proto = signer.version().to_owned();
	let init_msg = signer.get_init();

	let signature = signer.sign_challenge(challenge.clone()).unwrap();

	let name = if is_recovery {
		format!("recovered-{}", hex::encode(&challenge))
	} else {
		"default".to_string()
	};

	let device_cert = tls::generate_self_signed_device_cert(
		&hex::encode(node_id),
		&name,
		vec!["localhost".into()],
	);
	let device_csr = device_cert.serialize_request_pem().unwrap();

	let startup_msgs: Vec<StartupMessage> = signer
		.get_startup_messages()
		.into_iter()
		.map(|m| m.into())
		.collect();

	RegistrationRequest {
		bip32_key,
		challenge,
		signer_proto,
		init_msg,
		device_csr,
		signature,
		startup_msgs,
	}
}
