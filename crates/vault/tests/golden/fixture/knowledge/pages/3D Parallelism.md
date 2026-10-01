---
type: Class
title: 3D Parallelism
resource: urn:ngm:class:3d-parallelism
page_resource: urn:visionflow:page:3d-parallelism
public: true
status: stable
domain: machine-learning
maturity: draft
quality: 0.5
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[distributed-training|Distributed Training]]'
---

3D parallelism is a distributed training strategy that combines data parallelism, tensor parallelism and pipeline parallelism along three independent axes to train models too large for a single accelerator or single parallelism scheme alone. Each axis partitions a different dimension of the problem: data parallelism splits the batch, tensor parallelism splits individual layers across devices, and pipeline parallelism splits the layer stack across stages. Frameworks such as Megatron-LM and DeepSpeed implement 3D parallelism to scale training to thousands of GPUs.

### Provenance
