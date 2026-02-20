use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pirc_server::raft::{
    compute_election_timeout, is_log_up_to_date, ElectionTracker, NodeId, RaftConfig,
};
use pirc_server::raft::types::{LogIndex, Term};

fn bench_election_tracker_record_vote_3(c: &mut Criterion) {
    c.bench_function("election_tracker_record_vote_3node", |b| {
        b.iter(|| {
            let mut tracker = ElectionTracker::new(3);
            tracker.record_vote(black_box(NodeId::new(1)));
            tracker.record_vote(black_box(NodeId::new(2)));
            tracker.has_quorum()
        });
    });
}

fn bench_election_tracker_record_vote_5(c: &mut Criterion) {
    c.bench_function("election_tracker_record_vote_5node", |b| {
        b.iter(|| {
            let mut tracker = ElectionTracker::new(5);
            tracker.record_vote(black_box(NodeId::new(1)));
            tracker.record_vote(black_box(NodeId::new(2)));
            tracker.record_vote(black_box(NodeId::new(3)));
            tracker.has_quorum()
        });
    });
}

fn bench_election_tracker_record_vote_7(c: &mut Criterion) {
    c.bench_function("election_tracker_record_vote_7node", |b| {
        b.iter(|| {
            let mut tracker = ElectionTracker::new(7);
            for i in 1..=4 {
                tracker.record_vote(black_box(NodeId::new(i)));
            }
            tracker.has_quorum()
        });
    });
}

fn bench_compute_election_timeout_3(c: &mut Criterion) {
    let config = RaftConfig {
        node_id: NodeId::new(1),
        peers: vec![NodeId::new(2), NodeId::new(3)],
        ..RaftConfig::default()
    };
    c.bench_function("compute_election_timeout_3node", |b| {
        b.iter(|| compute_election_timeout(black_box(&config)));
    });
}

fn bench_compute_election_timeout_5(c: &mut Criterion) {
    let config = RaftConfig {
        node_id: NodeId::new(3),
        peers: vec![
            NodeId::new(1),
            NodeId::new(2),
            NodeId::new(4),
            NodeId::new(5),
        ],
        ..RaftConfig::default()
    };
    c.bench_function("compute_election_timeout_5node", |b| {
        b.iter(|| compute_election_timeout(black_box(&config)));
    });
}

fn bench_compute_election_timeout_7(c: &mut Criterion) {
    let config = RaftConfig {
        node_id: NodeId::new(4),
        peers: vec![
            NodeId::new(1),
            NodeId::new(2),
            NodeId::new(3),
            NodeId::new(5),
            NodeId::new(6),
            NodeId::new(7),
        ],
        ..RaftConfig::default()
    };
    c.bench_function("compute_election_timeout_7node", |b| {
        b.iter(|| compute_election_timeout(black_box(&config)));
    });
}

fn bench_is_log_up_to_date(c: &mut Criterion) {
    c.bench_function("is_log_up_to_date_same_term", |b| {
        b.iter(|| {
            is_log_up_to_date(
                black_box(Term::new(5)),
                black_box(LogIndex::new(100)),
                black_box(Term::new(5)),
                black_box(LogIndex::new(50)),
            )
        });
    });
}

fn bench_is_log_up_to_date_higher_term(c: &mut Criterion) {
    c.bench_function("is_log_up_to_date_higher_term", |b| {
        b.iter(|| {
            is_log_up_to_date(
                black_box(Term::new(6)),
                black_box(LogIndex::new(10)),
                black_box(Term::new(5)),
                black_box(LogIndex::new(100)),
            )
        });
    });
}

fn bench_election_tracker_reset(c: &mut Criterion) {
    c.bench_function("election_tracker_reset", |b| {
        let mut tracker = ElectionTracker::new(5);
        for i in 1..=3 {
            tracker.record_vote(NodeId::new(i));
        }
        b.iter(|| {
            tracker.reset();
            tracker.record_vote(black_box(NodeId::new(1)));
        });
    });
}

criterion_group!(
    benches,
    bench_election_tracker_record_vote_3,
    bench_election_tracker_record_vote_5,
    bench_election_tracker_record_vote_7,
    bench_compute_election_timeout_3,
    bench_compute_election_timeout_5,
    bench_compute_election_timeout_7,
    bench_is_log_up_to_date,
    bench_is_log_up_to_date_higher_term,
    bench_election_tracker_reset,
);
criterion_main!(benches);
