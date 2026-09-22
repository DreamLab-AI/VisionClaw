---
public: true
---

# 3D Parallelism

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v1",
  "@id": "urn:visionflow:page:3d-parallelism",
  "@type": "Page",
  "title": "3D Parallelism",
  "vc:slug": "3d-parallelism",
  "vc:public": true,
  "vc:schemaVersion": 2,
  "vc:outboundWikilinks": []
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:3d-parallelism",
  "@type": "Class",
  "label": "3D Parallelism",
  "definition": "3D parallelism is a distributed training strategy that combines data parallelism, tensor parallelism and pipeline parallelism along three independent axes to train models too large for a single accelerator or single parallelism scheme alone. Each axis partitions a different dimension of the problem: data parallelism splits the batch, tensor parallelism splits individual layers across devices, and pipeline parallelism splits the layer stack across stages. Frameworks such as Megatron-LM and DeepSpeed implement 3D parallelism to scale training to thousands of GPUs.",
  "domain": "machine-learning",
  "maturity": "draft",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:distributed-training",
      "label": "Distributed Training"
    }
  ],
  "relations": {},
  "sameAs": [],
  "quality": 0.5,
  "provenance": {
    "attributedTo": "did:nostr:ontology-mesh",
    "generatedAt": "2026-07-25T00:00:00Z",
    "inferenceRule": "GapMaterialisation"
  }
}
```

- ### Definition
	- 3D parallelism is a distributed training strategy that combines data parallelism, tensor parallelism and pipeline parallelism along three independent axes to train models too large for a single accelerator or single parallelism scheme alone. Each axis partitions a different dimension of the problem: data parallelism splits the batch, tensor parallelism splits individual layers across devices, and pipeline parallelism splits the layer stack across stages. Frameworks such as Megatron-LM and DeepSpeed implement 3D parallelism to scale training to thousands of GPUs.
	- It is a [[Distributed Training]] concept.
- ### Relationships
	- subClassOf:: [[Distributed Training]]
- ### Provenance
	- updated:: 2026-07-25
	- attributedTo:: did:nostr:ontology-mesh
	- inferenceRule:: GapMaterialisation
