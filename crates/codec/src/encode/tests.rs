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
