//! Exact-size zlib decompression.
//!
//! HashFS v2 stores the index tables and many entries as zlib streams
//! (RFC 1950: 2-byte header, DEFLATE data, Adler-32 trailer). The caller
//! always knows the uncompressed size from the metadata, so the output
//! buffer is allocated once with exactly that size and the decoder can
//! never write more. A result is only returned if:
//!
//! - the zlib header is valid and the stream ends properly,
//! - the Adler-32 checksum matches,
//! - the stream produces exactly the expected number of bytes,
//! - the stream uses all stored bytes (no trailing data).

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::{
    TINFL_FLAG_PARSE_ZLIB_HEADER, TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
};
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};

/// Upper bound of the DEFLATE compression ratio (about 1032:1). A declared
/// size above `stored * MAX_RATIO` cannot be produced by a valid stream, so
/// it is rejected before any memory is allocated for it.
pub(crate) const MAX_RATIO: u64 = 1032;

/// Decompresses `input`, which must inflate to exactly `expected` bytes.
///
/// The caller bounds `expected` (limits and [`MAX_RATIO`]) before calling.
pub(crate) fn inflate_exact(
    input: &[u8],
    expected: usize,
) -> std::result::Result<Vec<u8>, &'static str> {
    let mut out = vec![0u8; expected];
    let mut decoder = Box::<DecompressorOxide>::default();
    // No TINFL_FLAG_HAS_MORE_INPUT: `input` is the complete stream.
    // Parsing the zlib header also enables the Adler-32 check.
    let flags = TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let (status, consumed, written) = decompress(&mut decoder, input, &mut out, 0, flags);
    match status {
        TINFLStatus::Done if written != expected => {
            Err("the data is shorter than its declared size")
        }
        TINFLStatus::Done if consumed != input.len() => {
            Err("the compressed stream ends before its stored size")
        }
        TINFLStatus::Done => Ok(out),
        TINFLStatus::HasMoreOutput => Err("the data is longer than its declared size"),
        TINFLStatus::NeedsMoreInput | TINFLStatus::FailedCannotMakeProgress => {
            Err("the compressed stream is truncated")
        }
        TINFLStatus::Adler32Mismatch => Err("the zlib checksum does not match"),
        _ => Err("the compressed data is not a valid zlib stream"),
    }
}

#[cfg(test)]
mod tests {
    use super::inflate_exact;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    fn sample() -> Vec<u8> {
        (0..5000u32).map(|i| (i % 97) as u8).collect()
    }

    #[test]
    fn inflates_exactly() {
        let data = sample();
        let z = compress_to_vec_zlib(&data, 6);
        assert_eq!(inflate_exact(&z, data.len()).ok(), Some(data));
    }

    #[test]
    fn inflates_empty_stream() {
        let z = compress_to_vec_zlib(b"", 6);
        assert_eq!(inflate_exact(&z, 0).ok(), Some(Vec::new()));
    }

    #[test]
    fn rejects_wrong_declared_size() {
        let data = sample();
        let z = compress_to_vec_zlib(&data, 6);
        assert!(inflate_exact(&z, data.len() - 1).is_err());
        assert!(inflate_exact(&z, data.len() + 1).is_err());
    }

    #[test]
    fn rejects_truncated_stream() {
        let data = sample();
        let z = compress_to_vec_zlib(&data, 6);
        for cut in [0, 1, 2, z.len() / 2, z.len() - 1] {
            assert!(
                inflate_exact(&z[..cut], data.len()).is_err(),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn rejects_checksum_mismatch() {
        let data = sample();
        let mut z = compress_to_vec_zlib(&data, 6);
        let last = z.len() - 1;
        z[last] ^= 0xFF;
        assert_eq!(
            inflate_exact(&z, data.len()),
            Err("the zlib checksum does not match")
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let data = sample();
        let mut z = compress_to_vec_zlib(&data, 6);
        z.push(0);
        assert!(inflate_exact(&z, data.len()).is_err());
    }

    #[test]
    fn rejects_raw_deflate_without_zlib_header() {
        let data = sample();
        let z = compress_to_vec_zlib(&data, 6);
        assert!(inflate_exact(&z[2..], data.len()).is_err());
    }
}
