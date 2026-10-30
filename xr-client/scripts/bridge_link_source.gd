extends RefCounted
## Used via `const BridgeLinkSource := preload("res://scripts/bridge_link_source.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it (see backend_caps.gd).

## Knowledge ↔ ontology bridge links for the cross-volume link layer
## (ADR-2158 D6, ADR-2135, ADR-2164 D5).
##
## Once the knowledge and ontology volumes are separate, an edge of the
## VisionClaw topology whose two ends sit in different volumes can no longer
## be drawn inside either volume. This turns those edges into link records of
## kind `bridge`, the strongest MAX_BRIDGES first. Pure: it reads the
## connection's client and returns Dictionaries in the ADR-2164 D5 shape.

const MAX_BRIDGES := 256
const KIND := "bridge"
const VIA := "visionclaw-edge"
## The two volumes a bridge joins.
const VOLUMES: Array[StringName] = [&"knowledge", &"ontology"]


## Bridge records for `conn` (a BackendConnection, or any object with
## `conn_id`, `generation` and a `client` exposing `get_edges()` and
## `get_edge_weights()`). `ids_by_volume` maps a volume id to the
## PackedInt32Array of wire ids that volume draws. Ranked by weight
## (descending), ties by source then target id; at most MAX_BRIDGES.
static func records(conn: Object, ids_by_volume: Dictionary) -> Array:
	if conn == null:
		return []
	var client: Object = conn.get("client")
	if client == null or not client.has_method("get_edges"):
		return []
	var volume_of := {}
	for vol: StringName in VOLUMES:
		var ids: Variant = ids_by_volume.get(vol, ids_by_volume.get(String(vol), PackedInt32Array()))
		for id: int in ids:
			volume_of[id] = vol
	if volume_of.is_empty():
		return []
	var edges: PackedInt32Array = client.get_edges()
	var weights := PackedFloat32Array()
	if client.has_method("get_edge_weights"):
		weights = client.get_edge_weights()
	var cands: Array = []
	for i: int in range(edges.size() / 2):
		var s: int = edges[2 * i]
		var t: int = edges[2 * i + 1]
		var vs: Variant = volume_of.get(s)
		var vt: Variant = volume_of.get(t)
		if vs == null or vt == null or vs == vt:
			continue
		var w: float = weights[i] if i < weights.size() else 1.0
		cands.append([w, s, t, vs, vt])
	cands.sort_custom(_stronger)
	var conn_id := String(conn.get("conn_id"))
	var gen: int = int(conn.get("generation"))
	var out: Array = []
	for c: Array in cands.slice(0, MAX_BRIDGES):
		out.append({
			"source": {"volume": String(c[3]), "conn": conn_id, "key": str(c[1]), "id": c[1]},
			"target": {"volume": String(c[4]), "conn": conn_id, "key": str(c[2]), "id": c[2]},
			"kind": KIND,
			"provenance": {"via": VIA, "weight": c[0], "generations": [gen, gen]},
		})
	return out


static func _stronger(a: Array, b: Array) -> bool:
	if a[0] != b[0]:
		return a[0] > b[0]
	if a[1] != b[1]:
		return a[1] < b[1]
	return a[2] < b[2]
