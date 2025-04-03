use std::collections::BTreeSet;

use holochain_raft::{LogId, RaftOp, RaftSpace};

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

    /// Get raft info
    GetRaftInfo,
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
    /// Response to [`RaftInterfaceRequestPayload::Join`]
    /// Response to [`RaftInterfaceRequestPayload::Leave`]
    Ok,

    /// Response to [`RaftInterfaceRequestPayload::Propose`]
    #[unwrap(ignore)]
    Committed { log_id: LogId },

    /// Response to [`RaftInterfaceRequestPayload::GetUserLogEntries`]
    UserLogEntries(Vec<holochain_raft::LogOp>),

    /// Shared error type for all raft interface requests
    Error(holochain_raft::P2pRaftError),

    /// Response to [`RaftInterfaceRequestPayload::RaftInfo`]
    RaftInfo(RaftInfo),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, SerializedBytes)]
pub struct RaftInfo {
    pub current_leader: Option<AgentPubKey>,
    pub status: holochain_raft::openraft::ServerState,
    pub voters: BTreeSet<AgentPubKey>,
}
