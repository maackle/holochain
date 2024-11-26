#![allow(missing_docs)]

use std::{collections::HashMap, io::Write};

use crate::model::OpEvent;
use holo_hash::{
    ActionHash, AnyDhtHash, AnyDhtHashPrimitive, DhtOpHash, EntryHash, HashableContentExtSync,
};
use holochain_model::{
    op_family::OpFamilyAction,
    op_network::{OpNetworkAction, OpNetworkMachine},
    op_single::OpAction,
};
use parking_lot::Mutex;
use polestar::id::IdMap;

static OP_EVENT_WRITER: once_cell::sync::Lazy<Mutex<OpEventWriter>> =
    once_cell::sync::Lazy::new(|| Mutex::new(OpEventWriter::new("/tmp/op_events.json")));

pub fn write_op_event(tag: &str, event: OpEvent) {
    OP_EVENT_WRITER.lock().write((tag.to_string(), event));
}

pub type OpId = usize;
pub type NodeId = u8;
pub type NodeTag = String;
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
        let action = self.mapping.map_event(event.clone());
        if let Some(action) = action {
            let mut json = serde_json::to_string(&action).unwrap();
            json.push('\n');
            self.file.write_all(json.as_bytes()).unwrap();
        } else {
            tracing::warn!("no action for event: {event:?}");
        }
    }
}

#[derive(Default, Debug)]
pub struct OpEventMapping {
    node_ids: IdMap<NodeId, NodeTag>,
    action_ids: IdMap<OpId, ActionHash>,
    op_to_action: HashMap<DhtOpHash, ActionHash>,
    entry_to_action: HashMap<EntryHash, ActionHash>,
    sent_ops: HashMap<DhtOpHash, NodeTag>,
}

type SystemEvent = (NodeTag, OpEvent);
type ModelAction = <OpNetworkMachine<u8, usize, u8> as polestar::Machine>::Action;

impl OpEventMapping {
    pub fn node_id(&mut self, tag: NodeTag) -> NodeId {
        self.node_ids.lookup(tag).unwrap()
    }

    pub fn op_id(&mut self, hash: &DhtOpHash) -> OpId {
        let action_hash = self
            .op_to_action
            .get(hash)
            .unwrap_or_else(|| panic!("op_id miss {hash} state: {:#?}", self))
            .clone();
        self.action_id(&action_hash)
    }

    pub fn entry_id(&mut self, hash: &EntryHash) -> OpId {
        let action_hash = self
            .entry_to_action
            .get(hash)
            .unwrap_or_else(|| panic!("entry_id miss {hash} state: {:#?}", self))
            .clone();
        self.action_id(&action_hash)
    }

    pub fn action_id(&mut self, hash: &ActionHash) -> OpId {
        self.action_ids
            .lookup(hash.clone())
            .unwrap_or_else(|e| panic!("{e} {hash} state: {:#?}", self))
    }

    pub fn anydht_id(&mut self, hash: &AnyDhtHash) -> OpId {
        match hash.clone().into_primitive() {
            AnyDhtHashPrimitive::Action(action_hash) => self.action_id(&action_hash),
            AnyDhtHashPrimitive::Entry(entry_hash) => self.entry_id(&entry_hash),
        }
    }

    pub fn map_event(&mut self, (node, event): (NodeTag, OpEvent)) -> Option<ModelAction> {
        dbg!((&node, &event));

        let action = match event {
            OpEvent::Authored { op, action, entry } => {
                let action_hash = action.clone();
                self.op_to_action.insert(op, action_hash.clone());
                if let Some(entry_hash) = entry {
                    self.entry_to_action
                        .insert(entry_hash.clone(), action_hash.clone());
                }

                OpNetworkAction::Local {
                    op: self.action_id(&action_hash),
                    action: OpAction::Store(true).into(),
                }
            }
            OpEvent::Sent { op } => {
                self.sent_ops.insert(op, node);
                return None;
            }
            OpEvent::Fetched { op } => {
                let op_hash = op.to_hash();
                let from = self.sent_ops.get(&op_hash).cloned().unwrap();
                OpNetworkAction::Receive {
                    op: self.op_id(&op_hash),
                    from: self.node_id(from),
                    valid: true,
                }
            }
            OpEvent::AwaitingDeps { op, dep, kind } => OpNetworkAction::Local {
                op: self.op_id(&op),
                action: OpFamilyAction::Await(map_vt(kind), self.anydht_id(&dep)),
            },
            OpEvent::Validated { op, kind } => OpNetworkAction::Local {
                op: self.op_id(&op),
                action: OpAction::Validate(map_vt(kind)).into(),
            },
            OpEvent::Rejected { op } => OpNetworkAction::Local {
                op: self.op_id(&op),
                action: OpAction::Reject.into(),
            },
            OpEvent::Integrated { op } => OpNetworkAction::Local {
                op: self.op_id(&op),
                action: OpAction::Integrate.into(),
            },
            OpEvent::ReceivedValidationReceipt { receipt: _ } => return None,
        };

        Some((self.node_id(node), action))
    }
}

fn map_vt(v: crate::prelude::ValidationType) -> holochain_model::op_single::ValidationType {
    match v {
        crate::prelude::ValidationType::Sys => holochain_model::op_single::ValidationType::Sys,
        crate::prelude::ValidationType::App => holochain_model::op_single::ValidationType::App,
    }
}
