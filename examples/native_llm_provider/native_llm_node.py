#!/usr/bin/env python3
"""
MoFA Native Python LLM Provider Node
======================================
Implements a native dora-rs Python node that runs a local HuggingFace
model directly on GPU, communicating via the MoFA message bus.

Unlike the HTTP-based providers (see examples/python_bindings/01_llm_agent.py),
this node eliminates the network hop entirely — prompts and responses flow
through the dora-rs bus using zero-copy PyArrow arrays.

This is the Python-side counterpart to the Rust LinuxLocalProvider in
crates/mofa-local-llm/src/provider.rs. Where the Rust provider handles
hardware detection and backend dispatch, this node handles the HuggingFace
tokenization pipeline and GPU inference directly.

Related issue: mofa-org/mofa#820
Author: Samuel Alaba (@samuelmohel)

Usage:
    MOFA_MODEL_ID=allenai/OLMo-1B-hf python native_llm_node.py

Input port:
    user_prompt      — UTF-8 encoded prompt string via PyArrow array

Output ports:
    generated_response — UTF-8 encoded response string via PyArrow array
    status             — "ready" once model is loaded into VRAM
    error              — structured error message if anything fails
"""

import os
import sys
from dora import Node
from transformers import AutoModelForCausalLM, AutoTokenizer
import pyarrow as pa


def load_model(model_id: str):
    """
    Load tokenizer and model onto GPU.

    Enforces use_safetensors=True to prevent arbitrary code execution
    via malicious pickle-format model files. This mirrors the security
    standard implemented in the Rust provider's model validation step.

    Args:
        model_id: HuggingFace model identifier, e.g. "allenai/OLMo-1B-hf"

    Returns:
        (tokenizer, model) tuple with model mapped to best available device

    Raises:
        Exception: propagated to caller for structured error reporting
    """
    print(f"[MoFA-LLM-Node] Loading tokenizer: {model_id}")
    tokenizer = AutoTokenizer.from_pretrained(model_id)

    print(f"[MoFA-LLM-Node] Loading model to GPU: {model_id}")
    # use_safetensors=True — prevents arbitrary code execution via
    # pickle-format weights. Security standard per mofa-org/mofa#820.
    model = AutoModelForCausalLM.from_pretrained(
        model_id,
        use_safetensors=True,
        device_map="auto",
    )
    return tokenizer, model


def run_inference(tokenizer, model, prompt: str, max_new_tokens: int) -> str:
    """
    Tokenize prompt, run GPU inference, decode output.

    This implements the three-stage pipeline described in mofa-org/mofa#820:
      1. HuggingFace Tokenizer (encode)  — words → token IDs
      2. Local Model Inference on GPU    — token IDs → output IDs
      3. HuggingFace Tokenizer (decode)  — output IDs → words

    Args:
        tokenizer: loaded HuggingFace tokenizer
        model: loaded HuggingFace model on GPU
        prompt: raw input string from the MoFA orchestrator
        max_new_tokens: max tokens to generate (from MOFA_MAX_TOKENS)

    Returns:
        Decoded response string
    """
    inputs = tokenizer(prompt, return_tensors="pt").to(model.device)
    outputs = model.generate(**inputs, max_new_tokens=max_new_tokens)
    return tokenizer.decode(outputs[0], skip_special_tokens=True)


def main():
    # --- Configuration ---
    # Passed by Rust orchestrator at runtime via environment variables,
    # mirroring how LinuxInferenceConfig works in provider.rs
    model_id = os.getenv("MOFA_MODEL_ID", "allenai/OLMo-1B-hf")
    max_new_tokens = int(os.getenv("MOFA_MAX_TOKENS", "100"))

    # Connect to the dora-rs message bus BEFORE loading the model.
    # This registers the node with the orchestrator immediately, so MoFA
    # knows the node exists even while the model is loading into VRAM.
    # This mirrors the loaded=false → loaded=true lifecycle in provider.rs.
    node = Node()

    # --- Initialization ---
    # Load model and signal readiness, or report failure and exit cleanly.
    # Never hang silently — always emit a status or error event.
    try:
        tokenizer, model = load_model(model_id)

        # Signal to the Rust orchestrator: node is ready to receive prompts.
        # Mirrors the is_loaded() → True transition in LinuxLocalProvider.load()
        node.send_output("status", pa.array(["ready".encode("utf-8")]))
        print("[MoFA-LLM-Node] Ready. Awaiting prompts on the bus...")

    except Exception as exc:
        # Mirrors OrchestratorError::ModelLoadFailed in provider.rs
        node.send_output(
            "error",
            pa.array([f"Init failed: {exc}".encode("utf-8")])
        )
        print(f"[MoFA-LLM-Node] Initialization failed: {exc}", file=sys.stderr)
        return

    # --- Inference Loop ---
    # Listen on the dora-rs bus for incoming prompts.
    # Mirrors the infer() method in LinuxLocalProvider.
    for event in node:
        if event["type"] != "INPUT" or event["id"] != "user_prompt":
            continue

        try:
            prompt = event["value"][0].as_py().decode("utf-8")
            print(f"[MoFA-LLM-Node] Generating response for prompt ({len(prompt)} chars)...")

            response = run_inference(tokenizer, model, prompt, max_new_tokens)

            node.send_output(
                "generated_response",
                pa.array([response.encode("utf-8")])
            )
            print("[MoFA-LLM-Node] Response sent.")

        except Exception as exc:
            # Mirrors OrchestratorError::InferenceFailed in provider.rs.
            # Never crash silently — always report errors to the orchestrator.
            node.send_output(
                "error",
                pa.array([f"Inference failed: {exc}".encode("utf-8")])
            )
            print(f"[MoFA-LLM-Node] Inference error: {exc}", file=sys.stderr)


if __name__ == "__main__":
    main()