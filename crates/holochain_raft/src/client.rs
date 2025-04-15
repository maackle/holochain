use std::sync::Arc;

use crate::{message::*, HcNode, HcrTypes};
use holochain_keystore::MetaLairClient;
use holochain_p2p::actor::DynHcP2p;
use holochain_types::prelude::*;
use p2p_raft::P2pRaft;
use tokio::sync::Mutex;

use crate::RaftSpace;

#[derive(Clone)]
pub struct HcClient {
    pub local_agent: AgentPubKey,
    pub dna_hash: DnaHash,
    pub network: DynHcP2p,
    pub raft_space: RaftSpace,
    pub keystore: MetaLairClient,
    // XXX: circular reference, raft must be passed in after this is passed to raft
    pub raft: Arc<Mutex<Option<P2pRaft<HcrTypes, HcClient>>>>,
}

impl HcClient {
    pub async fn call_leader_with_retry(&self, message: P2pRequest) -> anyhow::Result<P2pResponse> {
        use p2p_raft::Error::*;

        let retries = 3;
        let mut target = self.local_agent.clone();
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        for _ in 0..retries {
            interval.tick().await;
            let res = self.call(target.clone(), message.clone().into()).await?;
            match res {
                RpcResponse::P2p(r) => match r {
                    P2pResponse::Error(ref e) => match e {
                        // Retry with the newly discovered leader
                        NotLeader(Some((leader, _))) => {
                            target = leader.agent();
                        }
                        // Return immediately if retrying won't help
                        Rejected | NotLeader(None) | Fatal(_) => return Ok(r.clone()),
                    },
                    // Return success immediately
                    r => return Ok(r),
                },
                // The target node is not responding appropriately, can't do anything about this.
                r => anyhow::bail!("unexpected non-p2p response: {:?}", r),
            }
        }
        anyhow::bail!("Failed to find the leader after {} retries", retries);
    }

    pub async fn call(
        &self,
        target: AgentPubKey,
        message: RpcRequest,
    ) -> anyhow::Result<RpcResponse> {
        let out = self.call_zome_remote(target.clone(), message).await?;
        let zcr = ZomeCallResponse::try_from(out)?;
        match zcr {
            ZomeCallResponse::Ok(out) => {
                let res = out.decode()?;
                if let Some(raft) = self.raft.lock().await.as_ref() {
                    let mut t = raft.tracker.lock().await;

                    t.touch(&HcNode(target.into()));
                    t.handle_absentees(&raft, raft.config.responsive_interval)
                        .await;
                } else {
                    tracing::warn!("raft not yet set in client");
                }
                Ok(res)
            }
            // ZomeCallResponse::Ok(out) => Ok(RaftRpcResponse::try_from(out)?),
            _ => anyhow::bail!("call: unexpected response: {:?}", zcr),
        }
    }

    pub async fn call_zome_remote(
        &self,
        target: AgentPubKey,
        message: RpcRequest,
    ) -> anyhow::Result<SerializedBytes> {
        let payload = ExternIO::encode(RpcRequestEnvelope {
            raft_id: self.raft_space.clone(),
            payload: message,
        })?;

        let now = Timestamp::now();
        let (nonce, expires_at) =
            holochain_nonce::fresh_nonce(now).map_err(|e| anyhow::anyhow!(e))?;

        let params = ZomeCallParams {
            cell_id: CellId::new(self.dna_hash.clone(), self.local_agent.clone()),
            zome_name: ZomeName::from("raft-hardwired-hack"),
            fn_name: FunctionName::from("raft-hardwired-hack"),
            cap_secret: None,
            provenance: self.local_agent.clone(),
            payload,
            nonce,
            expires_at,
        };

        let (bytes, bytes_hash) = params
            .serialize_and_hash()
            .map_err(|e| anyhow::anyhow!(e))?;
        let signature = params
            .provenance
            .sign_raw(&self.keystore, bytes_hash.into())
            .await?;
        let bytes = ExternIO::from(bytes);

        // let signature = self
        //     .local_agent
        //     .sign_raw(&self.keystore, params.as_bytes().into())
        //     .await?;

        Ok(self
            .network
            .call_remote(self.dna_hash.clone(), target, bytes, signature)
            .await?)
    }
}
