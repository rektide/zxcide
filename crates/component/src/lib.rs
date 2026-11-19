#![cfg(target_arch = "wasm32")]

wit_bindgen::generate!({ world: "codec" });

use exports::zxc::codec::buffers::{EncodeOptions, Guest, Mode};

struct Codec;

impl Guest for Codec {
    fn encode(plain: Vec<u8>, options: EncodeOptions) -> Result<Vec<u8>, String> {
        let options = zxc_codec::EncodeOptions {
            mode: match options.mode {
                Mode::Split => zxc_codec::Mode::Split,
                Mode::Packed => zxc_codec::Mode::Packed,
                Mode::Dense => zxc_codec::Mode::Dense,
            },
            block_log: options.block_log,
            checksum: options.checksum,
            seekable: options.seekable,
            dictionary: None,
        };
        let mut archive = Vec::new();
        zxc_codec::encode(&plain[..], &mut archive, options).map_err(|e| e.to_string())?;
        Ok(archive)
    }

    fn decode(archive: Vec<u8>, max_output: u64) -> Result<Vec<u8>, String> {
        let options = zxc_codec::DecodeOptions {
            max_output: Some(max_output),
            ..Default::default()
        };
        let mut plain = Vec::new();
        zxc_codec::decode(&archive[..], &mut plain, options).map_err(|e| e.to_string())?;
        Ok(plain)
    }
}

export!(Codec);
