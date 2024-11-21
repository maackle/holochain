use std::sync::Arc;

use crate::{
    dependencies::kitsune_p2p_types::{GossipType, KitsuneResult},
    gossip::sharded_gossip::{store::AgentInfoSession, RoundState, ShardedGossipWire},
};
use anyhow::bail;
use polestar::{fsm::Contextual, prelude::*};
use proptest_derive::Arbitrary;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary)]
pub enum RoundPhase {
    Initiated,
    Accepted,

    AgentDiffReceived,
    AgentsReceived,
    OpDiffReceived,

    Finished,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary, exhaustive::Exhaustive)]
#[must_use]
pub enum RoundAction {
    Initiate,
    Accept,
    AgentDiff,
    Agents,
    OpDiff,
    Ops,
    Close,
}

pub type RoundContext = GossipType;

impl Machine for RoundPhase {
    type Action = (RoundAction, Arc<RoundContext>);
    type Fx = Vec<RoundAction>;
    type Error = anyhow::Error;

    fn transition(mut self, (event, ctx): Self::Action) -> MachineResult<Self> {
        use GossipType as T;
        use RoundAction as E;
        use RoundPhase as P;

        Ok(match (*ctx, self, event) {
            (T::Recent, P::Initiated | P::Accepted, E::AgentDiff) => {
                (P::AgentDiffReceived, vec![E::Agents])
            }
            (T::Recent, P::AgentDiffReceived, E::Agents) => (P::AgentsReceived, vec![E::OpDiff]),

            (T::Historical, P::Initiated | P::Accepted, E::OpDiff) => {
                (P::OpDiffReceived, vec![E::Ops])
            }
            (T::Recent, P::AgentsReceived, E::OpDiff) => (P::OpDiffReceived, vec![E::Ops]),
            (_, P::OpDiffReceived, E::Ops) => (P::Finished, vec![]),
            (_, P::Finished, _) => bail!("terminal"),

            // This might not be right
            (_, _, E::Close) => (P::Finished, vec![]),

            tup => bail!("invalid transition: {tup:?}"),
        })
    }
}

pub type RoundFsm = Contextual<RoundPhase, RoundContext>;

pub fn map_event(msg: ShardedGossipWire) -> Option<RoundAction> {
    match msg {
        ShardedGossipWire::Initiate(initiate) => Some(RoundAction::Initiate),
        ShardedGossipWire::Accept(accept) => Some(RoundAction::Accept),
        ShardedGossipWire::Agents(agents) => Some(RoundAction::AgentDiff),
        ShardedGossipWire::MissingAgents(missing_agents) => Some(RoundAction::Agents),
        ShardedGossipWire::OpBloom(op_bloom) => Some(RoundAction::OpDiff),
        ShardedGossipWire::OpRegions(op_regions) => Some(RoundAction::OpDiff),
        ShardedGossipWire::MissingOpHashes(missing_op_hashes) => Some(RoundAction::Ops),
        ShardedGossipWire::OpBatchReceived(op_batch_received) => None,

        ShardedGossipWire::Error(_)
        | ShardedGossipWire::Busy(_)
        | ShardedGossipWire::NoAgents(_)
        | ShardedGossipWire::AlreadyInProgress(_) => Some(RoundAction::Close),
    }
}

pub fn map_state(state: RoundState) -> Option<RoundPhase> {
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
