extends Node

## Memory search from the headset (ADR-2133 client side; route look ADR-2134).
##
## The Query page's Memory mode takes a typed query from the on-screen
## keyboard (ADR-2136) and keeps presets as shortcuts (recent queries on this
## headset, a curated set, one question per namespace in the loaded snapshot).
## Either POSTs /api/memory-cloud/query with GraphScene's NIP-98 headers
## (ADR-2076) and shows the sidecar's top hits. The headset never runs HNSW,
## so the route it draws is **query point → sidecar top-k**: from the query's
## own point in the cloud (`query.position`, the snapshot's PCA basis) through
## the sampled hits in rank order (`memory_query.rs`). It goes through the
## same MemoryRoute gate as a desktop relay, so beads, comet, answer ring, the
## guide cue and the Memory-row line all work, and a later desktop relay
## replaces it. The HUD caption says which kind of route is shown.
##
## Pressing a hit sends the guide cue to its point (sampled hits only).
## There is no voice entry: the headset's only microphone path is the beat
## analyser, whose audio never leaves the device (Invariant 10).

const ENDPOINT := "/api/memory-cloud/query"
## The server's ceiling (memory_query.rs HEADSET_K): the cloud samples thinly,
## so a top-10 rarely has two hits with a point to route through. The HUD
## lists the top 8.
const K := 50
## How often the preset list checks for a new snapshot's namespaces.
const PRESET_POLL_SEC := 1.0
## Namespaces the keyboard's Scope key offers besides all of memory.
const SCOPE_NAMESPACES := 8
## The typed-query scope chosen when the snapshot has it.
const DEFAULT_SCOPE := "project-state"
## Estate namespaces offered first when the snapshot has them (the curated
## presets' namespaces, memory_query.rs CURATED): the most-sampled list alone
## is led by thinly covered corpora and left `patterns` out live (2026-10-08).
const ESTATE_SCOPES: Array[String] = ["project-state", "patterns", "coordination"]
const TIMEOUT_SEC := 15.0

## Recent queries persist here between launches (JSON array of presets).
var recent_path: String = "user://memory_search_recent.json"
var queries_sent: int = 0

var _query: RefCounted = null      # Rust MemoryQuery
var _http: HTTPRequest = null
var _http_base: String = ""
var _auth: Callable = Callable()
var _layer: Node = null            # memory_cloud_layer.gd
var _hud: Node = null
var _presets: Array = []
var _hits: Array = []
var _pending: bool = false
var _pending_preset: Dictionary = {}
var _presets_for: String = "\u0000"   # snapshot id the presets were built for
var _poll: float = 0.0


func _init() -> void:
	if ClassDB.class_exists("MemoryQuery"):
		_query = MemoryQuery.create()


## `auth` is GraphScene._auth_headers: (url, method) -> PackedStringArray.
func setup(http_base: String, auth: Callable, layer: Node, hud: Node) -> void:
	_http_base = http_base.rstrip("/")
	_auth = auth
	_layer = layer
	_hud = hud
	if _http == null:
		_http = HTTPRequest.new()
		_http.timeout = TIMEOUT_SEC
		add_child(_http)
		_http.request_completed.connect(on_query_completed)
	_load_recent()
	refresh_presets()
	_status("Type a query or pick one · hits come from the memory sidecar's own search")


func _process(delta: float) -> void:
	_poll += delta
	if _poll < PRESET_POLL_SEC:
		return
	_poll = 0.0
	if _snapshot_id() != _presets_for:
		refresh_presets()


## HUD intents: "memory_preset:<i>", "memory_hit:<i>",
## "memory_typed:<namespace>|<text>" (namespace "" = all). True when consumed.
func handle_control(action: String) -> bool:
	if action.begins_with("memory_preset:"):
		run_preset(int(action.get_slice(":", 1)))
		return true
	if action.begins_with("memory_typed:"):
		var rest: String = action.substr("memory_typed:".length())
		var bar: int = rest.find("|")
		if bar < 0:
			run_text(rest)
		else:
			run_text(rest.substr(bar + 1), rest.substr(0, bar))
		return true
	if action.begins_with("memory_hit:"):
		focus_hit(int(action.get_slice(":", 1)))
		return true
	return false


func presets() -> Array:
	return _presets


func refresh_presets() -> void:
	_presets_for = _snapshot_id()
	var ns := PackedStringArray()
	var counts := PackedInt32Array()
	if _layer != null and _layer.has_method("namespaces"):
		ns = _layer.namespaces()
		counts = _layer.namespace_row_counts()
	_presets = _query.presets(ns, counts) if _query != null else []
	if _hud != null and _hud.has_method("set_memory_presets"):
		_hud.set_memory_presets(_presets.map(func(p: Dictionary) -> String: return str(p["label"])))
	if _hud != null and _hud.has_method("set_memory_scopes"):
		var sc: Array = scopes()
		_hud.set_memory_scopes(sc, default_scope_index(sc))


## Run preset `i`. False when it was not sent (out of range, one already in
## flight, or the request could not start).
func run_preset(i: int) -> bool:
	if i < 0 or i >= _presets.size():
		return false
	return _run(_presets[i])


## Run a typed query (the on-screen keyboard) within namespace `scope` ("" = all of
## memory). False when it was not sent (blank, one already in flight, or the
## request could not start).
func run_text(text: String, scope: String = "") -> bool:
	var t: String = text.strip_edges()
	if t.is_empty():
		return false
	return _run({"label": t, "text": t, "namespace": scope.strip_edges()})


## Where a typed query may search: "" (all of memory), then the ESTATE_SCOPES
## the snapshot has, then its most-sampled namespaces, up to SCOPE_NAMESPACES
## in all (each with at least two sampled rows and no whitespace in the name,
## as for the presets).
func scopes() -> Array:
	var out: Array = [""]
	if _layer == null or not _layer.has_method("namespaces"):
		return out
	var ns: PackedStringArray = _layer.namespaces()
	var counts: PackedInt32Array = _layer.namespace_row_counts()
	var rows: Array = []
	for i in mini(ns.size(), counts.size()):
		var nm: String = ns[i]
		if counts[i] >= 2 and not nm.is_empty() and not (" " in nm or "\t" in nm):
			rows.append([nm, counts[i]])
	rows.sort_custom(func(a: Array, b: Array) -> bool: return a[1] > b[1] or (a[1] == b[1] and str(a[0]) < str(b[0])))
	var usable: Array = rows.map(func(r: Array) -> String: return str(r[0]))
	for e: String in ESTATE_SCOPES:
		if usable.has(e):
			out.append(e)
	for nm: String in usable:
		if out.size() > SCOPE_NAMESPACES:
			break
		if not out.has(nm):
			out.append(nm)
	return out


## The scope a typed query starts in: DEFAULT_SCOPE when the snapshot has it
## (a global top-k lands in the thinly sampled reference corpus: measured live
## 2026-10-08, 0 of 50 hits in the sample), else all of memory.
func default_scope_index(scopes: Array) -> int:
	return maxi(0, scopes.find(DEFAULT_SCOPE))


func _run(p: Dictionary) -> bool:
	if _pending or _query == null:
		return false
	var body: String = str(_query.request_body(str(p["text"]), K, str(p["namespace"])))
	if body.is_empty():
		_status("Query not sent: %s" % str(_query.last_error()))
		return false
	var url := "%s%s" % [_http_base, ENDPOINT]
	var headers := PackedStringArray()
	if _auth.is_valid():
		headers = _auth.call(url, "POST")
	if not _post(url, headers, body):
		_status("Query not sent: the request could not start")
		return false
	_pending = true
	_pending_preset = p
	queries_sent += 1
	# the route needs the cloud: turn it on (it loads the snapshot the hits name)
	if _layer != null and _layer.has_method("is_enabled") and not _layer.is_enabled():
		_layer.set_enabled(true)
	_status("Searching memory: “%s”…" % str(p["text"]))
	return true


## The HTTP door (a test seam).
func _post(url: String, headers: PackedStringArray, body: String) -> bool:
	if _http == null or _http_base.is_empty():
		return false
	return _http.request(url, headers, HTTPClient.METHOD_POST, body) == OK


func on_query_completed(result: int, code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	_pending = false
	var p: Dictionary = _pending_preset
	_pending_preset = {}
	if result != HTTPRequest.RESULT_SUCCESS:
		_fail("Memory search unreachable (result %d)" % result)
		return
	if code < 200 or code >= 300:
		_fail(_describe_failure(code, body))
		return
	var text := body.get_string_from_utf8()
	var d: Dictionary = _query.ingest_response(text)
	if not bool(d.get("ok", false)):
		_fail("Memory search answer could not be read: %s" % str(d.get("error", "")))
		return
	_hits = d.get("hits", [])
	if _hud != null and _hud.has_method("set_memory_hits"):
		_hud.set_memory_hits(str(d.get("caption", "")), _hits)
	if _layer != null and _layer.has_method("apply_query_response"):
		_layer.apply_query_response(text)
	if not p.is_empty():
		_query.record_recent(str(p["text"]), str(p["namespace"]))
		_save_recent()
		refresh_presets()


## Point the guide cue at hit `i`. False (with a HUD notice) when the hit has
## no point in the cloud or no route is drawn.
func focus_hit(i: int) -> bool:
	if i < 0 or i >= _hits.size():
		return false
	var row: int = int((_hits[i] as Dictionary).get("row", -1))
	if row < 0:
		_notice("That hit is not in the sampled cloud: no point to guide to")
		return false
	if _layer == null or not _layer.has_method("focus_row") or not _layer.focus_row(row):
		_notice("No route drawn: turn the memory cloud on, or fewer than 2 hits are in the sample")
		return false
	return true


func _describe_failure(code: int, body: PackedByteArray) -> String:
	match code:
		401, 403:
			return "Memory search locked (HTTP %d): needs a power-user NIP-98 key, or VISIONCLAW_DEV_MODE=1 on a dev backend" % code
		429:
			return "Memory search budget used up (HTTP 429): retry in a minute"
		503:
			return "Memory sidecar unavailable (HTTP 503): try again shortly"
		400:
			var err: Variant = JSON.parse_string(body.get_string_from_utf8())
			var why: String = str(err.get("error", "")) if typeof(err) == TYPE_DICTIONARY else ""
			return "Memory search rejected (HTTP 400): %s" % why
	return "Memory search failed (HTTP %d)" % code


func _fail(text: String) -> void:
	push_warning("MemorySearch: %s" % text)
	_status(text)


func _status(text: String) -> void:
	if _hud != null and _hud.has_method("set_memory_search_status"):
		_hud.set_memory_search_status(text)


func _notice(text: String) -> void:
	if _hud != null and _hud.has_method("flash_notice"):
		_hud.flash_notice(text, 4.0)


func _snapshot_id() -> String:
	if _layer != null and _layer.has_method("snapshot_id"):
		return str(_layer.snapshot_id())
	return ""


# --- recent queries (user://, per headset) -------------------------------------

func _load_recent() -> void:
	if _query == null or not FileAccess.file_exists(recent_path):
		return
	var v: Variant = JSON.parse_string(FileAccess.get_file_as_string(recent_path))
	if typeof(v) != TYPE_ARRAY:
		return
	# stored newest first: replay oldest first so the order survives
	var items: Array = v
	for i in range(items.size() - 1, -1, -1):
		var p: Variant = items[i]
		if typeof(p) == TYPE_DICTIONARY:
			_query.record_recent(str(p.get("text", "")), str(p.get("namespace", "")))
	refresh_presets()


func _save_recent() -> void:
	if _query == null:
		return
	var f := FileAccess.open(recent_path, FileAccess.WRITE)
	if f == null:
		push_warning("MemorySearch: cannot save recent queries to %s" % recent_path)
		return
	f.store_string(JSON.stringify(_query.recent()))
