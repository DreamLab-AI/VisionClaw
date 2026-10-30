extends RefCounted
## Used via `const BackendCaps := preload("res://scripts/backend_caps.gd")`
## rather than a class_name, so a headless run with a stale global class
## cache still resolves it.

## What each backend can do, as the XR client sees it (ADR-2143 A6).
##
## The client speaks one wire to two servers. VisionClaw serves every knowledge
## route the HUD was built around; Codescape serves the graph stream, presence,
## voice, a read-only physics view and settlement, and answers 404 (or 405 for
## the physics PUT) to the rest, by design: it is a code-work tool with no
## knowledge corpus (codescape-server/src/routes/mod.rs, http.rs). Rather than
## let every button fire a request the server refuses, GraphScene asks this
## table before issuing one and the HUD hides or greys the controls up front.
##
## Pure data, no I/O, so it is unit-testable without a scene.

## Capability keys. Each names a server surface, not a button, so one key can
## gate several controls and one control can need one key.
const PHYSICS_WRITE := "physics_write"   # PUT /api/settings/physics, POST .../reset-layout
const LAYOUT_MODE := "layout_mode"       # POST /api/layout/mode
const RADIAL := "radial"                 # POST /api/layout/radial
const FOLD := "fold"                     # GET /api/graph/fold
const CYCLES := "cycles"                 # GET /api/analytics/cycles
const DEMO := "demo"                     # GET/POST /api/agents/demo
const MEMORY_CLOUD := "memory_cloud"     # GET /api/memory-cloud, POST /api/memory-cloud/query
const PATTERN_QUERY := "pattern_query"   # POST /api/graph/query/pattern
const CANARY := "canary"                 # POST /api/canary/observe/*
const NODE_RELATIONS := "node_relations" # GET /api/graph/node/{id}/relations, POST .../expand
const INTERVENTION := "intervention"     # POST /api/broker/cases/{id}/decide
const PIN := "pin"                       # nodePin / nodeUnpin text frames on /wss (acked by VisionClaw only)
const DRAG := "drag"                     # nodeDragStart/Update/End text frames on /wss (ADR-2158 E9: VisionClaw only)

const ALL_KEYS: Array[String] = [
	PHYSICS_WRITE, LAYOUT_MODE, RADIAL, FOLD, CYCLES, DEMO, MEMORY_CLOUD,
	PATTERN_QUERY, CANARY, NODE_RELATIONS, INTERVENTION, PIN, DRAG,
]

## Codescape's route table (codescape-server/src/routes/mod.rs:404-415) serves
## none of the surfaces above, and its /wss ignores every text frame except
## authenticate, requestInitialData and ping (routes/ws.rs), so pins are never
## acknowledged there and a node drag is never applied (E9: the client refuses
## it locally rather than send a frame the server drops). Everything else the HUD does (edge slice, node size,
## colour mode, hulls, agent offsets, beat, comfort) is client-side and stays on.
const CODESCAPE_SUPPORTED: Array[String] = []

## HUD control action → the capability it needs. Controls absent here never
## need gating. Kept beside the table so a new server-bound button is one line.
const ACTION_CAPS: Dictionary = {
	"reset_layout": PHYSICS_WRITE,
	"spread_plus": PHYSICS_WRITE,
	"spread_minus": PHYSICS_WRITE,
	"hierarchy_toggle": PHYSICS_WRITE,
	"shells_plus": PHYSICS_WRITE,
	"shells_minus": PHYSICS_WRITE,
	"planes_toggle": PHYSICS_WRITE,
	"plane_gap_plus": PHYSICS_WRITE,
	"plane_gap_minus": PHYSICS_WRITE,
	"radial_off": PHYSICS_WRITE,
	"flat_toggle": PHYSICS_WRITE,
	"forces_toggle": PHYSICS_WRITE,
	"unpin_all": PIN,
	"radial_dag": RADIAL,
	"radial_type": RADIAL,
	"radial_ego": RADIAL,
	"layout_mode_cycle": LAYOUT_MODE,
	"fold_plus": FOLD,
	"fold_minus": FOLD,
	"cycles_toggle": CYCLES,
	"cycles_refresh": CYCLES,
	"toggle_demo": DEMO,
	"memory_cloud_toggle": MEMORY_CLOUD,
	"memory_cloud_colour": MEMORY_CLOUD,
}

## Dynamic action prefixes ("forces_step:3:1", "memory_hit:0") → capability.
const ACTION_PREFIX_CAPS: Dictionary = {
	"forces_step:": PHYSICS_WRITE,
	"memory_preset:": MEMORY_CLOUD,
	"memory_hit:": MEMORY_CLOUD,
	"memory_typed:": MEMORY_CLOUD,
}

## HUD tab → the capability every control on it needs. A tab whose only live
## controls are server-bound is hidden rather than shown full of grey buttons;
## tabs with a mix (graph, layout, shapes, query) stay and grey per control.
const TAB_CAPS: Dictionary = {
	"forces": PHYSICS_WRITE,
	"swarm": DEMO,
	"pins": PIN,
}

## Hint shown on a greyed control, naming the backend that has the feature.
const UNSUPPORTED_HINT := "VisionClaw only: Codescape does not serve this route"


## Capability flags for a backend mode: every key in ALL_KEYS → bool.
## VisionClaw is the reference server, so an unknown mode is treated as it
## (nothing hidden); Codescape is the explicit subset.
static func for_mode(mode: String) -> Dictionary:
	var caps: Dictionary = {}
	var is_codescape: bool = mode == "codescape"
	for key: String in ALL_KEYS:
		caps[key] = (not is_codescape) or CODESCAPE_SUPPORTED.has(key)
	return caps


## True when `caps` grants `key`. A key missing from `caps` is granted, so a
## partial dictionary (tests, future servers) never hides a control by accident.
static func supports(caps: Dictionary, key: String) -> bool:
	return bool(caps.get(key, true))


## Tab ids the HUD should hide under `caps`, in TAB_CAPS order.
static func hidden_tabs(caps: Dictionary) -> Array[String]:
	var out: Array[String] = []
	for tab: String in TAB_CAPS:
		if not supports(caps, TAB_CAPS[tab]):
			out.append(tab)
	return out


## Control action strings the HUD should grey under `caps`, in ACTION_CAPS order.
static func disabled_actions(caps: Dictionary) -> Array[String]:
	var out: Array[String] = []
	for action: String in ACTION_CAPS:
		if not supports(caps, ACTION_CAPS[action]):
			out.append(action)
	return out


## The capability an action needs, or "" when it is client-side.
static func cap_for_action(action: String) -> String:
	if ACTION_CAPS.has(action):
		return str(ACTION_CAPS[action])
	for prefix: String in ACTION_PREFIX_CAPS:
		if action.begins_with(prefix):
			return str(ACTION_PREFIX_CAPS[prefix])
	return ""


## True when `caps` lets `action` run; client-side actions always may.
static func action_allowed(caps: Dictionary, action: String) -> bool:
	var cap: String = cap_for_action(action)
	return cap.is_empty() or supports(caps, cap)
