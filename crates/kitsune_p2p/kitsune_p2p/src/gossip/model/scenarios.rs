use std::sync::Arc;

use anyhow::anyhow;
use kitsune_p2p_bin_data::NodeCert;
use kitsune_p2p_types::GossipType;
use peer_model::*;
use polestar::prelude::*;
use round_model::RoundAction;

use super::*;

#[test]
fn scenario1() -> anyhow::Result<()> {
    use polestar::fsm::checked::Predicate as P;
    use polestar::prelude::*;
    let id = NodeId::try_from(0).unwrap();

    {
        let tgt = P::atom("tgt".into(), |m: &PeerModel| m.initiate_tgt.is_some());
        let mut model = PeerModel::new(GossipType::Recent)
            .checked(|s| s)
            .predicate(P::always(tgt.clone().implies(P::eventually(P::not(tgt)))))
            .predicate(P::eventually(P::atom("1-round".into(), |m: &PeerModel| {
                m.rounds.len() == 1
            })));

        model = model
            .transition(NodeAction::SetInitiate(id.clone(), false))
            .unwrap()
            .0;
        assert_eq!(model.initiate_tgt.as_ref().unwrap().cert, id);
        model = model
            .transition((id.clone(), RoundAction::Accept).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundAction::AgentDiff).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundAction::Agents).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundAction::OpDiff).into())
            .unwrap()
            .0;
        model = model
            .transition((id.clone(), RoundAction::Ops).into())
            .unwrap()
            .0;
        assert!(model.initiate_tgt.is_none());
        assert_eq!(model.rounds.len(), 0);
    }
    Ok(())
}
