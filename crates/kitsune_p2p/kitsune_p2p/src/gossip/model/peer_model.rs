use std::collections::{BTreeMap, HashMap};

use crate::{
    dependencies::kitsune_p2p_types::{KitsuneError, KitsuneResult},
    gossip::sharded_gossip::{
        store::AgentInfoSession, Initiate, ShardedGossipEvent, ShardedGossipLocal,
        ShardedGossipWire,
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

use crate::gossip::model::round_model::{RoundAction, RoundPhase};

use super::{
    round_model::{RoundMachine, RoundMsg},
    NodeId, PEERS,
};

#[derive(derive_more::Deref)]
pub struct PeerMachine(RoundMachine);

impl PeerMachine {
    pub fn new(gossip_type: GossipType) -> Self {
        Self(RoundMachine(gossip_type))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub struct PeerState {
    pub rounds: BTreeMap<NodeId, RoundPhase>,
    pub initiate_tgt: Option<Tgt>,
}

impl PeerState {
    pub fn new() -> Self {
        Self {
            rounds: BTreeMap::default(),
            initiate_tgt: None,
        }
    }

    fn finish_round(&mut self, peer: &NodeId) {
        if self.initiate_tgt.as_ref().map(|t| &t.cert) == Some(peer) {
            self.initiate_tgt = None;
        }
        self.rounds.remove(peer);
    }
}

impl Machine for PeerMachine {
    type State = PeerState;
    type Action = NodeAction;
    type Fx = Vec<(NodeId, RoundMsg)>;
    type Error = anyhow::Error;

    fn is_terminal(&self, _: &Self::State) -> bool {
        false
    }

    #[allow(clippy::map_entry)]
    fn transition(&self, mut state: Self::State, action: Self::Action) -> TransitionResult<Self> {
        let fx = match action {
            NodeAction::SetInitiate(with_node, tie_break) => {
                if state.initiate_tgt.is_none() {
                    state.initiate_tgt = Some(Tgt {
                        cert: with_node.clone(),
                        tie_break,
                    });
                }
                vec![(with_node, RoundMsg::Initiate)]
            }
            NodeAction::Timeout(peer) => {
                state.rounds.remove(&peer);
                state.finish_round(&peer);
                vec![]
            }
            NodeAction::TieBreakLoser(peer) => {
                if let Some(tgt) = state.initiate_tgt.take() {
                    if tgt.cert == peer {
                        vec![]
                    } else {
                        bail!("model error: tie break winner is not the initiate target");
                    }
                } else {
                    bail!("model error: there was no initiate target");
                }
            }
            NodeAction::Incoming {
                from,
                msg: RoundMsg::Initiate,
            } => {
                if state.rounds.contains_key(&from) {
                    tracing::error!("already a round for {from:?}");
                    vec![]
                    // bail!("already a round for {from:?}");
                } else if state.initiate_tgt.as_ref().map(|t| &t.cert) == Some(&from) {
                    tracing::warn!("Initiate from node that is initiate_tgt. This is expected to happen once per pair when there is a tie breaker.");
                    vec![]
                } else {
                    state
                        .rounds
                        .insert(from.clone(), RoundPhase::Started(false));
                    vec![
                        (from.clone(), RoundMsg::Accept),
                        if self.gossip_type() == GossipType::Recent {
                            (from, RoundMsg::AgentDiff)
                        } else {
                            (from, RoundMsg::OpDiff)
                        },
                    ]
                }
            }
            NodeAction::Incoming {
                from,
                msg: RoundMsg::Accept,
            } => {
                if state.rounds.contains_key(&from) {
                    tracing::error!("already a round for {from:?}");
                    vec![]
                    // bail!("already a round for {from:?}");
                } else if state.initiate_tgt.as_ref().map(|t| &t.cert) != Some(&from) {
                    bail!("invalid Accept from node that is not initiate_tgt");
                } else {
                    state
                        .rounds
                        .insert(from.clone(), RoundPhase::Started(false));
                    if self.gossip_type() == GossipType::Recent {
                        vec![(from, RoundMsg::AgentDiff)]
                    } else {
                        vec![(from, RoundMsg::OpDiff)]
                    }
                }
            }
            NodeAction::Incoming {
                from,
                msg: RoundMsg::Close,
            } => {
                state.rounds.remove(&from);
                vec![]
            }

            action => {
                let from = action.node_id();
                if let Some(round_action) = action.into_round_action() {
                    let round = state
                        .rounds
                        .remove(&from)
                        .ok_or(anyhow!("no round for {from:?}"))?;

                    let (round, fx) = self.0.transition(round, round_action)?;
                    if matches!(round, RoundPhase::Finished) {
                        state.finish_round(&from);
                    } else {
                        state.rounds.insert(from.clone(), round);
                    }
                    fx.into_iter().map(|msg| (from.clone(), msg)).collect()
                } else {
                    bail!("nope")
                }
            }
        };
        Ok((state, fx))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, Exhaustive, derive_more::From)]
pub struct Tgt {
    pub cert: NodeId,
    /// In the SUT, the tie breaker is a random u32.
    /// To minimize state space and to greatly increase the chance of collision,
    /// we use a bool instead: true is greater than false.
    // TODO: this is pretty bogus, rethink this.
    pub tie_break: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, Exhaustive, derive_more::From)]
pub enum NodeAction {
    SetInitiate(NodeId, bool),
    Timeout(NodeId),
    #[from]
    Incoming {
        from: NodeId,
        msg: RoundMsg,
    },
    MustSend(NodeId),
    TieBreakLoser(NodeId),
}

impl NodeAction {
    pub fn node_id(&self) -> NodeId {
        match self {
            NodeAction::Incoming { from, .. } => *from,
            NodeAction::Timeout(node) => *node,
            NodeAction::SetInitiate(node, _) => *node,
            NodeAction::MustSend(node) => *node,
            NodeAction::TieBreakLoser(node) => *node,
        }
    }

    pub fn into_round_action(self) -> Option<RoundAction> {
        match self {
            NodeAction::Incoming { msg, .. } => Some(RoundAction::Msg(msg)),
            NodeAction::MustSend(from) => Some(RoundAction::MustSend),
            _ => None,
        }
    }

    pub fn from_round_action(from: NodeId, action: RoundAction) -> Self {
        match action {
            RoundAction::Msg(msg) => NodeAction::Incoming { from, msg },
            RoundAction::MustSend => NodeAction::MustSend(from),
        }
    }
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
    type Model = PeerMachine;
    type Event = ShardedGossipEvent;

    fn apply(&self, system: &mut Self::System, _: Self::Event) {
        unimplemented!("application not implemented")
    }

    fn map_event(&mut self, event: Self::Event) -> Option<NodeAction> {
        match event {
            ShardedGossipEvent::Msg(from, msg) => match msg {
                ShardedGossipWire::Initiate(initiate) => Some(RoundMsg::Initiate),
                ShardedGossipWire::Accept(accept) => Some(RoundMsg::Accept),
                ShardedGossipWire::Agents(agents) => Some(RoundMsg::AgentDiff),
                ShardedGossipWire::MissingAgents(missing_agents) => Some(RoundMsg::Agents),
                ShardedGossipWire::OpBloom(op_bloom) => Some(RoundMsg::OpDiff),
                ShardedGossipWire::OpRegions(op_regions) => Some(RoundMsg::OpDiff),
                ShardedGossipWire::MissingOpHashes(missing_op_hashes) => Some(RoundMsg::Ops),
                ShardedGossipWire::OpBatchReceived(op_batch_received) => None,

                ShardedGossipWire::Error(_)
                | ShardedGossipWire::Busy(_)
                | ShardedGossipWire::NoAgents(_)
                | ShardedGossipWire::AlreadyInProgress(_) => Some(RoundMsg::Close),
            }
            .map(|msg| NodeAction::Incoming {
                from: self.id(from),
                msg,
            }),

            ShardedGossipEvent::SetInitiate(tgt) => Some(NodeAction::SetInitiate(
                self.id(tgt.cert),
                tgt.tie_break > (u32::MAX / 2),
            )),
            ShardedGossipEvent::MustSend(node_cert) => {
                Some(NodeAction::MustSend(self.id(node_cert)))
            }
            ShardedGossipEvent::TieBreakLoser(node_cert) => {
                Some(NodeAction::TieBreakLoser(self.id(node_cert)))
            }
        }
    }

    fn map_state(&mut self, system: &Self::System) -> Option<PeerState> {
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
                            super::round_model::map_state(v.clone()).unwrap(),
                        )
                    })
                    .collect::<BTreeMap<NodeId, RoundPhase>>()
                    .into();

                let initiate_tgt = s.initiate_tgt.as_ref().map(|t| Tgt {
                    cert: self.ids.lookup(t.cert.clone()).unwrap(),
                    tie_break: t.tie_break > (u32::MAX / 2),
                });
                Ok(PeerState {
                    rounds,
                    initiate_tgt,
                })
            })
            .unwrap();
        Some(state)
    }

    fn gen_event(&mut self, generator: &mut impl Generator, event: NodeAction) -> Self::Event {
        unimplemented!("generation not implemented")
    }

    fn gen_state(&mut self, generator: &mut impl Generator, state: PeerState) -> Self::System {
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

        let machine = PeerMachine::new(GossipType::Recent);
        let state = PeerState::new();

        println!("{}", to_dot(state_diagram(machine, state, &config)));
    }
}
