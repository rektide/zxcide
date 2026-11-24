#![cfg(target_arch = "wasm32")]

wit_bindgen::generate!({ world: "codec" });

use exports::zxcide::codec::buffers::{EncodeOptions, Guest, Mode};

struct Codec;

impl Guest for Codec {
    fn encode(plain: Vec<u8>, options: EncodeOptions) -> Result<Vec<u8>, String> {
        let options = zxcide::EncodeOptions {
            mode: match options.mode {
                Mode::Split => zxcide::Mode::Split,
                Mode::Packed => zxcide::Mode::Packed,
                Mode::Dense => zxcide::Mode::Dense,
            },
            block_log: options.block_log,
            checksum: options.checksum,
            seekable: options.seekable,
            dictionary: None,
        };
        let mut archive = Vec::new();
        zxcide::encode(&plain[..], &mut archive, options).map_err(|e| e.to_string())?;
        Ok(archive)
    }

    fn decode(archive: Vec<u8>, max_output: u64) -> Result<Vec<u8>, String> {
        let options = zxcide::DecodeOptions {
            max_output: Some(max_output),
            ..Default::default()
        };
        let mut plain = Vec::new();
        zxcide::decode(&archive[..], &mut plain, options).map_err(|e| e.to_string())?;
        Ok(plain)
    }
}

export!(Codec);
