---
type: Class
title: ABI Encoding
resource: urn:ngm:class:abi-encoding
page_resource: urn:visionflow:page:abi-encoding
public: true
status: stable
domain: blockchain
maturity: draft
quality: 0.5
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[smart-contract|Smart Contract]]'
---

ABI encoding is the Application Binary Interface encoding scheme used by the Ethereum Virtual Machine to serialise function calls and their arguments into the byte layout that smart contracts expect on-chain. It defines fixed rules for encoding primitive types, dynamic types such as strings and arrays, and nested structures, so that a compiled contract can decode calldata deterministically regardless of the source language or tool that produced it. Development toolchains such as Foundry and compilers such as Vyper generate ABI-encoded calldata automatically from a contract's interface definition.

### Provenance
