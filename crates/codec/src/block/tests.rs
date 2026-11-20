use super::*;
use crate::{DecodeOptions, EncodeOptions, EncodedBlock, decode, frame::EncodeFrame};

fn dictionary(content: Vec<u8>) -> Dictionary {
    Dictionary {
        content,
        tree: None,
        id: 0,
        match_seed: Default::default(),
    }
}

// Use explicit tokens so encoder match choices cannot mask a decoder edge case.
fn single_match(kind: u8, literals: &[u8], len: usize, offset: u16) -> Vec<u8> {
    match_payload(kind, 2, literals, literals.len(), len, offset)
}

fn match_payload(kind: u8, width: usize, literals: &[u8], ll: usize, len: usize, offset: u16) -> Vec<u8> {
    let mask = if kind == 2 { 255 } else { 15 };
    let ml = len - 5;
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&(literals.len() as u32).to_le_bytes());
    payload.extend_from_slice(&[0, 0, 0, u8::from(width == 1)]);
    payload.extend_from_slice(literals);
    if kind == 2 {
        payload.extend_from_slice(&offset.to_le_bytes());
        payload.extend_from_slice(&[ml.min(mask) as u8, ll.min(mask) as u8]);
    } else {
        payload.push(((ll.min(mask) as u8) << 4) | ml.min(mask) as u8);
        payload.extend_from_slice(&offset.to_le_bytes()[..width]);
    }
    for extra in [ll, ml].into_iter().filter_map(|n| n.checked_sub(mask)) {
        match extra {
            0..128 => payload.push(extra as u8),
            128..16384 => payload.extend_from_slice(&[128 | (extra & 63) as u8, (extra >> 6) as u8]),
            16384..=2097151 => {
                payload.extend_from_slice(&[192 | (extra & 31) as u8, (extra >> 5) as u8, (extra >> 13) as u8]);
            }
            _ => panic!("length is out of range"),
        }
    }
    payload.resize(payload.len() + 32, 0);
    payload
}

#[test]
fn reconstruct_matches_bytewise_history() {
    let dict = dictionary((0..256).map(|n| (n * 31) as u8).collect());
    let literals: Vec<u8> = (0..280).map(|n| (n * 13) as u8).collect();
    for (kind, width) in [(1, 1), (1, 2), (2, 2)] {
        for ll in [0, 1, 4, 8, 9, 15, 16, 255, 256] {
            for ml in [5, 7, 8, 9, 15, 16, 17, 19, 20, 21, 259, 260, 261] {
                for distance in [1, 4, 5, 7, 8, 9, 15, 16, 17, 255, 256] {
                    for tail in [0, 19] {
                        let lit = &literals[..ll + tail];
                        let payload = match_payload(kind, width, lit, ll, ml, (distance - 1) as u16);
                        let mut expected = dict.content.clone();
                        expected.extend_from_slice(&lit[..ll]);
                        for _ in 0..ml {
                            expected.push(expected[expected.len() - distance]);
                        }
                        expected.extend_from_slice(&lit[ll..]);
                        let plain = &expected[dict.content.len()..];
                        // Exercise exact scratch bounds and a reused larger buffer
                        // whose initialized but unwritten bytes are not history.
                        for mut out in [Vec::new(), vec![0xA5; 1024]] {
                            assert_eq!(
                                unpack(kind, &payload, plain.len(), Some(&dict), &mut out).unwrap(),
                                plain
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn malformed_lengths_and_offsets_cannot_use_scratch() {
    let mut out = vec![0xA5; 8192];
    for (kind, width) in [(1, 1), (1, 2), (2, 2)] {
        for (ll, ml, offset, limit) in [(400, 5, 0, 4096), (1, 4096, 0, 4096), (1, 5, 1, 4096)] {
            let payload = match_payload(kind, width, b"X", ll, ml, offset);
            assert_eq!(
                unpack(kind, &payload, limit, None, &mut out).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        // Reject the oversized run before trying to slice a smaller output.
        let mut fresh = Vec::new();
        let mut payload = match_payload(kind, width, b"X", 1 << 20, 5, 0);
        assert_eq!(
            unpack(kind, &payload, 4096, None, &mut fresh).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let extra = payload.len() - 32 - 3;
        payload[extra] = 0xE0;
        assert_eq!(
            unpack(kind, &payload, 4096, None, &mut out).unwrap_err().to_string(),
            "invalid length prefix"
        );
    }
}

#[test]
fn owned_block_does_not_transfer_stale_scratch_bytes() {
    let mut file_header = [0; crate::wire::FRAME_HEADER_SIZE];
    file_header[..4].copy_from_slice(&crate::wire::MAGIC);
    file_header[4..6].copy_from_slice(&[9, 12]);
    let sum = crate::wire::header_sum(&file_header);
    file_header[14..].copy_from_slice(&sum.to_le_bytes());
    let frame = crate::wire::FrameHeader::parse(file_header).unwrap();
    for kind in [1, 2] {
        let mut decoder = BlockDecoder { out: vec![0xA5; 512] };
        let body = single_match(kind, b"Q", 8, 0);
        let header = DataBlockHeader::parse(crate::wire::block_header(kind, body.len() as u32), frame).unwrap();
        let block = decoder.decode_owned(header, body, None, 0).unwrap();
        assert_eq!(block.as_verified().bytes, b"QQQQQQQQQ");
        assert_eq!(block.into_buffer(), b"QQQQQQQQQ");
    }
}

#[test]
fn dictionary_match_crosses_into_overlapping_output() {
    let dict = dictionary(b"ABCDE".to_vec());
    let mut out = Vec::new();
    for kind in [1, 2] {
        for (literals, len, distance, expected) in [
            (&b"XY"[..], 10, 4, &b"XYDEXYDEXYDE"[..]),
            (&b""[..], 9, 2, &b"DEDEDEDED"[..]),
        ] {
            let payload = single_match(kind, literals, len, distance - 1);
            assert_eq!(unpack(kind, &payload, 4096, Some(&dict), &mut out).unwrap(), expected);
        }
    }
}

#[test]
fn maximum_match_distance_requires_enough_history() {
    let mut history = vec![0; 65535];
    history[..5].copy_from_slice(b"ABCDE");
    let dict = dictionary(history);
    let mut out = Vec::new();
    for kind in [1, 2] {
        let payload = single_match(kind, b"X", 5, u16::MAX);
        assert_eq!(unpack(kind, &payload, 4096, Some(&dict), &mut out).unwrap(), b"XABCDE");
        let payload = single_match(kind, b"", 5, u16::MAX);
        let err = unpack(kind, &payload, 4096, Some(&dict), &mut out).unwrap_err();
        assert_eq!(err.to_string(), "match before start of history");
    }
}

#[test]
fn dictionary_match_respects_block_budget() {
    let dict = dictionary(b"A".to_vec());
    let mut out = Vec::new();
    for kind in [1, 2] {
        let payload = single_match(kind, b"", 4096, 0);
        assert_eq!(
            unpack(kind, &payload, 4096, Some(&dict), &mut out).unwrap(),
            vec![b'A'; 4096]
        );
        let payload = single_match(kind, b"", 4097, 0);
        let err = unpack(kind, &payload, 4096, Some(&dict), &mut out).unwrap_err();
        assert_eq!(err.to_string(), "match exceeds block size");
    }
}

#[test]
fn dictionary_block_is_verified_and_bounded_before_write() {
    let dict = dictionary(b"ABCDE".to_vec());
    let plain = b"XYDEXYDEXYDE";
    let dec = DecodeOptions {
        dictionaries: std::slice::from_ref(&dict),
        max_output: Some(plain.len() as u64),
    };
    for kind in [1, 2] {
        let payload = single_match(kind, b"XY", 10, 3);
        let trailer = crate::wire::FRAME_HEADER_SIZE + crate::wire::BLOCK_HEADER_SIZE + payload.len();
        let mut archive = Vec::new();
        let mut sizes = Vec::new();
        let mut frame = EncodeFrame::start(
            &mut archive,
            EncodeOptions {
                dictionary: Some(&dict),
                block_log: 12,
                ..Default::default()
            },
            &mut sizes,
        )
        .unwrap();
        let block = EncodedBlock {
            kind,
            payload,
            plain_len: plain.len(),
            checksum: Some(checksum(plain, 0)),
        };
        frame.commit(&mut archive, &block).unwrap();
        frame.finish(&mut archive).unwrap();

        let mut out = Vec::new();
        decode(&archive[..], &mut out, dec).unwrap();
        assert_eq!(out, plain);
        out.clear();
        let bounded = DecodeOptions {
            max_output: Some(plain.len() as u64 - 1),
            ..dec
        };
        assert!(decode(&archive[..], &mut out, bounded).is_err());
        assert!(out.is_empty());

        archive[trailer] ^= 1;
        let err = decode(&archive[..], &mut out, dec).unwrap_err();
        assert_eq!(err.to_string(), "block checksum mismatch");
        assert!(out.is_empty());
    }
}
