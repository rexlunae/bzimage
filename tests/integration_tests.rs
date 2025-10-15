use bzimage::{BzImageHeader, MAGIC, VERSION};
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Seek, SeekFrom, Write, Read};

#[test]
fn round_trip_write_read_validate_decompress() {
    // prepare payload
    let payload = b"hello daemonizer world".to_vec();
    let uncompressed_size = payload.len() as u64;

    // compress
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&payload).unwrap();
    let compressed = enc.finish().unwrap();
    let compressed_size = compressed.len() as u64;

    // compute checksum
    let mut hasher = Sha256::new();
    hasher.update(&compressed);
    let checksum: [u8; 32] = hasher.finalize().into();

    // build header (endian-typed fields)
    let header = BzImageHeader {
        magic: *MAGIC,
        version: VERSION.into(),
        reserved1: 0u32.into(),
        uncompressed_size: (uncompressed_size).into(),
        compressed_size: (compressed_size).into(),
        checksum,
        reserved2: 0u32.into(),
    };

    // write header + compressed data into cursor
    let mut cur = Cursor::new(Vec::new());
    header.write_to(&mut cur).unwrap();
    cur.write_all(&compressed).unwrap();

    // rewind and read
    cur.seek(SeekFrom::Start(0)).unwrap();
    let read_header = BzImageHeader::read_from(&mut cur).unwrap();

    // validate magic & sizes via safe copies and endian conversion
    assert_eq!(&read_header.magic_copy(), MAGIC);
    // Copy endian-typed fields out of the packed struct safely.
    let uncompressed_field: simple_endian::u64le = unsafe {
        std::ptr::read_unaligned(std::ptr::addr_of!(read_header.uncompressed_size))
    };
    let compressed_field: simple_endian::u64le = unsafe {
        std::ptr::read_unaligned(std::ptr::addr_of!(read_header.compressed_size))
    };
    let un: u64 = uncompressed_field.into();
    let comp: u64 = compressed_field.into();
    assert_eq!(un, uncompressed_size);
    assert_eq!(comp, compressed_size);

    // read compressed bytes
    let mut compressed_read = Vec::new();
    cur.read_to_end(&mut compressed_read).unwrap();
    assert_eq!(compressed_read.len(), compressed_size as usize);

    // checksum
    assert!(read_header.validate_checksum(&compressed_read));

    // decompression via helper
    let decompressed = BzImageHeader::decompress_data(&compressed_read).unwrap();
    assert_eq!(decompressed, payload);
}

#[test]
fn invalid_magic_fails() {
    use std::io::Write;
    let mut cur = Cursor::new(Vec::new());
    cur.write_all(b"BAD!").unwrap();
    // pad out to header size
    cur.write_all(&vec![0u8; 60]).unwrap();
    cur.seek(SeekFrom::Start(0)).unwrap();
    let res = BzImageHeader::read_from(&mut cur);
    assert!(res.is_err());
}

#[test]
fn checksum_mismatch_detected() {
    // create a correct header but corrupt compressed data
    let payload = b"somedata".to_vec();
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&payload).unwrap();
    let compressed = enc.finish().unwrap();

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

    // write header + corrupted compressed data
    let mut cur = Cursor::new(Vec::new());
    header.write_to(&mut cur).unwrap();
    let mut corrupted = compressed.clone();
    if !corrupted.is_empty() { corrupted[0] ^= 0xff; }
    cur.write_all(&corrupted).unwrap();
    cur.seek(SeekFrom::Start(0)).unwrap();

    let read_header = BzImageHeader::read_from(&mut cur).unwrap();
    // read compressed bytes
    let mut compressed_read = Vec::new();
    cur.read_to_end(&mut compressed_read).unwrap();

    assert!(!read_header.validate_checksum(&compressed_read));
}

#[test]
fn header_write_size() {
    // ensure write_to writes exactly HEADER_SIZE bytes
    let header = BzImageHeader {
        magic: *MAGIC,
        version: VERSION.into(),
        reserved1: 0u32.into(),
        uncompressed_size: 0u64.into(),
        compressed_size: 0u64.into(),
        checksum: [0u8; 32],
        reserved2: 0u32.into(),
    };

    let mut cur = Cursor::new(Vec::new());
    header.write_to(&mut cur).unwrap();
    assert_eq!(cur.get_ref().len(), bzimage::HEADER_SIZE);
}

#[test]
fn truncated_header_fails() {
    // provide only the magic and partial header and ensure read_from errors
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&[0u8; 10]);
    let mut cur = Cursor::new(buf);
    let r = BzImageHeader::read_from(&mut cur);
    assert!(r.is_err());
}
