use std::collections::{BTreeMap, VecDeque};

use anyhow::anyhow;
use exhaustive::Exhaustive;
use kitsune_p2p_types::GossipType;
use polestar::{ext::MapExt, prelude::*};
use proptest_derive::Arbitrary;

use super::{
    peer_model::{NodeAction, PeerMachine, PeerState},
    NodeId,
};

#[derive(derive_more::Deref)]
pub struct GossipMachine(pub PeerMachine);

impl GossipMachine {
    pub fn new(gossip_type: GossipType) -> Self {
        Self(PeerMachine::new(gossip_type))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub struct GossipState {
    pub nodes: BTreeMap<NodeId, PeerState>,
    pub inflight: VecDeque<(NodeId, NodeAction)>,
}

impl GossipState {
    pub fn new(ids: &[NodeId]) -> Self {
        Self {
            nodes: ids.iter().map(|id| (*id, PeerState::new())).collect(),
            inflight: VecDeque::default(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, Exhaustive, derive_more::From)]
pub enum GossipAction {
    #[from]
    NodeAction(NodeId, NodeAction),
    Transmit,
}

impl Machine for GossipMachine {
    type State = GossipState;
    type Action = GossipAction;
    /// Whether the model has changed
    type Fx = bool;
    type Error = anyhow::Error;

    fn is_terminal(&self, _: &Self::State) -> bool {
        false
    }

    fn transition(&self, mut state: GossipState, action: Self::Action) -> TransitionResult<Self> {
        let changed = match action {
            GossipAction::NodeAction(node, action) => {
                let fx = state
                    .nodes
                    .owned_update(node, |_, s| self.0.transition(s, action))?;
                state.inflight.extend(
                    fx.into_iter()
                        .map(|(to, msg)| (to.clone(), NodeAction::Incoming { from: node, msg })),
                );
                true
            }
            GossipAction::Transmit => {
                if let Some(msg) = state.inflight.pop_front() {
                    let (next, fx) = self.transition(state, msg.into())?;
                    state = next;
                    fx
                } else {
                    false
                }
            }
        };
        Ok((state, changed))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use exhaustive::Exhaustive;
    use itertools::Itertools;
    use kitsune_p2p_types::GossipType;

    use super::*;

    #[test]
    fn test_gossip() {
        let ids = NodeId::iter_exhaustive(None).collect_vec();
        let machine = GossipMachine::new(GossipType::Recent);
        let mut state = GossipState::new(&ids);
        state = machine
            .transition(
                state,
                GossipAction::NodeAction(
                    ids[0].clone(),
                    NodeAction::SetInitiate(ids[1].clone(), false),
                ),
            )
            .unwrap()
            .0;

        loop {
            let (next, changed) = machine.transition(state, GossipAction::Transmit).unwrap();
            state = next;
            dbg!(&state);
            if !changed {
                break;
            }
        }
    }
}
