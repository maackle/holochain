use super::OpEvent;

use holo_hash::{DhtOpHash, HashableContentExtSync};
use holochain_model::{
    self,
    op_family::OpFamilyAction,
    op_network::{OpNetworkAction, OpNetworkMachine},
    op_single::OpAction,
};
use kitsune_p2p::NodeCert;
use parking_lot::Mutex;
use polestar::id::IdMap;

static OP_EVENT_MAPPING: once_cell::sync::Lazy<Mutex<OpEventMapping>> =
    once_cell::sync::Lazy::new(|| Mutex::new(OpEventMapping::default()));

pub type OpId = usize;
pub type NodeId = u8;

#[derive(Default)]
pub struct OpEventMapping {
    node_ids: IdMap<NodeId, NodeCert>,
    op_ids: IdMap<OpId, DhtOpHash>,
}

type ModelAction = <OpNetworkMachine<u8, usize> as polestar::Machine>::Action;

impl OpEventMapping {
    pub fn node_id(&mut self, cert: NodeCert) -> NodeId {
        self.node_ids.lookup(cert).unwrap()
    }

    pub fn op_id(&mut self, hash: DhtOpHash) -> OpId {
        self.op_ids.lookup(hash).unwrap()
    }

    pub fn map_event(&mut self, (node, event): (NodeCert, OpEvent)) -> Option<ModelAction> {
        let action = match event {
            OpEvent::Authored { op } => OpNetworkAction::Family {
                target: self.op_id(op.to_hash()),
                action: OpAction::Store.into(),
            },
            OpEvent::Fetched { op } => OpNetworkAction::Receive {
                op: self.op_id(op.to_hash()),
                from: self.node_id(todo!("get cert from fetch pool?")),
                valid: true,
            },
            OpEvent::AwaitingDeps { op, dep, kind } => OpNetworkAction::Family {
                target: self.op_id(op),
                action: OpFamilyAction::Await(map_vt(kind), self.op_id(dep)),
            },
            OpEvent::Validated { op, kind } => OpNetworkAction::Family {
                target: self.op_id(op),
                action: OpAction::Validate(map_vt(kind)).into(),
            },
            OpEvent::Integrated { op } => OpNetworkAction::Family {
                target: self.op_id(op),
                action: OpAction::Integrate.into(),
            },
            OpEvent::ReceivedValidationReceipt { receipt: _ } => return None,
        };

        Some((self.node_id(node), action))
    }
}

fn map_vt(
    v: holochain_types::prelude::ValidationType,
) -> holochain_model::op_single::ValidationType {
    match v {
        holochain_types::prelude::ValidationType::Sys => {
            holochain_model::op_single::ValidationType::Sys
        }
        holochain_types::prelude::ValidationType::App => {
            holochain_model::op_single::ValidationType::App
        }
    }
}
