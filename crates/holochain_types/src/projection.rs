#![allow(missing_docs)]

use std::{
    collections::{HashMap, HashSet},
    io::Write,
};

use crate::{dht_op::ChainOpType, model::OpEvent};
use holo_hash::{
    ActionHash, AnyDhtHash, AnyDhtHashPrimitive, DhtOpHash, DnaHash, EntryHash,
    HashableContentExtSync,
};
use holochain_model::{
    op_family::{OpFamilyAction, OpId},
    op_network::{OpNetworkAction, OpNetworkMachine, OpSendTarget},
    op_single::OpAction,
};
use parking_lot::Mutex;
use polestar::id::IdMap;

pub static OP_EVENT_WRITER: once_cell::sync::Lazy<Mutex<OpEventWriter>> =
    once_cell::sync::Lazy::new(|| Mutex::new(OpEventWriter::new("/tmp/op-events.json")));

pub fn polestar_write_op_event(tag: &str, event: OpEvent) {
    OP_EVENT_WRITER.lock().write_event((tag.to_string(), event));
}

pub fn polestar_set_dna(dna_hash: &DnaHash) {
    OP_EVENT_WRITER.lock().set_dna_hash(dna_hash.clone());
}

#[macro_export]
macro_rules! polestar_comment {
    ($what:expr) => {{
        let mut writer = $crate::projection::OP_EVENT_WRITER.lock();
        writer.write_line_raw(&format!("// [{}:{}] {}", file!(), line!(), $what));
    }};
}

pub type DepId = usize;
pub type TypeId = u8;
pub type OpIdT = OpId<DepId, TypeId>;
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

    pub fn set_dna_hash(&mut self, dna_hash: DnaHash) {
        self.mapping.dna_hash = Some(dna_hash);
    }

    pub fn write_line_raw(&mut self, what: &str) {
        self.file.write_all(what.as_bytes()).unwrap();
        self.file.write_all("\n".as_bytes()).unwrap();
    }

    pub fn write_event(&mut self, event: SystemEvent) {
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

#[derive(Debug, Default)]
pub struct OpEventMapping {
    dna_hash: Option<DnaHash>,

    node_ids: IdMap<NodeTag, NodeId>,
    action_ids: IdMap<ActionHash, DepId>,
    op_type_ids: IdMap<ChainOpType, TypeId>,

    op_ids: HashMap<DhtOpHash, OpIdT>,

    // Tracks hashes from other DNAs
    other_ops: HashSet<DhtOpHash>,
    other_actions: HashSet<ActionHash>,
    other_entries: HashSet<EntryHash>,

    entry_to_action: HashMap<EntryHash, ActionHash>,
    sent_ops: HashMap<(DhtOpHash, OpSendTarget), NodeTag>,
}

// impl OpEventMapping {
//     pub fn new(dna_hash: DnaHash) -> Self {
//         Self {
//             dna_hash,
//             node_ids: IdMap::new(),
//             action_ids: IdMap::new(),
//             op_type_ids: IdMap::new(),
//             op_ids: HashMap::new(),
//             entry_to_action: HashMap::new(),
//             sent_ops: HashMap::new(),
//         }
//     }
// }

type SystemEvent = (NodeTag, OpEvent);
type ModelAction = <OpNetworkMachine<u8, usize, u8> as polestar::Machine>::Action;

impl OpEventMapping {
    pub fn node_id(&mut self, tag: NodeTag) -> NodeId {
        self.node_ids.lookup(tag).unwrap()
    }

    pub fn op_id(&mut self, hash: &DhtOpHash) -> Option<OpIdT> {
        self.op_ids
            .get(hash)
            .or_else(|| {
                if self.other_ops.contains(hash) {
                    tracing::warn!("op_id miss {hash} state: {:#?}", self);
                    None
                } else {
                    panic!("op_id miss {hash} state: {:#?}", self)
                }
            })
            .cloned()
    }

    pub fn entry_dep_id(&mut self, hash: &EntryHash) -> Option<DepId> {
        let action_hash = self
            .entry_to_action
            .get(hash)
            .or_else(|| {
                if self.other_entries.contains(hash) {
                    tracing::warn!("entry_id miss {hash} state: {:#?}", self);
                    None
                } else {
                    panic!("entry_id miss {hash} state: {:#?}", self)
                }
            })
            .cloned()?;
        self.action_dep_id(&action_hash)
    }

    pub fn action_dep_id(&mut self, hash: &ActionHash) -> Option<DepId> {
        self.action_ids.lookup(hash.clone()).ok().or_else(|| {
            if self.other_actions.contains(hash) {
                tracing::warn!("action_id miss {hash} state: {:#?}", self);
                None
            } else {
                panic!("action_id miss {hash} state: {:#?}", self)
            }
        })
    }

    pub fn anydht_dep_id(&mut self, hash: &AnyDhtHash) -> Option<DepId> {
        match hash.clone().into_primitive() {
            AnyDhtHashPrimitive::Action(action_hash) => self.action_dep_id(&action_hash),
            AnyDhtHashPrimitive::Entry(entry_hash) => self.entry_dep_id(&entry_hash),
        }
    }

    pub fn map_event(&mut self, (node, event): (NodeTag, OpEvent)) -> Option<ModelAction> {
        let action = match event {
            OpEvent::Authored {
                op,
                op_type,
                action,
                entry,
                dna_hash,
            } => {
                if Some(&dna_hash) != self.dna_hash.as_ref() {
                    self.other_ops.insert(op);
                    self.other_actions.insert(action);
                    if let Some(entry_hash) = entry {
                        self.other_entries.insert(entry_hash);
                    }
                    tracing::warn!("skipping Authored event for unregistered DNA: {dna_hash}");
                    return None;
                }
                let action_hash = action.clone();
                let dep_id = self.action_ids.lookup(action_hash.clone()).unwrap();
                let type_id = self.op_type_ids.lookup(op_type).unwrap();
                let op_id = OpId(dep_id, type_id);
                self.op_ids.insert(op, op_id);
                if let Some(entry_hash) = entry {
                    self.entry_to_action
                        .insert(entry_hash.clone(), action_hash.clone());
                }

                OpNetworkAction::Local {
                    op: op_id,
                    action: OpAction::Store(true).into(),
                }
            }
            OpEvent::Sent { op, target } => {
                self.sent_ops.insert((op, target), node);
                return None;
            }
            OpEvent::Fetched { op, target } => {
                let op_hash = op.to_hash();
                let from = self
                    .sent_ops
                    .get(&(op_hash.clone(), target))
                    .cloned()
                    .unwrap();

                OpNetworkAction::Receive {
                    op: self.op_id(&op_hash)?,
                    from: self.node_id(from),
                    valid: true,
                    target,
                }
            }
            OpEvent::AwaitingDeps { op, dep, kind } => OpNetworkAction::Local {
                op: self.op_id(&op)?,
                action: OpFamilyAction::Await(map_vt(kind), self.anydht_dep_id(&dep)?),
            },
            OpEvent::Validated { op, kind } => OpNetworkAction::Local {
                op: self.op_id(&op)?,
                action: OpAction::Validate(map_vt(kind)).into(),
            },
            OpEvent::Rejected { op } => OpNetworkAction::Local {
                op: self.op_id(&op)?,
                action: OpAction::Reject.into(),
            },
            OpEvent::Integrated { op } => OpNetworkAction::Local {
                op: self.op_id(&op)?,
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
