\# Native Python LLM Provider Node



A native `dora-rs` Python node that runs a local HuggingFace model directly

on GPU via the MoFA message bus — no HTTP server, no network hop.



\## How It Fits Into MoFA



This is the Python-side counterpart to `crates/mofa-local-llm/src/provider.rs`.



| Component | Language | Role |

|---|---|---|

| `LinuxLocalProvider` (provider.rs) | Rust | Hardware detection, backend dispatch |

| `native\_llm\_node.py` (this file) | Python | HuggingFace tokenization, GPU inference |



Unlike `examples/python\_bindings/01\_llm\_agent.py` which calls an external

OpenAI HTTP API, this node communicates entirely through the internal

dora-rs message bus using zero-copy PyArrow arrays.



\## Setup

```bash

pip install dora-rs transformers torch pyarrow

```



\## Configuration



| Variable | Default | Description |

|---|---|---|

| `MOFA\_MODEL\_ID` | `allenai/OLMo-1B-hf` | HuggingFace model to load |

| `MOFA\_MAX\_TOKENS` | `100` | Max tokens to generate per response |



\## Running

```bash

MOFA\_MODEL\_ID=allenai/OLMo-1B-hf python native\_llm\_node.py

```



\## Bus Ports



| Port | Direction | Description |

|---|---|---|

| `user\_prompt` | INPUT | Prompt string from Rust orchestrator |

| `generated\_response` | OUTPUT | Model's generated response |

| `status` | OUTPUT | Emits `"ready"` once VRAM is loaded |

| `error` | OUTPUT | Structured error if init or inference fails |



\## Related



\- Issue: \[mofa-org/mofa#820](https://github.com/mofa-org/mofa/issues/820)

\- Author: Samuel Alaba (\[@samuelmohel](https://github.com/samuelmohel))

