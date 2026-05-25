# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "torch>=2.4",
#     "transformers>=4.45",
#     "datasets>=3.0",
#     "scikit-learn>=1.5",
#     "accelerate>=1.0",
#     "numpy>=1.26",
# ]
# ///
"""Train a DistilBERT classifier for bash command safety.

Three-class classification: safe / needs-approval / dangerous.
Uses class weights to handle imbalance, temperature scaling for calibration,
and computes energy scores for OOD detection thresholds.

Usage:
    uv run scripts/train.py -i data/training-dataset.jsonl -o models/classifier

    # Custom epochs / batch size:
    uv run scripts/train.py -i data/training-dataset.jsonl -o models/classifier --epochs 10 --batch-size 32
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
from torch.optim import LBFGS
from datasets import Dataset
from sklearn.metrics import (
    classification_report,
    f1_score,
    precision_score,
    recall_score,
)
from sklearn.model_selection import train_test_split
from transformers import (
    AutoModelForSequenceClassification,
    AutoTokenizer,
    Trainer,
    TrainingArguments,
)


LABELS = ["safe", "needs-approval", "dangerous"]
LABEL2ID = {l: i for i, l in enumerate(LABELS)}
ID2LABEL = {i: l for i, l in enumerate(LABELS)}


def load_dataset_from_jsonl(path: Path) -> list[dict]:
    records = []
    for line in path.open():
        rec = json.loads(line)
        label = rec.get("label", "").strip()
        command = rec.get("command", "").strip()
        if label in LABEL2ID and command:
            records.append({"text": command, "label": LABEL2ID[label]})
    return records


def compute_class_weights(labels: list[int], num_classes: int) -> torch.Tensor:
    counts = np.bincount(labels, minlength=num_classes).astype(float)
    weights = len(labels) / (num_classes * counts)
    return torch.tensor(weights, dtype=torch.float32)


def compute_metrics(eval_pred):
    logits, labels = eval_pred
    preds = np.argmax(logits, axis=-1)
    return {
        "f1_macro": f1_score(labels, preds, average="macro"),
        "f1_weighted": f1_score(labels, preds, average="weighted"),
        "precision_macro": precision_score(labels, preds, average="macro"),
        "recall_macro": recall_score(labels, preds, average="macro"),
    }


class WeightedTrainer(Trainer):
    def __init__(self, class_weights: torch.Tensor, **kwargs):
        super().__init__(**kwargs)
        self._class_weights = class_weights

    def compute_loss(self, model, inputs, return_outputs=False, **kwargs):
        labels = inputs.pop("labels")
        outputs = model(**inputs)
        logits = outputs.logits
        weights = self._class_weights.to(logits.device)
        loss = nn.CrossEntropyLoss(weight=weights)(logits, labels)
        return (loss, outputs) if return_outputs else loss


def fit_temperature(logits: torch.Tensor, labels: torch.Tensor) -> float:
    temperature = nn.Parameter(torch.ones(1) * 1.5)
    optimizer = LBFGS([temperature], lr=0.01, max_iter=100)
    criterion = nn.CrossEntropyLoss()

    def closure():
        optimizer.zero_grad()
        scaled = logits / temperature
        loss = criterion(scaled, labels)
        loss.backward()
        return loss

    optimizer.step(closure)
    return temperature.item()


def compute_energy_thresholds(
    logits: np.ndarray, labels: np.ndarray, temperature: float = 1.0
) -> dict:
    energies = -temperature * np.log(
        np.sum(np.exp(logits / temperature), axis=-1)
    )
    p95 = float(np.percentile(energies, 95))
    p99 = float(np.percentile(energies, 99))
    return {
        "energy_p95": p95,
        "energy_p99": p99,
        "energy_mean": float(np.mean(energies)),
        "energy_std": float(np.std(energies)),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-i", "--input", type=Path, required=True)
    parser.add_argument("-o", "--output", type=Path, default=Path("models/classifier"))
    parser.add_argument("--model", default="distilbert-base-uncased")
    parser.add_argument("--epochs", type=int, default=5)
    parser.add_argument("--batch-size", type=int, default=64)
    parser.add_argument("--lr", type=float, default=2e-5)
    parser.add_argument("--max-length", type=int, default=128)
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()

    print(f"Loading data from {args.input}...", file=sys.stderr)
    records = load_dataset_from_jsonl(args.input)
    print(f"  {len(records)} valid records", file=sys.stderr)

    train_recs, test_recs = train_test_split(
        records, test_size=0.1, random_state=args.seed,
        stratify=[r["label"] for r in records],
    )
    train_recs, val_recs = train_test_split(
        train_recs, test_size=0.1, random_state=args.seed,
        stratify=[r["label"] for r in train_recs],
    )

    print(f"  train: {len(train_recs)}, val: {len(val_recs)}, test: {len(test_recs)}", file=sys.stderr)

    train_ds = Dataset.from_list(train_recs)
    val_ds = Dataset.from_list(val_recs)
    test_ds = Dataset.from_list(test_recs)

    print(f"Loading tokenizer and model: {args.model}...", file=sys.stderr)
    tokenizer = AutoTokenizer.from_pretrained(args.model)
    model = AutoModelForSequenceClassification.from_pretrained(
        args.model,
        num_labels=len(LABELS),
        id2label=ID2LABEL,
        label2id=LABEL2ID,
    )

    def tokenize(batch):
        return tokenizer(
            batch["text"], truncation=True, padding="max_length",
            max_length=args.max_length,
        )

    train_ds = train_ds.map(tokenize, batched=True, remove_columns=["text"])
    val_ds = val_ds.map(tokenize, batched=True, remove_columns=["text"])
    test_ds = test_ds.map(tokenize, batched=True, remove_columns=["text"])

    train_ds.set_format("torch")
    val_ds.set_format("torch")
    test_ds.set_format("torch")

    class_weights = compute_class_weights(
        [r["label"] for r in train_recs], len(LABELS),
    )
    print(f"  class weights: {dict(zip(LABELS, class_weights.tolist()))}", file=sys.stderr)

    device = "mps" if torch.backends.mps.is_available() else "cpu"
    print(f"  device: {device}", file=sys.stderr)

    output_dir = args.output / "checkpoints"
    training_args = TrainingArguments(
        output_dir=str(output_dir),
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        per_device_eval_batch_size=args.batch_size * 2,
        learning_rate=args.lr,
        weight_decay=0.01,
        warmup_ratio=0.1,
        eval_strategy="epoch",
        save_strategy="epoch",
        load_best_model_at_end=True,
        metric_for_best_model="f1_macro",
        greater_is_better=True,
        logging_steps=50,
        logging_first_step=True,
        seed=args.seed,
        dataloader_pin_memory=False,
        report_to="none",
        save_total_limit=2,
        disable_tqdm=False,
        log_level="info",
    )

    trainer = WeightedTrainer(
        class_weights=class_weights,
        model=model,
        args=training_args,
        train_dataset=train_ds,
        eval_dataset=val_ds,
        compute_metrics=compute_metrics,
    )

    steps_per_epoch = len(train_ds) // args.batch_size + 1
    total_steps = steps_per_epoch * args.epochs
    print(f"\n{'='*60}", file=sys.stderr)
    print(f"  Training {args.model}", file=sys.stderr)
    print(f"  {len(train_ds)} samples, {args.epochs} epochs, batch {args.batch_size}", file=sys.stderr)
    print(f"  {steps_per_epoch} steps/epoch, {total_steps} total steps", file=sys.stderr)
    print(f"  device: {device}", file=sys.stderr)

    checkpoint = None
    if output_dir.exists():
        checkpoints = sorted(output_dir.glob("checkpoint-*"), key=lambda p: p.stat().st_mtime)
        if checkpoints:
            checkpoint = str(checkpoints[-1])
            print(f"  resuming from: {checkpoint}", file=sys.stderr)

    print(f"{'='*60}\n", file=sys.stderr)
    trainer.train(resume_from_checkpoint=checkpoint)

    print("\nEvaluating on test set...", file=sys.stderr)
    preds_output = trainer.predict(test_ds)
    test_logits = preds_output.predictions
    test_labels = preds_output.label_ids
    test_preds = np.argmax(test_logits, axis=-1)

    report = classification_report(
        test_labels, test_preds, target_names=LABELS, digits=4,
    )
    print(f"\n{report}", file=sys.stderr)

    print("\nCalibrating temperature...", file=sys.stderr)
    val_output = trainer.predict(val_ds)
    val_logits_t = torch.tensor(val_output.predictions, dtype=torch.float32)
    val_labels_t = torch.tensor(val_output.label_ids, dtype=torch.long)
    temperature = fit_temperature(val_logits_t, val_labels_t)
    print(f"  optimal temperature: {temperature:.4f}", file=sys.stderr)

    print("\nComputing energy OOD thresholds...", file=sys.stderr)
    energy_stats = compute_energy_thresholds(test_logits, test_labels, temperature)
    print(f"  {energy_stats}", file=sys.stderr)

    print(f"\nSaving model to {args.output}...", file=sys.stderr)
    args.output.mkdir(parents=True, exist_ok=True)
    trainer.save_model(str(args.output))
    tokenizer.save_pretrained(str(args.output))

    meta = {
        "labels": LABELS,
        "label2id": LABEL2ID,
        "id2label": ID2LABEL,
        "temperature": temperature,
        "energy_thresholds": energy_stats,
        "class_weights": dict(zip(LABELS, class_weights.tolist())),
        "max_length": args.max_length,
        "base_model": args.model,
        "train_size": len(train_recs),
        "val_size": len(val_recs),
        "test_size": len(test_recs),
        "epochs": args.epochs,
        "test_metrics": {
            "f1_macro": float(preds_output.metrics["test_f1_macro"]),
            "f1_weighted": float(preds_output.metrics["test_f1_weighted"]),
            "precision_macro": float(preds_output.metrics["test_precision_macro"]),
            "recall_macro": float(preds_output.metrics["test_recall_macro"]),
        },
    }
    (args.output / "classifier_meta.json").write_text(
        json.dumps(meta, indent=2) + "\n"
    )

    print(f"\nDone. Model + metadata saved to {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
