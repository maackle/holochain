use std::future::Future;

use anyerror::AnyError;
use openraft::{
    alias::VoteOf,
    error::{ReplicationClosed, StreamingError, Unreachable},
    network::v2::RaftNetworkV2,
    raft::SnapshotResponse,
    OptionalSend, Snapshot,
};
use openraft::{
    error::RPCError,
    network::RPCOption,
    raft::{AppendEntriesRequest, AppendEntriesResponse, VoteRequest, VoteResponse},
    RaftNetworkFactory,
};
use p2p_raft::message::RaftRequest;

use crate::{client::HcClient, HcNode, HcrTypes};

#[derive(Clone)]
pub struct HcNetwork {
    target: HcNode,
    client: HcClient,
}

impl RaftNetworkFactory<HcrTypes> for HcClient {
    type Network = HcNetwork;

    async fn new_client(&mut self, target: HcNode, _: &()) -> Self::Network {
        HcNetwork {
            target,
            client: self.clone(),
        }
    }
}

impl p2p_raft::network::P2pNetwork<HcrTypes> for HcClient {
    async fn send_p2p(
        &self,
        _source: HcNode,
        target: HcNode,
        req: p2p_raft::message::P2pRequest<HcrTypes>,
    ) -> Result<p2p_raft::message::P2pResponse<HcrTypes>, RPCError<HcrTypes>> {
        match self.call(target.agent(), req.into()).await {
            Ok(resp) => Ok(resp.unwrap_p_2_p()),
            Err(e) => {
                tracing::error!("{e:?}");
                Err(RPCError::Unreachable(Unreachable::new(&AnyError::from(e))))
            }
        }
    }
}

impl RaftNetworkV2<HcrTypes> for HcNetwork {
    /// Send an AppendEntries RPC to the target.
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<HcrTypes>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<HcrTypes>, RPCError<HcrTypes>> {
        // println!("<RAFT> append_entries {rpc:?}");
        match self
            .client
            .call(self.target.agent(), RaftRequest::from(rpc).into())
            .await
        {
            Ok(resp) => Ok(resp.unwrap_raft().unwrap_append()),
            Err(e) => {
                tracing::error!("{e:?}");
                Err(RPCError::Unreachable(Unreachable::new(&AnyError::from(e))))
            }
        }
    }

    async fn full_snapshot(
        &mut self,
        vote: VoteOf<HcrTypes>,
        snapshot: Snapshot<HcrTypes>,
        _cancel: impl Future<Output = ReplicationClosed> + OptionalSend + 'static,
        _option: RPCOption,
    ) -> Result<SnapshotResponse<HcrTypes>, StreamingError<HcrTypes>> {
        let rpc = RaftRequest::Snapshot {
            vote,
            snapshot_meta: snapshot.meta,
            snapshot_data: snapshot.snapshot,
        };
        match self.client.call(self.target.agent(), rpc.into()).await {
            Ok(resp) => Ok(resp.unwrap_raft().unwrap_snapshot()),
            Err(e) => {
                tracing::error!("{e:?}");
                Err(StreamingError::Unreachable(Unreachable::new(
                    &AnyError::from(e),
                )))
            }
        }
    }

    /// Send a RequestVote RPC to the target.
    async fn vote(
        &mut self,
        rpc: VoteRequest<HcrTypes>,
        _option: RPCOption,
    ) -> Result<VoteResponse<HcrTypes>, RPCError<HcrTypes>> {
        // println!("<RAFT> vote {rpc:?}");
        match self
            .client
            .call(self.target.agent(), RaftRequest::from(rpc).into())
            .await
        {
            Ok(resp) => Ok(resp.unwrap_raft().unwrap_vote()),
            Err(e) => {
                tracing::error!("{e:?}");
                Err(RPCError::Unreachable(Unreachable::new(&AnyError::from(e))))
            }
        }
    }
}
