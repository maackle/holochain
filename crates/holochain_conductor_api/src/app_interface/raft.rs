use holochain_raft::{RaftOp, RaftSpace};

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, SerializedBytes)]
pub struct RaftInterfaceRequest {
    /// Hash of the network which contains the peers to sync with, e.g. `syn`
    pub dna_hash: DnaHash,
    /// A new raft instance is created for each workspace
    pub raft_space: RaftSpace,
    /// The actual request
    pub payload: RaftInterfaceRequestPayload,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, SerializedBytes)]
pub struct RaftInterfaceResponse {
    /// The workspace the request was made for
    pub raft_id: RaftSpace,
    /// The actual response
    pub payload: RaftInterfaceResponsePayload,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, SerializedBytes)]
pub enum RaftInterfaceRequestPayload {
    /// Initialize the raft network with the provided peers
    Initialize(Vec<AgentPubKey>),
    /// Message these peers, telling them to add me to their raft cluster
    Join(Vec<AgentPubKey>),
    /// Leave the raft network
    Leave,
    /// Propose an operation to the raft network
    Propose(RaftOp),
    /// Get user-created log entries after the given log id
    GetUserLogEntries(Option<u64>),
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    SerializedBytes,
    derive_more::Unwrap,
)]
pub enum RaftInterfaceResponsePayload {
    /// Response to [`RaftInterfaceRequestPayload::Initialize`]
    Initialized,

    /// Response to [`RaftInterfaceRequestPayload::Join`]
    Joined,

    /// Response to [`RaftInterfaceRequestPayload::Join`]
    CouldNotJoin(holochain_raft::P2pRaftError),

    /// Response to [`RaftInterfaceRequestPayload::GetUserLogEntries`]
    UserLogEntries(Vec<holochain_raft::LogOp>),

    /// Response to [`RaftInterfaceRequestPayload::Propose`]
    /// and [`RaftInterfaceRequestPayload::Leave`]
    P2pResponse(holochain_raft::message::P2pResponse),
}
