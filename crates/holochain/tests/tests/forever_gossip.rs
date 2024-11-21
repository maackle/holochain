use holo_hash::ActionHash;
use holochain::{
    sweettest::{SweetConductorBatch, SweetConductorConfig, SweetDnaFile},
    test_utils::inline_zomes::simple_crud_zome,
};
use nanoid::nanoid;
use rand::Rng;
use tokio::time::Duration;

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn forever() {
    const N: usize = 3;
    const COMMIT_DELAY: Duration = Duration::from_secs(1);

    let config = SweetConductorConfig::rendezvous(true);
    let mut conductors = SweetConductorBatch::from_config_rendezvous(3, config.clone()).await;

    let (dna_file, _, _) = SweetDnaFile::unique_from_inline_zomes(simple_crud_zome()).await;
    let cells = conductors
        .setup_app("app", [&dna_file])
        .await
        .unwrap()
        .cells_flattened();

    // conductors.exchange_peer_info().await;

    loop {
        let i = rand::thread_rng().gen_range(0..N);
        let payload = nanoid!();
        let _: ActionHash = conductors[i]
            .call(&cells[i].zome("coordinator"), "create_string", payload)
            .await;
        tokio::time::sleep(COMMIT_DELAY).await;
    }
}
