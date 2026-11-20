use super::*;

fn dictionary(mut seed: u64, len: usize) -> Dictionary {
    let content = (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    // Two distinct objects may have the same ID. Match tables are tied to
    // content, not to a caller's frame ID or an address cached by a worker.
    Dictionary {
        content,
        tree: None,
        id: 0,
        match_seed: Default::default(),
    }
}

#[test]
fn seed_replays_all_boundary_hashes_and_last_collision_wins() {
    let dict = dictionary(42, 65535);
    let prefix = dict.content.len();
    let mut cached = Vec::new();
    for block in [b"ABC123", b"XYZ987", b"AXZ321"] {
        let mut history = dict.content.clone();
        history.extend_from_slice(block);

        let mut expected = vec![u32::MAX; MATCH_SLOTS];
        for i in 0..prefix {
            expected[key(&history, i)] = i as u32;
        }
        prepare_seed(&history, &dict, &mut cached);
        assert_eq!(cached, expected);
        let seed = dict.match_seed.get().unwrap();
        for i in prefix - 3..prefix {
            assert!((seed[key(&history, i)] as usize) < prefix - 3 || seed[key(&history, i)] == u32::MAX);
        }
        // Emulate block writes between calls, even if every slot was changed.
        cached.fill(1);
    }
}

#[test]
fn seeded_encoder_matches_unseeded_across_dictionary_switches() {
    let a = dictionary(42, 65535);
    let b = dictionary(314, MIN_SEED_DICTIONARY);
    let mut cached = BlockEncoder::default();
    let mut unseeded = BlockEncoder::default();
    let mut byte = 0;
    for mode in [Mode::Split, Mode::Packed, Mode::Dense] {
        for (index, dict, size) in [
            (0, Some(&a), 4096),
            (FIRST_SEEDED_INDEX, Some(&a), 4096),
            (FIRST_SEEDED_INDEX + 1, Some(&a), 4096),
            (FIRST_SEEDED_INDEX + 2, Some(&b), 4096),
            (0, Some(&b), 44),
            (1, None, 63),
            (FIRST_SEEDED_INDEX + 3, Some(&b), 45),
            (0, Some(&a), 4095),
        ] {
            let mut plain = vec![byte; size];
            if let Some(d) = dict {
                let span = size.min(256) / 2;
                plain[1..=span].copy_from_slice(&d.content[500..500 + span]);
            }
            byte += 1;
            let opt = EncodeOptions {
                block_log: 12,
                mode,
                dictionary: dict,
                ..Default::default()
            };
            let expected = unseeded.encode(&plain, EncodeOptions { block_log: 19, ..opt }, index);
            let actual = cached.encode(&plain, opt, index);
            assert_eq!(
                (actual.kind, actual.payload, actual.checksum),
                (expected.kind, expected.payload, expected.checksum)
            );
        }
    }
    assert_eq!(a.match_seed.get().unwrap().len(), MATCH_SLOTS);
    assert_eq!(b.match_seed.get().unwrap().len(), MATCH_SLOTS);
}

#[test]
fn dense_search_can_use_an_older_longer_match() {
    let plain = b"ABCDE01234!ABCDE9=====ABCDE01234?";
    let at = plain.len() - 11;
    let mut matches = Vec::new();
    dense_match::Parser::default().parse(plain, 0, &mut matches);
    assert!(
        matches
            .iter()
            .any(|m| (m.at, m.len, usize::from(m.off)) == (at, 10, at - 1))
    );
}

#[test]
fn dense_parse_can_shorten_a_match_to_start_the_next_one() {
    // Match lengths at plaintext positions 0 and 5 are 8 and 7. Taking
    // the full match at 0 would leave four literals rather than a match.
    let mut matches = Vec::new();
    dense_match::Parser::default().parse(b"abcdefgh#fghijkl#abcdefghijkl", 17, &mut matches);
    assert_eq!(
        matches.iter().map(|m| (m.at, m.len)).collect::<Vec<_>>(),
        [(0, 5), (5, 7)]
    );
}

fn dense_roundtrip(plain: &[u8], dict: Option<&Dictionary>, block_log: u8) {
    let mut archive = Vec::new();
    crate::encode(
        plain,
        &mut archive,
        EncodeOptions {
            mode: Mode::Dense,
            dictionary: dict,
            block_log,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(archive[16], 1, "expected a compressed SPL block");
    let mut decoded = Vec::new();
    crate::decode(
        &archive[..],
        &mut decoded,
        crate::DecodeOptions {
            dictionaries: dict.map_or(&[], std::slice::from_ref),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(plain, decoded);
}

#[test]
fn dense_history_includes_dictionary_join_and_exact_window_limit() {
    let mut parser = dense_match::Parser::default();
    let mut matches = Vec::new();
    let mut dict = dictionary(0, 3);
    dict.content.copy_from_slice(b"xyz");
    let plain = dict.content.repeat(64);
    let history = [&dict.content[..], &plain].concat();
    parser.parse(&history, 3, &mut matches);
    assert_eq!(matches.len(), 1);
    assert_eq!((matches[0].at, matches[0].len, matches[0].off), (0, plain.len(), 2));
    dense_roundtrip(&plain, Some(&dict), 12);

    let dict = dictionary(42, 65535);
    let bytes = &dict.content[..128];
    let plain = [&b"!"[..], bytes].concat();
    let history = [&dict.content[..], &plain].concat();
    parser.parse(&history, dict.content.len(), &mut matches);
    assert_eq!(matches.len(), 1);
    assert_eq!(
        (matches[0].at, matches[0].len, matches[0].off),
        (1, bytes.len(), u16::MAX)
    );
    dense_roundtrip(&plain, Some(&dict), 12);

    let beyond_window = [&dict.content[..], b"!?", bytes].concat();
    parser.parse(&beyond_window, dict.content.len(), &mut matches);
    assert!(matches.is_empty());
}

#[test]
fn dense_long_match_extends_past_parse_spans_and_roundtrips() {
    let plain = vec![0x42; 1 << 21];
    let mut matches = Vec::new();
    dense_match::Parser::default().parse(&plain, 0, &mut matches);
    assert_eq!(matches.len(), 1);
    assert_eq!((matches[0].at, matches[0].len, matches[0].off), (1, plain.len() - 1, 0));
    dense_roundtrip(&plain, None, 21);
}
