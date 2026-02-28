//! Framed message codec for reading/writing IRC messages over TCP.
//!
//! [`PircCodec`] implements [`tokio_util::codec::Decoder`] and
//! [`tokio_util::codec::Encoder`] to convert between raw bytes on a TCP
//! stream and parsed [`pirc_protocol::Message`] values.
//!
//! Messages are `\r\n`-delimited and capped at 512 bytes per RFC 2812.

use bytes::{Buf, BufMut, BytesMut};
use pirc_protocol::parser::MAX_MESSAGE_LEN;
use pirc_protocol::Message;
use tokio_util::codec::{Decoder, Encoder};

use crate::error::NetworkError;

/// A codec that frames IRC messages delimited by `\r\n`.
///
/// On decode, bytes are buffered until a complete `\r\n`-terminated line is
/// found, then parsed via [`pirc_protocol::parse`]. On encode, the message is
/// serialized to wire format and `\r\n` is appended.
#[derive(Debug, Default)]
pub struct PircCodec;

impl PircCodec {
    pub fn new() -> Self {
        Self
    }
}

impl Decoder for PircCodec {
    type Item = Message;
    type Error = NetworkError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Search for the \r\n delimiter
        let Some(crlf_pos) = src.windows(2).position(|w| w == b"\r\n") else {
            // No complete line yet. Check if buffered data already
            // exceeds the maximum message size — if so, the message is
            // oversized and will never be valid.
            if src.len() > MAX_MESSAGE_LEN {
                return Err(NetworkError::Protocol(
                    pirc_protocol::ProtocolError::MessageTooLong {
                        length: src.len(),
                        max: MAX_MESSAGE_LEN,
                    },
                ));
            }
            return Ok(None);
        };

        // The full frame includes the \r\n (2 bytes)
        let frame_len = crlf_pos + 2;

        // Check max message size (including \r\n)
        if frame_len > MAX_MESSAGE_LEN {
            // Consume the oversized frame so the decoder can continue
            src.advance(frame_len);
            return Err(NetworkError::Protocol(
                pirc_protocol::ProtocolError::MessageTooLong {
                    length: frame_len,
                    max: MAX_MESSAGE_LEN,
                },
            ));
        }

        // Extract the frame bytes and advance the buffer
        let frame = src.split_to(frame_len);

        // Convert to a string for the parser
        let line = std::str::from_utf8(&frame).map_err(|e| {
            NetworkError::Protocol(pirc_protocol::ProtocolError::UnknownCommand(format!(
                "invalid UTF-8: {e}"
            )))
        })?;

        // Parse the line (pirc_protocol::parse handles stripping \r\n)
        let msg = pirc_protocol::parse(line)?;
        Ok(Some(msg))
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // On EOF, try to decode any remaining complete message first.
        match self.decode(buf)? {
            Some(msg) => Ok(Some(msg)),
            None => {
                // Discard any remaining incomplete data — a partial message
                // at EOF is not recoverable.
                if !buf.is_empty() {
                    buf.clear();
                }
                Ok(None)
            }
        }
    }
}

impl Encoder<Message> for PircCodec {
    type Error = NetworkError;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let wire = item.to_string();
        let total_len = wire.len() + 2; // +2 for \r\n

        if total_len > MAX_MESSAGE_LEN {
            return Err(NetworkError::Protocol(
                pirc_protocol::ProtocolError::MessageTooLong {
                    length: total_len,
                    max: MAX_MESSAGE_LEN,
                },
            ));
        }

        dst.reserve(total_len);
        dst.put(wire.as_bytes());
        dst.put(&b"\r\n"[..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use pirc_protocol::{Command, Message, Prefix};

    #[test]
    fn round_trip_simple_message() {
        let mut codec = PircCodec::new();
        let msg = Message::new(Command::Ping, vec!["irc.example.com".to_owned()]);

        // Encode
        let mut buf = BytesMut::new();
        codec.encode(msg.clone(), &mut buf).unwrap();
        assert_eq!(&buf[..], b"PING irc.example.com\r\n");

        // Decode
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded, msg);
        assert!(buf.is_empty());
    }

    #[test]
    fn round_trip_message_with_prefix() {
        let mut codec = PircCodec::new();
        let msg = Message::with_prefix(
            Prefix::Server("irc.example.com".to_owned()),
            Command::Privmsg,
            vec!["#channel".to_owned(), "Hello, world!".to_owned()],
        );

        let mut buf = BytesMut::new();
        codec.encode(msg.clone(), &mut buf).unwrap();
        assert_eq!(
            &buf[..],
            b":irc.example.com PRIVMSG #channel :Hello, world!\r\n"
        );

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn partial_read_buffers_until_crlf() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();

        // Feed partial data (no \r\n yet)
        buf.extend_from_slice(b"PING irc.example");
        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert_eq!(buf.len(), 16); // data is retained

        // Feed the rest
        buf.extend_from_slice(b".com\r\n");
        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg.command, Command::Ping);
        assert_eq!(msg.params, vec!["irc.example.com"]);
        assert!(buf.is_empty());
    }

    #[test]
    fn oversized_message_rejected_on_decode() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();

        // Create a line that exceeds 512 bytes (including \r\n)
        let long_param = "x".repeat(500);
        let line = format!("PRIVMSG #channel :{long_param}\r\n");
        assert!(line.len() > MAX_MESSAGE_LEN);

        buf.extend_from_slice(line.as_bytes());
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            NetworkError::Protocol(pirc_protocol::ProtocolError::MessageTooLong { .. })
        ));
    }

    #[test]
    fn oversized_message_rejected_before_crlf() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();

        // Feed more than 512 bytes without any \r\n
        let data = "x".repeat(MAX_MESSAGE_LEN + 1);
        buf.extend_from_slice(data.as_bytes());
        let err = codec.decode(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            NetworkError::Protocol(pirc_protocol::ProtocolError::MessageTooLong { .. })
        ));
    }

    #[test]
    fn oversized_message_rejected_on_encode() {
        let mut codec = PircCodec::new();
        let long_param = "x".repeat(500);
        let msg = Message::new(Command::Privmsg, vec!["#channel".to_owned(), long_param]);

        let mut buf = BytesMut::new();
        let err = codec.encode(msg, &mut buf).unwrap_err();
        assert!(matches!(
            err,
            NetworkError::Protocol(pirc_protocol::ProtocolError::MessageTooLong { .. })
        ));
    }

    #[test]
    fn multiple_messages_in_buffer() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();

        buf.extend_from_slice(b"PING server1\r\nPONG server2\r\n");

        let msg1 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg1.command, Command::Ping);
        assert_eq!(msg1.params, vec!["server1"]);

        let msg2 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg2.command, Command::Pong);
        assert_eq!(msg2.params, vec!["server2"]);

        // No more messages
        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert!(buf.is_empty());
    }

    #[test]
    fn empty_buffer_returns_none() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn encode_message_no_params() {
        let mut codec = PircCodec::new();
        let msg = Message::new(Command::Quit, vec![]);

        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();
        assert_eq!(&buf[..], b"QUIT\r\n");
    }

    #[test]
    fn decode_eof_with_complete_message() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();
        buf.extend_from_slice(b"PING server1\r\n");

        let msg = codec.decode_eof(&mut buf).unwrap().unwrap();
        assert_eq!(msg.command, Command::Ping);
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_eof_discards_partial_message() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();
        buf.extend_from_slice(b"PING server1"); // no \r\n

        let result = codec.decode_eof(&mut buf).unwrap();
        assert!(result.is_none());
        assert!(buf.is_empty()); // partial data discarded
    }

    #[test]
    fn decode_eof_empty_buffer() {
        let mut codec = PircCodec::new();
        let mut buf = BytesMut::new();

        let result = codec.decode_eof(&mut buf).unwrap();
        assert!(result.is_none());
    }
}
