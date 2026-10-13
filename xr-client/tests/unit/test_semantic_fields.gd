extends "res://addons/gut/test.gd"

# The XR semantic-force controls, driven by the shared document (data/semantic_fields.json).

const Fields := preload("res://scripts/semantic_fields.gd")


func test_the_document_has_eight_forces_and_thirty_five_controls() -> void:
	var f: RefCounted = Fields.load_default()
	assert_eq((f.rows as Array).size(), 35, "35 controls")
	assert_eq((f.groups as Array).size(), 8, "8 forces")
	for g: String in (f.groups as Array):
		assert_eq((f.toggle_for(g) as Dictionary).get("kind"), "toggle", "%s has one enable toggle" % g)
		var n: int = (f.sliders_for(g) as Array).size()
		assert_between(n, 3, 4, "%s has 3-4 sliders (fits the stepper rows)" % g)


func test_every_default_lies_inside_its_range() -> void:
	var f: RefCounted = Fields.load_default()
	for r: Dictionary in (f.rows as Array):
		if r["kind"] == "slider":
			assert_between(float(r["default"]), float(r["min"]), float(r["max"]), r["path"])
		else:
			assert_typeof(r["default"], TYPE_BOOL)


func test_stepping_stays_on_the_grid_and_inside_the_stored_range() -> void:
	var row := {"min": 0.0, "max": 1.0, "step": 0.005}
	assert_almost_eq(Fields.stepped(row, 0.03, 1), 0.035, 0.0000001)
	assert_almost_eq(Fields.stepped(row, 0.03, -1), 0.025, 0.0000001)
	assert_eq(Fields.stepped(row, 1.0, 1), 1.0, "held at max")
	assert_eq(Fields.stepped(row, 0.0, -1), 0.0, "held at min")
	# repeated presses never drift off the grid
	var v: float = 0.0
	for i in 200:
		v = Fields.stepped(row, v, 1)
	assert_almost_eq(v, 1.0, 0.0000001)
	var wide := {"min": 10.0, "max": 1000.0, "step": 10.0}
	assert_eq(Fields.stepped(wide, 150.0, 1), 160.0)
	assert_eq(Fields.stepped(wide, 10.0, -1), 10.0)


func test_a_patch_nests_exactly_one_field() -> void:
	var p: Dictionary = Fields.patch_for("typeCluster.clusterRadius", 160.0)
	assert_eq(p, {"semantic": {"typeCluster": {"clusterRadius": 160.0}}})


func test_reading_a_server_body_reports_only_what_the_server_sent() -> void:
	var f: RefCounted = Fields.load_default()
	var body := {"semantic": {"dag": {"levelAttraction": 0.07, "enabled": true}}}
	var view: Dictionary = f.read_view(body)
	assert_eq(view.size(), 2)
	assert_eq(view["dag.levelAttraction"], 0.07)
	assert_eq(view["dag.enabled"], true)
	assert_false(view.has("collision.nodeRadius"), "an absent field is not invented")
	assert_eq((f.read_view({}) as Dictionary).size(), 0)


func test_values_format_to_the_precision_of_their_step() -> void:
	var f: RefCounted = Fields.load_default()
	var lvl: Dictionary = {}
	for r: Dictionary in (f.rows as Array):
		if r["path"] == "dag.levelAttraction":
			lvl = r
	assert_eq(Fields.format_value(lvl, 0.03), "0.030")
	assert_eq(Fields.format_value({"kind": "toggle"}, true), "on")
