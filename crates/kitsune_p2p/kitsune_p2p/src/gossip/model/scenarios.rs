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
    let certs = [
        NodeCert::from(Arc::new([0; 32])),
        NodeCert::from(Arc::new([1; 32])),
        NodeCert::from(Arc::new([2; 32])),
    ];
    let cert = certs[0].clone();

    {
        let mut model = GossipModel::new(GossipType::Recent);

        model = model
            .transition_(GossipAction::SetInitiate(cert.clone()))
            .unwrap();
        assert_eq!(model.initiate_tgt.as_ref().unwrap().cert, cert);
        model = model
            .transition_((certs[0].clone(), RoundAction::Accept).into())
            .unwrap();
        model = model
            .transition_((certs[0].clone(), RoundAction::AgentDiff).into())
            .unwrap();
        model = model
            .transition_((certs[0].clone(), RoundAction::Agents).into())
            .unwrap();
        model = model
            .transition_((certs[0].clone(), RoundAction::OpDiff).into())
            .unwrap();
        model = model
            .transition_((certs[0].clone(), RoundAction::Ops).into())
            .unwrap();
        assert!(model.initiate_tgt.is_none());
        assert_eq!(model.rounds.len(), 1);
    }
    Ok(())
}
