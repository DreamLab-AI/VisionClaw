extends RefCounted

## Writes the scene's pointer and query state into each connection's per-node
## highlight mask (ADR-2188 D4, ADR-2183 semantics). The mask itself lives in
## Rust (`highlight.rs`, one byte per render-store slot) and is applied on the
## per-frame pack path, so a write here never invalidates a pack plan.
##
## Bits (must match rust/src/highlight.rs):
##   SELECTED 1 · HOVERED 2 · SEARCH_HIT 4 · FILTERED_OUT 8 · FOCUS_NEIGHBOUR 16
## FILTERED_OUT is written by the filters inside Rust (the received nodeFilter and
## the type show/hide filter), never from here. Precedence (Rust `level`): selected
## or hovered outranks a filter; a filter outranks search hits and neighbours.
##
## Writes are event-driven and deduplicated: `write` compares against the last ids
## sent for that connection and bit and calls into Rust only on a change.

const SELECTED: int = 1
const HOVERED: int = 2
const SEARCH_HIT: int = 4
const FILTERED_OUT: int = 8
const FOCUS_NEIGHBOUR: int = 16
## Bits this writer owns.
const BITS: Array[int] = [SELECTED, HOVERED, SEARCH_HIT, FOCUS_NEIGHBOUR]
## The ray must have reported a node this recently for it to count as hovered.
const HOVER_HOLD_MS: int = 250
## Most selection neighbours emphasised (ADR-2186's selection tier is 100 labels;
## emphasis is cheaper than a label, but a hub's whole neighbourhood is not focus).
const NEIGHBOUR_MAX: int = 64

## conn_id → {bit → PackedInt32Array last written}
var _written: Dictionary = {}
var writes: int = 0


## Set `bit` on connection `conn_id` (whose client is `client`) to exactly `ids`.
## Returns true when Rust was called (the ids differ from the last write).
func write(conn_id: StringName, client: RefCounted, bit: int, ids: PackedInt32Array) -> bool:
	if client == null or not client.has_method("set_highlight_ids") or not BITS.has(bit):
		return false
	var per: Dictionary = _written.get(conn_id, {})
	if per.has(bit) and per[bit] == ids:
		return false
	per[bit] = ids
	_written[conn_id] = per
	client.set_highlight_ids(bit, ids)
	writes += 1
	return true


## Clear every bit this writer set on `conn_id` (selection dropped, connection reset).
func clear_conn(conn_id: StringName, client: RefCounted) -> void:
	for bit: int in BITS:
		write(conn_id, client, bit, PackedInt32Array())


## Forget what was written for `conn_id` (its store was reset by a new graph).
func forget(conn_id: StringName) -> void:
	_written.erase(conn_id)


## The ids last written for `conn_id` / `bit` (empty when none).
func written(conn_id: StringName, bit: int) -> PackedInt32Array:
	var per: Dictionary = _written.get(conn_id, {})
	return per.get(bit, PackedInt32Array())


## Direct neighbours of `id` on `client`: Rust's one-pass lookup when the client
## has it (BinaryProtocolClient.neighbours_of), else `neighbours_of` over `pairs`.
static func neighbours_for(client: RefCounted, id: int, max_n: int = NEIGHBOUR_MAX) -> PackedInt32Array:
	if client == null or id < 0:
		return PackedInt32Array()
	if client.has_method("neighbours_of"):
		return client.neighbours_of(id, max_n)
	if client.has_method("get_edges"):
		return neighbours_of(client.get_edges(), id, max_n)
	return PackedInt32Array()


## Direct neighbours of `id` in a flat pair list, without duplicates, at most
## `max_n`: the GDScript mirror of Rust `neighbours_in`, for clients without it.
static func neighbours_of(pairs: PackedInt32Array, id: int, max_n: int = NEIGHBOUR_MAX) -> PackedInt32Array:
	var out := PackedInt32Array()
	var seen := {}
	var n: int = pairs.size() / 2
	for i: int in range(n):
		if out.size() >= max_n:
			break
		var a: int = pairs[i * 2]
		var b: int = pairs[i * 2 + 1]
		var other: int = -1
		if a == id:
			other = b
		elif b == id:
			other = a
		if other >= 0 and other != id and not seen.has(other):
			seen[other] = true
			out.append(other)
	return out


## Whether a node reported at `targeted_ms` still counts as hovered at `now_ms`.
static func hover_live(targeted_ms: int, now_ms: int) -> bool:
	return targeted_ms >= 0 and now_ms - targeted_ms <= HOVER_HOLD_MS
