# bzimage

bzimage is a small Rust crate that defines a compact on-disk container format for a
gzip-compressed payload with a SHA-256 checksum. The format is deliberately simple:

- 64-byte packed header followed by the compressed payload bytes.

Header layout (in bytes, little-endian for integer fields):

- magic: 4 bytes — the ASCII magic `DMNZ`
- version: u32 (4 bytes) — format version (currently 1)
- reserved1: u32 (4 bytes) — reserved for future use
- uncompressed_size: u64 (8 bytes) — size of the data after decompression
- compressed_size: u64 (8 bytes) — size of the following compressed data
- checksum: 32 bytes — SHA-256 of the compressed payload
- reserved2: u32 (4 bytes) — reserved for future use

Total header size: 64 bytes.

Usage
-----

The crate exposes `BzImageHeader` and helper functions to read/write headers,
validate the checksum, and decompress the payload. See the `tests/` directory for
examples of creating a header, writing it to a buffer, and validating/decompressing
the stored payload.

Example (high level):

1. Compress some bytes with gzip.
2. Compute SHA-256 of the compressed bytes and store it in the header.
3. Write the header (64 bytes) followed by the compressed bytes.
4. On read: read the header, read `compressed_size` bytes, validate checksum,
   then decompress.

See the Rust docs in `src/lib.rs` for API details and the test suite for concrete
usage examples.