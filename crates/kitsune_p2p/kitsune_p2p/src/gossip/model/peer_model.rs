use std::collections::{BTreeMap, HashMap};

use crate::{
    dependencies::kitsune_p2p_types::{KitsuneError, KitsuneResult},
    gossip::sharded_gossip::{
        store::AgentInfoSession, Initiate, ShardedGossipLocal, ShardedGossipWire,
    },
};
use anyhow::{anyhow, bail};
use exhaustive::Exhaustive;
use kitsune_p2p_bin_data::NodeCert;
use kitsune_p2p_types::GossipType;
use polestar::{
    id::{IdMap, UpTo},
    prelude::*,
};
use proptest_derive::Arbitrary;

use crate::gossip::model::round_model::{RoundAction, RoundFsm, RoundPhase};

use super::{NodeId, PEERS};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub struct PeerModel {
    pub rounds: BTreeMap<NodeId, RoundFsm>,
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

    fn finish_round(&mut self, peer: &NodeId) {
        if self.initiate_tgt.as_ref().map(|t| &t.cert) == Some(peer) {
            self.initiate_tgt = None;
        }
        self.rounds.remove(peer);
    }
}

impl Machine for PeerModel {
    type Action = NodeAction;
    type Fx = Vec<(NodeId, RoundAction)>;
    type Error = anyhow::Error;

    #[allow(clippy::map_entry)]
    fn transition(mut self, action: Self::Action) -> MachineResult<Self> {
        let fx = match action {
            NodeAction::SetInitiate(with_node, tie_break) => {
                if self.initiate_tgt.is_none() {
                    self.initiate_tgt = Some(Tgt {
                        cert: with_node.clone(),
                        tie_break,
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
                        tracing::error!("already a round for {from:?}");
                        vec![]
                        // bail!("already a round for {from:?}");
                    } else if self.initiate_tgt.as_ref().map(|t| &t.cert) == Some(&from) {
                        bail!("invalid Initiate from node that is initiate_tgt");
                    } else {
                        self.rounds.insert(
                            from.clone(),
                            RoundPhase::Initiated.context(self.gossip_type),
                        );
                        vec![
                            (from.clone(), RoundAction::Accept),
                            if self.gossip_type == GossipType::Recent {
                                (from, RoundAction::AgentDiff)
                            } else {
                                (from, RoundAction::OpDiff)
                            },
                        ]
                    }
                }
                RoundAction::Accept => {
                    if self.rounds.contains_key(&from) {
                        tracing::error!("already a round for {from:?}");
                        vec![]
                        // bail!("already a round for {from:?}");
                    } else if self.initiate_tgt.as_ref().map(|t| &t.cert) != Some(&from) {
                        bail!("invalid Accept from node that is not initiate_tgt");
                    } else {
                        self.rounds.insert(
                            from.clone(),
                            RoundPhase::Initiated.context(self.gossip_type),
                        );
                        if self.gossip_type == GossipType::Recent {
                            vec![(from, RoundAction::AgentDiff)]
                        } else {
                            vec![(from, RoundAction::OpDiff)]
                        }
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

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, Exhaustive, derive_more::From)]
pub struct Tgt {
    pub cert: NodeId,
    /// In the SUT, the tie breaker is a random u32.
    /// To minimize state space and to greatly increase the chance of collision,
    /// we use a bool instead: true is greater than false.
    pub tie_break: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, Exhaustive, derive_more::From)]
pub enum NodeAction {
    SetInitiate(NodeId, bool),
    Timeout(NodeId),
    #[from]
    Incoming {
        from: NodeId,
        msg: RoundAction,
    },
}

#[derive(Default)]
pub struct PeerProjection {
    ids: IdMap<PEERS, NodeCert>,
}

impl PeerProjection {
    pub fn id(&mut self, cert: NodeCert) -> NodeId {
        self.ids.lookup(cert).unwrap()
    }
}

impl Projection for PeerProjection {
    type System = ShardedGossipLocal;
    type Model = PeerModel;
    type Event = (NodeId, ShardedGossipWire);

    fn apply(&self, system: &mut Self::System, (node, msg): Self::Event) {
        unimplemented!("application not implemented")
    }

    fn map_event(&mut self, (from, msg): Self::Event) -> Option<NodeAction> {
        super::round_model::map_event(msg).map(|msg| NodeAction::Incoming { from, msg })
    }

    fn map_state(&mut self, system: &Self::System) -> Option<PeerModel> {
        let state = system
            .inner
            .share_mut(|s, _| {
                let rounds = s
                    .round_map
                    .map
                    .iter()
                    .map(|(k, mut v)| {
                        (
                            self.ids.lookup(k.clone()).unwrap(),
                            super::round_model::map_state(v.clone())
                                .unwrap()
                                .context(system.gossip_type),
                        )
                    })
                    .collect::<BTreeMap<NodeId, RoundFsm>>()
                    .into();

                let initiate_tgt = s.initiate_tgt.as_ref().map(|t| Tgt {
                    cert: self.ids.lookup(t.cert.clone()).unwrap(),
                    tie_break: t.tie_break > (u32::MAX / 2),
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

    fn gen_event(&mut self, generator: &mut impl Generator, event: NodeAction) -> Self::Event {
        unimplemented!("generation not implemented")
    }

    fn gen_state(&mut self, generator: &mut impl Generator, state: PeerModel) -> Self::System {
        unimplemented!("generation not implemented")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use crate::wire::Gossip;

    use super::*;
    use polestar::diagram::{exhaustive::*, to_dot};
    use proptest::prelude::*;
    use proptest::*;

    // TODO: map this to an even simpler model with symmetry around the NodeCerts?
    // TODO: how to do symmetry?
    #[test]
    #[ignore = "diagram, and it's way too big to even make sense of"]
    fn diagram_peer_model() {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();

        let config = DiagramConfig {
            max_actions: None,
            max_distance: None,
            max_iters: Some(100000),
            ignore_loopbacks: true,
        };

        let model = PeerModel::new(GossipType::Recent);

        println!("{}", to_dot(state_diagram(model, &config)));
    }
}
