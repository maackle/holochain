use std::sync::Arc;

use anyhow::anyhow;
use gossip_model::*;
use kitsune_p2p_bin_data::NodeCert;
use kitsune_p2p_types::GossipType;
use polestar::prelude::*;
use round_model::RoundAction;

use super::*;

#[test]
fn scenario1() -> anyhow::Result<()> {
    use polestar::fsm::checked::Predicate as P;
    use polestar::prelude::*;
    let certs = [
        NodeCert::from(Arc::new([0; 32])),
        NodeCert::from(Arc::new([1; 32])),
        NodeCert::from(Arc::new([2; 32])),
    ];
    let cert = certs[0].clone();

    {
        let tgt = P::atom("tgt".into(), |m: &GossipModel| m.initiate_tgt.is_some());
        let mut model = GossipModel::new(GossipType::Recent)
            .checked(|s| s)
            .predicate(P::always(tgt.clone().implies(P::eventually(P::not(tgt)))));

        model = model
            .transition_(GossipAction::SetInitiate(cert.clone()))
            .unwrap();
        assert_eq!(model.initiate_tgt.as_ref().unwrap().cert, cert);
        model = model
            .transition_((cert.clone(), RoundAction::Accept).into())
            .unwrap();
        model = model
            .transition_((cert.clone(), RoundAction::AgentDiff).into())
            .unwrap();
        model = model
            .transition_((cert.clone(), RoundAction::Agents).into())
            .unwrap();
        model = model
            .transition_((cert.clone(), RoundAction::OpDiff).into())
            .unwrap();
        model = model
            .transition_((cert.clone(), RoundAction::Ops).into())
            .unwrap();
        assert!(model.initiate_tgt.is_none());
        assert_eq!(model.rounds.len(), 0);
    }
    Ok(())
}
