# Local Compute Mesh Demo

Demonstration of streaming inference responses using `LocalFirstWithCloudFallback` routing policy in a MoFA compute mesh.

## Overview

This example demonstrates how the MoFA compute mesh intelligently routes inference requests between local and cloud backends based on resource availability using the `LocalFirstWithCloudFallback` routing policy.

## Features

- **LocalFirstWithCloudFallback Routing**: Tries to route requests to local compute first, falls back to cloud when local resources are exhausted
- **Memory Admission Control**: Prevents OOM by checking memory thresholds before loading models
- **Streaming Response Support**: Demonstrates the streaming inference pattern
- **Transparent Failover**: Cloud fallback happens transparently without user intervention

## Usage

```bash
# Build the demo
cargo build -p local_compute_mesh_demo

# Run the demo
cargo run -p local_compute_mesh_demo
```

## Configuration

The demo uses the following configuration (defined in `workflow.yaml`):

- **Local Memory Capacity**: 16 GB
- **Defer Threshold**: 75% - when local memory usage exceeds this, new requests may be deferred
- **Reject Threshold**: 90% - when local memory usage exceeds this, requests are rejected from local and can fall back to cloud

## Key Concepts

### Compute Mesh

The compute mesh is a logical abstraction that combines local and cloud compute resources into a unified inference backend. It automatically selects the best backend based on:
- Available memory
- Model availability
- Latency requirements
- Cost optimization

### Routing Policy

The `LocalFirstWithCloudFallback` policy:
1. Attempts to route to local compute first
2. If local memory is insufficient (exceeds defer threshold), queues the request
3. If local memory is exhausted (exceeds reject threshold), routes to cloud
4. This ensures maximum local utilization while guaranteeing availability

## Architecture

```
┌─────────────────────────────────────────┐
│         Inference Request               │
└─────────────────┬───────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────┐
│      Orchestrator (Mesh Brain)          │
│  - Memory tracking                      │
│  - Routing policy                       │
│  - Admission control                    │
└─────────────┬───────────────────────────┘
              │
     ┌────────┴────────┐
     ▼                 ▼
┌─────────┐     ┌───────────┐
│ Local   │     │  Cloud    │
│ Compute │     │  Backend  │
│ (GPU)   │     │  (API)    │
└─────────┘     └───────────┘
```

## Requirements

- Rust 1.75+
- MoFA crates (mofa-kernel, mofa-foundation)

## See Also

- [Admission Gate Demo](../admission_gate_demo) - Memory admission control demonstration
- [MoFA Foundation Inference](../crates/mofa-foundation/src/inference) - Core inference orchestration
