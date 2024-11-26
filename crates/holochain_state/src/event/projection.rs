use std::{collections::HashMap, io::Write};

use super::{op_event::NodeTag, OpEvent};

use holo_hash::{
    ActionHash, AnyDhtHash, AnyDhtHashPrimitive, DhtOpHash, EntryHash, HashableContentExtSync,
};
use holochain_model::{
    self,
    op_family::OpFamilyAction,
    op_network::{OpNetworkAction, OpNetworkMachine},
    op_single::OpAction,
};
use parking_lot::Mutex;
use polestar::id::IdMap;

static OP_EVENT_WRITER: once_cell::sync::Lazy<Mutex<OpEventWriter>> =
    once_cell::sync::Lazy::new(|| Mutex::new(OpEventWriter::new("op_events.json")));

pub fn write_op_event(tag: &str, event: OpEvent) {
    OP_EVENT_WRITER.lock().write((tag.to_string(), event));
}

pub type OpId = usize;
pub type NodeId = u8;

pub struct OpEventWriter {
    mapping: OpEventMapping,
    file: std::fs::File,
}

impl OpEventWriter {
    pub fn new(location: impl AsRef<std::path::Path>) -> Self {
        let file = std::fs::File::create(location).unwrap();
        Self {
            mapping: OpEventMapping::default(),
            file,
        }
    }

    pub fn write(&mut self, event: SystemEvent) {
        let action = self.mapping.map_event(event);
        if let Some(action) = action {
            let json = serde_json::to_string(&action).unwrap();
            self.file.write_all(json.as_bytes()).unwrap();
        }
    }
}

#[derive(Default)]
pub struct OpEventMapping {
    node_ids: IdMap<NodeId, NodeTag>,
    op_ids: IdMap<OpId, ActionHash>,
    op_to_action: HashMap<DhtOpHash, ActionHash>,
    entry_to_action: HashMap<EntryHash, ActionHash>,
}

type SystemEvent = (NodeTag, OpEvent);
type ModelAction = <OpNetworkMachine<u8, usize> as polestar::Machine>::Action;

impl OpEventMapping {
    pub fn node_id(&mut self, tag: NodeTag) -> NodeId {
        self.node_ids.lookup(tag).unwrap()
    }

    pub fn op_id(&mut self, hash: &DhtOpHash) -> OpId {
        let action_hash = self.op_to_action.get(hash).unwrap().clone();
        self.action_id(&action_hash)
    }

    pub fn entry_id(&mut self, hash: &EntryHash) -> OpId {
        let action_hash = self.entry_to_action.get(hash).unwrap().clone();
        self.action_id(&action_hash)
    }

    pub fn action_id(&mut self, hash: &ActionHash) -> OpId {
        self.op_ids.lookup(hash.clone()).unwrap()
    }

    pub fn anydht_id(&mut self, hash: &AnyDhtHash) -> OpId {
        match hash.clone().into_primitive() {
            AnyDhtHashPrimitive::Action(action_hash) => self.action_id(&action_hash),
            AnyDhtHashPrimitive::Entry(entry_hash) => self.entry_id(&entry_hash),
        }
    }

    pub fn map_event(&mut self, (node, event): (NodeTag, OpEvent)) -> Option<ModelAction> {
        let action = match event {
            OpEvent::Authored { op, action, entry } => {
                let action_hash = action.clone();
                self.op_to_action.insert(op.to_hash(), action_hash.clone());
                if let Some(entry_hash) = entry {
                    self.entry_to_action
                        .insert(entry_hash.clone(), action_hash.clone());
                }

                OpNetworkAction::Family {
                    target: self.action_id(&action_hash),
                    action: OpAction::Store.into(),
                }
            }
            OpEvent::Fetched { op, from } => OpNetworkAction::Receive {
                op: self.op_id(&op.to_hash()),
                from: self.node_id(from.expect("OpEvent::Fetched must specify `from` tag")),
                valid: true,
            },
            OpEvent::AwaitingDeps { op, dep, kind } => OpNetworkAction::Family {
                target: self.op_id(&op),
                action: OpFamilyAction::Await(map_vt(kind), self.anydht_id(&dep)),
            },
            OpEvent::Validated { op, kind } => OpNetworkAction::Family {
                target: self.op_id(&op),
                action: OpAction::Validate(map_vt(kind)).into(),
            },
            OpEvent::Integrated { op } => OpNetworkAction::Family {
                target: self.op_id(&op),
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
