use std::collections::{BTreeMap, HashMap};

use crate::{
    dependencies::kitsune_p2p_types::{KitsuneError, KitsuneResult},
    gossip::sharded_gossip::{
        store::AgentInfoSession, Initiate, ShardedGossipLocal, ShardedGossipWire,
    },
    NodeCert,
};
use anyhow::{anyhow, bail};
use kitsune_p2p_types::GossipType;
use polestar::prelude::*;
use proptest_derive::Arbitrary;

use crate::gossip::model::round_model::{RoundAction, RoundFsm, RoundPhase};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub struct PeerModel {
    pub rounds: BTreeMap<NodeCert, RoundFsm>,
    pub initiate_tgt: Option<Tgt>,
    pub gossip_type: GossipType,
}

impl PeerModel {
    pub fn new(gossip_type: GossipType) -> Self {
        Self {
            rounds: BTreeMap::default(),
            initiate_tgt: None,
            gossip_type,
        }
    }

    fn finish_round(&mut self, peer: &NodeCert) {
        if self.initiate_tgt.as_ref().map(|t| &t.cert) == Some(peer) {
            self.initiate_tgt = None;
        }
        self.rounds.remove(peer);
    }
}

impl Machine for PeerModel {
    type Action = NodeAction;
    type Fx = Vec<(NodeCert, RoundAction)>;
    type Error = anyhow::Error;

    #[allow(clippy::map_entry)]
    fn transition(mut self, action: Self::Action) -> MachineResult<Self> {
        let fx = match action {
            NodeAction::SetInitiate(with_node) => {
                if self.initiate_tgt.is_none() {
                    self.initiate_tgt = Some(Tgt {
                        cert: with_node.clone(),
                        tie_break: 0,
                    });
                }
                vec![(with_node, RoundAction::Initiate)]
            }
            NodeAction::Timeout(peer) => {
                self.rounds.remove(&peer);
                self.finish_round(&peer);
                vec![]
            }
            NodeAction::Incoming { from, msg } => match msg {
                RoundAction::Initiate => {
                    if self.rounds.contains_key(&from) {
                        bail!("already a round for {from:?}");
                    } else if self.initiate_tgt.as_ref().map(|t| &t.cert) == Some(&from) {
                        bail!("invalid Initiate from node that is initiate_tgt");
                    } else {
                        self.rounds.insert(
                            from.clone(),
                            RoundPhase::Initiated.context(self.gossip_type),
                        );
                    }

                    vec![
                        (from.clone(), RoundAction::Accept),
                        if self.gossip_type == GossipType::Recent {
                            (from, RoundAction::AgentDiff)
                        } else {
                            (from, RoundAction::OpDiff)
                        },
                    ]
                }
                RoundAction::Accept => {
                    if self.rounds.contains_key(&from) {
                        bail!("already a round for {from:?}");
                    } else if self.initiate_tgt.as_ref().map(|t| &t.cert) != Some(&from) {
                        bail!("invalid Accept from node that is not initiate_tgt");
                    } else {
                        self.rounds.insert(
                            from.clone(),
                            RoundPhase::Initiated.context(self.gossip_type),
                        );
                    }

                    if self.gossip_type == GossipType::Recent {
                        vec![(from, RoundAction::AgentDiff)]
                    } else {
                        vec![(from, RoundAction::OpDiff)]
                    }
                }
                RoundAction::Close => {
                    self.rounds.remove(&from);
                    vec![]
                }
                action => {
                    let (round, fx) = self
                        .rounds
                        .remove(&from)
                        .ok_or(anyhow!("no round for {from:?}"))?
                        .transition(action)?;
                    if matches!(&*round, RoundPhase::Finished) {
                        self.finish_round(&from);
                    } else {
                        self.rounds.insert(from.clone(), round);
                    }
                    fx.into_iter().map(|msg| (from.clone(), msg)).collect()
                }
            },
        };
        Ok((self, fx))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub struct Tgt {
    pub cert: NodeCert,
    // TODO: handle tie breaker
    pub tie_break: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub enum NodeAction {
    SetInitiate(NodeCert),
    Timeout(NodeCert),
    #[from]
    Incoming {
        from: NodeCert,
        msg: RoundAction,
    },
}

pub struct PeerProjection;

impl Projection for PeerProjection {
    type System = ShardedGossipLocal;
    type Model = PeerModel;
    type Event = (NodeCert, ShardedGossipWire);

    fn apply(&self, system: &mut Self::System, (node, msg): Self::Event) {
        unimplemented!("application not implemented")
    }

    fn map_event(&self, (from, msg): Self::Event) -> Option<NodeAction> {
        super::round_model::map_event(msg).map(|msg| NodeAction::Incoming { from, msg })
    }

    fn map_state(&self, system: &Self::System) -> Option<PeerModel> {
        let state = system
            .inner
            .share_mut(|s, _| {
                let rounds = s
                    .round_map
                    .map
                    .iter()
                    .map(|(k, mut v)| {
                        (
                            k.clone(),
                            super::round_model::map_state(v.clone())
                                .unwrap()
                                .context(system.gossip_type),
                        )
                    })
                    .collect::<BTreeMap<NodeCert, RoundFsm>>()
                    .into();

                let initiate_tgt = s.initiate_tgt.as_ref().map(|t| Tgt {
                    cert: t.cert.clone(),
                    tie_break: t.tie_break,
                });
                Ok(PeerModel {
                    rounds,
                    initiate_tgt,
                    gossip_type: system.gossip_type,
                })
            })
            .unwrap();
        Some(state)
    }

    fn gen_event(&self, generator: &mut impl Generator, event: NodeAction) -> Self::Event {
        unimplemented!("generation not implemented")
    }

    fn gen_state(&self, generator: &mut impl Generator, state: PeerModel) -> Self::System {
        unimplemented!("generation not implemented")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use crate::wire::Gossip;

    use super::*;
    use polestar::diagram::{montecarlo::*, to_dot};
    use proptest::prelude::*;
    use proptest::*;

    // TODO: map this to an even simpler model with symmetry around the NodeCerts?
    // TODO: how to do symmetry?
    #[test]
    #[ignore = "diagram"]
    fn diagram_gossip_model() {
        tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::new()).unwrap();

        let config = DiagramConfig {
            steps: 100,
            walks: 10,
            ignore_loopbacks: true,
        };

        struct GossipModelDiagrammer {
            certs: Vec<NodeCert>,
        }

        impl GossipModelDiagrammer {
            pub fn new() -> Self {
                Self {
                    certs: vec![
                        NodeCert::from(Arc::new([1u8; 32])),
                        NodeCert::from(Arc::new([2u8; 32])),
                        NodeCert::from(Arc::new([3u8; 32])),
                    ],
                }
            }
        }

        impl MonteCarloDiagramState<PeerModel> for GossipModelDiagrammer {
            fn strategy(&self) -> BoxedStrategy<NodeAction> {
                prop_oneof![
                    sample::select(self.certs.clone()).prop_map(NodeAction::SetInitiate),
                    (sample::select(self.certs.clone()), RoundAction::arbitrary())
                        .prop_map(|(from, msg)| NodeAction::Incoming { from, msg })
                ]
                .boxed()
            }
        }

        let model = PeerModel::new(GossipType::Recent);

        println!(
            "{}",
            to_dot(state_diagram(
                model,
                &mut GossipModelDiagrammer::new(),
                &config
            ))
        );
    }
}
