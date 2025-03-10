use holochain_raft::{HcrTypes, RaftId, RaftOp};

use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, SerializedBytes)]
pub struct RaftInterfaceRequest {
    /// Hash of the network which contains the peers to sync with, e.g. `syn`
    pub dna_hash: DnaHash,
    /// A new raft instance is created for each workspace
    pub raft_id: RaftId,
    /// The actual request
    pub payload: RaftInterfaceRequestPayload,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, SerializedBytes)]
pub struct RaftInterfaceResponse {
    /// The workspace the request was made for
    pub raft_id: RaftId,
    /// The actual response
    pub payload: RaftInterfaceResponsePayload,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, SerializedBytes)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RaftInterfaceRequestPayload {
    /// Initialize the raft network with the provided peers
    Initialize(Vec<AgentPubKey>),
    /// Message these peers, telling them to add me to their raft cluster
    Join(Vec<AgentPubKey>),
    /// Leave the raft network
    Leave,
    /// Propose an operation to the raft network
    Propose(RaftOp),
    /// Get log entries after the given log id
    GetAllLogEntries(Option<u64>),
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
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RaftInterfaceResponsePayload {
    AllLogEntries(Vec<holochain_raft::Entry<HcrTypes>>),
    UserLogEntries(Vec<LogOp>),
    Ok,
    Error(holochain_raft::message::P2pResponse),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, SerializedBytes)]
pub struct LogOp {
    pub log_id: holochain_raft::LogId<HcrTypes>,
    pub op: RaftOp,
}
