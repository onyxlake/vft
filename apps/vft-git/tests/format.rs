use vft_git::{Header, ObjectFormat, DEFAULT_MAX_PACKAGE, HEADER_LEN};
#[test]
fn independent_v1_header_vector_and_strict_fields() {
    // Python struct <HHBBHQ plus explicit byte sequences and the standard SHA256(abc)
    // digest. This vector is a header fixture, not a valid Git bundle.
    let hex = include_str!("vectors/header-v1.hex")
        .split_whitespace()
        .collect::<String>();
    let bytes: Vec<_> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    let bytes: [u8; HEADER_LEN] = bytes.try_into().unwrap();
    let header = Header {
        format: ObjectFormat::Sha1,
        base: "000102030405060708090a0b0c0d0e0f10111213".into(),
        target: "1415161718191a1b1c1d1e1f2021222324252627".into(),
        bundle_len: 3,
        bundle_sha256: [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ],
    };
    assert_eq!(header.encode().unwrap(), bytes);
    assert_eq!(
        Header::decode(&bytes, 131, DEFAULT_MAX_PACKAGE).unwrap(),
        header
    );
    for i in [0, 8, 9, 10, 11, 12, 13, 14, 15, 44, 55, 76, 87, 120, 127] {
        let mut bad = bytes;
        bad[i] = 0xff;
        assert!(
            Header::decode(&bad, 131, DEFAULT_MAX_PACKAGE).is_err(),
            "offset {i}"
        );
    }
    for len in [0, 128, 130, 132, u64::MAX] {
        assert!(Header::decode(&bytes, len, u64::MAX).is_err());
    }
    let mut overflow = bytes;
    overflow[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(Header::decode(&overflow, u64::MAX, u64::MAX).is_err());
    let mut no_change = header.clone();
    no_change.target = no_change.base.clone();
    assert!(no_change.encode().is_err());
    let mut zero = header;
    zero.base = "0".repeat(40);
    assert!(zero.encode().is_err());
}
