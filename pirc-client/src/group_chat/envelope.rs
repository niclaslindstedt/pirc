//! Message envelope with ordering metadata for group chat messages.

/// A plaintext envelope wrapping the user's message with ordering metadata.
///
/// Wire format:
/// ```text
/// [8 bytes sequence_number (big-endian)]
/// [8 bytes timestamp_ms (big-endian)]
/// [remaining bytes: plaintext]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEnvelope {
    /// Per-sender monotonic sequence number.
    pub sequence_number: u64,
    /// Unix timestamp in milliseconds when the message was created.
    pub timestamp_ms: u64,
    /// The actual message content.
    pub plaintext: Vec<u8>,
}

/// Size of the envelope header (sequence + timestamp).
pub(crate) const ENVELOPE_HEADER_SIZE: usize = 16;

impl MessageEnvelope {
    /// Serialize the envelope to bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(ENVELOPE_HEADER_SIZE + self.plaintext.len());
        buf.extend_from_slice(&self.sequence_number.to_be_bytes());
        buf.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        buf.extend_from_slice(&self.plaintext);
        buf
    }

    /// Deserialize an envelope from bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the data is too short to contain the header.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < ENVELOPE_HEADER_SIZE {
            return Err(format!(
                "envelope too short: expected at least {ENVELOPE_HEADER_SIZE} bytes, got {}",
                bytes.len()
            ));
        }

        let sequence_number = u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let timestamp_ms = u64::from_be_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let plaintext = bytes[ENVELOPE_HEADER_SIZE..].to_vec();

        Ok(Self {
            sequence_number,
            timestamp_ms,
            plaintext,
        })
    }
}
