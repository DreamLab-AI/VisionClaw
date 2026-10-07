extends Node

## Renders the HUD SubViewport only when its content changes.
##
## UPDATE_ALWAYS re-rendered the whole 2D canvas every frame: 59 draw calls and
## about 8 200 triangles on HP-Desktop (perf/benchmark.gd extras), most of the
## frame's 50-call budget, for a panel that changes a few times a second. Every
## CanvasItem in the viewport re-records its commands through `draw` when it
## changes (text, theme state under the wand pointer, relayout, show/hide), and
## that happens in the idle pass whether or not the viewport renders. So each
## `draw` here asks for one render (UPDATE_ONCE), and a static HUD costs only
## its textured quad. Items added later are tracked through child_entered_tree.

var _vp: SubViewport = null
var _requests: int = 0


## Attach to `vp` (HUD.tscn's HudViewport). Called once from hud.gd _ready.
static func attach(vp: SubViewport) -> Node:
	var od: Node = (load("res://scripts/hud_render_on_demand.gd") as GDScript).new()
	od.name = "HudRenderOnDemand"
	vp.get_parent().add_child(od)
	od.call("_bind", vp)
	return od


func _bind(vp: SubViewport) -> void:
	_vp = vp
	_vp.child_entered_tree.connect(_on_node_added)
	_track(_vp)
	request_render()


func _track(n: Node) -> void:
	if n is CanvasItem and not (n as CanvasItem).draw.is_connected(request_render):
		(n as CanvasItem).draw.connect(request_render)
	if not n.child_entered_tree.is_connected(_on_node_added):
		n.child_entered_tree.connect(_on_node_added)
	for c in n.get_children(true):  # include internal children (hud_batching backgrounds)
		_track(c)


func _on_node_added(n: Node) -> void:
	_track(n)
	request_render()


## Render the HUD once at the end of this frame. Public for changes that do
## not redraw a CanvasItem (e.g. a texture swapped on the panel material).
func request_render() -> void:
	_requests += 1
	if _vp != null:
		_vp.render_target_update_mode = SubViewport.UPDATE_ONCE


## Test seam: renders requested so far.
func requests() -> int:
	return _requests
