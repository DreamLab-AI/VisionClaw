extends Node3D

## In-headset HUD — VR control centre (redesign, task #20).
##
## A tabbed panel rendered into a SubViewport (1280×800) shown on a world-space
## quad (HudPanel, 1.40×0.875 — aspect 1.6, matched to the viewport so the wand
## ray → UV mapping in graph_scene.gd is undistorted). The tab pages, header and
## hover-hint bar are built PROGRAMMATICALLY under HudControl so the layout is one
## source of truth and every control fits its page (no below-the-fold overflow —
## the pre-redesign single-VBox stacked 34 controls past the 640px fold with no
## scroll, hiding the flat-toggle, fold ladder, status and query builder).
##
## The intervention panel, ACSP indicator, dwell reticle and document card remain
## as .tscn OVERLAY nodes (they interrupt any tab). All decision logic/signing
## stays in Rust (NostrAuth); this script wires signals to nodes and issues the
## cold decide POST.
##
## STABLE NODE PATHS (for the flagship rebases — task #18 query builder, #19 fold
## ladder — target these; do not assume the old VBox layout):
##   HudViewport/HudControl/Root/Header            — persistent header row
##   HudViewport/HudControl/Root/TabBar            — wand-clickable tab buttons
##   HudViewport/HudControl/Root/Tabs              — page host (one visible page)
##   HudViewport/HudControl/Root/Tabs/GraphPage    — physics/layout controls
##   HudViewport/HudControl/Root/Tabs/QueryPage    — visual query builder
##   HudViewport/HudControl/Root/Tabs/PinsPage     — pinned nodes + Unpin All
##   HudViewport/HudControl/Root/Tabs/KeyPage      — colour swatch key (legend)
##   HudViewport/HudControl/Root/Tabs/SessionPage  — room/mute/connection/debug
##   HudViewport/HudControl/Root/Tabs/HelpPage     — controller cheat-sheet
##   HudViewport/HudControl/Root/HintBar           — hover-hint strip (bottom)

# --- Overlay nodes (kept in HUD.tscn — paths unchanged) ----------------------
@onready var hud_control: Control = $HudViewport/HudControl
@onready var acsp_label: Label = $HudViewport/HudControl/AcspIndicator/AcspLabel
@onready var acsp_glow: ColorRect = $HudViewport/HudControl/AcspIndicator/AcspGlow
@onready var intervention_panel: PanelContainer = $HudViewport/HudControl/InterventionPanel
@onready var case_title: Label = $HudViewport/HudControl/InterventionPanel/IVBox/CaseTitle
@onready var case_summary: Label = $HudViewport/HudControl/InterventionPanel/IVBox/CaseSummary
@onready var approve_button: Button = $HudViewport/HudControl/InterventionPanel/IVBox/Buttons/ApproveButton
@onready var deny_button: Button = $HudViewport/HudControl/InterventionPanel/IVBox/Buttons/DenyButton
@onready var dwell_reticle: Control = $HudViewport/HudControl/DwellReticle
@onready var decide_http: HTTPRequest = $DecideHttp

# Double-click document view (narrativegoldmine page card) — kept in HUD.tscn.
@onready var document_panel: PanelContainer = $HudViewport/HudControl/DocumentPanel
@onready var doc_title_label: Label = $HudViewport/HudControl/DocumentPanel/DVBox/DHeader/DocTitle
@onready var close_doc_button: Button = $HudViewport/HudControl/DocumentPanel/DVBox/DHeader/CloseDocButton
@onready var doc_scroll: ScrollContainer = $HudViewport/HudControl/DocumentPanel/DVBox/DocScroll
@onready var doc_text: RichTextLabel = $HudViewport/HudControl/DocumentPanel/DVBox/DocScroll/DocText
@onready var scroll_up_button: Button = $HudViewport/HudControl/DocumentPanel/DVBox/DScroll/ScrollUpButton
@onready var scroll_down_button: Button = $HudViewport/HudControl/DocumentPanel/DVBox/DScroll/ScrollDownButton
@onready var doc_http: HTTPRequest = $DocHttp

const NG_PAGE_BASE: String = "https://narrativegoldmine.com/api/pages/"
const DOC_TIMEOUT_SEC: float = 10.0
const DOC_SCROLL_STEP: int = 140
var _doc_title: String = ""

# --- Layout constants --------------------------------------------------------
const XRTheme = preload("res://scripts/xr_theme.gd")
const BTN_H: int = 56                       # min wand hit-target height
const MARGIN: int = 24
const HEADER_H: int = 48
const TABBAR_H: int = 56
const HINTBAR_H: int = 44
const HINT_META: StringName = &"hint"
const HINT_DEFAULT: String = "Point at a control for help"
const ACCENT: Color = Color(0.55, 0.80, 1.0)          # active tab / on-state
const IDLE: Color = Color(0.62, 0.66, 0.75)           # inactive tab

# --- Signals (ALL preserved — GraphScene wiring depends on these) ------------
signal join_requested(room_urn: String)
signal mute_toggled(muted: bool)
## Emitted the instant the operator approves/denies — the M2 intervention intent,
## independent of the HTTP round-trip.
signal decision_submitted(case_id: String, outcome: String)
## Emitted when the decide POST resolves; `accepted` is the server verdict.
signal case_decided(case_id: String, outcome: String, accepted: bool)
## Emitted when an operator control-panel button is pressed. GraphScene owns the
## effect (physics POST/PUT, edge/node runtime factors); the HUD only reports the
## intent so all state lives in one place. `action` is one of: reset_layout,
## spread_plus, spread_minus, edges_plus, edges_minus, node_size_plus,
## node_size_minus, hierarchy_toggle, shells_plus, shells_minus, flat_toggle,
## fold_plus, fold_minus, unpin_all.
signal control_pressed(action: String)
## Visual query builder (flagship). GraphScene owns the query state + count fetch;
## the HUD only surfaces the summary/count and re-broadcasts Execute/Clear intent.
signal query_execute_pressed()
signal query_clear_pressed()
## Emitted when the operator asks to reconnect the graph/presence sockets.
signal reconnect_pressed()

const ACSP_GLOW_COLOR: Color = Color(1.0, 0.62, 0.12, 1.0)
# Hard cap on a single decide POST. A stalled request fires request_completed with
# RESULT_TIMEOUT at this point, releasing the single-in-flight decision gate.
const DECIDE_TIMEOUT_SEC: float = 10.0

var _avatar_count: int = 0
var _mtp_ms: float = 0.0
var _connected: bool = false

# M2 intervention state.
var _http_base: String = ""
var _nostr_auth: RefCounted = null  # Rust NostrAuth
var _current_case_id: String = ""
var _last_outcome: String = ""
# Case id captured at POST time. show_case() can swap _current_case_id between the
# request and its response; the completion handler must attribute the verdict to
# the case that was actually submitted, not whatever is on screen now.
var _pending_case_id: String = ""
var _open_case_count: int = 0
var _pulse_time: float = 0.0

# --- Built-UI node references (assigned in _build_ui) -------------------------
var _root: VBoxContainer = null
var _tabs_host: Control = null
var _hint_bar: Label = null
var _conn_dot: Label = null
var _room_header: Label = null
var _fps_header: Label = null
var _pages: Dictionary = {}          # tab id → page Control
var _tab_buttons: Dictionary = {}    # tab id → Button
var _active_tab: String = "graph"
var _motion_toggle: CheckButton
var _quality_toggle: CheckButton
var _overflow_warned: Dictionary = {}
var _scroll_regions: Array = []      # scroll-region wrapper HBoxes (▲▼ visibility)

# Graph-page controls needing live state / status.
var _controls_status: Label = null
var _hierarchy_button: Button = null
var _flat_toggle_button: Button = null
var _planes_toggle_button: Button = null
var _layout_mode_button: Button = null
# Wave 2, Feature 3 — type show/hide toggles (Graph tab). Each tracks its own
# visible bool so the label/tint reflects state; the class code is in the action.
var _type_knowledge_button: Button = null
var _type_ontology_button: Button = null
var _type_agent_button: Button = null
var _type_visible: Dictionary = {"knowledge": true, "ontology": true, "agent": true}
var _fold_plus_button: Button = null
var _fold_minus_button: Button = null
var _demo_button: Button = null
var _demo_active: bool = false
# Query page.
var _query_summary_label: Label = null
var _query_count_label: Label = null
var _query_vars_list: VBoxContainer = null
# Pins page.
var _pins_count_label: Label = null
var _unpin_all_button: Button = null
var _pins_list: VBoxContainer = null
var _swarm_list: VBoxContainer = null
var _swarm_count_label: Label = null
# Session page.
var _room_label: Label = null
var _room_entry: LineEdit = null
var _mute_toggle: CheckButton = null
var _debug_stats: Label = null
var _conn_status_label: Label = null

const TAB_ORDER: Array[String] = ["graph", "layout", "query", "pins", "swarm", "key", "session", "help"]
const TAB_LABELS: Dictionary = {
	"graph": "Graph", "layout": "Layout", "query": "Query", "pins": "Pins", "swarm": "Swarm", "key": "Key", "session": "Session", "help": "Help",
}

# Agent status → roster dot colour (ADR-140, Pillar 3). Mirrors
# render_store::agent_status_color: idle slate / working green / blocked amber-red /
# done cyan-white.
const SWARM_STATUS_COLORS: Dictionary = {
	0: Color(0.50, 0.58, 0.68),
	1: Color(0.30, 0.90, 0.72),
	2: Color(1.0, 0.35, 0.20),
	3: Color(0.60, 0.85, 1.0),
}

# --- Colour key (Key tab) ----------------------------------------------------
# Every swatch mirrors the constant that actually paints the scene, so the key
# is a legend of the live palette rather than a decorative one. Keep each entry
# in lockstep with its cited source; agent status rows reuse SWARM_STATUS_COLORS.
const GOLDEN_RATIO_CONJ: float = 0.61803398875               # render_store::community_color / query_var_color hue walk
const KEY_ANOMALY: Color = Color(1.0, 0.15, 0.1)             # render_store::community_color warn blend
const KEY_EDGE_FLOW: Color = Color(0.30, 0.72, 1.0)          # materials/edge_flow.gdshader flow_color
const KEY_EDGE_SUBCLASS: Color = Color(0.62, 0.50, 1.0)      # materials/edge_flow.gdshader subclass_color
const KEY_EDGE_INFERRED: Color = Color(1.0, 0.66, 0.12)      # materials/edge_flow.gdshader inferred_color
const KEY_RAY_IDLE: Color = Color(0.35, 0.7, 1.0)            # graph_scene.gd RAY_IDLE_COLOR
const KEY_RAY_ACTIVE: Color = Color(0.3, 1.0, 0.4)           # graph_scene.gd RAY_ACTIVE_COLOR
const KEY_PANEL_HOVER: Color = Color(0.4, 0.9, 0.94)         # spatial_environment.gd HOVER_COLOR
const KEY_PANEL_GRAB: Color = Color(1.0, 0.72, 0.32)         # spatial_environment.gd GRAB_COLOR
const KEY_AVATAR_IDLE: Color = Color(0.42, 0.6, 0.9)         # agent_avatar.gd COLOR_IDLE
const KEY_AVATAR_AWAITING: Color = Color(1.0, 0.62, 0.12)    # agent_avatar.gd COLOR_AWAITING
const KEY_AVATAR_SPEAKING: Color = Color(0.7, 0.85, 1.0)     # agent_avatar.gd COLOR_SPEAKING
const KEY_META: StringName = &"key"                          # set on every key row (label text) — tests count these
const SWATCH_PX: int = 34
const KEY_REGION_H: int = 452                                # header + region must fit the 532px page host

# --- Transient operator notice (bottom strip) --------------------------------
# A rejected server write used to vanish into push_warning where no one in the
# headset could see it. flash_notice() overrides the hover hint on EVERY tab for
# a few seconds so the failure (and its remedy) is readable in-HMD.
const NOTICE_SEC: float = 8.0
const NOTICE_COLOR: Color = Color(1.0, 0.72, 0.32)
var _notice_text: String = ""
var _notice_until_ms: int = 0
var _hint_bar_notice_mode: bool = false


func _ready() -> void:
	# Bind after attachment: nested PackedScene textures have no viewport yet.
	($HudPanel.material_override as StandardMaterial3D).albedo_texture = $HudViewport.get_texture()
	_build_ui()
	# Overlay wiring (nodes from HUD.tscn).
	approve_button.pressed.connect(_on_approve_pressed)
	deny_button.pressed.connect(_on_deny_pressed)
	if decide_http != null:
		decide_http.request_completed.connect(_on_decide_completed)
		# Bound every decide POST: without a finite timeout a stalled connection
		# never fires request_completed, so _pending_case_id would block the
		# decision gate forever.
		decide_http.timeout = DECIDE_TIMEOUT_SEC
	close_doc_button.pressed.connect(hide_document)
	scroll_up_button.pressed.connect(func() -> void: doc_scroll.scroll_vertical -= DOC_SCROLL_STEP)
	scroll_down_button.pressed.connect(func() -> void: doc_scroll.scroll_vertical += DOC_SCROLL_STEP)
	# Hints on the overlay controls too (their hover-hints resolve the same way).
	approve_button.set_meta(HINT_META, "Approve this case")
	deny_button.set_meta(HINT_META, "Deny this case")
	close_doc_button.set_meta(HINT_META, "Close the page card")
	scroll_up_button.set_meta(HINT_META, "Scroll the page up")
	scroll_down_button.set_meta(HINT_META, "Scroll the page down")
	if doc_http != null:
		doc_http.request_completed.connect(_on_doc_completed)
		doc_http.timeout = DOC_TIMEOUT_SEC
	_show_tab("graph")
	set_process(true)


# --- UI construction ---------------------------------------------------------

func _build_ui() -> void:
	# One base font size for the whole panel so text is legible on the 1.4m quad in
	# VR (default theme font is tuned for desktop and reads tiny at headset scale).
	hud_control.theme = XRTheme.create()
	var backdrop := Panel.new()
	backdrop.name = "InstrumentSurface"
	backdrop.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	backdrop.mouse_filter = Control.MOUSE_FILTER_IGNORE
	hud_control.add_child(backdrop)
	hud_control.move_child(backdrop, 0)
	# Overlays remain above all programmatically constructed pages.
	for overlay in [intervention_panel, document_panel, dwell_reticle]:
		overlay.z_index = 10
	$HudViewport/HudControl/AcspIndicator.z_index = 2
	approve_button.add_theme_stylebox_override("normal", XRTheme.box(Color("163b36"), Color("6ce0bb")))
	deny_button.add_theme_stylebox_override("normal", XRTheme.box(Color("402635"), Color("ef99a9")))

	_root = VBoxContainer.new()
	_root.name = "Root"
	# _root is a child of the (non-container) HudControl, so anchors apply: fill it
	# minus a uniform margin.
	_root.anchor_right = 1.0
	_root.anchor_bottom = 1.0
	_root.offset_left = MARGIN
	_root.offset_top = MARGIN
	_root.offset_right = -MARGIN
	_root.offset_bottom = -MARGIN
	_root.add_theme_constant_override("separation", 10)
	hud_control.add_child(_root)

	_build_header()
	_build_tab_bar()
	_build_pages_host()
	_build_hint_bar()


func _build_header() -> void:
	var header := HBoxContainer.new()
	header.name = "Header"
	header.custom_minimum_size = Vector2(0, HEADER_H)
	header.add_theme_constant_override("separation", 16)
	_conn_dot = _mk_label("● OFF", "Graph socket connection status")
	_conn_dot.add_theme_color_override("font_color", Color(0.9, 0.35, 0.3))
	_room_header = _mk_label("Room: -", "Current collaboration room")
	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_fps_header = _mk_label("FPS --", "Render framerate")
	header.add_child(_conn_dot)
	header.add_child(_room_header)
	header.add_child(spacer)
	header.add_child(_fps_header)
	# Reserve the overlay badge footprint; FPS and long room names must not
	# render underneath the persistent ACSP indicator.
	_room_header.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
	_room_header.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var badge_space := Control.new()
	badge_space.custom_minimum_size = Vector2(272, 0)
	header.add_child(badge_space)
	_root.add_child(header)
	_root.add_child(_hsep())


# ── ADR-2033: every ray-driven control fires on PRESS ───────────────────────
#
# Pulling the Vive trigger jolts the pointer ray 20-30px. In Godot's default
# release action mode the button waits for the release, which by then often lands
# outside the control — so the press is silently cancelled and the user sees a
# dead button (observed live 2026-08-31). Firing on press removes the jolt from
# the dispatch path entirely.
#
# The rule is "every ray-driven control", so it must not be applied by hand at
# each construction site: the closeout found eleven Button/CheckButton sites in
# this file and only three assignments, leaving Query Execute/Clear, swarm
# teleport, Join Room, Mute, Reconnect and both scroll arrows on release mode.
# This helper is the single place the mode is set; route every new control
# through it rather than assigning action_mode again.
#
# Returns the button so it can wrap a constructor inline:
#     var b := _press_fire(Button.new())
func _press_fire(b: BaseButton) -> BaseButton:
	b.action_mode = BaseButton.ACTION_MODE_BUTTON_PRESS
	return b


func _build_tab_bar() -> void:
	var bar := HBoxContainer.new()
	bar.name = "TabBar"
	bar.custom_minimum_size = Vector2(0, TABBAR_H)
	bar.add_theme_constant_override("separation", 8)
	for id: String in TAB_ORDER:
		var b := _press_fire(Button.new()) as Button
		b.text = TAB_LABELS[id]
		b.custom_minimum_size = Vector2(0, TABBAR_H)
		b.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		b.set_meta(HINT_META, "Show the %s tab" % TAB_LABELS[id])
		b.pressed.connect(_show_tab.bind(id))
		bar.add_child(b)
		_tab_buttons[id] = b
	_root.add_child(bar)


func _build_pages_host() -> void:
	_tabs_host = Control.new()
	_tabs_host.name = "Tabs"
	_tabs_host.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_tabs_host.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_tabs_host.clip_contents = true
	_root.add_child(_tabs_host)

	_pages["graph"] = _build_graph_page()
	_pages["layout"] = _build_layout_page()
	_pages["query"] = _build_query_page()
	_pages["pins"] = _build_pins_page()
	_pages["swarm"] = _build_swarm_page()
	_pages["key"] = _build_key_page()
	_pages["session"] = _build_session_page()
	_pages["help"] = _build_help_page()
	for id: String in _pages:
		var page: Control = _pages[id]
		page.name = "%sPage" % TAB_LABELS[id]
		# Pages fill the (non-container) Tabs host via anchors.
		page.anchor_right = 1.0
		page.anchor_bottom = 1.0
		page.visible = false
		_tabs_host.add_child(page)


func _build_hint_bar() -> void:
	_root.add_child(_hsep())
	_hint_bar = Label.new()
	_hint_bar.name = "HintBar"
	_hint_bar.custom_minimum_size = Vector2(0, HINTBAR_H)
	_hint_bar.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	_hint_bar.text = "ⓘ " + HINT_DEFAULT
	_hint_bar.add_theme_color_override("font_color", ACCENT)
	_root.add_child(_hint_bar)


# --- Pages -------------------------------------------------------------------

func _build_graph_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	page.add_theme_constant_override("separation", 8)
	page.add_child(_group_header("Layout"))
	var g1 := _grid(3)
	g1.add_child(_action_btn("Reset Layout", "reset_layout", "Re-randomise positions & reset physics to safe defaults"))
	g1.add_child(_action_btn("Spread +", "spread_plus", "Push nodes apart (increase repulsion)"))
	g1.add_child(_action_btn("Spread -", "spread_minus", "Pull nodes together (decrease repulsion)"))
	g1.add_child(_action_btn("Edges +", "edges_plus", "Show more edges"))
	g1.add_child(_action_btn("Edges -", "edges_minus", "Show fewer edges"))
	g1.add_child(_action_btn("Node +", "node_size_plus", "Enlarge node markers"))
	g1.add_child(_action_btn("Node -", "node_size_minus", "Shrink node markers"))
	page.add_child(g1)

	# Wave 2, Feature 3 — type show/hide filter. One wand-clickable toggle per node
	# class; pressing flips visibility and emits control_pressed
	# "type_toggle:<class>:<0|1>" (1 = now visible). graph_scene forwards to the
	# render store's set_type_visible.
	page.add_child(_group_header("Layers"))
	var g3 := _grid(3)
	_type_knowledge_button = _type_toggle_btn("Knowledge", "knowledge")
	_type_ontology_button = _type_toggle_btn("Ontology", "ontology")
	_type_agent_button = _type_toggle_btn("Agents", "agent")
	g3.add_child(_type_knowledge_button)
	g3.add_child(_type_ontology_button)
	g3.add_child(_type_agent_button)
	page.add_child(g3)

	page.add_child(_group_header("Demo"))
	var g_demo := _grid(1)
	_demo_button = _action_btn("Start Agent Demo", "toggle_demo", "Inject 6 synthetic demo agents that cycle through work→idle to exercise sprites, nudges and fade")
	g_demo.add_child(_demo_button)
	page.add_child(g_demo)

	page.add_child(_group_header("Status"))
	_controls_status = _mk_label("repelK --  restLen --  edges --  node x--", "Live physics & layout state")
	_controls_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	page.add_child(_controls_status)
	return page


# ADR-141 constrained-layout controls. Split out of the Graph tab 2026-08-31:
# the combined page needed 1050px in a 532px host, hiding everything below the
# Planes group (the overflow guard warned, but only into the log). Both tabs
# now fit; keep the height arithmetic in mind before adding groups here.
func _build_layout_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	# 3 (not the usual 8): four groups put this page at 564px with the default
	# separation — 32px past the 532px host. Tighter separation buys ~35px.
	page.add_theme_constant_override("separation", 3)

	page.add_child(_group_header("Hierarchy & View"))
	var g2 := _grid(3)
	_hierarchy_button = _action_btn("Hierarchy: Off", "hierarchy_toggle", "Toggle the DAG radial rank-bias force (concentric shells)")
	_flat_toggle_button = _action_btn("View: 3D", "flat_toggle", "Toggle between full 3D and flat facing discs")
	_fold_plus_button = _action_btn("Fold + (L0)", "fold_plus", "Collapse detail up one fold level (denser)")
	_fold_minus_button = _action_btn("Fold -", "fold_minus", "Expand detail down one fold level")
	g2.add_child(_hierarchy_button)
	g2.add_child(_action_btn("Shells +", "shells_plus", "Increase spacing between hierarchy shells"))
	g2.add_child(_action_btn("Shells -", "shells_minus", "Decrease spacing between hierarchy shells"))
	g2.add_child(_flat_toggle_button)
	g2.add_child(_fold_plus_button)
	g2.add_child(_fold_minus_button)
	page.add_child(g2)

	page.add_child(_group_header("Planes"))
	var g_planes := _grid(3)
	_planes_toggle_button = _action_btn("Planes: Off", "planes_toggle", "Toggle the plane spring force that stratifies nodes into parallel planes by type")
	g_planes.add_child(_planes_toggle_button)
	g_planes.add_child(_action_btn("Plane Gap +", "plane_gap_plus", "Increase the gap between type planes"))
	g_planes.add_child(_action_btn("Plane Gap -", "plane_gap_minus", "Decrease the gap between type planes"))
	page.add_child(g_planes)

	# ADR-141 Phase 3 — radial-shell layout modes. Each button POSTs
	# /api/layout/radial via graph_scene; "Radial Off" disables the shell term.
	page.add_child(_group_header("Radial Shells"))
	var g_radial := _grid(2)
	g_radial.add_child(_action_btn("Radial: DAG", "radial_dag", "Concentric shells by DAG depth, centred on the origin"))
	g_radial.add_child(_action_btn("Radial: Type", "radial_type", "Concentric shells grouped by node type, centred on the origin"))
	g_radial.add_child(_action_btn("Ego Focus", "radial_ego", "Shells by hop-distance from the selected node — open a node's radial first"))
	g_radial.add_child(_action_btn("Radial Off", "radial_off", "Disable the radial shell force"))
	page.add_child(g_radial)

	# ADR-141 Phase 1 — constrained-layout engine picker. Single cycling button
	# steps through the backend LayoutMode enum; graph_scene POSTs /api/layout/mode.
	page.add_child(_group_header("Layout Mode"))
	var g_layout := _grid(1)
	_layout_mode_button = _action_btn("Layout: Force", "layout_mode_cycle", "Cycle graph layout mode (force, hierarchical, radial, spectral, temporal, clustered)")
	g_layout.add_child(_layout_mode_button)
	page.add_child(g_layout)
	return page


func _build_query_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	page.add_theme_constant_override("separation", 8)
	page.add_child(_group_header("Query Builder"))
	_query_summary_label = _mk_label("No active query", "Pattern of the query you are building")
	page.add_child(_query_summary_label)
	_query_count_label = _mk_label("—", "Live match count for the current pattern")
	page.add_child(_query_count_label)
	var region := _scroll_region(300)
	_query_vars_list = VBoxContainer.new()
	_query_vars_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_query_vars_list.add_child(_mk_label("(mark nodes with the wand MENU button)", "Marked query variables appear here"))
	(region.get_meta("scroll") as ScrollContainer).add_child(_query_vars_list)
	page.add_child(region)
	var btns := _grid(2)
	var exec_enabled: bool = _execute_enabled()
	var exec := _press_fire(Button.new()) as Button
	exec.custom_minimum_size = Vector2(0, BTN_H)
	exec.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	if exec_enabled:
		exec.text = "Execute"
		exec.set_meta(HINT_META, "Run the query and jump to the results")
		exec.pressed.connect(func() -> void: emit_signal("query_execute_pressed"))
	else:
		exec.text = "Execute (soon)"
		exec.disabled = true
		exec.set_meta(HINT_META, "Query execution ships in a later phase")
	var clr := _press_fire(Button.new()) as Button
	clr.text = "Clear"
	clr.custom_minimum_size = Vector2(0, BTN_H)
	clr.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	clr.set_meta(HINT_META, "Clear all marked variables and the pattern")
	clr.pressed.connect(func() -> void: emit_signal("query_clear_pressed"))
	btns.add_child(exec)
	btns.add_child(clr)
	page.add_child(btns)
	return page


func _build_pins_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	page.add_theme_constant_override("separation", 8)
	page.add_child(_group_header("Pinned Nodes"))
	_pins_count_label = _mk_label("0 pinned", "Nodes you have pinned this session")
	page.add_child(_pins_count_label)
	_unpin_all_button = _action_btn("Unpin All", "unpin_all", "Release every node you pinned (hands them back to physics)")
	page.add_child(_unpin_all_button)
	var region := _scroll_region(360)
	_pins_list = VBoxContainer.new()
	_pins_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_pins_list.add_child(_mk_label("(none pinned — grab & release a node to pin it)", "Pinned node ids appear here"))
	(region.get_meta("scroll") as ScrollContainer).add_child(_pins_list)
	page.add_child(region)
	return page


# Swarm tab (ADR-140, Pillar 4 / P5): the roster of live agents — a status dot,
# name → target-node label, and the current task line, each row tap-to-teleport.
# GraphScene pushes the roster via set_swarm_roster(); this page is pure layout.
func _build_swarm_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	page.add_theme_constant_override("separation", 8)
	page.add_child(_group_header("Agent Swarm"))
	_swarm_count_label = _mk_label("0 agents", "Live agents working the graph")
	page.add_child(_swarm_count_label)
	var region := _scroll_region(360)
	_swarm_list = VBoxContainer.new()
	_swarm_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_swarm_list.add_child(_mk_label("(no agents active)", "Agents appear here as they work"))
	(region.get_meta("scroll") as ScrollContainer).add_child(_swarm_list)
	page.add_child(region)
	return page


func _swarm_status_color(status: int) -> Color:
	return SWARM_STATUS_COLORS.get(status, SWARM_STATUS_COLORS[0])


# Build one roster row: [● dot] [Name → Target] teleport button, with the task line
# beneath. Tap emits control_pressed("teleport:<agent_id>") — agent wire ids ARE
# node ids, so GraphScene's existing _teleport_to_node glide handles it unchanged.
func _mk_swarm_row(r: Dictionary) -> Control:
	var row := VBoxContainer.new()
	var head := HBoxContainer.new()
	head.add_theme_constant_override("separation", 6)
	var dot := Label.new()
	dot.text = "●"
	dot.add_theme_color_override("font_color", _swarm_status_color(int(r.get("status", 0))))
	head.add_child(dot)
	var aid := int(r.get("id", 0))
	var name_s: String = str(r.get("name", ""))
	if name_s == "":
		name_s = "agent %d" % aid
	var target_s: String = str(r.get("target", ""))
	var btn := _press_fire(Button.new()) as Button
	btn.text = "%s → %s" % [name_s, target_s if target_s != "" else "…"]
	btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	btn.set_meta(HINT_META, "Teleport to %s" % name_s)
	btn.pressed.connect(func() -> void: emit_signal("control_pressed", "teleport:%d" % aid))
	head.add_child(btn)
	row.add_child(head)
	var task_s: String = str(r.get("task", ""))
	if task_s != "":
		row.add_child(_mk_label("    ⚙ %s" % task_s, "Current task"))
	return row


## Populate the Swarm tab from the live agent roster. `rows` is an Array of
## Dictionaries {id:int, name:String, status:int, target:String, task:String}.
## Cheap — GraphScene calls this at ~4 Hz and only when the roster changed.
func set_swarm_roster(rows: Array) -> void:
	if _swarm_list == null:
		return
	for c in _swarm_list.get_children():
		c.queue_free()
	if _swarm_count_label != null:
		_swarm_count_label.text = "%d agent%s" % [rows.size(), "" if rows.size() == 1 else "s"]
	if rows.is_empty():
		_swarm_list.add_child(_mk_label("(no agents active)", "Agents appear here as they work"))
		call_deferred("_update_scroll_arrows")
		return
	for r: Variant in rows:
		_swarm_list.add_child(_mk_swarm_row(r as Dictionary))
	call_deferred("_update_scroll_arrows")


# Colour swatch key. One scroll region (like Help) so the page can never fall
# below the fold however many entries are added; the ▲▼ column appears only on
# overflow. Rows are [swatch…][label] pairs in a 2-column grid per section.
func _build_key_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	page.add_theme_constant_override("separation", 6)
	page.add_child(_group_header("Colour Key"))
	var region := _scroll_region(KEY_REGION_H)
	var body := VBoxContainer.new()
	body.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	body.add_theme_constant_override("separation", 4)
	for section: Dictionary in _key_sections():
		var sub := Label.new()
		sub.text = String(section["title"])
		sub.add_theme_color_override("font_color", IDLE)
		body.add_child(sub)
		var g := _grid(2)
		for row: Array in section["rows"]:
			g.add_child(_key_row(row[0], String(row[1]), String(row[2])))
		body.add_child(g)
	(region.get_meta("scroll") as ScrollContainer).add_child(body)
	page.add_child(region)
	return page


# The key's content, grouped. Each row: [Array[Color] swatches, label, hover hint].
func _key_sections() -> Array:
	return [
		{"title": "Nodes", "rows": [
			[[community_swatch(1), community_swatch(2), community_swatch(3), community_swatch(4)],
				"Community hue", "Each node takes a golden-ratio hue keyed by its community — same colour = same cluster"],
			[[KEY_ANOMALY], "Anomaly (blends to red)", "Anomalous nodes blend toward warning red; brighter rim = higher centrality"],
			[[query_swatch(0), query_swatch(1), query_swatch(2), query_swatch(3)],
				"Query mark ?v1…?v8", "Nodes marked as query variables take a saturated palette colour and a rim flag"],
		]},
		{"title": "Agents — node halo and Swarm dot", "rows": [
			[[SWARM_STATUS_COLORS[0]], "Idle", "Agent with no active work"],
			[[SWARM_STATUS_COLORS[1]], "Working (beam to target)", "Agent acting on a node — a beam links it to the node it is working on"],
			[[SWARM_STATUS_COLORS[2]], "Blocked / error", "Agent blocked or errored — needs attention"],
			[[SWARM_STATUS_COLORS[3]], "Done", "Agent finished or offline"],
		]},
		{"title": "Edges", "rows": [
			[[KEY_EDGE_FLOW], "Link", "Ordinary graph edge (flow pulses along it)"],
			[[KEY_EDGE_SUBCLASS], "Subclass-of", "Ontology subclass / hierarchy edge"],
			[[KEY_EDGE_INFERRED], "Inferred", "Edge inferred by the reasoner, not asserted"],
		]},
		{"title": "Wand and panel", "rows": [
			[[KEY_RAY_IDLE], "Ray idle", "Wand ray while tracking"],
			[[KEY_RAY_ACTIVE], "Ray firing", "Wand ray as the trigger pulls"],
			[[KEY_PANEL_HOVER], "Panel hover", "This panel when a wand is near enough to grab it"],
			[[KEY_PANEL_GRAB], "Panel grabbed", "This panel while being carried"],
		]},
		{"title": "Agent avatars", "rows": [
			[[KEY_AVATAR_IDLE], "Idle", "Embodied agent at rest"],
			[[SWARM_STATUS_COLORS[1]], "Working", "Embodied agent busy"],
			[[KEY_AVATAR_AWAITING], "Awaiting input", "Embodied agent waiting on you"],
			[[KEY_AVATAR_SPEAKING], "Speaking", "Embodied agent talking"],
		]},
	]


# One key row: swatches then a label. Carries KEY_META (the label) and a hover hint.
func _key_row(colors: Array, label: String, hint: String) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 6)
	row.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.set_meta(KEY_META, label)
	row.set_meta(HINT_META, hint)
	for c: Color in colors:
		var sw := ColorRect.new()
		sw.name = "Swatch"
		sw.color = c
		sw.custom_minimum_size = Vector2(SWATCH_PX, SWATCH_PX)
		sw.size_flags_vertical = Control.SIZE_SHRINK_CENTER
		sw.mouse_filter = Control.MOUSE_FILTER_IGNORE
		row.add_child(sw)
	var l := Label.new()
	l.text = label
	l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	l.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_child(l)
	return row


## Sample community colour for community `k` (k ≥ 1). Mirrors
## render_store::community_color / graph_scene._community_color (no anomaly).
static func community_swatch(k: int) -> Color:
	return Color.from_hsv(fmod(float(k) * GOLDEN_RATIO_CONJ, 1.0), 0.6, 0.95)


## Query-variable overlay colour for palette `slot`. Mirrors render_store::query_var_color.
static func query_swatch(slot: int) -> Color:
	return Color.from_hsv(fmod(float(slot % 8) * GOLDEN_RATIO_CONJ, 1.0), 0.85, 1.0)


func _build_session_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	page.add_theme_constant_override("separation", 8)
	page.add_child(_group_header("Session"))
	_room_label = _mk_label("Room: -", "Current room")
	page.add_child(_room_label)
	_room_entry = LineEdit.new()
	_room_entry.placeholder_text = "urn:visionclaw:room:sha256-12-..."
	_room_entry.custom_minimum_size = Vector2(0, BTN_H)
	_room_entry.set_meta(HINT_META, "Type or paste a room URN, then Join")
	page.add_child(_room_entry)
	var join := _press_fire(Button.new()) as Button
	join.text = "Join Room"
	join.custom_minimum_size = Vector2(0, BTN_H)
	join.set_meta(HINT_META, "Join the room in the field above")
	join.pressed.connect(_on_join_pressed)
	page.add_child(join)
	_mute_toggle = _press_fire(CheckButton.new()) as CheckButton
	_mute_toggle.text = "Mute"
	_mute_toggle.custom_minimum_size = Vector2(0, BTN_H)
	_mute_toggle.set_meta(HINT_META, "Mute / unmute your microphone")
	_mute_toggle.toggled.connect(_on_mute_toggled)
	page.add_child(_mute_toggle)
	_conn_status_label = _mk_label("Connection: off", "Graph socket state")
	page.add_child(_conn_status_label)
	var reconnect := _press_fire(Button.new()) as Button
	reconnect.text = "Reconnect"
	reconnect.custom_minimum_size = Vector2(0, BTN_H)
	reconnect.set_meta(HINT_META, "Force-reconnect the graph & presence sockets")
	reconnect.pressed.connect(func() -> void: emit_signal("reconnect_pressed"))
	page.add_child(reconnect)
	_debug_stats = _mk_label("FPS: --  MTP: --ms  Avatars: 0  Net: OFF", "Render & network diagnostics")
	_debug_stats.name = "DebugStats"
	page.add_child(_debug_stats)
	return page


func _build_help_page() -> VBoxContainer:
	var page := VBoxContainer.new()
	page.add_theme_constant_override("separation", 8)
	page.add_child(_group_header("Vive Wand — Controls"))
	var comfort := HBoxContainer.new()
	for entry in [["Reduce motion", "visual_motion", true], ["Low-cost visuals", "visual_quality", false]]:
		var toggle := _press_fire(CheckButton.new()) as CheckButton
		toggle.text = entry[0]
		toggle.custom_minimum_size.y = BTN_H
		toggle.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		toggle.button_pressed = entry[2]
		toggle.set_meta(HINT_META, "Keep the scene calm" if entry[1] == "visual_motion" else "Reduce visual cost for slower hardware")
		var action: String = entry[1]
		toggle.toggled.connect(func(on: bool) -> void: control_pressed.emit(action + (":1" if on else ":0")))
		comfort.add_child(toggle)
		if action == "visual_motion":
			_motion_toggle = toggle
		else:
			_quality_toggle = toggle
	page.add_child(comfort)
	var region := _scroll_region(420)
	var rt := RichTextLabel.new()
	rt.bbcode_enabled = true
	rt.fit_content = true
	rt.scroll_active = false
	rt.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	rt.set_meta(HINT_META, "Every wand binding")
	rt.text = _cheat_sheet_bbcode()
	(region.get_meta("scroll") as ScrollContainer).add_child(rt)
	page.add_child(region)
	return page


func _cheat_sheet_bbcode() -> String:
	var rows: Array[Array] = [
		["[b]NODE[/b]", ""],
		["Trigger — point at a node & pull", "Grab it"],
		["…release the trigger", "Pins the node in place"],
		["Trigger — double-pull on a node", "Open its page card"],
		["Menu button (or A/X)", "Node menu (mark variable…)"],
		["", ""],
		["[b]GRAPH[/b]", ""],
		["BOTH grips (two hands)", "Seize the whole graph"],
		["  hands apart / together", "Scale"],
		["  twist your hands", "Rotate"],
		["  move hands together", "Carry"],
		["", ""],
		["[b]PANEL & MOVE[/b]", ""],
		["One grip while near this panel", "Pick up & move the panel"],
		["Trackpad / thumbstick", "Fly through the graph"],
		["Point at the panel + trigger", "Click a button"],
	]
	var lines: Array[String] = []
	for r: Array in rows:
		if String(r[1]).is_empty():
			lines.append(String(r[0]))
		else:
			lines.append("%s  [color=#8fd0ff]→[/color]  %s" % [r[0], r[1]])
	return "\n".join(lines)


# --- Small builders ----------------------------------------------------------

func _mk_label(text: String, hint: String) -> Label:
	var l := Label.new()
	l.text = text
	if not hint.is_empty():
		l.set_meta(HINT_META, hint)
	return l


func _group_header(text: String) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_color_override("font_color", ACCENT)
	return l


func _hsep() -> HSeparator:
	return HSeparator.new()


func _grid(cols: int) -> GridContainer:
	var g := GridContainer.new()
	g.columns = cols
	g.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	g.add_theme_constant_override("h_separation", 8)
	g.add_theme_constant_override("v_separation", 8)
	return g


# A button that re-broadcasts a named control_pressed intent (GraphScene owns the
# effect). Carries a hover hint and a ≥56px hit target.
func _action_btn(text: String, action: String, hint: String) -> Button:
	var b := _press_fire(Button.new()) as Button
	b.text = text
	b.custom_minimum_size = Vector2(0, BTN_H)
	b.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	b.set_meta(HINT_META, hint)
	b.pressed.connect(func() -> void: emit_signal("control_pressed", action))
	return b


# A stateful type show/hide toggle (Feature 3). Starts visible (accent tint). On
# press it flips its tracked bool, restyles ("Knowledge ✓" / "Knowledge ✕"), and
# emits control_pressed "type_toggle:<key>:<1|0>" so GraphScene drives the store.
func _type_toggle_btn(label: String, key: String) -> Button:
	var b := _press_fire(Button.new()) as Button
	b.custom_minimum_size = Vector2(0, BTN_H)
	b.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	b.set_meta(HINT_META, "Show / hide all %s nodes" % label.to_lower())
	_style_type_toggle(b, label, true)
	b.pressed.connect(func() -> void:
		var now_visible: bool = not bool(_type_visible.get(key, true))
		_type_visible[key] = now_visible
		_style_type_toggle(b, label, now_visible)
		emit_signal("control_pressed", "type_toggle:%s:%d" % [key, 1 if now_visible else 0]))
	return b


func _style_type_toggle(b: Button, label: String, visible: bool) -> void:
	b.text = "%s %s" % [label, "☑" if visible else "☐"]
	b.add_theme_color_override("font_color", ACCENT if visible else IDLE)


# A fixed-height scroll region for a legitimately-unbounded list: an inner
# ScrollContainer (wand drag-scrolls it natively) plus a ▲/▼ button column for
# wand pointing. The wrapper HBox is what you add to the page; retrieve the inner
# ScrollContainer via `wrapper.get_meta("scroll")` to add content. The ▲▼ column
# is shown only when content overflows the container (see _update_scroll_arrows).
func _scroll_region(height: int) -> HBoxContainer:
	var wrap := HBoxContainer.new()
	wrap.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	wrap.custom_minimum_size = Vector2(0, height)
	wrap.add_theme_constant_override("separation", 6)
	var sc := ScrollContainer.new()
	sc.custom_minimum_size = Vector2(0, height)
	sc.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	sc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	sc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	wrap.add_child(sc)
	var arrows := VBoxContainer.new()
	arrows.add_theme_constant_override("separation", 6)
	var up := _press_fire(Button.new()) as Button
	up.text = "▲"
	up.custom_minimum_size = Vector2(72, BTN_H)
	up.set_meta(HINT_META, "Scroll up")
	up.pressed.connect(func() -> void: sc.scroll_vertical -= DOC_SCROLL_STEP)
	var dn := _press_fire(Button.new()) as Button
	dn.text = "▼"
	dn.custom_minimum_size = Vector2(72, BTN_H)
	dn.set_meta(HINT_META, "Scroll down")
	dn.pressed.connect(func() -> void: sc.scroll_vertical += DOC_SCROLL_STEP)
	arrows.add_child(up)
	arrows.add_child(dn)
	wrap.add_child(arrows)
	wrap.set_meta("scroll", sc)
	wrap.set_meta("arrows", arrows)
	_scroll_regions.append(wrap)
	return wrap


# Show a scroll region's ▲▼ column only when its content overflows the container.
# Cheap; called deferred on tab switch (and once after build).
func _update_scroll_arrows() -> void:
	for wrap: HBoxContainer in _scroll_regions:
		if not is_instance_valid(wrap):
			continue
		var sc: ScrollContainer = wrap.get_meta("scroll")
		var arrows: Control = wrap.get_meta("arrows")
		if sc == null or arrows == null or sc.get_child_count() == 0:
			continue
		var content_h: float = sc.get_child(0).get_combined_minimum_size().y
		arrows.visible = content_h > sc.size.y + 1.0


func _execute_enabled() -> bool:
	# query_builder.gd owns the EXECUTE_ENABLED gate (Execute is a stub until a
	# later phase). Matches the proven pre-redesign access pattern.
	return preload("res://scripts/query_builder.gd").EXECUTE_ENABLED


# --- Tab switching -----------------------------------------------------------

## Show a tab by id (graph|query|pins|session|help). Hides the others, highlights
## the active tab button, and runs the dev-only overflow guard for the shown page.
func _show_tab(id: String) -> void:
	if not _pages.has(id):
		return
	_active_tab = id
	for pid: String in _pages:
		(_pages[pid] as Control).visible = pid == id
	for tid: String in _tab_buttons:
		var b: Button = _tab_buttons[tid]
		XRTheme.apply_tab(b, tid == id)
	call_deferred("_check_overflow", id)
	call_deferred("_update_scroll_arrows")


# Overlay shield: while a full-panel overlay (Document card or Intervention panel)
# is up, hide the tab root so a stray wand ray can't click a control BEHIND the
# overlay (the overlays previously only became visible, leaving the tabs live and
# clickable underneath). The active tab is preserved — restoring just re-shows the
# root with the same page selected.
func _refresh_overlay_shield() -> void:
	if _root == null:
		return
	var overlay_up := (document_panel != null and document_panel.visible) \
		or (intervention_panel != null and intervention_panel.visible)
	_root.visible = not overlay_up


# Dev-only layout-overflow guard: warn ONCE per tab per session if a page's
# content min-height exceeds its host, so the pre-redesign "below the fold" bug
# (controls rendered past the viewport with no scroll) cannot silently return.
# ScrollContainers are exempt (their overflow is intentional). Stripped in release.
func _check_overflow(id: String) -> void:
	if not OS.is_debug_build() or _overflow_warned.has(id):
		return
	var page: Control = _pages.get(id, null)
	if page == null or _tabs_host == null:
		return
	var need: float = page.get_combined_minimum_size().y
	var have: float = _tabs_host.size.y
	if have > 1.0 and need > have + 1.0:
		_overflow_warned[id] = true
		push_warning("[HUD overflow] tab '%s' needs %.0fpx but page host is %.0fpx — content below the fold" % [id, need, have])


# --- Public API (signatures preserved) --------------------------------------

## Set by GraphScene after each control press: the current physics params, edge
## count and node-size factor. Called on press only — no per-frame cost.
func set_controls_status(text: String) -> void:
	if _controls_status != null:
		_controls_status.text = text


## Show a transient operator notice in the bottom strip on every tab (e.g. a
## server write the backend rejected). Overrides the hover hint for `seconds`.
func flash_notice(text: String, seconds: float = NOTICE_SEC) -> void:
	_notice_text = text
	_notice_until_ms = Time.get_ticks_msec() + int(maxf(seconds, 0.0) * 1000.0)


## Whether a flashed notice is still on screen. Public-ish for tests.
func _notice_active() -> bool:
	return not _notice_text.is_empty() and Time.get_ticks_msec() < _notice_until_ms


## Reflect the Hierarchy/View toggle state and pinned-node count on the button
## faces + Pins tab. Press-only, no per-frame cost.
func set_control_states(hierarchy_on: bool, is_flat: bool, pinned_count: int, planes_on: bool = false) -> void:
	if _planes_toggle_button != null:
		_planes_toggle_button.text = "Planes: On" if planes_on else "Planes: Off"
		_planes_toggle_button.add_theme_color_override("font_color", ACCENT if planes_on else IDLE)
	if _hierarchy_button != null:
		_hierarchy_button.text = "Hierarchy: On" if hierarchy_on else "Hierarchy: Off"
		_hierarchy_button.add_theme_color_override("font_color", ACCENT if hierarchy_on else IDLE)
	if _flat_toggle_button != null:
		_flat_toggle_button.text = "View: Flat" if is_flat else "View: 3D"
		_flat_toggle_button.add_theme_color_override("font_color", ACCENT if is_flat else IDLE)
	if _unpin_all_button != null:
		_unpin_all_button.text = "Unpin All (%d)" % pinned_count if pinned_count > 0 else "Unpin All"
	if _pins_count_label != null:
		_pins_count_label.text = "%d pinned" % pinned_count


## Reflect the active layout mode on the Layout Mode cycling button face.
func set_layout_mode_label(mode_label: String) -> void:
	if _layout_mode_button != null:
		_layout_mode_button.text = "Layout: %s" % mode_label


## Reflect the fold-ladder level (Wave 3) on the Fold +/- button faces. `level`
## is clamped [0,3]; Fold + disables at the top of the ladder, Fold - at ∅.
func set_fold_state(level: int) -> void:
	if _fold_plus_button != null:
		_fold_plus_button.text = "Fold + (L%d)" % level
		_fold_plus_button.disabled = level >= 3
	if _fold_minus_button != null:
		_fold_minus_button.disabled = level <= 0


## Show/refresh the query preview. `summary` e.g. "2 vars · 1 edge". `count` < 0
## means unknown (—); `pending` shows the counting spinner; `truncated` renders a
## floor (≥ N) because the server scan hit its cap.
func set_query_preview(summary: String, count: int, pending: bool, truncated: bool) -> void:
	if _query_summary_label != null:
		_query_summary_label.text = summary
	if _query_count_label != null:
		if pending:
			_query_count_label.text = "… counting"
		elif count < 0:
			_query_count_label.text = "—"
		elif truncated:
			_query_count_label.text = "≥ %d matches" % count
		else:
			_query_count_label.text = "%d matches" % count


## Clear the query preview (no active query).
func hide_query_preview() -> void:
	if _query_summary_label != null:
		_query_summary_label.text = "No active query"
	if _query_count_label != null:
		_query_count_label.text = "—"


## Optional hook for the query-builder flagship (task #18): populate the marked-
## variable rows. Each entry is a Dictionary {var: String, label: String}. Safe to
## omit — the list shows a placeholder until called.
func set_marked_vars(rows: Array) -> void:
	if _query_vars_list == null:
		return
	for c in _query_vars_list.get_children():
		c.queue_free()
	if rows.is_empty():
		_query_vars_list.add_child(_mk_label("(mark nodes with the wand MENU button)", "Marked query variables appear here"))
		call_deferred("_update_scroll_arrows")
		return
	for r: Variant in rows:
		if typeof(r) != TYPE_DICTIONARY:
			continue
		var v := str((r as Dictionary).get("var", "?"))
		var lbl := str((r as Dictionary).get("label", ""))
		_query_vars_list.add_child(_mk_label("?%s  ·  %s" % [v, lbl], "Marked variable %s" % v))
	# List height changed → recompute ▲▼ visibility once the new rows have laid out.
	call_deferred("_update_scroll_arrows")


## Optional hook: populate the Pins tab list from the pinned node ids. Cheap —
## call on pin/unpin only. Rows show the id (label lookup stays GraphScene-side).
func set_pinned_ids(ids: Array) -> void:
	if _pins_list == null:
		return
	for c in _pins_list.get_children():
		c.queue_free()
	if ids.is_empty():
		_pins_list.add_child(_mk_label("(none pinned — grab & release a node to pin it)", "Pinned node ids appear here"))
		call_deferred("_update_scroll_arrows")
		return
	for id: Variant in ids:
		_pins_list.add_child(_mk_label("• node %d" % int(id), "Pinned node %d — Unpin All releases it" % int(id)))
	# List height changed → recompute ▲▼ visibility once the new rows have laid out.
	call_deferred("_update_scroll_arrows")


# --- Document view (double-click node → narrativegoldmine page card) ----------

## Open the document view for `slug`, fetching its narrativegoldmine page JSON.
## `title` is the node label shown while loading. Wand-clickable Close/scroll.
func show_document(title: String, slug: String) -> void:
	_doc_title = title
	if document_panel != null:
		document_panel.visible = true
	_refresh_overlay_shield()
	if doc_title_label != null:
		doc_title_label.text = title
	if doc_text != null:
		doc_text.text = "[i]Loading…[/i]"
	if doc_scroll != null:
		doc_scroll.scroll_vertical = 0
	if doc_http == null:
		return
	doc_http.cancel_request()
	var url := "%s%s.json" % [NG_PAGE_BASE, slug.uri_encode()]
	var err := doc_http.request(url)
	if err != OK:
		if doc_text != null:
			doc_text.text = "[i]Could not reach the page service.[/i]"


func hide_document() -> void:
	if document_panel != null:
		document_panel.visible = false
	_refresh_overlay_shield()


func _on_doc_completed(
	result: int,
	response_code: int,
	_headers: PackedStringArray,
	body: PackedByteArray
) -> void:
	if doc_text == null:
		return
	var ok := result == HTTPRequest.RESULT_SUCCESS and response_code >= 200 and response_code < 300
	if not ok:
		doc_text.text = "[i]No linked page.[/i]"
		return
	var parsed: Variant = JSON.parse_string(body.get_string_from_utf8())
	if typeof(parsed) != TYPE_DICTIONARY:
		doc_text.text = "[i]No linked page.[/i]"
		return
	doc_text.text = render_ng_card(_doc_title, parsed)


## Pure builder: narrativegoldmine page Dictionary → BBCode card. Total — never
## errors on missing/oddly-typed fields; unknown sections are skipped.
func render_ng_card(fallback_title: String, data: Dictionary) -> String:
	var lines: Array = []
	var title: String = _dict_str(data, "title", fallback_title)
	lines.append("[font_size=34][b]%s[/b][/font_size]" % _bb(title))
	var sub: Array = []
	var domain := _dict_str(data, "domain", "")
	if domain != "":
		sub.append(domain)
	var entity := _dict_str(data, "entityType", "")
	if entity != "":
		sub.append(entity)
	var maturity := _dict_str(data, "maturity", "")
	if maturity != "":
		sub.append(maturity)
	if data.has("qualityScore") and (data["qualityScore"] is float or data["qualityScore"] is int):
		sub.append("quality %.2f" % float(data["qualityScore"]))
	if not sub.is_empty():
		lines.append("[color=#8fa6c8]%s[/color]" % _bb(" · ".join(sub)))
	var definition := _dict_str(data, "definition", "")
	if definition != "":
		lines.append("")
		lines.append(_bb(definition))
	var rels: Array = _rel_lines(data.get("relationships", []))
	if not rels.is_empty():
		lines.append("")
		lines.append("[b]Relationships[/b]")
		for r: String in rels:
			lines.append("  • %s" % _bb(r))
	var wl: Array = _link_chips(data.get("wikilinks", []), 10)
	var bl: Array = _link_chips(data.get("backlinks", []), 10)
	if not wl.is_empty() or not bl.is_empty():
		lines.append("")
		lines.append("[b]Links[/b]")
		if not wl.is_empty():
			lines.append("[color=#7fd0ff]→[/color] " + " ".join(wl))
		if not bl.is_empty():
			lines.append("[color=#c8a0ff]←[/color] " + " ".join(bl))
	return "\n".join(lines)


func _link_chips(arr: Variant, cap: int) -> Array:
	var out: Array = []
	if not (arr is Array):
		return out
	var items: Array = arr
	var n: int = items.size()
	var shown: int = mini(n, cap)
	for i: int in range(shown):
		var name := _any_name(items[i])
		if name != "":
			out.append("[color=#cfe3ff]%s[/color]" % _bb(name))
	if n > cap:
		out.append("[i]+%d more[/i]" % (n - cap))
	return out


func _rel_lines(arr: Variant) -> Array:
	var out: Array = []
	if not (arr is Array):
		return out
	var items: Array = arr
	for i: int in range(mini(items.size(), 15)):
		var it: Variant = items[i]
		if it is String:
			if it != "":
				out.append(it)
		elif it is Dictionary:
			var pred := _dict_str(it, "predicate", _dict_str(it, "relation", _dict_str(it, "type", "")))
			var tgt := _dict_str(it, "target", _dict_str(it, "object", _dict_str(it, "value", "")))
			if pred != "" and tgt != "":
				out.append("%s → %s" % [pred, tgt])
			elif tgt != "":
				out.append(tgt)
			elif pred != "":
				out.append(pred)
	return out


func _any_name(v: Variant) -> String:
	if v is String:
		return v
	if v is Dictionary:
		return _dict_str(v, "title", _dict_str(v, "label", _dict_str(v, "slug", "")))
	return ""


func _dict_str(d: Dictionary, key: String, fallback: String) -> String:
	if d.has(key) and d[key] is String:
		return d[key]
	return fallback


# Escape BBCode-significant '[' so page content can't inject tags.
func _bb(s: String) -> String:
	return s.replace("[", "[lb]")


func _process(delta: float) -> void:
	# Header + Session diagnostics (per-frame, cheap).
	var conn_str: String = "OK" if _connected else "OFF"
	if _fps_header != null:
		_fps_header.text = "FPS %d" % Engine.get_frames_per_second()
	if _debug_stats != null:
		_debug_stats.text = "FPS: %d  MTP: %.1fms  Avatars: %d  Net: %s" % [
			Engine.get_frames_per_second(), _mtp_ms, _avatar_count, conn_str,
		]
	# Hover-hint bar: resolve the control under the (synthetic) wand pointer via
	# gui_get_hovered_control — which the pushed InputEventMouseMotion updates, so
	# it tracks the ray. Godot's idle tooltip never fires for our synthetic pointer,
	# hence this custom always-visible bar.
	if _hint_bar != null:
		var notice := _notice_active()
		if notice != _hint_bar_notice_mode:
			_hint_bar_notice_mode = notice
			_hint_bar.add_theme_color_override("font_color", NOTICE_COLOR if notice else ACCENT)
		_hint_bar.text = ("⚠ " + _notice_text) if notice else ("ⓘ " + _resolve_hint())
	# Ambient ACSP glow: pulse amber while cases are open, transparent when clear.
	if acsp_glow != null:
		if _open_case_count > 0:
			_pulse_time += delta
			var a: float = 0.2 + 0.25 * (0.5 + 0.5 * sin(_pulse_time * 3.0))
			acsp_glow.color = Color(ACSP_GLOW_COLOR.r, ACSP_GLOW_COLOR.g, ACSP_GLOW_COLOR.b, a)
		else:
			acsp_glow.color = Color(ACSP_GLOW_COLOR.r, ACSP_GLOW_COLOR.g, ACSP_GLOW_COLOR.b, 0.0)


# Walk from the hovered control up its ancestors to the nearest node carrying a
# "hint" meta; fall back to the default. Public-ish for tests.
func _resolve_hint() -> String:
	if hud_control == null:
		return HINT_DEFAULT
	var vp := hud_control.get_viewport()
	if vp == null:
		return HINT_DEFAULT
	var node: Node = vp.gui_get_hovered_control()
	while node != null:
		if node.has_meta(HINT_META):
			return str(node.get_meta(HINT_META))
		node = node.get_parent()
	return HINT_DEFAULT


func set_avatar_count(count: int) -> void:
	_avatar_count = count


func set_mtp_ms(ms: float) -> void:
	_mtp_ms = ms


func _on_connection_status(connected: bool) -> void:
	_connected = connected
	if _conn_dot != null:
		_conn_dot.text = "● OK" if connected else "● OFF"
		_conn_dot.add_theme_color_override("font_color",
			Color(0.4, 0.85, 0.45) if connected else Color(0.9, 0.35, 0.3))
	if _conn_status_label != null:
		_conn_status_label.text = "Connection: on" if connected else "Connection: off"


func _on_join_pressed() -> void:
	if _room_entry == null:
		return
	var urn := _room_entry.text.strip_edges()
	if urn.is_empty():
		push_warning("Empty room URN")
		return
	emit_signal("join_requested", urn)
	var label := "Room: %s" % urn
	if _room_label != null:
		_room_label.text = label
	if _room_header != null:
		_room_header.text = label


func _on_mute_toggled(state: bool) -> void:
	emit_signal("mute_toggled", state)


# --- M2 intervention ---------------------------------------------------------

## Wire the decide path. `http_base` is the visionclaw-server origin (scheme +
## host + port, no trailing slash); `nostr_auth` is the Rust NostrAuth that mints
## the NIP-98 header.
func configure_intervention(http_base: String, nostr_auth: RefCounted) -> void:
	_http_base = http_base.rstrip("/")
	_nostr_auth = nostr_auth


## Show the intervention panel for a broker case awaiting the operator's approval.
func show_case(case_id: String, summary: String) -> void:
	_current_case_id = case_id
	if case_title != null:
		case_title.text = "Awaiting approval — %s" % case_id
	if case_summary != null:
		case_summary.text = summary
	if intervention_panel != null:
		intervention_panel.visible = true
	_refresh_overlay_shield()


func clear_case() -> void:
	_current_case_id = ""
	if intervention_panel != null:
		intervention_panel.visible = false
	_refresh_overlay_shield()


## Ambient ACSP indicator: the count of open broker cases.
func set_case_count(count: int) -> void:
	_open_case_count = maxi(count, 0)
	if acsp_label != null:
		acsp_label.text = "ACSP: %d open" % _open_case_count


## Drive the gaze-dwell charging reticle (Rust SelectionArbiterNode.charge_ratio).
func set_dwell_charge(ratio: float) -> void:
	if dwell_reticle != null and dwell_reticle.has_method("set_charge"):
		dwell_reticle.set_charge(ratio)


func _on_approve_pressed() -> void:
	_submit_decision("approve")


func _on_deny_pressed() -> void:
	_submit_decision("reject")


func approve_selected_case() -> void:
	_submit_decision("approve")


func deny_selected_case() -> void:
	_submit_decision("reject")


# POST the decision to the shared broker decide core (power-user-gated). Signing
# is Rust-side: each NIP-98 token is single-use, enforced by a server-side replay
# cache that rejects any event id presented twice within its validity window (on
# top of the ±60s freshness gate). The intent is emitted immediately so the loop
# is observable even before the round-trip resolves.
func _submit_decision(outcome: String) -> void:
	if _current_case_id.is_empty():
		push_warning("HUD: no case selected for decision")
		return
	if not _pending_case_id.is_empty():
		push_warning("HUD: a decision is already in flight; ignoring new submission")
		return
	var case_id := _current_case_id
	emit_signal("decision_submitted", case_id, outcome)
	if _nostr_auth == null or _http_base.is_empty() or decide_http == null:
		push_warning("HUD: intervention not configured; decision not dispatched")
		return
	var url := "%s/api/broker/cases/%s/decide" % [_http_base, case_id]
	var auth: String = _nostr_auth.nip98_header(url, "POST")
	var pubkey: String = _nostr_auth.pubkey_hex()
	var body := {
		"outcome": outcome,
		"broker_pubkey": pubkey,
		"reasoning": "in-headset operator decision",
	}
	var headers := PackedStringArray([
		"Authorization: %s" % auth,
		"Content-Type: application/json",
	])
	var err := decide_http.request(url, headers, HTTPClient.METHOD_POST, JSON.stringify(body))
	if err != OK:
		push_warning("HUD: decide request failed to start (%d)" % err)
		return
	_last_outcome = outcome
	_pending_case_id = case_id


func _on_decide_completed(
	result: int,
	response_code: int,
	_headers: PackedStringArray,
	_body: PackedByteArray
) -> void:
	var transport_ok := result == HTTPRequest.RESULT_SUCCESS
	var accepted := transport_ok and response_code >= 200 and response_code < 300
	var decided_case := _pending_case_id
	_pending_case_id = ""
	if not accepted:
		push_warning("HUD: decide failed for %s (result=%d code=%d)" % [
			decided_case, result, response_code])
	emit_signal("case_decided", decided_case, _last_outcome, accepted)
	if accepted and _current_case_id == decided_case:
		clear_case()


func set_demo_active(active: bool) -> void:
	_demo_active = active
	if _demo_button != null:
		_demo_button.text = "Stop Agent Demo" if active else "Start Agent Demo"
		_demo_button.add_theme_color_override("font_color", ACCENT if active else IDLE)


func set_visual_comfort(reduced_motion: bool, low_cost: bool) -> void:
	if _motion_toggle != null:
		_motion_toggle.set_pressed_no_signal(reduced_motion)
	if _quality_toggle != null:
		_quality_toggle.set_pressed_no_signal(low_cost)
