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
    let mask = if kind == 2 { 255 } else { 15 };
    assert!(literals.len() < mask);
    let ml = len - 5;
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&(literals.len() as u32).to_le_bytes());
    payload.extend_from_slice(&[0; 4]);
    payload.extend_from_slice(literals);
    if kind == 2 {
        payload.extend_from_slice(&offset.to_le_bytes());
        payload.extend_from_slice(&[ml.min(mask) as u8, literals.len() as u8]);
    } else {
        payload.push(((literals.len() as u8) << 4) | ml.min(mask) as u8);
        payload.extend_from_slice(&offset.to_le_bytes());
    }
    if ml >= mask {
        let extra = ml - mask;
        assert!(extra < 16384);
        if extra < 128 {
            payload.push(extra as u8);
        } else {
            payload.extend_from_slice(&[128 | (extra & 63) as u8, (extra >> 6) as u8]);
        }
    }
    payload.resize(payload.len() + 32, 0);
    payload
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
