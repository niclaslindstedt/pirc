use bytes::BytesMut;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pirc_network::codec::PircCodec;
use pirc_protocol::{Command, Message, Prefix};
use tokio_util::codec::{Decoder, Encoder};

fn bench_decode_simple(c: &mut Criterion) {
    c.bench_function("codec_decode_ping", |b| {
        b.iter(|| {
            let mut codec = PircCodec::new();
            let mut buf = BytesMut::from(&b"PING irc.example.com\r\n"[..]);
            codec.decode(black_box(&mut buf)).unwrap().unwrap()
        });
    });
}

fn bench_decode_privmsg_with_prefix(c: &mut Criterion) {
    c.bench_function("codec_decode_privmsg", |b| {
        b.iter(|| {
            let mut codec = PircCodec::new();
            let mut buf = BytesMut::from(
                &b":nick!user@host.example.com PRIVMSG #channel :Hello, world!\r\n"[..],
            );
            codec.decode(black_box(&mut buf)).unwrap().unwrap()
        });
    });
}

fn bench_encode_simple(c: &mut Criterion) {
    let msg = Message::new(Command::Ping, vec!["irc.example.com".to_owned()]);
    c.bench_function("codec_encode_ping", |b| {
        b.iter(|| {
            let mut codec = PircCodec::new();
            let mut buf = BytesMut::with_capacity(64);
            codec.encode(black_box(msg.clone()), &mut buf).unwrap();
            buf
        });
    });
}

fn bench_encode_privmsg_with_prefix(c: &mut Criterion) {
    let msg = Message::with_prefix(
        Prefix::Server("irc.example.com".to_owned()),
        Command::Privmsg,
        vec!["#channel".to_owned(), "Hello, world!".to_owned()],
    );
    c.bench_function("codec_encode_privmsg", |b| {
        b.iter(|| {
            let mut codec = PircCodec::new();
            let mut buf = BytesMut::with_capacity(128);
            codec.encode(black_box(msg.clone()), &mut buf).unwrap();
            buf
        });
    });
}

fn bench_roundtrip(c: &mut Criterion) {
    let msg = Message::with_prefix(
        Prefix::Server("irc.example.com".to_owned()),
        Command::Privmsg,
        vec!["#channel".to_owned(), "Hello, world!".to_owned()],
    );
    c.bench_function("codec_roundtrip_privmsg", |b| {
        b.iter(|| {
            let mut codec = PircCodec::new();
            let mut buf = BytesMut::with_capacity(128);
            codec.encode(msg.clone(), &mut buf).unwrap();
            codec.decode(black_box(&mut buf)).unwrap().unwrap()
        });
    });
}

fn bench_decode_multiple_messages(c: &mut Criterion) {
    c.bench_function("codec_decode_batch_10", |b| {
        b.iter(|| {
            let mut codec = PircCodec::new();
            let mut buf = BytesMut::new();
            for _ in 0..10 {
                buf.extend_from_slice(b"PING server\r\n");
            }
            let mut count = 0;
            while let Ok(Some(_)) = codec.decode(black_box(&mut buf)) {
                count += 1;
            }
            count
        });
    });
}

fn bench_encode_quit_no_params(c: &mut Criterion) {
    let msg = Message::new(Command::Quit, vec![]);
    c.bench_function("codec_encode_quit", |b| {
        b.iter(|| {
            let mut codec = PircCodec::new();
            let mut buf = BytesMut::with_capacity(16);
            codec.encode(black_box(msg.clone()), &mut buf).unwrap();
            buf
        });
    });
}

criterion_group!(
    benches,
    bench_decode_simple,
    bench_decode_privmsg_with_prefix,
    bench_encode_simple,
    bench_encode_privmsg_with_prefix,
    bench_roundtrip,
    bench_decode_multiple_messages,
    bench_encode_quit_no_params,
);
criterion_main!(benches);
