extends RefCounted

## The HUD's information architecture (ADR-2189): the seven sections shared with
## the desktop, read from `data/ia.json`, and where each XR page sits under them.
##
## `data/ia.json` is generated on the desktop from the settings registry
## (`client/scripts/emit-settings-manifest.ts`, `buildIa()`) and committed
## byte-identical next to the desktop copy; a Vitest test pins both copies to the
## registry. Its fields are desktop registry paths, which the headset does not
## render one by one. What the HUD takes from it is the vocabulary: section ids,
## labels, order and hotkeys, the subsection names, and the scope texts. The XR
## controls are placed into those sections by `XR_PAGES` below, and the GUT parity
## test (`tests/unit/test_hud_ia.gd`) checks that every XR subsection name is one
## of that section's ia.json subsections or is listed in `XR_ONLY`.
##
## Two levels at most (ADR-2188 §6, ADR-2189 §1): a section tab, then a
## subsection on the rail beside the page.

const IA_PATH := "res://data/ia.json"

## The seven section ids in their fixed order (ADR-2189 §1).
const SECTION_IDS: Array[String] = ["layers", "explore", "encode", "layout", "look", "agents", "system"]

## Used when ia.json is missing or malformed, so the headset still has its menus.
## `test_hud_ia.gd` checks this against the committed ia.json.
const FALLBACK_SECTIONS: Array = [
	{"id": "layers", "label": "Layers", "hotkey": "1"},
	{"id": "explore", "label": "Explore", "hotkey": "2"},
	{"id": "encode", "label": "Encode", "hotkey": "3"},
	{"id": "layout", "label": "Layout", "hotkey": "4"},
	{"id": "look", "label": "Look", "hotkey": "5"},
	{"id": "agents", "label": "Agents & Governance", "hotkey": "6"},
	{"id": "system", "label": "System", "hotkey": "7"},
]
const FALLBACK_SCOPES: Dictionary = {
	"shared": "Changes the layout for everyone",
	"server": "Saved to your account",
	"browser": "This browser only",
}

## Section → its XR subsections, in rail order. `page` is the HUD page id
## (hud.gd `_pages`); `label` is the rail text; `groups` are the group headers the
## page shows (each a subsection name too). One page per rail entry.
const XR_PAGES: Dictionary = {
	"layers": [
		{"page": "layers", "label": "Populations", "groups": ["Populations", "Memory cloud", "Cluster hulls", "Splat scene"]},
	],
	"explore": [
		{"page": "query", "label": "Query builder", "groups": ["Query builder", "Memory"]},
		{"page": "pins", "label": "Pins", "groups": ["Pins"]},
	],
	"encode": [
		{"page": "encode", "label": "Colour & size", "groups": ["Colour & size", "Labels"]},
		{"page": "key", "label": "Legend", "groups": ["Legend"]},
	],
	"layout": [
		{"page": "layout", "label": "Mode", "groups": ["Mode", "Forces", "Shape"]},
		{"page": "shapes", "label": "Shape", "groups": ["Shape"]},
		{"page": "forces", "label": "Semantic forces", "groups": ["Semantic forces"]},
	],
	"look": [
		{"page": "look", "label": "Nodes", "groups": ["Nodes", "Edges", "Animation"]},
	],
	"agents": [
		{"page": "swarm", "label": "Agent operations", "groups": ["Agent operations"]},
	],
	"system": [
		{"page": "session", "label": "Connection", "groups": ["Connection"]},
		{"page": "headset", "label": "Headset", "groups": ["Headset", "Performance"]},
		{"page": "help", "label": "Help & keys", "groups": ["Help & keys"]},
	],
}

## XR subsection names that have no desktop field (headset-only controls), by section.
const XR_ONLY: Dictionary = {
	"layers": ["Splat scene"],
	"explore": ["Query builder", "Pins"],
	"encode": ["Legend"],
	"layout": ["Semantic forces"],
	"look": [],
	"agents": [],
	"system": ["Headset", "Help & keys"],
}

## Parse an ia.json document. Returns {"ok", "error", "sections", "scopes"}, where
## each section is {"id", "label", "hotkey", "subsections": Array[String]}. On any
## problem `ok` is false and the fallback sections and scopes are returned.
static func load_doc(path: String = IA_PATH) -> Dictionary:
	var fallback := {"ok": false, "error": "", "sections": _fallback_sections(), "scopes": FALLBACK_SCOPES.duplicate()}
	if not FileAccess.file_exists(path):
		fallback["error"] = "missing %s" % path
		return fallback
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(path))
	if not (parsed is Dictionary) or not (parsed.get("sections") is Array):
		fallback["error"] = "%s is not an IA document" % path
		return fallback
	var sections: Array = []
	for s: Variant in parsed["sections"]:
		if not (s is Dictionary):
			continue
		var subs: Array[String] = []
		for sub: Variant in s.get("subsections", []):
			if sub is Dictionary:
				subs.append(str(sub.get("label", "")))
		sections.append({"id": str(s.get("id", "")), "label": str(s.get("label", "")),
			"hotkey": str(s.get("hotkey", "")), "subsections": subs})
	var ids: Array[String] = []
	for s: Dictionary in sections:
		ids.append(s["id"])
	if ids != SECTION_IDS:
		fallback["error"] = "%s sections %s are not the seven of ADR-2189" % [path, str(ids)]
		return fallback
	var scopes: Dictionary = FALLBACK_SCOPES.duplicate()
	if parsed.get("scopes") is Dictionary:
		for k: String in FALLBACK_SCOPES:
			if parsed["scopes"].has(k):
				scopes[k] = str(parsed["scopes"][k])
	return {"ok": true, "error": "", "sections": sections, "scopes": scopes}


static func _fallback_sections() -> Array:
	var out: Array = []
	for s: Dictionary in FALLBACK_SECTIONS:
		var d: Dictionary = s.duplicate()
		d["subsections"] = [] as Array[String]
		out.append(d)
	return out


## The section that holds HUD page `page_id`, or "" when no section does.
static func section_of_page(page_id: String) -> String:
	for sid: String in XR_PAGES:
		for e: Dictionary in XR_PAGES[sid]:
			if e["page"] == page_id:
				return sid
	return ""


## The HUD page ids of section `sid`, in rail order.
static func pages_of(sid: String) -> Array[String]:
	var out: Array[String] = []
	for e: Dictionary in XR_PAGES.get(sid, []):
		out.append(e["page"])
	return out


## Rail label of HUD page `page_id` ("" when unplaced).
static func rail_label(page_id: String) -> String:
	for sid: String in XR_PAGES:
		for e: Dictionary in XR_PAGES[sid]:
			if e["page"] == page_id:
				return e["label"]
	return ""


## Names in `XR_PAGES[sid]` (rail labels and group headers) that are neither an
## ia.json subsection of that section nor XR-only. Empty when the map is consistent.
static func unknown_names(sid: String, ia_subsections: Array) -> Array[String]:
	var out: Array[String] = []
	var allowed: Array = ia_subsections.duplicate()
	allowed.append_array(XR_ONLY.get(sid, []))
	for e: Dictionary in XR_PAGES.get(sid, []):
		for name: String in [e["label"]] + Array(e["groups"]):
			if not allowed.has(name) and not out.has(name):
				out.append(name)
	return out
