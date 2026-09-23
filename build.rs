use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

const MODEL_PATH: &str = "classifier/model.onnx";
const LFS_POINTER_PREFIX: &[u8] = b"version https://git-lfs.github.com/spec/v1";
const MINIMUM_MODEL_BYTES: u64 = 1_000_000;

fn main() {
    println!("cargo:rerun-if-changed={MODEL_PATH}");

    let path = Path::new(MODEL_PATH);
    let metadata = fs::metadata(path).unwrap_or_else(|error| {
        panic!("embedded classifier model is unavailable at {MODEL_PATH}: {error}")
    });
    let mut prefix = vec![0; LFS_POINTER_PREFIX.len()];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut prefix))
        .unwrap_or_else(|error| panic!("could not read embedded classifier model: {error}"));

    if prefix.starts_with(LFS_POINTER_PREFIX) {
        panic!(
            "{MODEL_PATH} is a Git LFS pointer, not the ONNX model; run `git lfs pull` before building"
        );
    }
    if metadata.len() < MINIMUM_MODEL_BYTES {
        panic!(
            "{MODEL_PATH} is unexpectedly small ({} bytes); expected an expanded ONNX model",
            metadata.len()
        );
    }
}
