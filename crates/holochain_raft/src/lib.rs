mod client;
pub mod message;
mod network;

pub use client::HcClient;
use holo_hash::AgentPubKey;

pub use openraft::error;
pub use openraft::storage::RaftLogStorage;
pub use openraft::{Config as OpenraftConfig, Entry, EntryPayload, LogId, RaftLogReader};

pub use p2p_raft::Config;

pub type LeaderId = openraft::impls::leader_id_adv::LeaderId<HcrTypes>;
pub type P2pRaft = p2p_raft::P2pRaft<HcrTypes, HcClient>;
pub type RaftEvent = p2p_raft::signal::RaftEvent<HcrTypes>;
pub type LogOp = p2p_raft::LogOp<HcrTypes>;

openraft::declare_raft_types!(
    #[derive(serde::Serialize, serde::Deserialize)]
    pub HcrTypes:
        D = RaftOp,
        R = (),
        NodeId = HcNode,
        Node = (),
        SnapshotData = p2p_raft::StateMachineData<Self>,
);

impl p2p_raft::TypeCfg for HcrTypes {}

/// State for a raft instance in the conductor.
///
/// A step up from a P2pRaft.
#[derive(Clone)]
pub struct Catamaran {
    /// The raft instance
    pub raft: P2pRaft,
    /// The client for making remote calls to other conductors' rafts
    pub client: HcClient,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::From,
    derive_more::Deref,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct RaftSpace(String);

impl From<&str> for RaftSpace {
    fn from(s: &str) -> Self {
        RaftSpace(s.to_string())
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Deref,
)]
#[serde(transparent)]
pub struct HcNode(holo_hash::AgentPubKeyB64);

impl std::fmt::Display for HcNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", AgentPubKey::from(self.0.clone()).suffix(4))
    }
}

impl openraft::NodeId for HcNode {}

impl From<holo_hash::AgentPubKey> for HcNode {
    fn from(agent: holo_hash::AgentPubKey) -> Self {
        HcNode(agent.into())
    }
}

impl HcNode {
    pub fn agent(&self) -> holo_hash::AgentPubKey {
        self.0.clone().into()
    }
}

impl Default for HcNode {
    fn default() -> Self {
        HcNode(holo_hash::AgentPubKey::from_raw_32(vec![0; 32]).into())
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Deref,
    derive_more::From,
    derive_more::Into,
)]
pub struct RaftOp(#[serde(with = "serde_bytes")] Vec<u8>);
