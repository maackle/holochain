#![allow(missing_docs, unused)]

use polestar::id::IdU8;

pub mod gossip_model;
pub mod peer_model;
pub mod round_model;

#[cfg(test)]
mod scenarios;

const PEERS: usize = 3;
pub type NodeId = IdU8<PEERS>;
