use crate::HcrTypes;

// #[derive(Clone, Debug, derive_more::From, serde::Serialize, serde::Deserialize)]
// pub enum RaftRpc {
//     Request(RaftRpcRequest),
//     Response(RaftRpcResponse),
// }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RpcRequestEnvelope {
    pub raft_id: crate::RaftSpace,
    pub payload: RpcRequest,
}

pub type RpcRequest = p2p_raft::message::Request<HcrTypes>;
pub type RpcResponse = p2p_raft::message::Response<HcrTypes>;

pub type P2pRequest = p2p_raft::message::P2pRequest<HcrTypes>;
pub type P2pResponse = p2p_raft::message::P2pResponse<HcrTypes>;

pub type RaftRequest = p2p_raft::message::RaftRequest<HcrTypes>;
pub type RaftResponse = p2p_raft::message::RaftResponse<HcrTypes>;
