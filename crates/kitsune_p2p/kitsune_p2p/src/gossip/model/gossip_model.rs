use std::collections::{BTreeMap, VecDeque};

use kitsune_p2p_types::GossipType;
use polestar::prelude::*;
use proptest_derive::Arbitrary;

use crate::NodeCert;

use super::peer_model::{NodeAction, PeerModel};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub struct GossipModel {
    pub nodes: BTreeMap<NodeCert, PeerModel>,
    pub inflight: VecDeque<(NodeCert, NodeAction)>,
}

impl GossipModel {
    pub fn new(gossip_type: GossipType, certs: &[NodeCert]) -> Self {
        Self {
            nodes: certs
                .iter()
                .map(|cert| (cert.clone(), PeerModel::new(gossip_type)))
                .collect(),
            inflight: VecDeque::default(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, derive_more::From)]
pub enum GossipAction {
    #[from]
    NodeAction(NodeCert, NodeAction),
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
                let fx = self.nodes.transition_mut(node.clone(), action).unwrap()?;
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

    use kitsune_p2p_types::GossipType;

    use super::*;

    #[test]
    fn test_gossip() {
        let certs = vec![
            NodeCert::from(Arc::new([1u8; 32])),
            NodeCert::from(Arc::new([2u8; 32])),
            NodeCert::from(Arc::new([3u8; 32])),
        ];
        let mut model = GossipModel::new(GossipType::Recent, &certs);
        model = model
            .transition(GossipAction::NodeAction(
                certs[0].clone(),
                NodeAction::SetInitiate(certs[1].clone()),
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
