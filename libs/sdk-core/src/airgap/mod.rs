//pub mod primitives;
pub mod receive_payment;
pub mod send_payment;
pub mod sync_response;
pub mod greenlight;

// Re-export network messages.
pub mod grpc_primitives {
    pub mod breez {
        pub use crate::grpc::*;
    }
    pub mod greenlight {
        pub use gl_client::pb::cln::*;
    }
}
