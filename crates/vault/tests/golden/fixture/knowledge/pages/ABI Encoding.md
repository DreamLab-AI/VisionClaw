---
public: true
---

# ABI Encoding

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v1",
  "@id": "urn:visionflow:page:abi-encoding",
  "@type": "Page",
  "title": "ABI Encoding",
  "vc:slug": "abi-encoding",
  "vc:public": true,
  "vc:schemaVersion": 2,
  "vc:outboundWikilinks": []
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:abi-encoding",
  "@type": "Class",
  "label": "ABI Encoding",
  "definition": "ABI encoding is the Application Binary Interface encoding scheme used by the Ethereum Virtual Machine to serialise function calls and their arguments into the byte layout that smart contracts expect on-chain. It defines fixed rules for encoding primitive types, dynamic types such as strings and arrays, and nested structures, so that a compiled contract can decode calldata deterministically regardless of the source language or tool that produced it. Development toolchains such as Foundry and compilers such as Vyper generate ABI-encoded calldata automatically from a contract's interface definition.",
  "domain": "blockchain",
  "maturity": "draft",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:smart-contract",
      "label": "Smart Contract"
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
	- ABI encoding is the Application Binary Interface encoding scheme used by the Ethereum Virtual Machine to serialise function calls and their arguments into the byte layout that smart contracts expect on-chain. It defines fixed rules for encoding primitive types, dynamic types such as strings and arrays, and nested structures, so that a compiled contract can decode calldata deterministically regardless of the source language or tool that produced it. Development toolchains such as Foundry and compilers such as Vyper generate ABI-encoded calldata automatically from a contract's interface definition.
	- It is a [[Smart Contract]] concept.
- ### Relationships
	- subClassOf:: [[Smart Contract]]
- ### Provenance
	- updated:: 2026-07-25
	- attributedTo:: did:nostr:ontology-mesh
	- inferenceRule:: GapMaterialisation
