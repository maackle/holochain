# MVP

assuming all of the below works, let's start with idea 2, with no merging:

- when the leader notices someone has not responded in a while, the leader demotes them to learner
- if someone finds themselves suddenly out of the cluster, they'll ask to rejoin, and the leader will allow it


# idea 1: use sub-rafts to represent forks whenever quorum is lost

do a bunch of logic whenever an incoming raft message is received
(maybe need to run on a regular schedule, but hopefully that is sufficient):

as long as there is a leader, we don't do anything special.
the following applies whenever there is no leader:
- determine the current set of visible peers: a peer is visible only if we've seen a raft message from them in the last X seconds.
- check if the visible peers makes up quorum
- if, continuously for Y seconds, every time this check is run it is found that there is not a quorum, then it's Time To Fork.

Forking:
- when it's time to fork, put the raft in a special Forking state. read/write/management requests are blocked in this state.
- choose a random timeout based on the raft election timeout parameters.
- after the timeout has elapsed, create a new raft and initialize it with whatever peers are visible at the time of forking.
  - the timeout is meant to help prevent duplicate forks from being created.
- keep the parent raft running (but maybe tune down the election timeout to reduce traffic), so that the lost peers can be rediscovered.
- as soon as a leader is elected on the child fork, the leader will install a snapshot from the parent raft.
- after this snapshot is installed, the Forking state is removed, and the raft functions as normal.
- NOTE: probably good for the new raft to start with an entry designating it as a fork of the old, including the log id which indicates the fork point

Fork ID:
- rafts are identified by EntryHash plus optional ChildId, a random u64.
- if at any time you receive a raft message addressed to a fork of a raft you're already in, then create and initialize it with whoever you see as the active set of peers in the parent raft, and process it through that
  - TODO: what if the raft already has a log? do we need to differentiate between whether the sender is leader or not?
  - TODO: what if your peer set and the sender's peer set are different? will it matter?

Merging:
- if at any time you receive a raft message addressed to a parent raft (one which was already forked), respond to it as normal, because: if nodes which remained in the parent cluster are now available, then everyone can start talking on the parent cluster again
- if a leader is elected on the parent raft, then its child(ren) should have a leader too, since they have a subset of members which should also be well connected
- as soon as a leader is elected on the parent raft (because quorum was re-established), put each raft in a special Merging state, which will ignore further read/write/management requests (normal raft messages still get processed)
- the child Leader sends a special MergeSnapshot request to the parent Leader
- the parent Leader does some merging magic to merge its existing snapshot with the child snapshot.
  - TODO: how exactly does the parent leader merge? the app needs to say something about this.
- after the merge, the Merging state can be removed, along with installing the merged snapshot.

Overlaps/edge cases:
- if the parent raft regains its leader while a child is in Forking state, just remove the child, since it doesn't contain anything new
- if the parent loses its leader while Merging, that means there was another partition, and it may be a totally different one than the one that is being merged.
  - the only thing to do is exit the Merging state



# idea 2: dynamically change the membership set to try to keep quorum for as long as possible

TODO: is it even possible for two partitioned rafts to maintain quorum in this way? I think only if there were a partial partition? Even so, even in the worst case where the leader has unilateral decision making power in removing some member, as soon as the leader and the member see each other again, or as soon as a new leader is elected, the member can rejoin.
TODO: rework this with the understanding that the "smaller partition" is realistically an "idea 1" forked raft

- if any node has not been visible for N seconds, send a message to the leader asking to remove them from the member set due to absence.
- the leader collects these requests, and once a majority has been received, that node is removed and will have to rejoin (or maybe they become a learner but lose voting ability?)
- if the node simply went offline, this is no problem. they were in a raft of one, and they should know they're offline and not produce new data.
- if the node went into a partition, they might remove the missing nodes and reduce quorum, and still produce new data.
- either way, if they produced no new data (including electing a new leader), there is no problem -- the only complexity arises when there is data to merge. 
- any time a raft reduces its member set in this way, it should track the nodes that it dropped, and the leader should send join requests to all of the members periodically (because those members likely removed their counterparts as well). see [IUudh9uh]
- if contact is reestablished, then the leader will see conflicts coming in from the other leader once it gets added into the other raft's members, and this will be a signal to perform a merge

## merge detection:

- when a merge is needed, the logs will be irreconcilable. there will be overlap in the terms of leaders, there will be data entries and membership entries with the same index.
- the leaders can detect this state by noting that they are in two different rafts with a shared common history
  - TODO: if only snapshots are replicated and not entire logs, can this actually be detected?
    - ~~perhaps whenever an absentee member is removed, the log id needs to be recorded, as this is a moment when a fork could happen~~ - but no, detecting absenteeism at a given time is not a guarantee that the absentee didn't create a new raft before that time
  - it is a bit redundant to require the full shared history to be replicated so that it can be discovered and one of the rafts discarded again
    - TODO: would be better to figure out the shared history more directly
- more realistically some messaging needs to happen to find the fork point more directly
  - for instance, we already know this is a merge attempt via [IUudh9uh], so we can expect that if we regain contact, we have a merge to do

## entering "merging" state:

- when one leader gets back in touch with the leader of the other raft, a merge can begin.
- the leaders should decide which raft will be the mainline and which raft will get merged in to that as a branch.
  - the larger raft should probably fold the smaller raft's snapshot in. if the sizes are equal, a tiebreaker is needed: perhaps an ordered set of member ids
- both raft leaders should commit a special Merging entry containing the other raft's data to merge 
  - this puts the raft into a special Merging state so that no new user data will be committed (membership, reads, and voting is still fine)

## handling the "merging" state:

- how the merge is done is application-specific
  - maybe the app wants to use a CRDT to merge the two snapshots
  - or maybe it wants to collect the operations that happened after the fork point and replay them onto the main branch as OTs
- the way to let the application decide, is for the leader's application to receive a signal that a merge is happening, so that...
- the leader's application can do a read and find the "merging" state entry, combine the data, and append a "merged" entry with the final result
  - IF a raft state machine is able to be provided by the application, this happens for free, because the state machine can handle the merge itself
    - see https://github.com/databendlabs/openraft/blob/e7237893046b2e99d673261464ce4a840f511a0c/openraft/src/storage/v2/raft_state_machine.rs for trait that needs to be (partially) implemented by the app
  - if a generic SM is used, then either one of these should be done (which one I'm not sure due to my ignorance of raft):
    - a "snapshot" entry should be appended which wipes out all other context and just uses that snapshot as the snapshot from this point forward
    - or, if possible, a snapshot can be installed directly without committing a new entry

## leaving "merging" state:

- once a leader has performed a merge, it can add all the members of the other raft into its own raft
- another entry may need to be committed to remove the "merging" state


## potentially helpful:
- in a "careful mode", both leaders of both rafts can perform the merge, and check that the merged snapshots are identical before discarding one of the rafts.
- offline detection: if all other nodes go away within the same small time window, you're probably offline and shouldn't create a raft of one.
- partition detection: if a majority of nodes go away within the same small time window, that's probably a partition, and you can expect them to produce new data (does this make a difference though, if you want to continue within your smaller partition?)



# joining the two ideas

realistically we may need both of these ideas:

- idea 1 is for the benefit of the smaller partition who lost quorum and can't make any progress, and needs to create a new raft
- idea 2 is for the benefit of the larger partition who wants to allow members to come and go (e.g. as they close and open their laptop lids) without the disruption of needing to leave the main raft

would be good for as much as possible to be shared between the two. 
for instance, merge detection should be the same.
when a small partition forms a sub-raft per idea 1, it should track the missing members in the same way as absentees are tracked in idea 2

# take a step back

TODO: if all that's needed is clerk selection, can this be simpler?
