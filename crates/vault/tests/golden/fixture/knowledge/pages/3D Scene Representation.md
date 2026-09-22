---
public: true
---

# 3D Scene Representation

```json-ld
{ "@context":"https://narrativegoldmine.com/ns/v1", "@id":"urn:visionflow:page:3-d-scene-representation", "@type":"Page", "title":"3D Scene Representation", "vc:slug":"3-d-scene-representation", "vc:public":true, "vc:schemaVersion":2, "vc:outboundWikilinks":[] }
```

```json-ld
{
  "@context":"https://narrativegoldmine.com/ns/v2.jsonld",
  "@id":"urn:ngm:class:3-d-scene-representation",
  "@type":"Class",
  "label":"3D Scene Representation",
  "definition":"A 3D scene representation is a data structure that encodes the geometry, appearance and spatial layout of a three-dimensional environment, such as point clouds, meshes, voxel grids or neural implicit fields. It underpins 3D content generation and reconstruction pipelines, providing the intermediate format from which renderable or editable scenes are produced. Point clouds are one common instance, forming a sparse, unstructured representation that is often converted into denser structures for downstream use.",
  "domain":"spatial-computing",
  "maturity":"draft",
  "subClassOf":[{"@id":"urn:ngm:class:scene-representation","label":"Scene Representation"}],
  "relations":{
    "partOf":[{"@id":"urn:ngm:class:point-cloud","label":"Point Cloud"}]
  },
  "sameAs":[],
  "quality":0.5,
  "provenance":{"attributedTo":"did:nostr:ontology-mesh","generatedAt":"2026-07-25T00:00:00Z","inferenceRule":"GapMaterialisation"}
}
```

- ### Definition
	- [[3D Scene Representation]] A 3D scene representation is a data structure that encodes the geometry, appearance and spatial layout of a three-dimensional environment, such as point clouds, meshes, voxel grids or neural implicit fields. It underpins 3D content generation and reconstruction pipelines, providing the intermediate format from which renderable or editable scenes are produced. Point clouds are one common instance, forming a sparse, unstructured representation that is often converted into denser structures for downstream use.
- ### Relationships
	- subClassOf:: [[Scene Representation]]
	- partOf:: [[Point Cloud]]
- ### Provenance
	- updated:: 2026-07-25
	- attributedTo:: did:nostr:ontology-mesh
	- inferenceRule:: GapMaterialisation
