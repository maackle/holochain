use std::sync::Arc;

use crate::{message::*, HcNode, HcrTypes};
use holochain_keystore::MetaLairClient;
use holochain_p2p::{HolochainP2pDna, HolochainP2pDnaT};
use holochain_types::prelude::*;
use p2p_raft::P2pRaft;
use tokio::sync::Mutex;

use crate::RaftSpace;

#[derive(Clone)]
pub struct HcClient {
    pub local_agent: AgentPubKey,
    pub network: HolochainP2pDna,
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
        let (nonce, expires_at) =
            holochain_nonce::fresh_nonce(Timestamp::now()).map_err(|e| anyhow::anyhow!(e))?;

        let dna_hash = self.network.dna_hash();
        let cell_id = CellId::new(dna_hash, target.clone());

        let payload = ExternIO::encode(RpcRequestEnvelope {
            raft_id: self.raft_space.clone(),
            payload: message,
        })?;

        let zome_call_unsigned = ZomeCallUnsigned {
            provenance: self.local_agent.clone(),
            cell_id,
            zome_name: "raft-hardwired-hack".into(),
            fn_name: "raft-hardwired-hack".into(),
            cap_secret: None,
            payload,
            nonce,
            expires_at,
        };

        Ok(self
            .network
            .call_remote(
                self.local_agent.clone(),
                zome_call_unsigned
                    .provenance
                    .sign_raw(&self.keystore, zome_call_unsigned.data_to_sign()?)
                    .await?,
                target,
                zome_call_unsigned.zome_name,
                zome_call_unsigned.fn_name,
                zome_call_unsigned.cap_secret,
                zome_call_unsigned.payload,
                zome_call_unsigned.nonce,
                zome_call_unsigned.expires_at,
            )
            .await?)
    }
}
