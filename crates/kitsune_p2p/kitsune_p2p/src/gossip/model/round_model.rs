use std::sync::Arc;

use crate::{
    dependencies::kitsune_p2p_types::{GossipType, KitsuneResult},
    gossip::sharded_gossip::{store::AgentInfoSession, ShardedGossipWire},
};
use anyhow::bail;
use polestar::{dfa::Contextual, prelude::*};
use proptest_derive::Arbitrary;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary)]
pub enum RoundPhase {
    /// bool is true if we initiated
    Started(bool),

    AgentDiffReceived,
    AgentsReceived,

    OpDiffReceived,
    Finished,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary)]
pub struct RoundState {
    pub phase: RoundPhase,
    pub no_diff: bool,
}

impl RoundState {
    pub fn new(phase: RoundPhase) -> Self {
        Self {
            phase,
            no_diff: false,
        }
    }
}

#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Arbitrary, exhaustive::Exhaustive, derive_more::From,
)]
pub enum RoundAction {
    /// A message from another peer
    Msg(RoundMsg),
    /// Special model-specific action indicating that no diff was found,
    /// letting us bump to the next phase.
    /// (this is kind of like an epsilon transition, in DFA terms.)
    NoDiff,
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
                let (phase, fx) = match (*ctx, msg, self.phase, self.no_diff) {
                    (T::Recent, M::AgentDiff, P::Started(_), false) => {
                        (P::AgentDiffReceived, vec![M::Agents])
                    }
                    (T::Recent, M::AgentDiff, P::Started(_), true) => {
                        (P::AgentsReceived, vec![M::OpDiff])
                    }

                    (T::Recent, M::Agents, P::AgentDiffReceived, false) => {
                        (P::AgentsReceived, vec![M::OpDiff])
                    }

                    (T::Recent, M::OpDiff, P::AgentsReceived, false) => {
                        (P::OpDiffReceived, vec![M::Ops])
                    }
                    (T::Recent, M::OpDiff, P::AgentsReceived, true) => (P::Finished, vec![]),

                    (T::Historical, M::OpDiff, P::Started(_), false) => {
                        (P::OpDiffReceived, vec![M::Ops])
                    }
                    (T::Historical, M::OpDiff, P::Started(_), true) => (P::Finished, vec![]),

                    (_, M::Ops, P::OpDiffReceived, false) => (P::Finished, vec![]),
                    (_, _, P::Finished, _) => bail!("terminal"),

                    // This might not be right
                    (_, M::Close, _, _) => (P::Finished, vec![]),

                    tup => bail!("invalid transition: {tup:?}"),
                };
                (
                    Self {
                        phase,
                        no_diff: false,
                    },
                    fx,
                )
            }
            RoundAction::NoDiff => {
                if self.no_diff {
                    bail!("no_diff already set");
                } else {
                    self.no_diff = true;
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

    print_dot_state_diagram(RoundPhase::Initiated.context(GossipType::Recent), &config);

    print_dot_state_diagram(
        RoundPhase::Initiated.context(GossipType::Historical),
        &config,
    );
}
