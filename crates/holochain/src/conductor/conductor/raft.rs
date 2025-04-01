use futures::stream::FuturesUnordered;
use holochain_conductor_api::{
    RaftInterfaceRequest, RaftInterfaceRequestPayload, RaftInterfaceResponsePayload,
};
use holochain_raft::{message::*, *};

use super::*;

fn make_config() -> p2p_raft::Config {
    p2p_raft::Config {
        p2p_config: Default::default(),
        raft_config: OpenraftConfig {
            heartbeat_interval: 500,
            election_timeout_min: 1500,
            election_timeout_max: 3000,
            // max_in_snapshot_log_to_keep: 0,
            ..Default::default()
        },
    }
}

impl Conductor {
    /// Get a raft instance
    pub async fn get_raft(
        &self,
        installed_app_id: InstalledAppId,
        dna_hash: DnaHash,
        raft_id: RaftSpace,
    ) -> Yacht {
        let provenance = crate::core::workflow::sys_validation_workflow::get_representative_agent(
            self, &dna_hash,
        )
        .expect("TODO");

        self.lookup_raft(installed_app_id, dna_hash, provenance, raft_id)
            .await
    }

    pub(crate) async fn handle_raft_rpc_call(
        &self,
        installed_app_id: InstalledAppId,
        dna_hash: DnaHash,
        request: RpcRequestEnvelope,
        remote_agent: AgentPubKey,
    ) -> ConductorResult<RpcResponse> {
        // TODO: the representative agent must change if this agent ever leaves the network (and there are other local agents)
        let local_agent = crate::core::workflow::sys_validation_workflow::get_representative_agent(
            self, &dna_hash,
        )
        .expect("TODO");

        let raft_id = request.raft_id;

        let data = self
            .lookup_raft(
                installed_app_id,
                dna_hash.clone(),
                local_agent.clone(),
                raft_id.clone(),
            )
            .await;

        let res = data
            .raft
            .raft
            .handle_rpc(remote_agent.clone().into(), request.payload)
            .await
            .map_err(|e| {
                ConductorError::other(format!("TODO handle_incoming_request error: {e:?}"))
            })?;

        {
            let mut t = data.raft.raft.tracker.lock().await;
            t.touch(&holochain_raft::HcNode::from(remote_agent));
            t.handle_absentees(
                &data.raft.raft,
                data.raft.raft.config.p2p_config.responsive_interval,
            )
            .await;
        }

        Ok(res)
    }

    pub(crate) async fn handle_raft_interface_call(
        &self,
        installed_app_id: InstalledAppId,
        raft_call: RaftInterfaceRequest,
    ) -> ConductorResult<RaftInterfaceResponsePayload> {
        let dna_hash = raft_call.dna_hash.clone();

        // TODO: the representative agent must change if this agent ever leaves the network (and there are other local agents)
        let local_agent = crate::core::workflow::sys_validation_workflow::get_representative_agent(
            self, &dna_hash,
        )
        .expect("TODO");

        let Catamaran {
            client, mut raft, ..
        } = self
            .lookup_raft(
                installed_app_id,
                dna_hash.clone(),
                local_agent.clone(),
                raft_call.raft_space,
            )
            .await
            .raft;

        match raft_call.payload {
            RaftInterfaceRequestPayload::Initialize(peers) => {
                raft.initialize(peers.into_iter().map(HcNode::from))
                    .await
                    .map_err(|e| ConductorError::other(format!("can't initialize: {e:?}")))?;

                Ok(RaftInterfaceResponsePayload::Initialized)
            }
            RaftInterfaceRequestPayload::Join(peers) => {
                // Ask all known peers to join
                let mut futs: FuturesUnordered<_> = peers
                    .into_iter()
                    .map(move |peer| {
                        let client = client.clone();
                        let msg = P2pRequest::Join;
                        async move {
                            anyhow::Ok(client.call(peer.clone(), msg.into()).await?.unwrap_p_2_p())
                        }
                    })
                    .collect();

                let mut errors = Vec::new();

                while let Some(res) = futs.next().await {
                    match res {
                        Ok(res) => {
                            if res.is_ok() {
                                // Return early if we successfully joined
                                return Ok(RaftInterfaceResponsePayload::Joined);
                            } else {
                                errors.push(format!("p2p error joining: {res:?}"));
                            }
                        }
                        Err(e) => errors.push(e.to_string()),
                    }
                }

                Ok(RaftInterfaceResponsePayload::CouldNotJoin(errors))
            }
            RaftInterfaceRequestPayload::Leave => {
                let res = client
                    .call_leader_with_retry(P2pRequest::Leave.into())
                    .await
                    .map_err(|e| ConductorError::other(format!("Raft Leave call failed: {e:?}")))?
                    .unwrap_p_2_p();
                Ok(RaftInterfaceResponsePayload::P2pResponse(res))
            }
            RaftInterfaceRequestPayload::Propose(op) => {
                // XXX: first call is to self. No need to use the client for this.
                let res = client
                    .call_leader_with_retry(P2pRequest::Propose(op).into())
                    .await
                    .map_err(|e| ConductorError::other(format!("Raft Propose call failed: {e:?}")))?
                    .unwrap_p_2_p();

                Ok(RaftInterfaceResponsePayload::P2pResponse(res))
            }
            RaftInterfaceRequestPayload::GetUserLogEntries(index) => {
                let mut reader = raft.store.get_log_reader().await;

                let entries = if let Some(index) = index {
                    reader.try_get_log_entries(index..).await
                } else {
                    reader.try_get_log_entries(..).await
                }
                .map_err(|e| ConductorError::other(e.to_string()))?
                .into_iter()
                .filter_map(|l| match l.payload {
                    EntryPayload::Normal(n) => Some(holochain_raft::LogOp {
                        log_id: l.log_id,
                        op: n,
                    }),
                    _ => None,
                })
                .collect();

                Ok(RaftInterfaceResponsePayload::UserLogEntries(entries))
            }
        }
    }

    async fn lookup_raft(
        &self,
        installed_app_id: InstalledAppId,
        dna_hash: DnaHash,
        local_agent: AgentPubKey,
        raft_id: RaftSpace,
    ) -> Yacht {
        let yacht = {
            let mut rafts = self.rafts.lock().await;
            match rafts.entry((dna_hash.clone(), raft_id.clone())) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    let hc_raft = self
                        .create_raft(installed_app_id.clone(), dna_hash, local_agent, raft_id)
                        .await;
                    v.insert(hc_raft.clone());
                    hc_raft
                }
                std::collections::hash_map::Entry::Occupied(o) => o.get().clone(),
            }
        };

        if yacht.installed_app_id != installed_app_id {
            panic!("can't lookup raft for two different installed app ids");
        }

        yacht
    }

    async fn create_raft(
        &self,
        installed_app_id: InstalledAppId,
        dna_hash: DnaHash,
        local_agent: AgentPubKey,
        raft_space: RaftSpace,
    ) -> Yacht {
        let client = HcClient {
            local_agent: local_agent.clone(),
            keystore: self.keystore().clone(),
            raft_space: raft_space.clone(),
            network: self.holochain_p2p().to_dna(dna_hash.clone(), None),
            raft: Arc::new(Mutex::new(None)),
        };

        let raft_lock = client.raft.clone();

        let (signal_tx, signal_rx) = tokio::sync::mpsc::channel(100);

        let config = make_config();
        let raft_id = local_agent.clone().into();
        let raft = holochain_raft::P2pRaft::spawn_memory(
            raft_id,
            config,
            client.clone(),
            Some(signal_tx),
            |_| (),
        )
        .await
        .expect("couldn't create raft");

        *raft_lock.lock().await = Some(raft.clone());

        if let Err(err) = self
            .raft_signal_receiver_sender
            .send((installed_app_id.clone(), raft_space, signal_rx))
            .await
        {
            tracing::warn!("raft signal receiver receiver dropped: {err:?}");
        }

        let cat = Catamaran { client, raft };

        // let sink = {
        //     let tx = self
        //         .app_broadcast
        //         .create_send_handle(installed_app_id.clone());
        //     Box::new(
        //         tokio_util::sync::PollSender::new(tx)
        //             .with(|signal| futures::future::ok((raft_id, signal))),
        //     )
        // };

        Yacht {
            raft: cat,
            installed_app_id,
        }
    }
}

#[derive(Clone, derive_more::Deref)]
pub struct Yacht {
    #[deref]
    pub raft: Catamaran,
    installed_app_id: InstalledAppId,
}
