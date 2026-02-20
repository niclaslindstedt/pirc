use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pirc_protocol::parse;

fn bench_parse_simple_command(c: &mut Criterion) {
    c.bench_function("parse_simple_command", |b| {
        b.iter(|| parse(black_box("QUIT\r\n")).unwrap());
    });
}

fn bench_parse_ping(c: &mut Criterion) {
    c.bench_function("parse_ping", |b| {
        b.iter(|| parse(black_box("PING irc.example.com\r\n")).unwrap());
    });
}

fn bench_parse_privmsg_with_prefix(c: &mut Criterion) {
    c.bench_function("parse_privmsg_with_prefix", |b| {
        b.iter(|| {
            parse(black_box(
                ":nick!user@host.example.com PRIVMSG #channel :Hello, world!\r\n",
            ))
            .unwrap()
        });
    });
}

fn bench_parse_numeric_reply(c: &mut Criterion) {
    c.bench_function("parse_numeric_reply", |b| {
        b.iter(|| {
            parse(black_box(
                ":irc.server.com 001 nick :Welcome to the network\r\n",
            ))
            .unwrap()
        });
    });
}

fn bench_parse_server_prefix(c: &mut Criterion) {
    c.bench_function("parse_server_prefix", |b| {
        b.iter(|| {
            parse(black_box(
                ":irc.server.com NOTICE * :Server restarting\r\n",
            ))
            .unwrap()
        });
    });
}

fn bench_parse_pirc_version(c: &mut Criterion) {
    c.bench_function("parse_pirc_version", |b| {
        b.iter(|| parse(black_box("PIRC VERSION 1.0\r\n")).unwrap());
    });
}

fn bench_parse_pirc_cluster(c: &mut Criterion) {
    c.bench_function("parse_pirc_cluster_join", |b| {
        b.iter(|| parse(black_box("PIRC CLUSTER JOIN token123\r\n")).unwrap());
    });
}

fn bench_parse_many_params(c: &mut Criterion) {
    // Build a message with many parameters (approaching MAX_PARAMS=15)
    let msg = format!(
        ":nick!user@host MODE #channel +o {} {} {} {} {} {} {} {} {} {} {} {}\r\n",
        "p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9", "p10", "p11", "p12"
    );
    c.bench_function("parse_many_params", |b| {
        b.iter(|| parse(black_box(&msg)).unwrap());
    });
}

fn bench_parse_max_length_message(c: &mut Criterion) {
    // Build a message near the 512-byte limit
    let padding = "x".repeat(450);
    let msg = format!("PRIVMSG #ch :{padding}\r\n");
    assert!(msg.len() <= 512);
    c.bench_function("parse_max_length_message", |b| {
        b.iter(|| parse(black_box(&msg)).unwrap());
    });
}

fn bench_parse_join(c: &mut Criterion) {
    c.bench_function("parse_join", |b| {
        b.iter(|| parse(black_box("JOIN #channel\r\n")).unwrap());
    });
}

fn bench_message_to_wire(c: &mut Criterion) {
    use pirc_protocol::{Command, Message, Prefix};
    let msg = Message::with_prefix(
        Prefix::Server("irc.example.com".to_owned()),
        Command::Privmsg,
        vec!["#channel".to_owned(), "Hello, world!".to_owned()],
    );
    c.bench_function("message_to_wire", |b| {
        b.iter(|| black_box(msg.to_string()));
    });
}

criterion_group!(
    benches,
    bench_parse_simple_command,
    bench_parse_ping,
    bench_parse_privmsg_with_prefix,
    bench_parse_numeric_reply,
    bench_parse_server_prefix,
    bench_parse_pirc_version,
    bench_parse_pirc_cluster,
    bench_parse_many_params,
    bench_parse_max_length_message,
    bench_parse_join,
    bench_message_to_wire,
);
criterion_main!(benches);
