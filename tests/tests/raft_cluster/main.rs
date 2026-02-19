//! Integration tests for Raft consensus cluster formation.
//!
//! Tests multi-node cluster formation, leader election, deterministic
//! election succession, leader failure and re-election, log replication,
//! and `ClusterCommand` state machine replication.
//!
//! Test modules are organized by scenario category:
//! - `formation` — cluster startup and leader agreement
//! - `leader_election` — leader failover, re-election, fault tolerance
//! - `log_replication` — log entry replication and consistency
//! - `state_machine` — `ClusterCommand` replication and state consistency

mod formation;
mod leader_election;
mod log_replication;
mod state_machine;

pub use pirc_integration_tests::cluster_harness::{
    MemStorage, RaftTestCluster as TestCluster, test_config,
};
