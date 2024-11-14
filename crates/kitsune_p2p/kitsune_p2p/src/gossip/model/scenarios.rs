use std::sync::Arc;

use gossip_model::*;
use kitsune_p2p_bin_data::NodeCert;
use kitsune_p2p_types::GossipType;
use polestar::prelude::*;
use round_model::RoundAction;

use super::*;

#[test]
fn scenario1() {
    // TODO: understand more clearly how initiate_tgt really works
    let model = GossipModel::new(GossipType::Recent);

    let certs = [
        NodeCert::from(Arc::new([0; 32])),
        NodeCert::from(Arc::new([1; 32])),
        NodeCert::from(Arc::new([2; 32])),
    ];

    let actions = [
        GossipAction::SetInitiate(certs[0].clone()),
        GossipAction::Incoming {
            from: certs[0].clone(),
            msg: RoundAction::Accept,
        },
    ];

    let (final_model, _) = model.apply_actions(actions).unwrap();

    assert_eq!(final_model.rounds.len(), 1);
}
