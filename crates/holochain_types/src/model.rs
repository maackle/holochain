#![allow(missing_docs)]

pub use holochain_model::op_network::OpSendTarget;

use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OpEvent {
    /// The node has authored this op, including validation and integration
    Authored {
        op: DhtOpHash,
        op_type: ChainOpType,
        action: ActionHash,
        entry: Option<EntryHash>,
        dna_hash: DnaHash,
    },

    /// This is a hack, it lets the projector register that the op was sent from
    /// a node so that it can match up on the receive side to know who sent it,
    /// because kitsune doesn't know who we got an op from.
    Sent { op: DhtOpHash, target: OpSendTarget },

    /// The node has fetched this op from another node via the FetchPool
    /// The Option is because Holochain does not currently store the origin of
    /// an op in the database, but once it does, this can be non-optional.
    Fetched { op: DhtOp, target: OpSendTarget },

    /// The node has validated an op authored by someone else
    Validated { op: DhtOpHash, kind: ValidationType },

    /// The node has rejected an op
    Rejected { op: DhtOpHash },

    /// The node is awaiting a dependency before being able to validate
    AwaitingDeps {
        op: DhtOpHash,
        dep: AnyDhtHash,
        kind: ValidationType,
    },

    /// The node has integrated an op authored by someone else
    Integrated { op: DhtOpHash },

    /// The node has received a validation receipt from another
    /// agent for op it authored
    ReceivedValidationReceipt { receipt: SignedValidationReceipt },
}

impl OpEvent {
    pub fn authored(op: ChainOpLite, op_hash: DhtOpHash, dna_hash: DnaHash) -> Self {
        let action_hash = op.action_hash().clone();
        let entry_hash = op.entry_hash().cloned();
        Self::Authored {
            op: op_hash,
            op_type: op.get_type(),
            action: action_hash,
            entry: entry_hash,
            dna_hash,
        }
    }
}
