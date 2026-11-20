use std::{fs, path::Path};
use rapidhash::v3::{RapidSecrets, rapidhash_v3_seeded};
use crate::{Dictionary, EncodeOptions, EncodedBlock, stream::EncodeFrame};

fn sum(bytes: &[u8], seed: u64) -> u32 {
    let h = rapidhash_v3_seeded(bytes, &RapidSecrets::seed_cpp(seed));
    (h ^ (h >> 32)) as u32
}

#[test]
fn make_traversal_dictionary_fixtures() {
    let out = std::env::var_os("QUEUE_FIXTURES").unwrap();
    let out = Path::new(&out);
    let content = b"DE";
    let mut table = [0; 128];
    table[32..34].copy_from_slice(&[0x10, 0x22]); // A: 0, B: 10, C: 11
    let id = sum(&table, u64::from(sum(content, 0)));
    let mut header = [0; 16];
    header[..4].copy_from_slice(&0x9cb0d1c7u32.to_le_bytes());
    header[4] = 2;
    header[6..8].copy_from_slice(&2u16.to_le_bytes());
    header[8..12].copy_from_slice(&id.to_le_bytes());
    let a = u64::from_le_bytes(header[..8].try_into().unwrap());
    let b = u64::from(id);
    let c = 0xd2d84a61d2d84a61;
    let g = 0x9e3779b97f4a7c15;
    let hsum = ((a ^ c).wrapping_mul(c).wrapping_add(b).wrapping_add(g).wrapping_mul(g) >> 48) as u16;
    header[14..].copy_from_slice(&hsum.to_le_bytes());
    let dict_bytes = [&header[..], content, &table].concat();
    fs::write(out.join("traversal.zxd"), &dict_bytes).unwrap();
    let dictionary = Dictionary::from_bytes(&dict_bytes).unwrap();

    let nl = 4091u32;
    let literal_bytes = nl.div_ceil(8) + (nl / 2).div_ceil(8);
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&nl.to_le_bytes());
    payload.extend_from_slice(&[3, 2, 0, 1]); // shared literals; inline tokens; narrow offsets
    payload.extend_from_slice(&literal_bytes.to_le_bytes());
    payload.extend_from_slice(&129u32.to_le_bytes());
    payload.resize(payload.len() + literal_bytes as usize, 0xaa);
    payload.push(1); // token 0: copy five, consume no literals
    payload.resize(payload.len() + 128, 0);
    payload.push(1); // distance 2: DE, then overlap into the output
    payload.resize(payload.len() + 32, 0);
    let expected: Vec<_> = b"DEDED".iter().copied().chain(b"ABAC".iter().copied().cycle().take(nl as usize)).collect();

    for (name, blocks) in [("dict-cold", 1), ("dict-many", 256)] {
        let mut bytes = Vec::new();
        let mut sizes = Vec::new();
        let mut frame = EncodeFrame::start(&mut bytes, EncodeOptions {
            dictionary: Some(&dictionary), block_log: 12, seekable: true, ..Default::default()
        }, &mut sizes).unwrap();
        for index in 0..blocks {
            frame.commit(&mut bytes, &EncodedBlock {
                kind: 1, payload: &payload, plain_len: expected.len(), checksum: Some(sum(&expected, index)),
            }).unwrap();
        }
        frame.finish(&mut bytes).unwrap();
        fs::write(out.join(format!("{name}.zxc")), bytes).unwrap();
        fs::write(out.join(name), expected.repeat(blocks as usize)).unwrap();
    }
}
