//! Integration tests for cluster failover and user migration.
//!
//! Validates server failure detection, automatic leader re-election with
//! state consistency, user migration tracking, membership changes, and
//! degraded-mode behavior when quorum is lost.
//!
//! Test modules are organized by scenario category:
//! - `failover` — leader failure, re-election, and user migration
//! - `state_consistency` — state replication preserved after failover
//! - `membership` — dynamic membership changes (add/remove server)
//! - `degraded_mode` — quorum loss and degraded cluster behavior

mod degraded_mode;
mod failover;
mod membership;
mod state_consistency;

pub use pirc_integration_tests::cluster_harness::FailoverTestCluster as TestCluster;
