extends RefCounted
## The semantic-force controls, driven by `data/semantic_fields.json`: the document the Rust
## definition (`semantic_config::FIELDS`) emits, which the web registry loads too. Labels, ranges,
## steps and defaults are therefore not typed in here; a Rust test pins the committed copy of the
## document to the definition. Nothing in this module scales, clamps beyond the stored range, or
## substitutes a default for a value the server reports.

const DOC_PATH := "res://data/semantic_fields.json"
const SELF_PATH := "res://scripts/semantic_fields.gd"

var rows: Array = []        ## Array[Dictionary], document order
var groups: Array = []      ## Array[String], unique force names in document order


static func load_default() -> RefCounted:
	return from_text(FileAccess.get_file_as_string(DOC_PATH))


static func from_text(text: String) -> RefCounted:
	var f: RefCounted = (load(SELF_PATH) as GDScript).new()
	f._load(text)
	return f


func _load(text: String) -> void:
	var json := JSON.new()
	if json.parse(text) != OK or typeof(json.data) != TYPE_DICTIONARY:
		push_error("SemanticFields: %s is not a JSON object" % DOC_PATH)
		return
	var doc: Dictionary = json.data
	for r: Variant in doc.get("fields", []):
		if typeof(r) != TYPE_DICTIONARY:
			continue
		rows.append(r)
		var g: String = String(r["group"])
		if not groups.has(g):
			groups.append(g)


## Rows of one force, document order. The enable toggle is first.
func rows_for(group: String) -> Array:
	return rows.filter(func(r: Dictionary) -> bool: return r["group"] == group)


func toggle_for(group: String) -> Dictionary:
	for r: Dictionary in rows_for(group):
		if r["kind"] == "toggle":
			return r
	return {}


func sliders_for(group: String) -> Array:
	return rows_for(group).filter(func(r: Dictionary) -> bool: return r["kind"] == "slider")


## path -> default, for every row.
func defaults() -> Dictionary:
	var d := {}
	for r: Dictionary in rows:
		d[r["path"]] = r["default"]
	return d


## The value one press of -/+ gives: current moved by one `step`, snapped to the step grid
## anchored at `min`, and held inside [min, max] (the stored range, nothing tighter).
static func stepped(row: Dictionary, current: float, direction: int) -> float:
	var lo: float = float(row["min"])
	var hi: float = float(row["max"])
	var step: float = float(row["step"])
	var moved: float = current + step * float(signi(direction))
	var snapped: float = lo + roundf((moved - lo) / step) * step
	# Strip float noise (0.0300000004) by round-tripping through the step's own precision.
	var decimals: int = _decimals(step)
	snapped = snappedf(snapped, pow(10.0, -float(decimals)))
	return clampf(snapped, lo, hi)


static func _decimals(step: float) -> int:
	var s := "%s" % step
	var dot := s.find(".")
	return 0 if dot < 0 else s.length() - dot - 1


## The nested PUT body for ONE changed field: {"semantic": {group: {field: value}}}. The server
## deep-merges it, so the other forces and fields are left exactly as stored.
static func patch_for(path: String, value: Variant) -> Dictionary:
	var parts: PackedStringArray = path.split(".")
	return {"semantic": {parts[0]: {parts[1]: value}}}


## Flatten the `semantic` object of a GET /api/settings/physics body into path -> value, keeping
## only paths the document knows. Anything absent is simply not reported (no default invented).
func read_view(body: Dictionary) -> Dictionary:
	var out := {}
	var sem: Variant = body.get("semantic")
	if typeof(sem) != TYPE_DICTIONARY:
		return out
	for r: Dictionary in rows:
		var parts: PackedStringArray = String(r["path"]).split(".")
		var g: Variant = (sem as Dictionary).get(parts[0])
		if typeof(g) == TYPE_DICTIONARY and (g as Dictionary).has(parts[1]):
			out[r["path"]] = (g as Dictionary)[parts[1]]
	return out


## "0.03" / "150" / "on": how a value reads on a HUD button.
static func format_value(row: Dictionary, value: Variant) -> String:
	if row["kind"] == "toggle":
		return "on" if bool(value) else "off"
	var decimals: int = _decimals(float(row["step"]))
	return "%.*f" % [decimals, float(value)]
