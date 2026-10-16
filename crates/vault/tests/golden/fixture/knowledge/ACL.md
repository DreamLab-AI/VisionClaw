---
type: Class
title: ACL
resource: urn:ngm:class:acl
page_resource: urn:visionflow:page:acl
public: true
status: stable
domain: security
maturity: established
quality: 0.8
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[access-control|Access Control]]'
contrasts-with:
- '[[policy-enforcement-point|Policy Enforcement Point]]'
enables:
- '[[policy-enforcement|Policy Enforcement]]'
- '[[zero-trust-architecture|Zero Trust Architecture]]'
related-to:
- '[[identity-and-access-management|Identity and Access Management]]'
- '[[access-control-system|Access Control System]]'
- '[[digital-identity|Digital Identity]]'
uses:
- '[[encryption|Encryption]]'
- '[[authentication|Authentication]]'
---

An Access Control List (ACL) is a data structure attached to a resource — such as a file, directory, or network interface — that enumerates which subjects (users, groups, or processes) are permitted to perform which operations on that resource. Each entry in the list is called an Access Control Entry (ACE) and specifies a principal, a set of permissions (read, write, execute, delete), and whether those permissions are granted or denied. ACLs originated in file-system security (POSIX, NTFS) and were subsequently extended to networking, where routers and firewalls use IP-level ACLs to filter packets by source address, destination port, and protocol. In distributed and cloud environments ACLs underpin fine-grained authorisation that complements role-based and attribute-based access-control models.

### Content

- Access Control Lists are one of the oldest and most widely deployed security primitives in computing. Their origins trace to the Multics operating system in the 1960s, but they became ubiquitous through POSIX filesystem semantics (owner/group/other permission bits extended to named ACLs in POSIX.1e), Microsoft NTFS, and network router access lists in Cisco IOS. Each ACE within a list carries a trustee identifier, a rights mask, and an allow/deny flag; the operating system evaluates ACEs in sequence until a matching entry is found or the list is exhausted.
- In networking contexts, IP ACLs are applied to router interfaces or firewall rule sets to filter traffic by source and destination IP address, TCP/UDP port, and protocol type. Standard ACLs filter on source address alone, while extended ACLs inspect both source and destination as well as protocol metadata. Network ACLs are stateless by default — each packet is evaluated independently — making them complementary to stateful firewall inspection rather than a replacement.
- Modern cloud and distributed systems have extended ACL concepts to object storage (bucket policies), API gateways, Kubernetes RBAC, and smart-contract permission systems. Despite the growth of role-based and attribute-based access control, ACLs remain important for per-resource granularity that role models alone cannot express. They are routinely audited for privilege creep, and automated tooling increasingly generates and validates ACL entries against declared security policies to reduce misconfiguration risk.
