use super::*;
use crate::Task;
use clap::Parser;
use std::{collections::BTreeSet, path::PathBuf};

#[test]
fn known_recipe_and_plaintext() {
    assert_eq!(GENERATOR_REV, 1);
    let case = generate(42, 10);
    assert_eq!(case.family, "concatenation");
    assert!(case.indexed);
    assert_eq!(case.dictionary, None);
    assert_eq!(
        case.frames,
        [
            Frame {
                len: 4133,
                mode: "packed",
                level: 5,
                block_log: 12,
                checksum: false,
            },
            Frame {
                len: 0,
                mode: "dense",
                level: 7,
                block_log: 13,
                checksum: true,
            },
            Frame {
                len: 16457,
                mode: "split",
                level: 1,
                block_log: 14,
                checksum: true,
            },
        ]
    );
    assert_eq!(
        case.ranges,
        [(4079, 36), (4116, 36), (16173, 80), (20500, 36), (20590, 0)]
    );
    // FNV-1a of the full plaintext locks byte order and generated content.
    let hash = case.plain.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    assert_eq!(hash, 0x86e4_9905_5ed1_c4c1);
}

#[test]
fn case_address_is_independent_of_order_and_supports_full_u64() {
    let seed = u64::MAX;
    let first = generate(seed, 8);
    for i in [9, 1, u64::MAX, 11, 0, 7] {
        generate(seed, i);
    }
    assert!(first == generate(seed, 8));
    assert!(first.plain != generate(0, 8).plain);
    assert!(first.plain != generate(seed, 8 + SHAPES).plain);
}

#[test]
fn default_schedule_covers_structures_and_stays_bounded() {
    let cases: Vec<_> = (0..DEFAULT_CASES).map(|i| generate(DEFAULT_SEED, i)).collect();
    let sizes: Vec<_> = cases[..8].iter().map(|case| case.plain.len()).collect();
    assert_eq!(sizes, [0, 1, 255, 256, 257, 4095, 4096, 4097]);
    let distant = &cases[8];
    let second = 1024 + 65_535;
    assert_eq!(distant.frames[0].block_log, 17);
    assert!(distant.plain[1024..1536] == distant.plain[second..second + 512]);
    assert!(distant.plain[LARGE_BLOCK..LARGE_BLOCK + 128] == distant.plain[3 * LARGE_BLOCK..3 * LARGE_BLOCK + 128]);
    let group = &cases[9];
    assert_eq!(group.plain.len(), 66 * BLOCK);
    assert!(group.plain[63 * BLOCK..63 * BLOCK + 2] != group.plain[GROUP_EDGE..GROUP_EDGE + 2]);
    for (case, boundary) in [
        (&cases[7], BLOCK),
        (distant, LARGE_BLOCK),
        (group, GROUP_EDGE),
        (&cases[10], 4133),
    ] {
        assert!(
            case.ranges
                .iter()
                .any(|&(start, length)| start < boundary && boundary < start + length)
        );
    }
    assert_eq!(cases[11].dictionary, Some(HTTP_DICT));
    assert!(cases[11].plain.starts_with(b"GET /api/v1/users/"));
    let frames = || cases.iter().flat_map(|case| &case.frames);
    assert_eq!(
        frames().map(|frame| frame.mode).collect::<BTreeSet<_>>(),
        BTreeSet::from(["split", "packed", "dense"])
    );
    assert_eq!(
        frames().map(|frame| frame.level).collect::<BTreeSet<_>>(),
        BTreeSet::from([1, 3, 5, 7])
    );
    assert_eq!(
        frames().map(|frame| frame.checksum).collect::<BTreeSet<_>>(),
        BTreeSet::from([false, true])
    );
    assert!(cases.iter().any(|case| !case.indexed));
    assert!(cases.iter().map(|case| case.plain.len()).sum::<usize>() < 1024 * 1024);
    for case in cases {
        assert_eq!(
            case.frames.iter().map(|frame| frame.len).sum::<usize>(),
            case.plain.len()
        );
        assert_eq!(case.ranges.is_empty(), !case.indexed);
        if case.indexed {
            assert!(case.ranges.contains(&(case.plain.len(), 0)));
            assert!(case.ranges.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(
                case.ranges
                    .iter()
                    .all(|&(start, len)| len <= 97 && start + len <= case.plain.len())
            );
        }
    }
}

#[test]
fn accepts_existing_positionals_and_replay_arguments() {
    let Task::Interop {
        c_cli,
        rust_cli,
        seed,
        cases,
        case,
    } = Task::try_parse_from(["xtask", "interop", "./c-zxc", "./rust-zxc"]).unwrap()
    else {
        panic!("not interop");
    };
    assert_eq!(c_cli, Some(PathBuf::from("./c-zxc")));
    assert_eq!(rust_cli, Some(PathBuf::from("./rust-zxc")));
    assert_eq!((seed, cases, case), (DEFAULT_SEED, DEFAULT_CASES, None));
    let Task::Interop {
        c_cli,
        rust_cli,
        seed,
        case,
        ..
    } = Task::try_parse_from([
        "xtask", "interop", "--seed", "42", "--case", "10", "--", "c zxc", "rust'zxc",
    ])
    .unwrap()
    else {
        panic!("not interop replay");
    };
    assert_eq!((seed, case), (42, Some(10)));
    assert_eq!(c_cli, Some(PathBuf::from("c zxc")));
    assert_eq!(rust_cli, Some(PathBuf::from("rust'zxc")));
    assert!(Task::try_parse_from(["xtask", "interop", "--cases", "12", "--case", "3"]).is_err());
}

#[test]
fn differences_use_global_plaintext_offsets() {
    let err = exact(b"abc", b"abx", 30).unwrap_err().to_string();
    assert!(err.contains("first difference at plaintext offset 32"));
    let err = exact(b"ab", b"a", 30).unwrap_err().to_string();
    assert!(err.contains("length mismatch at plaintext offset 31"));
    assert!(err.contains("expected length 2, got 1"));
    assert!(exact(b"", b"", 30).is_ok());
    assert!(exact(b"", b"x", 30).is_err());
}
