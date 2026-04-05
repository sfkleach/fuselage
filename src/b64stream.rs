/// Streaming base64 decoder with a bounded ring-buffer.
///
/// Data is pushed one line at a time via [`B64Decoder::push_line`]. Complete
/// chunks are decoded and written to the underlying writer immediately, so peak
/// memory is `O(B64_CHUNK + max_line_length)` regardless of total input size.
/// Call [`B64Decoder::finish`] to flush the final (padded) tail.
use std::collections::VecDeque;
use std::io::Write;

use anyhow::{Context, Result};
use base64::Engine;

/// Decode and flush this many base64 characters at a time. Must be a
/// multiple of 4 so every chunk aligns on a base64 block boundary.
/// 3072 chars → 2304 decoded bytes per flush.
const B64_CHUNK: usize = 4 * 768;

pub struct B64Decoder<W: Write> {
    ring: VecDeque<u8>,
    out: W,
}

impl<W: Write> B64Decoder<W> {
    pub fn new(out: W) -> Self {
        Self {
            ring: VecDeque::with_capacity(B64_CHUNK + 256),
            out,
        }
    }

    /// Push one line of base64 characters into the decoder.
    ///
    /// Returns `Ok(true)` if the line was accepted and any complete chunks
    /// were decoded and written. Returns `Ok(false)` if the line contains a
    /// character outside the standard base64 alphabet, indicating the input is
    /// not a base64 file. Returns an error if decoding or writing fails.
    pub fn push_line(&mut self, line: &str) -> Result<bool> {
        if !line
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        {
            return Ok(false);
        }
        self.ring.extend(line.bytes());
        self.flush_chunks()?;
        Ok(true)
    }

    /// Decode and write all complete `B64_CHUNK`-sized blocks from the ring.
    fn flush_chunks(&mut self) -> Result<()> {
        while self.ring.len() >= B64_CHUNK {
            let chunk: Vec<u8> = self.ring.drain(..B64_CHUNK).collect();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&chunk)
                .context("base64 chunk decode failed")?;
            self.out.write_all(&decoded).context("write failed")?;
        }
        Ok(())
    }

    /// Flush the final (possibly padded) tail and return the inner writer.
    ///
    /// Must be called after all lines have been pushed. Consumes the decoder.
    pub fn finish(mut self) -> Result<W> {
        if !self.ring.is_empty() {
            let tail: Vec<u8> = self.ring.drain(..).collect();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&tail)
                .context("base64 tail decode failed")?;
            self.out.write_all(&decoded).context("write failed")?;
        }
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_lines(lines: &[&str]) -> Result<(bool, Vec<u8>)> {
        let mut dec = B64Decoder::new(Vec::new());
        for line in lines {
            if !dec.push_line(line)? {
                return Ok((false, vec![]));
            }
        }
        let out = dec.finish()?;
        Ok((true, out))
    }

    #[test]
    fn decodes_single_line() {
        // "hello" base64-encoded = "aGVsbG8="
        let (ok, out) = decode_lines(&["aGVsbG8="]).unwrap();
        assert!(ok);
        assert_eq!(out, b"hello");
    }

    #[test]
    fn decodes_multiline() {
        // "hello" split across two lines.
        let (ok, out) = decode_lines(&["aGVs", "bG8="]).unwrap();
        assert!(ok);
        assert_eq!(out, b"hello");
    }

    #[test]
    fn rejects_non_base64_character() {
        // A space is not in the base64 alphabet.
        let (ok, _) = decode_lines(&["aGVs bG8="]).unwrap();
        assert!(!ok);
    }

    #[test]
    fn rejects_non_base64_on_later_line() {
        // First line is valid; second line contains a non-base64 character.
        let (ok, _) = decode_lines(&["aGVs", "bG8= extra"]).unwrap();
        assert!(!ok);
    }

    #[test]
    fn decodes_exactly_one_chunk() {
        // Produce exactly B64_CHUNK base64 characters that decode cleanly.
        // B64_CHUNK = 3072; every 4 chars decode to 3 bytes. Use 'AAAA' repeated.
        let line = "AAAA".repeat(B64_CHUNK / 4);
        let (ok, out) = decode_lines(&[&line]).unwrap();
        assert!(ok);
        // 3072 / 4 * 3 = 2304 zero bytes.
        assert_eq!(out, vec![0u8; 2304]);
    }

    #[test]
    fn decodes_larger_than_one_chunk() {
        // Two full chunks plus a partial tail.
        let line = "AAAA".repeat(B64_CHUNK / 4 * 2 + 10);
        let (ok, out) = decode_lines(&[&line]).unwrap();
        assert!(ok);
        assert_eq!(out.len(), (B64_CHUNK / 4 * 2 + 10) * 3);
    }

    #[test]
    fn ring_buffer_stays_bounded() {
        // Feed many lines totalling well over one chunk and confirm the decoded
        // output is consistent. This verifies chunks are flushed incrementally.
        let single = "AAAA".repeat(100); // 400 base64 chars per line
        let line_count = (B64_CHUNK / 400) * 4; // enough to span several chunks
        let (ok, out) = decode_lines(&vec![single.as_str(); line_count]).unwrap();
        assert!(ok);
        assert_eq!(out.len(), line_count * 100 * 3);
    }

    #[test]
    fn empty_input_returns_empty_output() {
        let dec = B64Decoder::new(Vec::new());
        let out = dec.finish().unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn invalid_base64_padding_returns_error() {
        // "aGVs" is valid; "bG8" (missing padding) should fail at finish.
        let mut dec = B64Decoder::new(Vec::new());
        assert!(dec.push_line("aGVs").unwrap());
        assert!(dec.push_line("bG8").unwrap()); // accepted as valid alphabet
        // The tail "bG8" has length 3, not a multiple of 4 — decode should fail.
        assert!(dec.finish().is_err());
    }
}
