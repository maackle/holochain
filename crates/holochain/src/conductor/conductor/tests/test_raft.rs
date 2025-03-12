use std::{collections::BTreeSet, time::Duration};

use holochain_conductor_api::{
    AppRequest, AppResponse, LogOp, RaftInterfaceRequest, RaftInterfaceRequestPayload,
    RaftInterfaceResponse, RaftInterfaceResponsePayload, RaftSignal,
};
use holochain_raft::{
    error::{ClientWriteError, ForwardToLeader, RaftError},
    Dinghy, LeaderId, LogId, RaftEvent, RaftOp,
};
use holochain_wasm_test_utils::TestWasm;
use p2p_raft::{message::P2pError, testing::await_partition_stability};

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn serialize_raft_types() {
    let agents = vec![AgentPubKey::from_raw_32(vec![11; 32])];
    let request_payloads = [
        RaftInterfaceRequestPayload::Initialize(agents.clone()),
        RaftInterfaceRequestPayload::Join(agents.clone()),
        RaftInterfaceRequestPayload::Leave,
        RaftInterfaceRequestPayload::Propose(RaftOp::from(vec![1, 2, 3])),
        RaftInterfaceRequestPayload::GetUserLogEntries(Some(42)),
    ];

    let response_payloads = [
        RaftInterfaceResponsePayload::UserLogEntries(vec![LogOp {
            log_id: LogId {
                index: 42,
                leader_id: Default::default(),
            },
            op: RaftOp::from(vec![1, 2, 3]),
        }]),
        RaftInterfaceResponsePayload::Ok,
        RaftInterfaceResponsePayload::Error(holochain_raft::message::P2pResponse::RaftError(
            RaftError::APIError(ClientWriteError::ForwardToLeader(ForwardToLeader {
                leader_id: Some(AgentPubKey::from_raw_32(vec![11; 32]).into()),
                leader_node: Some(()),
            })),
        )),
        RaftInterfaceResponsePayload::Error(holochain_raft::message::P2pResponse::P2pError(
            P2pError::NotVoter,
        )),
    ];

    let signals = [RaftEvent::EntryCommitted {
        log_id: LogId {
            index: 42,
            leader_id: Default::default(),
        },
        data: vec![1, 2, 3].into(),
    }];

    let mut errors = vec![];

    println!();
    println!("REQUESTS");
    println!("--------");
    for p in request_payloads {
        let r = AppRequest::Raft(RaftInterfaceRequest {
            dna_hash: DnaHash::from_raw_32(vec![22; 32]),
            raft_space: EntryHash::from_raw_32(vec![33; 32]).into(),
            payload: p,
        });
        // let serialized = serde_json::to_string_pretty(&r).unwrap();
        let serialized = serde_json::to_string(&r).unwrap();
        let deserialized: AppRequest = serde_json::from_str(&serialized).unwrap();
        assert_eq!(format!("{r:?}"), format!("{deserialized:?}"));
        println!("{}", serialized);
    }

    println!();
    println!("RESPONSES");
    println!("---------");
    for p in response_payloads {
        let r = AppResponse::Raft(p);
        // let serialized = serde_json::to_string_pretty(&r).unwrap();
        let serialized = serde_json::to_string(&r).unwrap();
        let deserialized: AppResponse = serde_json::from_str(&serialized).unwrap();

        let (left, right) = (format!("{r:?}"), format!("{deserialized:?}"));
        if left != right {
            // Some serialization fails because HcrTypes::Node = (), and Some(()) deserializes to None
            errors.push(format!(
                "NOTE: deserialization is different:\nLEFT:  {left}\nRIGHT: {right}"
            ));
        }

        println!("{}", serialized);
    }

    println!();
    println!("SIGNALS");
    println!("--------");
    for e in signals {
        let s = Signal::Raft(RaftSignal {
            space: EntryHash::from_raw_32(vec![33; 32]).into(),
            event: e,
        });
        let serialized = serde_json::to_string(&s).unwrap();
        let deserialized: Signal = serde_json::from_str(&serialized).unwrap();
        assert_eq!(format!("{s:?}"), format!("{deserialized:?}"));

        println!("{}", serialized);
    }

    println!();
    println!("ERRORS");
    println!("------");
    for e in errors {
        println!("{e}");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[cfg(feature = "slow_tests")]
async fn test_raft() {
    use either::Either;
    use holochain_conductor_api::{AppResponse, RaftSignal};
    use holochain_raft::RaftEvent;
    use holochain_types::websocket::AllowedOrigins;
    use holochain_websocket::ReceiveMessage;

    holochain_trace::test_run();

    // tokio::spawn(async move {
    //     let mut t = 0;
    //     let mut interval = tokio::time::interval(Duration::from_secs(1));
    //     loop {
    //         interval.tick().await;
    //         t += 1;
    //         println!("     t = {t}");
    //     }
    // });

    const NUM: usize = 5;
    let raft_id: RaftSpace = EntryHash::from_raw_32(vec![55; 32]).into();
    let config = SweetConductorConfig::standard();
    let mut conductors = SweetConductorBatch::from_config(NUM, config).await;

    let (dna_file, _, _) = SweetDnaFile::unique_from_test_wasms(vec![TestWasm::Anchor]).await;
    let dna_hash = dna_file.dna_hash().clone();

    let apps = conductors.setup_app("app", &[dna_file]).await.unwrap();
    let app_id = apps[0].installed_app_id().clone();
    let cells = apps.cells_flattened();

    let mut ports = vec![];
    for (_i, c) in conductors.iter().enumerate() {
        let port = c
            .raw_handle()
            .add_app_interface(Either::Left(0), AllowedOrigins::Any, Some(app_id.clone()))
            .await
            .unwrap();
        ports.push(port);
    }
    dbg!(&ports);

    // let port = conductors[0].list_app_interfaces().await.unwrap()[0]
    //     .clone()
    //     .port;

    let signals = Arc::new(Mutex::new(Vec::new()));

    for i in 0..NUM {
        let admin_port = conductors[i].get_arbitrary_admin_websocket_port().unwrap();
        dbg!(admin_port);
        let _task = {
            let (tx, mut rx) = websocket_client_by_port(ports[i]).await.unwrap();
            authenticate_app_ws_client(tx, admin_port, app_id.clone()).await;
            let sigs = signals.clone();
            tokio::task::spawn(async move {
                while let Ok(r) = rx.recv::<AppResponse>().await {
                    match r {
                        ReceiveMessage::Signal(s) => {
                            let signal = Signal::try_from_vec(s).unwrap();
                            println!(">>> SIGNAL {i:3}  {signal:?}");
                            sigs.lock().await.push((i, signal));
                        }
                        _ => {}
                    }
                }
            })
        };
    }

    for (i, c) in cells.iter().enumerate() {
        println!("cell {}: {}", i, c.agent_pubkey().suffix(4));
    }
    conductors.exchange_peer_info().await;

    println!("exchanged peer info");

    let mk_payload = |payload| RaftInterfaceRequest {
        dna_hash: dna_hash.clone(),
        raft_space: raft_id.clone(),
        payload,
    };

    let rafts = futures::future::join_all(conductors.iter().map(|c| {
        c.get_raft(app_id.clone(), dna_hash.clone(), raft_id.clone())
            .map(|r| r.raft.raft)
    }))
    .await;

    dbg!();

    // spawn_info_task(rafts.clone());

    // Initialize the first conductor with a raft with only itself
    conductors[0]
        .handle_raft_interface_call(
            app_id.clone(),
            mk_payload(RaftInterfaceRequestPayload::Initialize(vec![cells[0]
                .agent_pubkey()
                .clone()])),
        )
        .await
        .unwrap();

    dbg!();

    // wait for self-election
    let leader_index = await_leader([&conductors[0]], [&cells[0]], &app_id, &raft_id, None).await;
    assert_eq!(leader_index, 0);

    dbg!();

    for i in 1..NUM {
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
            .handle_raft_interface_call(
                app_id.clone(),
                mk_payload(RaftInterfaceRequestPayload::Initialize(peers.clone())),
            )
            .await
            .unwrap();

        // Broadcast a request to all known peers to be added to their raft cluster.
        // In reality the message only needs to be sent to the leader, and in fact only the leader
        // can process the request. Broadcasting is just a quicker way to get the message out to the leader.
        // If the cluster has no elected leader at the time of the request, this will fail and need to be retried.
        // TODO: test the above.
        // TODO: Join and Initialize will pretty much always go together, so maybe they should be combined.
        let res = conductors[i]
            .handle_raft_interface_call(
                app_id.clone(),
                mk_payload(RaftInterfaceRequestPayload::Join(peers.clone())),
            )
            .await;
        println!(
            "JOIN {i}: {:?}  {res:?}",
            peers.iter().map(|p| p.suffix(4)).collect_vec()
        );
    }

    // Wait for all clusters to agree on a leader
    let leader_index = await_leader(conductors.iter(), &cells, &app_id, &raft_id, None).await;

    // Let each node propose an op
    for i in 0..NUM {
        conductors[i]
            .handle_raft_interface_call(
                app_id.clone(),
                mk_payload(RaftInterfaceRequestPayload::Propose(RaftOp::from(vec![
                    i as u8,
                ]))),
            )
            .await
            .unwrap();

        dbg!();
    }

    println!("wrote data");

    // Make more than half of the conductors crash
    for i in 0..(NUM + 1) / 2 {
        conductors[i].shutdown().await;
        println!("SHUTDOWN {i}");
        await_partition_stability(&rafts[i + 1..]).await;
    }

    // Wait for the survivors to agree on a new leader
    let leader2 = await_leader(
        conductors.iter(),
        &cells,
        &app_id,
        &raft_id,
        Some(leader_index),
    )
    .await;
    dbg!(leader2);
    assert_ne!(leader_index, leader2);

    // Check that all ops are still retrievable by the remaining voters
    for i in 0..NUM {
        if i == leader_index || !conductors[i].is_running() {
            continue;
        }

        let ops = conductors[i]
            .handle_raft_interface_call(
                app_id.clone(),
                mk_payload(RaftInterfaceRequestPayload::GetUserLogEntries(None)),
            )
            .await
            .unwrap();

        assert_eq!(
            ops.unwrap_user_log_entries().len(),
            NUM,
            "agent {i} can't get all the ops"
        );
    }

    // Make the crashed conductors come back
    for i in 0..(NUM + 1) / 2 {
        conductors[i].startup().await;
    }

    // re-fetch the newly created rafts
    let rafts = futures::future::join_all(conductors.iter().map(|c| {
        c.get_raft(app_id.clone(), dna_hash.clone(), raft_id.clone())
            .map(|r| r.raft.raft)
    }))
    .await;

    await_partition_stability(&rafts).await;

    {
        // let mut m = BTreeMap::new();
        let mut ss = signals.lock().await.clone();
        ss.sort();
        let sorted = ss.clone();
        ss.dedup();
        assert_eq!(sorted, ss, "duplicate signals found.");

        println!("\n\n<><><><><><><><><> SIGNALS <><><><><><><><><>");
        for (i, s) in ss {
            match s {
                Signal::Raft(RaftSignal { space: _, event }) => match event {
                    RaftEvent::EntryCommitted { log_id, data } => {
                        println!("COMMITTED  {:3}: {:3} {:?}", i, log_id.index, data);
                    }
                    RaftEvent::MembershipChanged { log_id, members } => {
                        println!(
                            "MEMBERSHIP {:3}: {:3} {:?}",
                            i,
                            log_id.index,
                            members.iter().map(|m| m.agent().suffix(4)).collect_vec()
                        );
                    }
                },
                _ => unreachable!(),
            }
        }
        println!("<><><><><><><><><>>>>>X<<<<<><><><><><><><><>\n\n");
        // for (i, _s) in ss {
        //     let e = m.entry(i).or_insert(0);
        //     *e += 1;
        // }
    };

    // Check that all conductors are voters
    for i in 0..NUM {
        for j in 0..NUM {
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
    app_id: &InstalledAppId,
    raft_id: &RaftSpace,
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
                let data = cond
                    .get_raft(app_id.clone(), dna_hash.clone(), raft_id.clone())
                    .await;
                let leader = data.raft.raft.current_leader().await.map(|l| l.agent());
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
