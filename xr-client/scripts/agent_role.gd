extends RefCounted

## Agent role vocabulary for embodiment (embodiment plan §2.2, revived from the
## 2025 desktop type→geometry vocabulary): six roles, each with a distinct
## silhouette ("frame" of struts around the core), an accent colour and a
## two-letter badge — colour alone is never the only cue. Frames are built
## procedurally at runtime from box struts (SurfaceTool → one cached ArrayMesh
## per role, shared by every avatar of that role), so no external assets.
##
## Roles are inferred from an agent's display name and task text; anything
## unrecognised gets the neutral "generic" hoop.

const FRAME_HALF_WIDTH := 0.21      # overall width ≈ 0.42 m
const STRUT := 0.018                # strut thickness (m)
const RING_SEGMENTS := 28

const ROLES: Dictionary = {
	"architect": {"badge": "AR", "label": "Architect", "color": Color("#56CFE1")},
	"analyst":   {"badge": "AN", "label": "Analyst",   "color": Color("#F6BD60")},
	"coder":     {"badge": "CO", "label": "Coder",     "color": Color("#7B9EFF")},
	"reviewer":  {"badge": "RE", "label": "Reviewer",  "color": Color("#D98ACD")},
	"tester":    {"badge": "TE", "label": "Tester",    "color": Color("#8FD175")},
	"optimizer": {"badge": "OP", "label": "Optimizer", "color": Color("#F08A62")},
	"generic":   {"badge": "AG", "label": "Agent",     "color": Color(0.78, 0.82, 0.9)},
}

# Keyword → role, checked in order (first hit wins). Lower-case substrings.
const KEYWORDS: Array = [
	["architect", "architect"], ["design", "architect"], ["mapping", "architect"],
	["analy", "analyst"], ["research", "analyst"],
	["coder", "coder"], ["coding", "coder"], ["refactor", "coder"], ["implement", "coder"], ["develop", "coder"],
	["review", "reviewer"], ["audit", "reviewer"], ["judge", "reviewer"],
	["tester", "tester"], ["testing", "tester"], ["test", "tester"], ["qa", "tester"], ["verif", "tester"],
	["optimi", "optimizer"], ["perf", "optimizer"], ["tuning", "optimizer"],
]

static var _mesh_cache: Dictionary = {}


static func role_keys() -> Array:
	return ["architect", "analyst", "coder", "reviewer", "tester", "optimizer"]


static func is_role(key: String) -> bool:
	return ROLES.has(key)


static func badge_of(key: String) -> String:
	return String((ROLES.get(key, ROLES["generic"]) as Dictionary)["badge"])


static func color_of(key: String) -> Color:
	return (ROLES.get(key, ROLES["generic"]) as Dictionary)["color"]


static func label_of(key: String) -> String:
	return String((ROLES.get(key, ROLES["generic"]) as Dictionary)["label"])


## Infer a role from free text (display name + task line). Name wins over task
## because the task verb of a generic agent can mention testing or review.
static func infer(display_name: String, task: String = "") -> String:
	var from_name: String = _match(display_name)
	if from_name != "generic":
		return from_name
	return _match(task)


static func _match(text: String) -> String:
	var t: String = text.to_lower()
	if t.is_empty():
		return "generic"
	for pair: Array in KEYWORDS:
		if t.contains(String(pair[0])):
			return String(pair[1])
	return "generic"


## One shared ArrayMesh per role (built on first use).
static func frame_mesh(key: String) -> ArrayMesh:
	var k: String = key if ROLES.has(key) else "generic"
	if _mesh_cache.has(k):
		return _mesh_cache[k]
	var mesh: ArrayMesh = _build_frame(k)
	_mesh_cache[k] = mesh
	return mesh


static func _build_frame(key: String) -> ArrayMesh:
	var st := SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)
	var w: float = FRAME_HALF_WIDTH
	match key:
		"architect":
			# Square bracket cage: "[ ]" in the XY plane and again in the ZY plane.
			for plane: int in range(2):
				for side: float in [-1.0, 1.0]:
					var x: float = side * w
					_strut(st, _p(plane, x, -0.17, 0.0), _p(plane, x, 0.17, 0.0))
					_strut(st, _p(plane, x, 0.17, 0.0), _p(plane, side * (w - 0.09), 0.17, 0.0))
					_strut(st, _p(plane, x, -0.17, 0.0), _p(plane, side * (w - 0.09), -0.17, 0.0))
		"analyst":
			# One ring, tilted 30° about X so it reads as a ring from the front too.
			_ring(st, w, Basis(Vector3.RIGHT, deg_to_rad(30.0)))
		"coder":
			# Two opposing chevrons "< >" in XY, repeated in ZY.
			for plane: int in range(2):
				for side: float in [-1.0, 1.0]:
					var tip: float = side * w
					var base: float = side * (w - 0.10)
					_strut(st, _p(plane, base, 0.16, 0.0), _p(plane, tip, 0.0, 0.0))
					_strut(st, _p(plane, tip, 0.0, 0.0), _p(plane, base, -0.16, 0.0))
		"reviewer":
			# Diamond frame in XY, repeated in ZY.
			for plane: int in range(2):
				var pts: Array = [_p(plane, 0.0, w, 0.0), _p(plane, w, 0.0, 0.0), _p(plane, 0.0, -w, 0.0), _p(plane, -w, 0.0, 0.0)]
				for i: int in range(4):
					_strut(st, pts[i], pts[(i + 1) % 4])
		"tester":
			# Three radial fins at 120°, each a spoke plus a short cap, in XY.
			for i: int in range(3):
				var ang: float = deg_to_rad(90.0 + 120.0 * float(i))
				var dir := Vector3(cos(ang), sin(ang), 0.0)
				var tangent := Vector3(-sin(ang), cos(ang), 0.0)
				_strut(st, dir * 0.15, dir * w)
				_strut(st, dir * w - tangent * 0.05, dir * w + tangent * 0.05)
		"optimizer":
			# Two parallel horizontal hoops above and below the equator.
			_ring(st, w - 0.02, Basis.IDENTITY, Vector3(0.0, 0.09, 0.0))
			_ring(st, w - 0.02, Basis.IDENTITY, Vector3(0.0, -0.09, 0.0))
		_:
			# Generic: a single horizontal hoop.
			_ring(st, w - 0.02, Basis.IDENTITY)
	st.generate_normals()
	return st.commit()


# Point in the XY plane (plane 0) or the ZY plane (plane 1).
static func _p(plane: int, x: float, y: float, z: float) -> Vector3:
	return Vector3(x, y, z) if plane == 0 else Vector3(z, y, x)


# A ring of struts in the XZ plane of `basis`, radius r, centred at `centre`.
static func _ring(st: SurfaceTool, r: float, basis: Basis, centre: Vector3 = Vector3.ZERO) -> void:
	var prev: Vector3 = centre + basis * Vector3(r, 0.0, 0.0)
	for i: int in range(1, RING_SEGMENTS + 1):
		var ang: float = TAU * float(i) / float(RING_SEGMENTS)
		var p: Vector3 = centre + basis * Vector3(cos(ang) * r, 0.0, sin(ang) * r)
		_strut(st, prev, p, STRUT * 0.8)
		prev = p


# One box strut from a to b: 8 corners, 12 triangles, outward normals.
static func _strut(st: SurfaceTool, a: Vector3, b: Vector3, thickness: float = STRUT) -> void:
	var axis: Vector3 = b - a
	var len: float = axis.length()
	if len < 0.0005:
		return
	var y: Vector3 = axis / len
	var up := Vector3.UP if absf(y.dot(Vector3.UP)) < 0.99 else Vector3.RIGHT
	var x: Vector3 = up.cross(y).normalized()
	var z: Vector3 = x.cross(y).normalized()
	var hx: Vector3 = x * thickness * 0.5
	var hz: Vector3 = z * thickness * 0.5
	var mid: Vector3 = (a + b) * 0.5
	var hy: Vector3 = y * len * 0.5
	var c: Array = [
		mid - hx - hy - hz, mid + hx - hy - hz, mid + hx + hy - hz, mid - hx + hy - hz,
		mid - hx - hy + hz, mid + hx - hy + hz, mid + hx + hy + hz, mid - hx + hy + hz,
	]
	_quad(st, c[0], c[3], c[2], c[1], -z)
	_quad(st, c[4], c[5], c[6], c[7], z)
	_quad(st, c[0], c[1], c[5], c[4], -y)
	_quad(st, c[3], c[7], c[6], c[2], y)
	_quad(st, c[0], c[4], c[7], c[3], -x)
	_quad(st, c[1], c[2], c[6], c[5], x)


static func _quad(st: SurfaceTool, a: Vector3, b: Vector3, c: Vector3, d: Vector3, n: Vector3) -> void:
	for v: Vector3 in [a, b, c, a, c, d]:
		st.set_normal(n)
		st.add_vertex(v)


## Approximate triangle count of a role's frame (for budget assertions).
static func triangle_count(key: String) -> int:
	var m: ArrayMesh = frame_mesh(key)
	if m.get_surface_count() == 0:
		return 0
	return int(m.surface_get_array_len(0) / 3)
