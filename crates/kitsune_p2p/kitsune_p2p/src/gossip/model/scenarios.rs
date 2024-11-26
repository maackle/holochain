use std::sync::Arc;

use anyhow::anyhow;
use kitsune_p2p_bin_data::NodeCert;
use kitsune_p2p_types::GossipType;
use peer_model::{NodeAction, PeerMachine, PeerState};
use polestar::prelude::*;
use round_model::{RoundAction, RoundMsg};

use super::*;

#[test]
fn scenario1() -> anyhow::Result<()> {
    use polestar::machine::checked::Predicate as P;
    use polestar::prelude::*;
    let id = NodeId::new(0);

    {
        let tgt = P::atom("tgt", |m: &PeerState| m.initiate_tgt.is_some());
        let one_round = P::atom("one_round", |m: &PeerState| m.rounds.len() == 1);

        let machine = PeerMachine::new(GossipType::Recent)
            .checked()
            .with_predicates([
                P::always(tgt.clone().implies(P::eventually(P::not(tgt)))),
                P::eventually(one_round),
            ]);

        let mut state = machine.initial(PeerState::new());
        state = machine
            .transition(state, NodeAction::SetInitiate(id.clone(), false))
            .unwrap()
            .0;
        assert_eq!(state.initiate_tgt.as_ref().unwrap().cert, id);
        state = machine
            .transition(state, (id, RoundMsg::Accept).into())
            .unwrap()
            .0;
        state = machine
            .transition(state, (id, RoundMsg::AgentDiff).into())
            .unwrap()
            .0;
        state = machine
            .transition(state, (id, RoundMsg::Agents).into())
            .unwrap()
            .0;
        state = machine
            .transition(state, (id, RoundMsg::OpDiff).into())
            .unwrap()
            .0;
        state = machine
            .transition(state, (id, RoundMsg::Ops).into())
            .unwrap()
            .0;
        assert!(state.initiate_tgt.is_none());
        assert_eq!(state.rounds.len(), 0);
    }
    Ok(())
}
