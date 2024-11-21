use std::collections::{BTreeMap, VecDeque};

use anyhow::anyhow;
use exhaustive::Exhaustive;
use kitsune_p2p_types::GossipType;
use polestar::prelude::*;
use proptest_derive::Arbitrary;

use super::{
    peer_model::{NodeAction, PeerModel},
    NodeId,
};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub struct GossipModel {
    pub nodes: BTreeMap<NodeId, PeerModel>,
    pub inflight: VecDeque<(NodeId, NodeAction)>,
}

impl GossipModel {
    pub fn new(gossip_type: GossipType, ids: &[NodeId]) -> Self {
        Self {
            nodes: ids
                .iter()
                .map(|id| (id.clone(), PeerModel::new(gossip_type)))
                .collect(),
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

impl Machine for GossipModel {
    type Action = GossipAction;
    /// Whether the model has changed
    type Fx = bool;
    type Error = anyhow::Error;

    fn transition(mut self, action: Self::Action) -> MachineResult<Self> {
        let fx = match action {
            GossipAction::NodeAction(node, action) => {
                let fx = self
                    .nodes
                    .transition_mut(node.clone(), action)
                    .ok_or(anyhow!("no node {node}"))??;
                self.inflight.extend(fx.into_iter().map(|(to, msg)| {
                    (
                        to.clone(),
                        NodeAction::Incoming {
                            from: node.clone(),
                            msg,
                        },
                    )
                }));
                true
            }
            GossipAction::Transmit => {
                if let Some(msg) = self.inflight.pop_front() {
                    let (next, fx) = self.transition(msg.into())?;
                    self = next;
                    fx
                } else {
                    false
                }
            }
        };
        Ok((self, fx))
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
        let mut model = GossipModel::new(GossipType::Recent, &ids);
        model = model
            .transition(GossipAction::NodeAction(
                ids[0].clone(),
                NodeAction::SetInitiate(ids[1].clone(), false),
            ))
            .unwrap()
            .0;

        loop {
            let (next, changed) = model.transition(GossipAction::Transmit).unwrap();
            model = next;
            dbg!(&model);
            if !changed {
                break;
            }
        }
    }
}
