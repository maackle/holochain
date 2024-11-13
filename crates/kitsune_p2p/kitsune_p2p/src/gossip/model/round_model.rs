use std::sync::Arc;

use crate::{
    dependencies::kitsune_p2p_types::{GossipType, KitsuneResult},
    gossip::sharded_gossip::{store::AgentInfoSession, RoundState, ShardedGossipWire},
    NodeCert,
};
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

#[derive(Debug, Clone, Eq, PartialEq, Hash, Arbitrary)]
pub enum RoundEvent {
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
    type Action = (RoundEvent, Arc<RoundContext>);
    type Fx = ();
    type Error = Option<anyhow::Error>;

    fn transition(mut self, (event, ctx): Self::Action) -> MachineResult<Self> {
        use GossipType as T;
        use RoundEvent as E;
        use RoundPhase as P;

        let next = match (*ctx, self, event) {
            (T::Recent, P::Initiated | P::Accepted, E::AgentDiff) => P::AgentDiffReceived,
            (T::Historical, P::Initiated | P::Accepted, E::OpDiff) => P::OpDiffReceived,
            (T::Recent, P::AgentDiffReceived, E::Agents) => P::AgentsReceived,
            (T::Recent, P::AgentsReceived, E::OpDiff) => P::OpDiffReceived,
            (_, P::OpDiffReceived, E::Ops) => P::Finished,
            (_, P::Finished, _) => return Err(None),

            // This might not be right
            (_, _, E::Close) => P::Finished,

            _ => return Err(Some(anyhow::anyhow!("invalid transition"))),
        };
        Ok((next, ()))
    }
}

pub type RoundFsm = Contextual<RoundPhase, RoundContext>;

pub fn map_event(msg: ShardedGossipWire) -> Option<RoundEvent> {
    match msg {
        ShardedGossipWire::Initiate(initiate) => Some(RoundEvent::Initiate),
        ShardedGossipWire::Accept(accept) => Some(RoundEvent::Accept),
        ShardedGossipWire::Agents(agents) => Some(RoundEvent::AgentDiff),
        ShardedGossipWire::MissingAgents(missing_agents) => Some(RoundEvent::Agents),
        ShardedGossipWire::OpBloom(op_bloom) => Some(RoundEvent::OpDiff),
        ShardedGossipWire::OpRegions(op_regions) => Some(RoundEvent::OpDiff),
        ShardedGossipWire::MissingOpHashes(missing_op_hashes) => Some(RoundEvent::Ops),
        ShardedGossipWire::OpBatchReceived(op_batch_received) => None,

        ShardedGossipWire::Error(_)
        | ShardedGossipWire::Busy(_)
        | ShardedGossipWire::NoAgents(_)
        | ShardedGossipWire::AlreadyInProgress(_) => Some(RoundEvent::Close),
    }
}

pub fn map_state(state: RoundState) -> Option<RoundPhase> {
    todo!()
}

#[test]
#[ignore = "diagram"]
fn diagram_round_state() {
    use polestar::diagram::*;

    tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::new()).unwrap();

    let config = DiagramConfig {
        steps: 100,
        walks: 100,
        ignore_loopbacks: false,
    };

    print_dot_state_diagram(RoundPhase::Initiated.context(GossipType::Recent), &config);

    print_dot_state_diagram(
        RoundPhase::Initiated.context(GossipType::Historical),
        &config,
    );
}
