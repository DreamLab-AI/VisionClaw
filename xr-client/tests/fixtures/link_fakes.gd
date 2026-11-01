extends RefCounted
## Fakes for the link-layer tests. A volume is duck-typed (a content-to-world transform and its
## visibility); a connection owns a REAL gdext `BinaryProtocolClient`, because the Rust pack reads
## node positions from the connection's render store and there is no GDScript position path to fake.


class FakeVolume extends RefCounted:
	var offset := Vector3.ZERO
	var visible_state := true
	var is_closed := false

	func content_to_world() -> Transform3D:
		return Transform3D(Basis.IDENTITY, offset)

	func world_from_content(p: Vector3) -> Vector3:
		return content_to_world() * p


class FakeConn extends RefCounted:
	var conn_id: StringName
	var generation := 1
	var connected := true
	var client: RefCounted = null

	func _init(id: StringName) -> void:
		conn_id = id
		client = BinaryProtocolClient.create()
		# Every id the tests name sits at (id, 0, 0) unless a test moves it.
		var ids := PackedInt32Array()
		var at := PackedVector3Array()
		for i: int in range(0, 5600):
			ids.append(i)
			at.append(Vector3(float(i), 0.0, 0.0))
		seed_nodes(ids, at)

	## Place nodes through the client's decode door, then snap the drawn position onto them.
	func seed_nodes(ids: PackedInt32Array, at: PackedVector3Array) -> void:
		var b := PackedByteArray()
		b.append(0x03)
		for i: int in ids.size():
			var rec := PackedByteArray()
			rec.resize(52)
			rec.encode_u32(0, ids[i])
			rec.encode_float(4, at[i].x)
			rec.encode_float(8, at[i].y)
			rec.encode_float(12, at[i].z)
			b.append_array(rec)
		client.ingest(b)
		client.hunt(1.0, -1, Vector3.ZERO)

	func move_node(id: int, to: Vector3) -> void:
		seed_nodes(PackedInt32Array([id]), PackedVector3Array([to]))

	func is_connected_now() -> bool:
		return connected
