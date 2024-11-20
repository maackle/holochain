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
    let certs = [
        NodeCert::from(Arc::new([0; 32])),
        NodeCert::from(Arc::new([1; 32])),
        NodeCert::from(Arc::new([2; 32])),
    ];
    let cert = certs[0].clone();

    {
        let tgt = P::atom("tgt".into(), |m: &PeerModel| m.initiate_tgt.is_some());
        let mut model = PeerModel::new(GossipType::Recent)
            .checked(|s| s)
            .predicate(P::always(tgt.clone().implies(P::eventually(P::not(tgt)))))
            .predicate(P::eventually(P::atom("1-round".into(), |m: &PeerModel| {
                m.rounds.len() == 1
            })));

        model = model
            .transition(NodeAction::SetInitiate(cert.clone()))
            .unwrap()
            .0;
        assert_eq!(model.initiate_tgt.as_ref().unwrap().cert, cert);
        model = model
            .transition((cert.clone(), RoundAction::Accept).into())
            .unwrap()
            .0;
        model = model
            .transition((cert.clone(), RoundAction::AgentDiff).into())
            .unwrap()
            .0;
        model = model
            .transition((cert.clone(), RoundAction::Agents).into())
            .unwrap()
            .0;
        model = model
            .transition((cert.clone(), RoundAction::OpDiff).into())
            .unwrap()
            .0;
        model = model
            .transition((cert.clone(), RoundAction::Ops).into())
            .unwrap()
            .0;
        assert!(model.initiate_tgt.is_none());
        assert_eq!(model.rounds.len(), 0);
    }
    Ok(())
}
