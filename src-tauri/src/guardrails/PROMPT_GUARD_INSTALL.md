# PromptGuard 2 — Installation guide (LlamaFirewall Phase 2)

> Sprint D · May 2026. Optional ML guardrail for prompt-injection detection.
> When NOT installed, Lucy's regex-based guardrail keeps protecting you.
> When installed, the ML pass catches the ~30-40% of attacks regex misses.

## Why optional

PromptGuard 2 adds three weights to Lucy's footprint:

1. **ONNX Runtime DLLs** (~50 MB) — Microsoft's inference engine
2. **PromptGuard 2 model** (~280 MB) — Meta's BERT classifier
3. **Tokenizer** (~5 MB)

For users who don't need ML-grade injection detection (the regex bank already
catches the obvious patterns), shipping all three by default would mean a
~340 MB heavier installer. So Lucy ships LEAN and lets you opt in.

## Three-step install

### Step 1 — Rebuild Lucy with the `ml-guard` feature

```bash
cd src-tauri
cargo build --release --features ml-guard
```

(or `cargo tauri build --features ml-guard` for the full installer)

The first build will compile the `ort` + `tokenizers` + `ndarray` crates
(~2-3 min extra on a cold cache). No external binaries are downloaded at
build time — the `load-dynamic` ort feature defers DLL loading to runtime.

### Step 2 — Install ONNX Runtime DLLs

Download the Windows x64 build from Microsoft:
https://github.com/microsoft/onnxruntime/releases/latest

Extract `onnxruntime.dll` and place it either:

- **System-wide**: `C:\Windows\System32\onnxruntime.dll` (admin required)
- **Lucy-local**: next to `lucy.exe` in the install dir
- **PATH**: anywhere in your `%PATH%`

Lucy will load it via the OS's standard DLL search order on first ML call.

### Step 3 — Download the PromptGuard 2 model

The model + tokenizer go in:
`%APPDATA%\Lucy\guardrails\prompt_guard_2\`

Two files needed:

| File | Source |
|---|---|
| `model.onnx` | HuggingFace: `meta-llama/Llama-Prompt-Guard-2-86M` → ONNX export |
| `tokenizer.json` | Same repo, root of the folder |

The Meta model requires accepting their license. From HuggingFace CLI:

```bash
huggingface-cli login
huggingface-cli download meta-llama/Llama-Prompt-Guard-2-86M \
    model.onnx tokenizer.json \
    --local-dir "%APPDATA%\Lucy\guardrails\prompt_guard_2"
```

If the repo doesn't ship pre-built ONNX, convert from the PyTorch
checkpoint with:

```python
from transformers import AutoModelForSequenceClassification, AutoTokenizer
import torch

m = AutoModelForSequenceClassification.from_pretrained("meta-llama/Llama-Prompt-Guard-2-86M")
t = AutoTokenizer.from_pretrained("meta-llama/Llama-Prompt-Guard-2-86M")

dummy = t("test", return_tensors="pt", padding="max_length", max_length=512, truncation=True)
torch.onnx.export(
    m, (dummy["input_ids"], dummy["attention_mask"]), "model.onnx",
    input_names=["input_ids", "attention_mask"],
    output_names=["logits"],
    dynamic_axes={"input_ids": {1: "seq"}, "attention_mask": {1: "seq"}},
    opset_version=14,
)
t.save_pretrained(".")
```

## Verifying the install

Launch Lucy and check the status:

```js
// From DevTools console
await __TAURI__.core.invoke('prompt_guard_status')
// → { status: 'active', model_path: 'C:\\Users\\...\\model.onnx', note: null }
```

States you might see:

| Status | Meaning |
|---|---|
| `feature_disabled` | Lucy was built without `--features ml-guard`. Rebuild step 1. |
| `model_missing` | Feature is in, model files aren't. Do step 3. |
| `runtime_missing` | Model is there but ORT DLL can't be loaded. Do step 2. |
| `failed` | Other error — see `note` for details. |
| `active` | Everything's wired. ML inference is running. |

## What changes once `active`

Lucy's `guardrail_scan()` pipeline becomes:

1. **Regex bank** runs first (same as before) — `Block` if any pattern matches outright.
2. If the regex's decision was `Allow` (for `User` role) → ML is skipped to save cost.
3. Otherwise, `PromptGuard 2 ML` runs:
   - `score >= 0.85` → promote to **Block**
   - `score >= 0.50` → promote to **HumanInTheLoop**
   - `score < 0.50` → keep the regex decision

ML can only make the guardrail STRICTER. A regex `Block` never gets weakened by ML.

## Latency

Per ML inference call, on CPU (no GPU acceleration needed for an 86M model):

- Tokenization: ~1 ms
- ONNX forward pass: ~10-15 ms on a desktop x64 CPU
- Total added latency: ~12-16 ms per scan

For Lucy's typical flow (one scan per user turn + a few for tool outputs),
this is invisible.

## Disabling without uninstall

If you want to keep the model on disk but temporarily disable ML inference,
remove the `onnxruntime.dll` from the search path. The status command will
report `runtime_missing` and the regex bank takes over. Faster than
rebuilding without the feature.

## Removing entirely

1. Delete `%APPDATA%\Lucy\guardrails\prompt_guard_2\`
2. (Optional) Rebuild Lucy without `--features ml-guard` to drop the
   `ort` crate from the binary

The status will return to `model_missing` then `feature_disabled`.
