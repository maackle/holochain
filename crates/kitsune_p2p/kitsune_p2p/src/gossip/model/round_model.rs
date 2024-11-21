use std::{ops::Deref, sync::Arc};

use crate::{
    dependencies::kitsune_p2p_types::{GossipType, KitsuneResult},
    gossip::sharded_gossip::{store::AgentInfoSession, ShardedGossipWire},
};
use anyhow::bail;
use polestar::{dfa::Contextual, prelude::*};
use proptest_derive::Arbitrary;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary)]
pub enum RoundPhase {
    /// bool is true if we "must send"
    Started(bool),

    AgentDiffReceived,
    /// bool is true if we "must send"
    AgentsReceived(bool),

    OpDiffReceived,
    Finished,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum RoundPhaseCombined {
    /// bool is true if we "must send"
    Started,

    AgentDiffReceived,
    /// bool is true if we "must send"
    AgentsReceived,

    OpDiffReceived,
    Finished,
}

impl RoundPhase {
    fn combined(&self) -> RoundPhaseCombined {
        match self {
            RoundPhase::Started(_) => RoundPhaseCombined::Started,
            RoundPhase::AgentDiffReceived => RoundPhaseCombined::AgentDiffReceived,
            RoundPhase::AgentsReceived(_) => RoundPhaseCombined::AgentsReceived,
            RoundPhase::OpDiffReceived => RoundPhaseCombined::OpDiffReceived,
            RoundPhase::Finished => RoundPhaseCombined::Finished,
        }
    }
}

pub type RoundState = RoundPhase;
// #[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary)]
// pub struct RoundState {
//     pub phase: RoundPhase,
// }

// impl RoundState {
//     pub fn new(phase: RoundPhase) -> Self {
//         Self { phase }
//     }
// }

#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Arbitrary, exhaustive::Exhaustive, derive_more::From,
)]
pub enum RoundAction {
    /// A message from another peer
    Msg(RoundMsg),
    /// Special model-specific action indicating that no diff was found,
    /// letting us bump to the next phase.
    /// (this is kind of like an epsilon transition, in DFA terms.)
    MustSend,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, exhaustive::Exhaustive)]
#[must_use]
pub enum RoundMsg {
    Initiate,
    Accept,

    AgentDiff,
    Agents,

    OpDiff,
    Ops,

    Close,
}

pub type RoundContext = GossipType;

impl Machine for RoundState {
    type Action = (RoundAction, Arc<RoundContext>);
    type Fx = Vec<RoundMsg>;
    type Error = anyhow::Error;

    fn transition(mut self, (action, ctx): Self::Action) -> MachineResult<Self> {
        use GossipType as T;
        use RoundMsg as M;
        use RoundPhase as P;

        Ok(match action {
            RoundAction::Msg(msg) => {
                let (phase, fx) = match (*ctx, msg, self) {
                    (T::Recent, M::AgentDiff, P::Started(true)) => {
                        (P::AgentDiffReceived, vec![M::Agents])
                    }
                    (T::Recent, M::AgentDiff, P::Started(false)) => {
                        (P::AgentsReceived(false), vec![M::OpDiff])
                    }

                    (T::Recent, M::Agents, P::AgentDiffReceived) => {
                        (P::AgentsReceived(false), vec![M::OpDiff])
                    }

                    // OpDiff can be received in Started when agents already match
                    (T::Recent, M::OpDiff, P::Started(true) | P::AgentsReceived(true)) => {
                        (P::OpDiffReceived, vec![M::Ops])
                    }
                    (T::Recent, M::OpDiff, P::AgentsReceived(false)) => (P::Finished, vec![]),

                    // TODO: this was Started(true), but it may be the case that in historical gossip we always send ops?
                    (T::Historical, M::OpDiff, P::Started(_)) => (P::OpDiffReceived, vec![M::Ops]),
                    // (T::Historical, M::OpDiff, P::Started(false)) => (P::Finished, vec![]),
                    (T::Recent, M::Ops, P::OpDiffReceived) => (P::Finished, vec![]),
                    (T::Historical, M::Ops, P::OpDiffReceived) => (P::Finished, vec![]),

                    (_, _, P::Finished) => bail!("terminal"),

                    // This might not be right
                    (_, M::Close, _) => (P::Finished, vec![]),

                    tup => bail!("invalid transition: {tup:?}"),
                };
                (phase, fx)
            }

            RoundAction::MustSend => {
                match (*ctx, &mut self) {
                    (
                        T::Recent,
                        P::Started(ref mut must_send) | P::AgentsReceived(ref mut must_send),
                    )
                    | (T::Historical, P::Started(ref mut must_send)) => {
                        if *must_send {
                            bail!("must_send already set")
                        } else {
                            *must_send = true
                        }
                    }

                    tup => bail!("invalid transition: {tup:?}"),
                }
                (self, vec![])
            }
        })
    }
}

pub type RoundFsm = Contextual<RoundState, RoundContext>;

pub fn map_event(msg: ShardedGossipWire) -> Option<RoundMsg> {
    match msg {
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
}

pub fn map_state(state: crate::gossip::sharded_gossip::RoundState) -> Option<RoundState> {
    todo!()
}

#[test]
#[ignore = "diagram"]
fn diagram_round_state() {
    use polestar::diagram::exhaustive::*;

    tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::new()).unwrap();

    let config = DiagramConfig {
        max_actions: None,
        max_distance: Some(10),
        max_iters: None,
        ignore_loopbacks: false,
    };

    print_dot_state_diagram_mapped(
        (RoundPhase::Started(false)).context(GossipType::Recent),
        &config,
        |m| m.combined(),
    );

    print_dot_state_diagram_mapped(
        (RoundPhase::Started(false)).context(GossipType::Historical),
        &config,
        |m| m.combined(),
    );
}
