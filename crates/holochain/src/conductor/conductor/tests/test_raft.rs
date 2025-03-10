use std::{collections::BTreeSet, time::Duration};

use holochain_conductor_api::{RaftInterfaceRequest, RaftInterfaceRequestPayload};
use holochain_raft::{Dinghy, RaftOp};
use holochain_wasm_test_utils::TestWasm;
use p2p_raft::testing::await_partition_stability;

use super::*;

#[tokio::test(flavor = "multi_thread")]
#[cfg(feature = "slow_tests")]
async fn test_raft() {
    holochain_trace::test_run();

    tokio::spawn(async move {
        let mut t = 0;
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            t += 1;
            println!("     t = {t}");
        }
    });

    let num = 5;
    let raft_id: RaftId = EntryHash::from_raw_32(vec![55; 32]).into();
    let config = SweetConductorConfig::standard();
    let mut conductors = SweetConductorBatch::from_config(num, config).await;

    let (dna_file, _, _) = SweetDnaFile::unique_from_test_wasms(vec![TestWasm::Anchor]).await;
    let dna_hash = dna_file.dna_hash().clone();

    let apps = conductors.setup_app("app", &[dna_file]).await.unwrap();
    let cells = apps.cells_flattened();

    for (i, c) in cells.iter().enumerate() {
        println!("cell {}: {}", i, c.agent_pubkey().suffix(4));
    }
    conductors.exchange_peer_info().await;

    println!("exchanged peer info");

    let mk_payload = |payload| RaftInterfaceRequest {
        dna_hash: dna_hash.clone(),
        raft_id: raft_id.clone(),
        payload,
    };

    let rafts = futures::future::join_all(conductors.iter().map(|c| {
        c.get_raft(dna_hash.clone(), raft_id.clone())
            .map(|r| r.raft)
    }))
    .await;

    dbg!();

    // spawn_info_task(rafts.clone());

    // Initialize the first conductor with a raft with only itself
    conductors[0]
        .handle_raft_interface_call(mk_payload(RaftInterfaceRequestPayload::Initialize(vec![
            cells[0].agent_pubkey().clone(),
        ])))
        .await
        .unwrap();

    dbg!();

    // wait for self-election
    let leader_index = await_leader([&conductors[0]], [&cells[0]], &raft_id, None).await;
    assert_eq!(leader_index, 0);

    dbg!();

    for i in 1..num {
        // All known peers up to this point
        let peers = cells
            .iter()
            .take(i + 1)
            .map(|c| c.agent_pubkey().clone())
            .collect_vec();

        // Set up the raft with all known nodes up to this point
        //
        // This may error with NotAllowed if a raft message was already sent from another initialized node.
        // If so it's safe to ignore.
        conductors[i]
            .handle_raft_interface_call(mk_payload(RaftInterfaceRequestPayload::Initialize(
                peers.clone(),
            )))
            .await
            .unwrap();

        // Broadcast a request to all known peers to be added to their raft cluster.
        // In reality the message only needs to be sent to the leader, and in fact only the leader
        // can process the request. Broadcasting is just a quicker way to get the message out to the leader.
        // If the cluster has no elected leader at the time of the request, this will fail and need to be retried.
        // TODO: test the above.
        // TODO: Join and Initialize will pretty much always go together, so maybe they should be combined.
        let res = conductors[i]
            .handle_raft_interface_call(mk_payload(RaftInterfaceRequestPayload::Join(
                peers.clone(),
            )))
            .await;
        println!(
            "JOIN {i}: {:?}  {res:?}",
            peers.iter().map(|p| p.suffix(4)).collect_vec()
        );
    }

    // Wait for all clusters to agree on a leader
    let leader_index = await_leader(conductors.iter(), &cells, &raft_id, None).await;

    dbg!();

    // Let each node propose an op
    for i in 0..num {
        conductors[i]
            .handle_raft_interface_call(mk_payload(RaftInterfaceRequestPayload::Propose(
                RaftOp::from(vec![i as u8]),
            )))
            .await
            .unwrap();

        dbg!();
    }

    println!("wrote data");

    // Make more than half of the conductors crash
    for i in 0..(num + 1) / 2 {
        conductors[i].shutdown().await;
        println!("SHUTDOWN {i}");
        await_partition_stability(&rafts[i + 1..]).await;
    }

    // Wait for the survivors to agree on a new leader
    let leader2 = await_leader(conductors.iter(), &cells, &raft_id, Some(leader_index)).await;
    dbg!(leader2);
    assert_ne!(leader_index, leader2);

    // Check that all ops are still retrievable by the remaining voters
    for i in 0..num {
        if i == leader_index || !conductors[i].is_running() {
            continue;
        }

        let ops = conductors[i]
            .handle_raft_interface_call(mk_payload(RaftInterfaceRequestPayload::GetUserLogEntries(
                None,
            )))
            .await
            .unwrap();

        assert_eq!(
            ops.unwrap_user_log_entries().len(),
            num,
            "agent {i} can't get all the ops"
        );
    }

    // Make the crashed conductors come back
    for i in 0..(num + 1) / 2 {
        conductors[i].startup().await;
    }

    // re-fetch the newly created rafts
    let rafts = futures::future::join_all(conductors.iter().map(|c| {
        c.get_raft(dna_hash.clone(), raft_id.clone())
            .map(|r| r.raft)
    }))
    .await;

    await_partition_stability(&rafts).await;

    // Check that all conductors are voters
    for i in 0..num {
        for j in 0..num {
            if i != j {
                assert!(
                    rafts[i].is_voter(&rafts[j].id).await.unwrap(),
                    "{i} sees {j} as voter"
                );
            }
        }
    }
}

/// Wait for the cluster to settle on an elected leader. Only returns when all running conductors agree.
async fn await_leader(
    batch: impl IntoIterator<Item = &SweetConductor>,
    cells: impl IntoIterator<Item = &SweetCell>,
    raft_id: &RaftId,
    not_this_one: Option<usize>,
) -> usize {
    let batch = batch.into_iter().collect_vec();
    let cells = cells.into_iter().collect_vec();
    let dna_hash = cells[0].dna_hash();
    let start = std::time::Instant::now();
    loop {
        let mut leaders = BTreeSet::new();
        for (cond, cell) in batch.iter().zip(cells.iter()) {
            if cond.is_running() {
                let data = cond.get_raft(dna_hash.clone(), raft_id.clone()).await;
                let leader = data.raft.current_leader().await.map(|l| l.agent());
                leaders.insert(leader.clone());

                // let tracker = data.raft.tracker.lock().await;
                // let present: BTreeSet<String> = tracker
                //     .responsive_peers(tokio::time::Duration::from_secs(3))
                //     .into_iter()
                //     .map(|a| a.agent().suffix(4))
                //     .collect();
                // println!(
                //     "{} <{:?}>: {:?}",
                //     cell.agent_pubkey().suffix(4),
                //     leader.map(|l| l.suffix(4)),
                //     present
                // );

                // for (a, t) in data.client.peer_tracker.lock().await.last_seen().iter() {
                //     println!("{}->{}: {:?}", cell.agent_pubkey(), a, t.elapsed());
                // }
            }
        }
        // println!("-----------");
        if leaders.len() == 1 {
            if let Some(agent) = leaders.pop_first().unwrap() {
                let (leader_index, _) = cells
                    .iter()
                    .find_position(|c| c.agent_pubkey() == &agent)
                    .unwrap();

                // Skip the one we're not interested in
                if Some(leader_index) != not_this_one {
                    println!("leader {leader_index} found in {:?}", start.elapsed());
                    return leader_index;
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
}

fn spawn_info_task(rafts: impl IntoIterator<Item = Dinghy>) {
    let rafts = rafts.into_iter().collect_vec();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        println!("spawned info task");
        loop {
            println!();
            println!("........................................................");
            interval.tick().await;
            for r in rafts.iter() {
                let t = r.tracker.lock().await;
                let peers = t.responsive_peers(r.config.p2p_config.responsive_interval);
                let members = r
                    .raft
                    .with_raft_state(|s| {
                        s.membership_state
                            .committed()
                            .voter_ids()
                            .collect::<BTreeSet<_>>()
                    })
                    .await
                    .ok();

                // let log = r.read_log_data().await;
                // let snapshot = r
                //     .raft
                //     .get_snapshot()
                //     .await
                //     .ok()
                //     .and_then(|s| Some(s?.snapshot.data));

                if let Some(members) = members {
                    let lines = [
                        format!("... "),
                        format!("{}", r.id),
                        format!("<{:?}>", r.current_leader().await.map(|l| l.to_string())),
                        format!(
                            "members {:?}",
                            members.iter().map(ToString::to_string).collect_vec()
                        ),
                        format!(
                            "sees {:?}",
                            peers.iter().map(ToString::to_string).collect_vec()
                        ),
                        // format!("snapshot {:?}", snapshot),
                        // format!("log {:?}", log),
                    ];

                    println!("{}", lines.into_iter().join(" "));
                } else {
                    println!("...  {} <shutdown>", r.id);
                }
            }
            println!("........................................................");
            println!();
        }
    });
}
