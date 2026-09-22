#!/usr/bin/env python3
"""Pack the vault pipeline's build output into the pod's /public/ontology resources.

Input:  <vault_build_dir> as written by `python -m pipeline.build` in the
        `vault build` output (data/ontology.ttl, context/v1.jsonld,
        api/search-index.json).
Output: <out_dir>/ontology-ttl/{visionflow.ttl,visionflow.stats.json}
        <out_dir>/ontology-jsonld/{ontology.jsonld,context.jsonld,index.jsonld}

The Turtle is the vault's own public-only OWL graph, copied byte-for-byte; the
JSON-LD is that graph compacted against the vault's own context, so both
resources carry the narrativegoldmine.com IRI scheme that the public site
serves. A substance floor refuses to pack an ontology that has collapsed:
run 34045488066 shipped 0 classes with valid syntax and matching digests
because every gate checked form, not content.
"""
import hashlib
import json
import os
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

from rdflib import Graph, URIRef
from rdflib.namespace import OWL, RDF

# Half of the 2026-09-06 vault (8,434 classes / 265,455 triples). A legitimate
# corpus change that halves the ontology should move these numbers on purpose.
MIN_CLASSES = 4000
VC_NS = "https://narrativegoldmine.com/ns/v1#"
MIN_TRIPLES = 100_000


def sha1(path: Path) -> str:
    return hashlib.sha1(path.read_bytes()).hexdigest()


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: pack-pod-resources.py <vault_build_dir> <out_dir>", file=sys.stderr)
        return 2
    build = Path(sys.argv[1])
    out = Path(sys.argv[2])
    ttl_src = build / "data" / "ontology.ttl"
    ctx_src = build / "context" / "v1.jsonld"  # `vault build` layout (ns/v2.jsonld is the same document)
    idx_src = build / "api" / "search-index.json"
    for p in (ttl_src, ctx_src, idx_src):
        if not p.is_file():
            print(f"::error title=Vault build incomplete::missing {p}", file=sys.stderr)
            return 1

    g = Graph()
    g.parse(str(ttl_src), format="turtle")
    # Count corpus classes only: the emitter also declares schema-support classes
    # (skos:Concept, ngm:MaturityLevel) as owl:Class; a corpus class is one a page
    # minted, and every page-minted class carries a vc:slug. The corpus-class count is the
    # number Loom /health, VisionClaw /api/ontology/classes and `vault build --stats`
    # report (PRD-sovereign-corpus acceptance 2), so the pod reports it too.
    slugged = set(g.subjects(URIRef(VC_NS + "slug"), None))
    classes = len({c for c in g.subjects(RDF.type, OWL.Class) if c in slugged})
    properties = len(
        set(g.subjects(RDF.type, OWL.ObjectProperty))
        | set(g.subjects(RDF.type, OWL.DatatypeProperty))
        | set(g.subjects(RDF.type, OWL.AnnotationProperty))
    )
    pages = len(json.loads(idx_src.read_text()))
    print(f"vault graph: {len(g)} triples, {classes} owl:Class, {properties} properties, {pages} pages")
    if classes < MIN_CLASSES or len(g) < MIN_TRIPLES:
        print(
            f"::error title=Ontology substance floor::{classes} classes / {len(g)} triples "
            f"is below the floor of {MIN_CLASSES} / {MIN_TRIPLES}; refusing to pack an empty ontology",
            file=sys.stderr,
        )
        return 1

    ttl_dir = out / "ontology-ttl"
    ld_dir = out / "ontology-jsonld"
    ttl_dir.mkdir(parents=True, exist_ok=True)
    ld_dir.mkdir(parents=True, exist_ok=True)

    ttl_dst = ttl_dir / "visionflow.ttl"
    shutil.copyfile(ttl_src, ttl_dst)
    ttl_sha = sha1(ttl_dst)
    (ttl_dir / "visionflow.stats.json").write_text(json.dumps({
        "triples": len(g),
        "classes": classes,
        "properties": properties,
        "pages_processed": pages,
        "sha1": ttl_sha,
    }, indent=2))

    ctx_dst = ld_dir / "context.jsonld"
    shutil.copyfile(ctx_src, ctx_dst)
    context = json.loads(ctx_src.read_text())["@context"]

    ld_dst = ld_dir / "ontology.jsonld"
    ld_dst.write_text(g.serialize(format="json-ld", context=context, indent=None))
    ld_sha = sha1(ld_dst)
    ctx_sha = sha1(ctx_dst)

    manifest = {
        "@context": "https://www.w3.org/ns/ldp",
        "@id": "/public/ontology/",
        "@type": ["ldp:Container", "ldp:BasicContainer"],
        "dcterms:title": "VisionFlow Ontology Federation",
        "dcterms:modified": datetime.now(timezone.utc).isoformat(),
        "ldp:contains": [
            {"@id": "ontology.jsonld", "@type": "ldp:Resource",
             "dcterms:format": "application/ld+json", "digest:sha1": ld_sha},
            {"@id": "context.jsonld", "@type": "ldp:Resource",
             "dcterms:format": "application/ld+json", "digest:sha1": ctx_sha},
            {"@id": "visionflow.ttl", "@type": "ldp:Resource",
             "dcterms:format": "text/turtle", "digest:sha1": ttl_sha},
        ],
        "visionflow:sourceRepository": os.environ.get("ONTOLOGY_SOURCE_REPO", ""),
        "visionflow:sourceSha": os.environ.get("ONTOLOGY_SOURCE_SHA", ""),
        "visionflow:buildSha": os.environ.get("GITHUB_SHA", ""),
        "visionflow:buildNumber": os.environ.get("GITHUB_RUN_NUMBER", ""),
        "visionflow:classes": classes,
        "visionflow:triples": len(g),
    }
    (ld_dir / "index.jsonld").write_text(json.dumps(manifest, indent=2))

    gh_out = os.environ.get("GITHUB_OUTPUT")
    if gh_out:
        with open(gh_out, "a") as f:
            f.write(f"ttl_sha={ttl_sha}\njsonld_sha={ld_sha}\ncontext_sha={ctx_sha}\nstatus=success\n")
    print(f"packed: visionflow.ttl {ttl_sha}  ontology.jsonld {ld_sha}  context.jsonld {ctx_sha}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
