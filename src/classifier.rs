use std::sync::Mutex;
use std::sync::OnceLock;

use ort::session::Session;
use ort::value::{DynTensorValueType, Tensor};
use tokenizers::Tokenizer;

use crate::Decision;

const MODEL_BYTES: &[u8] = include_bytes!("../classifier/model.onnx");
const TOKENIZER_BYTES: &[u8] = include_bytes!("../classifier/tokenizer.json");

const MAX_LENGTH: usize = 128;
const TEMPERATURE: f32 = 1.3366;
const ENERGY_P95: f32 = -2.5434;

struct ClassifierState {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

static STATE: OnceLock<Option<ClassifierState>> = OnceLock::new();

fn get_state() -> Option<&'static ClassifierState> {
    STATE
        .get_or_init(|| {
            let session = Session::builder()
                .ok()?
                .commit_from_memory(MODEL_BYTES)
                .ok()?;
            let tokenizer = serde_json::from_slice::<Tokenizer>(TOKENIZER_BYTES).ok()?;
            Some(ClassifierState {
                session: Mutex::new(session),
                tokenizer,
            })
        })
        .as_ref()
}

fn softmax(logits: &[f32], temperature: f32) -> Vec<f32> {
    let scaled: Vec<f32> = logits.iter().map(|&x| x / temperature).collect();
    let max = scaled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = scaled.iter().map(|&x| (x - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    exps.iter().map(|&x| x / sum).collect()
}

fn energy_score(logits: &[f32], temperature: f32) -> f32 {
    let scaled: Vec<f32> = logits.iter().map(|&x| x / temperature).collect();
    let max = scaled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let sum_exp: f32 = scaled.iter().map(|&x| (x - max).exp()).sum();
    -(temperature * (max + sum_exp.ln()))
}

/// Classify a command string using the embedded DistilBERT model.
/// Returns `Approve` only when the model is confident the command is safe.
/// Returns `Deny` for commands classified as dangerous.
/// Returns `Defer` for ambiguous, needs-approval, or OOD commands.
pub fn classify_command(cmd: &str) -> Decision {
    let Some(state) = get_state() else {
        return Decision::Defer;
    };

    let encoding = match state.tokenizer.encode(cmd, true) {
        Ok(e) => e,
        Err(_) => return Decision::Defer,
    };

    let ids = encoding.get_ids();
    let mask = encoding.get_attention_mask();
    let len = ids.len().min(MAX_LENGTH);

    let mut input_ids = vec![0i64; MAX_LENGTH];
    let mut attention_mask = vec![0i64; MAX_LENGTH];
    for i in 0..len {
        input_ids[i] = ids[i] as i64;
        attention_mask[i] = mask[i] as i64;
    }

    let Ok(input_ids_tensor) = Tensor::from_array(([1, MAX_LENGTH], input_ids)) else {
        return Decision::Defer;
    };
    let Ok(attention_mask_tensor) = Tensor::from_array(([1, MAX_LENGTH], attention_mask)) else {
        return Decision::Defer;
    };

    let mut session = match state.session.lock() {
        Ok(s) => s,
        Err(_) => return Decision::Defer,
    };
    let outputs = match session.run(ort::inputs![
        "input_ids" => input_ids_tensor,
        "attention_mask" => attention_mask_tensor,
    ]) {
        Ok(o) => o,
        Err(_) => return Decision::Defer,
    };

    let tensor_ref = match outputs[0].downcast_ref::<DynTensorValueType>() {
        Ok(t) => t,
        Err(_) => return Decision::Defer,
    };
    let logits: Vec<f32> = match tensor_ref.try_extract_tensor::<f32>() {
        Ok((_shape, data)) => data.to_vec(),
        Err(_) => return Decision::Defer,
    };

    if energy_score(&logits, TEMPERATURE) > ENERGY_P95 {
        return Decision::Defer;
    }

    let probs = softmax(&logits, TEMPERATURE);
    let (max_idx, &max_prob) = probs
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .unwrap();

    match max_idx {
        0 if max_prob > 0.90 => Decision::Approve,
        2 => Decision::Deny,
        _ => Decision::Defer,
    }
}
