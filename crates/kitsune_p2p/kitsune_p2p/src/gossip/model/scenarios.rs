use std::sync::Arc;

use anyhow::anyhow;
use kitsune_p2p_bin_data::NodeCert;
use kitsune_p2p_types::GossipType;
use peer_model::*;
use polestar::prelude::*;
use round_model::{RoundAction, RoundMsg};

use super::*;

#[test]
fn scenario1() -> anyhow::Result<()> {
    use polestar::dfa::checked::Predicate as P;
    use polestar::prelude::*;
    let id = NodeId::try_from(0).unwrap();

    {
        let tgt = P::atom("tgt".into(), |m: &PeerState| m.initiate_tgt.is_some());
        let mut model = PeerState::new(GossipType::Recent)
            .checked(|s| s)
            .predicate(P::always(tgt.clone().implies(P::eventually(P::not(tgt)))))
            .predicate(P::eventually(P::atom("1-round".into(), |m: &PeerState| {
                m.rounds.len() == 1
            })));

        model = model
            .transition(NodeAction::SetInitiate(id.clone(), false))
            .unwrap()
            .0;
        assert_eq!(model.initiate_tgt.as_ref().unwrap().cert, id);
        model = model
            .transition((id.clone(), RoundMsg::Accept).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundMsg::AgentDiff).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundMsg::Agents).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundMsg::OpDiff).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundMsg::Ops).into())
            .unwrap()
            .0;
        assert!(model.initiate_tgt.is_none());
        assert_eq!(model.rounds.len(), 0);
    }
    Ok(())
}
