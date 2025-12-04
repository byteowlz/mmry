# Fine-tuning mmBERT for Multilingual NER

This guide covers fine-tuning mmBERT (jhu-clsp/mmBERT-base) for Named Entity Recognition using the WikiANN dataset, then exporting to ONNX for use in Rust.

## Overview

### Why mmBERT?

- **Released**: September 2025 (very recent)
- **Languages**: 1,800+ languages (trained on 3T+ tokens)
- **Architecture**: ModernBERT with Flash Attention 2
- **Size**: 307M params (base), 140M params (small)
- **License**: MIT
- **Advantages over XLM-R**: Faster, more languages, better low-resource performance

### Why WikiANN?

- **Languages**: 176 languages with balanced splits
- **Entities**: PER (person), ORG (organization), LOC (location)
- **Format**: IOB2 tagging scheme
- **Size**: ~2M examples across all languages
- **License**: Academic use

## Prerequisites

```bash
# Create project directory
mkdir mmbert-ner && cd mmbert-ner

# Initialize with uv
uv init

# Add dependencies
uv add torch transformers datasets accelerate seqeval onnx onnxruntime
```

## Step 1: Dataset Preparation

```python
# prepare_data.py
from datasets import load_dataset, concatenate_datasets

# Languages to include (high-resource subset for training)
LANGUAGES = [
    "en", "de", "es", "fr", "it", "nl", "pt", "ru", "zh", "ja",
    "ar", "ko", "pl", "tr", "vi", "th", "id", "cs", "ro", "hu"
]

def load_wikiann_multilingual(languages, max_samples_per_lang=5000):
    """Load WikiANN for multiple languages."""
    train_datasets = []
    val_datasets = []
    test_datasets = []
    
    for lang in languages:
        print(f"Loading {lang}...")
        try:
            ds = load_dataset("unimelb-nlp/wikiann", lang)
            
            # Sample if too large
            train_ds = ds["train"]
            if len(train_ds) > max_samples_per_lang:
                train_ds = train_ds.shuffle(seed=42).select(range(max_samples_per_lang))
            
            val_ds = ds["validation"]
            if len(val_ds) > max_samples_per_lang // 5:
                val_ds = val_ds.shuffle(seed=42).select(range(max_samples_per_lang // 5))
            
            test_ds = ds["test"]
            if len(test_ds) > max_samples_per_lang // 5:
                test_ds = test_ds.shuffle(seed=42).select(range(max_samples_per_lang // 5))
            
            train_datasets.append(train_ds)
            val_datasets.append(val_ds)
            test_datasets.append(test_ds)
        except Exception as e:
            print(f"  Skipping {lang}: {e}")
    
    return {
        "train": concatenate_datasets(train_datasets).shuffle(seed=42),
        "validation": concatenate_datasets(val_datasets).shuffle(seed=42),
        "test": concatenate_datasets(test_datasets).shuffle(seed=42),
    }

if __name__ == "__main__":
    dataset = load_wikiann_multilingual(LANGUAGES)
    print(f"Train: {len(dataset['train'])} examples")
    print(f"Val: {len(dataset['validation'])} examples")
    print(f"Test: {len(dataset['test'])} examples")
    
    # Save to disk
    dataset["train"].save_to_disk("data/train")
    dataset["validation"].save_to_disk("data/validation")
    dataset["test"].save_to_disk("data/test")
```

## Step 2: Fine-tuning Script

```python
# train_ner.py
import torch
from datasets import load_from_disk
from transformers import (
    AutoTokenizer,
    AutoModelForTokenClassification,
    TrainingArguments,
    Trainer,
    DataCollatorForTokenClassification,
)
from seqeval.metrics import classification_report, f1_score, precision_score, recall_score
import numpy as np

# Configuration
MODEL_NAME = "jhu-clsp/mmBERT-base"  # or "jhu-clsp/mmBERT-small" for smaller model
OUTPUT_DIR = "./mmbert-ner-multilingual"
MAX_LENGTH = 128
BATCH_SIZE = 32
LEARNING_RATE = 2e-5
NUM_EPOCHS = 3
WEIGHT_DECAY = 0.01

# Label mapping (WikiANN uses these tags)
LABEL_LIST = ["O", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC"]
LABEL2ID = {label: i for i, label in enumerate(LABEL_LIST)}
ID2LABEL = {i: label for label, i in LABEL2ID.items()}

def main():
    # Load tokenizer and model
    tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)
    model = AutoModelForTokenClassification.from_pretrained(
        MODEL_NAME,
        num_labels=len(LABEL_LIST),
        id2label=ID2LABEL,
        label2id=LABEL2ID,
    )
    
    # Load datasets
    train_dataset = load_from_disk("data/train")
    val_dataset = load_from_disk("data/validation")
    
    def tokenize_and_align_labels(examples):
        """Tokenize inputs and align labels with subword tokens."""
        tokenized = tokenizer(
            examples["tokens"],
            truncation=True,
            padding=False,
            max_length=MAX_LENGTH,
            is_split_into_words=True,
        )
        
        labels = []
        for i, label in enumerate(examples["ner_tags"]):
            word_ids = tokenized.word_ids(batch_index=i)
            label_ids = []
            previous_word_idx = None
            
            for word_idx in word_ids:
                if word_idx is None:
                    # Special tokens get -100 (ignored in loss)
                    label_ids.append(-100)
                elif word_idx != previous_word_idx:
                    # First token of a word gets the label
                    label_ids.append(label[word_idx])
                else:
                    # Subsequent tokens of a word:
                    # - For B-* tags, use corresponding I-* tag
                    # - For I-* or O tags, use the same tag
                    current_label = label[word_idx]
                    if current_label % 2 == 1:  # B-* tag (odd numbers)
                        label_ids.append(current_label + 1)  # Convert to I-*
                    else:
                        label_ids.append(current_label)
                
                previous_word_idx = word_idx
            
            labels.append(label_ids)
        
        tokenized["labels"] = labels
        return tokenized
    
    # Tokenize datasets
    train_tokenized = train_dataset.map(
        tokenize_and_align_labels,
        batched=True,
        remove_columns=train_dataset.column_names,
    )
    val_tokenized = val_dataset.map(
        tokenize_and_align_labels,
        batched=True,
        remove_columns=val_dataset.column_names,
    )
    
    # Data collator
    data_collator = DataCollatorForTokenClassification(tokenizer=tokenizer)
    
    def compute_metrics(eval_pred):
        """Compute NER metrics using seqeval."""
        predictions, labels = eval_pred
        predictions = np.argmax(predictions, axis=2)
        
        # Remove ignored index (-100) and convert to label names
        true_labels = []
        true_predictions = []
        
        for prediction, label in zip(predictions, labels):
            true_label = []
            true_pred = []
            
            for p, l in zip(prediction, label):
                if l != -100:
                    true_label.append(ID2LABEL[l])
                    true_pred.append(ID2LABEL[p])
            
            true_labels.append(true_label)
            true_predictions.append(true_pred)
        
        return {
            "precision": precision_score(true_labels, true_predictions),
            "recall": recall_score(true_labels, true_predictions),
            "f1": f1_score(true_labels, true_predictions),
        }
    
    # Training arguments
    training_args = TrainingArguments(
        output_dir=OUTPUT_DIR,
        evaluation_strategy="epoch",
        save_strategy="epoch",
        learning_rate=LEARNING_RATE,
        per_device_train_batch_size=BATCH_SIZE,
        per_device_eval_batch_size=BATCH_SIZE,
        num_train_epochs=NUM_EPOCHS,
        weight_decay=WEIGHT_DECAY,
        load_best_model_at_end=True,
        metric_for_best_model="f1",
        greater_is_better=True,
        logging_steps=100,
        warmup_ratio=0.1,
        fp16=torch.cuda.is_available(),
        push_to_hub=False,
        report_to="none",  # or "wandb" if you want logging
    )
    
    # Trainer
    trainer = Trainer(
        model=model,
        args=training_args,
        train_dataset=train_tokenized,
        eval_dataset=val_tokenized,
        tokenizer=tokenizer,
        data_collator=data_collator,
        compute_metrics=compute_metrics,
    )
    
    # Train
    print("Starting training...")
    trainer.train()
    
    # Save best model
    trainer.save_model(f"{OUTPUT_DIR}/best")
    tokenizer.save_pretrained(f"{OUTPUT_DIR}/best")
    
    # Final evaluation
    print("\nFinal evaluation on validation set:")
    results = trainer.evaluate()
    print(results)

if __name__ == "__main__":
    main()
```

## Step 3: Export to ONNX

```python
# export_onnx.py
import torch
from transformers import AutoTokenizer, AutoModelForTokenClassification
from pathlib import Path

MODEL_PATH = "./mmbert-ner-multilingual/best"
ONNX_PATH = "./mmbert-ner-multilingual/model.onnx"
MAX_LENGTH = 128

def export_to_onnx():
    """Export fine-tuned model to ONNX format."""
    print("Loading model...")
    tokenizer = AutoTokenizer.from_pretrained(MODEL_PATH)
    model = AutoModelForTokenClassification.from_pretrained(MODEL_PATH)
    model.eval()
    
    # Create dummy input
    dummy_text = "John works at Google in New York."
    inputs = tokenizer(
        dummy_text,
        return_tensors="pt",
        padding="max_length",
        max_length=MAX_LENGTH,
        truncation=True,
    )
    
    # Export
    print(f"Exporting to {ONNX_PATH}...")
    torch.onnx.export(
        model,
        (inputs["input_ids"], inputs["attention_mask"]),
        ONNX_PATH,
        input_names=["input_ids", "attention_mask"],
        output_names=["logits"],
        dynamic_axes={
            "input_ids": {0: "batch_size", 1: "sequence_length"},
            "attention_mask": {0: "batch_size", 1: "sequence_length"},
            "logits": {0: "batch_size", 1: "sequence_length"},
        },
        opset_version=14,
        do_constant_folding=True,
    )
    
    print("Verifying ONNX model...")
    import onnx
    onnx_model = onnx.load(ONNX_PATH)
    onnx.checker.check_model(onnx_model)
    print("ONNX model is valid!")
    
    # Get model size
    model_size = Path(ONNX_PATH).stat().st_size / (1024 * 1024)
    print(f"Model size: {model_size:.2f} MB")

def test_onnx_inference():
    """Test ONNX inference with onnxruntime."""
    import onnxruntime as ort
    import numpy as np
    
    tokenizer = AutoTokenizer.from_pretrained(MODEL_PATH)
    
    # Load ONNX model
    session = ort.InferenceSession(ONNX_PATH)
    
    # Test inference
    test_text = "Angela Merkel met with Emmanuel Macron in Berlin."
    inputs = tokenizer(
        test_text,
        return_tensors="np",
        padding="max_length",
        max_length=MAX_LENGTH,
        truncation=True,
    )
    
    # Run inference
    outputs = session.run(
        None,
        {
            "input_ids": inputs["input_ids"],
            "attention_mask": inputs["attention_mask"],
        },
    )
    
    logits = outputs[0]
    predictions = np.argmax(logits, axis=-1)
    
    # Decode
    tokens = tokenizer.convert_ids_to_tokens(inputs["input_ids"][0])
    labels = ["O", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC"]
    
    print("\nONNX Inference Test:")
    print("-" * 50)
    for token, pred, mask in zip(tokens, predictions[0], inputs["attention_mask"][0]):
        if mask == 1 and token not in ["[PAD]", "<pad>"]:
            label = labels[pred]
            if label != "O":
                print(f"  {token}: {label}")

if __name__ == "__main__":
    export_to_onnx()
    test_onnx_inference()
```

## Step 4: Quantization (Optional)

For smaller model size and faster inference:

```python
# quantize_onnx.py
import onnx
from onnxruntime.quantization import quantize_dynamic, QuantType

INPUT_MODEL = "./mmbert-ner-multilingual/model.onnx"
OUTPUT_MODEL = "./mmbert-ner-multilingual/model_int8.onnx"

def quantize_model():
    """Apply dynamic INT8 quantization."""
    print(f"Quantizing {INPUT_MODEL}...")
    
    quantize_dynamic(
        model_input=INPUT_MODEL,
        model_output=OUTPUT_MODEL,
        weight_type=QuantType.QInt8,
    )
    
    # Compare sizes
    from pathlib import Path
    original_size = Path(INPUT_MODEL).stat().st_size / (1024 * 1024)
    quantized_size = Path(OUTPUT_MODEL).stat().st_size / (1024 * 1024)
    
    print(f"Original size: {original_size:.2f} MB")
    print(f"Quantized size: {quantized_size:.2f} MB")
    print(f"Reduction: {(1 - quantized_size/original_size)*100:.1f}%")

if __name__ == "__main__":
    quantize_model()
```

## Step 5: Rust Integration

Once you have the ONNX model, integrate with Rust using the `ort` crate:

```rust
// In mmry-core/src/ner/mod.rs

use ort::{Session, GraphOptimizationLevel, inputs};
use tokenizers::Tokenizer;

pub struct NerModel {
    session: Session,
    tokenizer: Tokenizer,
    label_map: Vec<String>,
}

impl NerModel {
    pub fn new(model_path: &str, tokenizer_path: &str) -> Result<Self> {
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(model_path)?;
        
        let tokenizer = Tokenizer::from_file(tokenizer_path)?;
        
        let label_map = vec![
            "O".to_string(),
            "B-PER".to_string(), "I-PER".to_string(),
            "B-ORG".to_string(), "I-ORG".to_string(),
            "B-LOC".to_string(), "I-LOC".to_string(),
        ];
        
        Ok(Self { session, tokenizer, label_map })
    }
    
    pub fn predict(&self, text: &str) -> Result<Vec<Entity>> {
        // Tokenize
        let encoding = self.tokenizer.encode(text, true)?;
        let input_ids: Vec<i64> = encoding.get_ids()
            .iter()
            .map(|&id| id as i64)
            .collect();
        let attention_mask: Vec<i64> = encoding.get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        
        // Run inference
        let outputs = self.session.run(inputs![
            "input_ids" => ndarray::Array2::from_shape_vec(
                (1, input_ids.len()),
                input_ids
            )?,
            "attention_mask" => ndarray::Array2::from_shape_vec(
                (1, attention_mask.len()),
                attention_mask
            )?,
        ]?)?;
        
        // Process outputs
        let logits = outputs[0].try_extract_tensor::<f32>()?;
        // ... decode predictions into entities
        
        Ok(entities)
    }
}
```

## Directory Structure

```
mmbert-ner/
├── prepare_data.py
├── train_ner.py
├── export_onnx.py
├── quantize_onnx.py
├── data/
│   ├── train/
│   ├── validation/
│   └── test/
├── mmbert-ner-multilingual/
│   ├── best/
│   │   ├── config.json
│   │   ├── model.safetensors
│   │   ├── tokenizer.json
│   │   └── ...
│   ├── model.onnx
│   └── model_int8.onnx
└── pyproject.toml
```

## Training Commands

```bash
# 1. Prepare data
uv run python prepare_data.py

# 2. Train model (GPU recommended)
uv run python train_ner.py

# 3. Export to ONNX
uv run python export_onnx.py

# 4. Quantize (optional)
uv run python quantize_onnx.py
```

## Expected Results

With the full WikiANN training (20 languages, ~100k examples):
- **Training time**: ~2-4 hours on a single GPU (A100/V100)
- **F1 score**: ~85-90% on high-resource languages
- **Model size**: ~600MB (FP32), ~150MB (INT8 quantized)
- **Inference**: ~5-10ms per sentence on CPU

## Alternatives

If mmBERT doesn't meet your needs:

| Model | Size | Languages | Notes |
|-------|------|-----------|-------|
| jhu-clsp/mmBERT-small | 140M | 1800+ | Smaller, faster |
| xlm-roberta-base | 278M | 100+ | Well-tested, 2019 |
| Davlan/xlm-roberta-base-ner-hrl | 278M | 10 | Pre-trained for NER |
| onnx-community/gliner_small-v2.1 | 166M | English | Zero-shot NER |

## References

- [mmBERT Paper](https://arxiv.org/abs/2509.06888)
- [WikiANN Dataset](https://huggingface.co/datasets/unimelb-nlp/wikiann)
- [Hugging Face Token Classification Guide](https://huggingface.co/docs/transformers/tasks/token_classification)
- [ONNX Runtime](https://onnxruntime.ai/)
