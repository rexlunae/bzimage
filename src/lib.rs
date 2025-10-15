use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use simple_endian::{u32le, u64le, read_specific};
use std::io::{Read, Seek, Write};

/// Four-byte ASCII magic that identifies a bzimage header on disk: `DMNZ`.
pub const MAGIC: &[u8; 4] = b"DMNZ";

/// Current on-disk format version.
pub const VERSION: u32 = 1;

/// The header size in bytes (the packed header is 64 bytes).
pub const HEADER_SIZE: usize = 64;

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct BzImageHeader {
    pub magic: [u8; 4],
    pub version: u32le,
    pub reserved1: u32le,
    pub uncompressed_size: u64le,
    pub compressed_size: u64le,
    pub checksum: [u8; 32],
    pub reserved2: u32le,
}

impl BzImageHeader {
    /// API contract (inputs/outputs and errors)
    ///
    /// - Inputs: read/write operate on types implementing `Read`/`Write` (and `Seek` for read helpers).
    /// - Outputs: `write_to` writes exactly `HEADER_SIZE` bytes; `read_from` returns a header with
    ///   endian-typed integer wrappers so callers can convert to native integers via `Into`.
    /// - Error modes: IO errors, invalid magic, truncated header, or decompression failures.
    ///
    /// Safety: callers should avoid taking references into the packed struct; helper accessors
    /// like `magic_copy` and `checksum_copy` are provided to safely access those byte fields.

    /// Top-level documentation
    ///
    /// `BzImageHeader` describes the 64-byte packed on-disk header used by the `bzimage` crate.
    /// The header stores a magic, version, sizes and a SHA-256 checksum of the compressed payload.
    /// Use `write_to` to write the header into a writer and `read_from` to parse it back from a
    /// reader. To read both header and payload, use `read_header_and_payload` which returns the
    /// header and the compressed payload bytes.

    pub fn size() -> usize {
        std::mem::size_of::<BzImageHeader>()
    }

    pub fn write_to<W: Write>(&self, mut w: W) -> Result<()> {
        // Write the struct as bytes
        let bytes = unsafe {
            std::slice::from_raw_parts(self as *const BzImageHeader as *const u8, Self::size())
        };
        w.write_all(bytes).context("writing header bytes")?;
        Ok(())
    }

    /// Read a header from the reader and return the endian-typed `BzImageHeader`.
    /// Callers should use the provided accessor methods to get native values.
    pub fn read_from<R: Read + Seek>(mut r: R) -> Result<BzImageHeader> {
        // Read fields individually using read_specific to avoid taking references into packed struct
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic).context("reading magic")?;

        if &magic != MAGIC {
            anyhow::bail!("invalid magic");
        }

    let version: u32le = read_specific(&mut r).context("reading version")?;
    let reserved1: u32le = read_specific(&mut r).context("reading reserved1")?;
    let uncompressed_size: u64le = read_specific(&mut r).context("reading uncompressed_size")?;
    let compressed_size: u64le = read_specific(&mut r).context("reading compressed_size")?;

        let mut checksum = [0u8; 32];
        r.read_exact(&mut checksum).context("reading checksum")?;

    let reserved2: u32le = read_specific(&mut r).context("reading reserved2")?;

        // Build endian-typed packed header to return. Callers should use accessors.
        Ok(BzImageHeader {
            magic,
            version: version,
            reserved1: reserved1,
            uncompressed_size: uncompressed_size,
            compressed_size: compressed_size,
            checksum,
            reserved2: reserved2,
        })
    }
    
    /// Return a copy of the 4-byte magic.
    pub fn magic_copy(&self) -> [u8; 4] {
        let mut out = [0u8; 4];
        unsafe {
            std::ptr::copy_nonoverlapping(
                std::ptr::addr_of!(self.magic) as *const u8,
                out.as_mut_ptr(),
                4,
            );
        }
        out
    }

    /// Return a copy of the checksum bytes.
    pub fn checksum_copy(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        unsafe {
            std::ptr::copy_nonoverlapping(
                std::ptr::addr_of!(self.checksum) as *const u8,
                out.as_mut_ptr(),
                32,
            );
        }
        out
    }

    pub fn validate_checksum(&self, compressed_data: &[u8]) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(compressed_data);
        let actual: [u8; 32] = hasher.finalize().into();
        actual == self.checksum_copy()
    }

    pub fn decompress_data(compressed: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = GzDecoder::new(compressed);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .context("decompressing gzip data")?;
        Ok(out)
    }
    
    /// Read a header and the following compressed payload from `r`.
    /// Returns the header and the compressed bytes as a Vec<u8>.
    pub fn read_header_and_payload<R: Read + Seek>(mut r: R) -> Result<(BzImageHeader, Vec<u8>)> {
        // Read header
        let header = Self::read_from(&mut r).context("reading header")?;

        // Extract compressed size safely from endian-typed field.
    let compressed_field: u64le = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!(header.compressed_size)) };
    let compressed_size_u64: u64 = compressed_field.into();
    let compressed_size: usize = compressed_size_u64 as usize;

        // read the compressed payload. `read_exact` will error if there are fewer bytes than stated.
        let mut compressed = vec![0u8; compressed_size];
        r.read_exact(&mut compressed).context("reading compressed payload")?;

        Ok((header, compressed))
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Seek, SeekFrom, Write};

    #[test]
    fn write_header_and_payload_roundtrip() {
        let payload = b"unit test payload".to_vec();

        // compress
        let mut enc = GzEncoder::new(Vec::new(), Compression::best());
        enc.write_all(&payload).unwrap();
        let compressed = enc.finish().unwrap();

        // checksum
        let mut hasher = Sha256::new();
        hasher.update(&compressed);
        let checksum: [u8; 32] = hasher.finalize().into();

        let header = BzImageHeader {
            magic: *MAGIC,
            version: VERSION.into(),
            reserved1: 0u32.into(),
            uncompressed_size: (payload.len() as u64).into(),
            compressed_size: (compressed.len() as u64).into(),
            checksum,
            reserved2: 0u32.into(),
        };

        let mut buf = Cursor::new(Vec::new());
        // write header + payload using existing APIs
        header.write_to(&mut buf).unwrap();
        buf.write_all(&compressed).unwrap();

        // rewind and use helper to read both
        buf.seek(SeekFrom::Start(0)).unwrap();
        let (read_header, read_compressed) = BzImageHeader::read_header_and_payload(&mut buf).unwrap();

        assert_eq!(&read_header.magic_copy(), MAGIC);
        assert_eq!(read_compressed.len(), compressed.len());
        assert!(read_header.validate_checksum(&read_compressed));
        let decompressed = BzImageHeader::decompress_data(&read_compressed).unwrap();
        assert_eq!(decompressed, payload);
    }
}

