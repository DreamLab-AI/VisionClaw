extends RefCounted

## Text entry for the HUD's Memory Search (ADR-2136): the model behind the
## on-screen QWERTY keyboard that hud.gd draws as press-fire buttons.
##
## The headset has no text entry of its own and no speech service may be
## added (the only microphone path is the beat analyser, Invariant 10), so a
## query is typed with the wand: every key is a button that fires on trigger
## press. This script owns the buffer and the key semantics only; it builds no
## nodes, so it is tested without a HUD.
##
## `press(key)` takes a key id and returns what happened:
##   {"event": "typed", "text": <buffer>}   a character, space or backspace
##   {"event": "submit", "text": <query>}   enter with a non-blank buffer
##   {"event": "cancel", "text": ""}        close without searching
##   {"event": "none", "text": <buffer>}    enter on a blank buffer, an unknown
##                                          key, or a full buffer

## Key ids that are not characters.
const SPACE := "space"
const BACKSPACE := "backspace"
const ENTER := "enter"
const CANCEL := "cancel"

## Longest query the keyboard accepts. The server takes 2000 characters
## (memory_query.rs MAX_QUERY_CHARS); a typed headset query is a short
## question, and the HUD echo line shows about this many.
const MAX_CHARS := 120

## Character rows, top to bottom. Digits, then QWERTY; the punctuation keys
## are the ones a memory question needs (hyphenated keys, ADR numbers,
## possessives, questions).
const ROWS: Array = [
	["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
	["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
	["a", "s", "d", "f", "g", "h", "j", "k", "l", "'"],
	["z", "x", "c", "v", "b", "n", "m", "-", ".", "?"],
]

var text: String = ""


## Every character key, row by row.
static func character_keys() -> Array:
	var out: Array = []
	for row: Array in ROWS:
		out.append_array(row)
	return out


func press(key: String) -> Dictionary:
	match key:
		SPACE:
			# no leading or doubled spaces: they would only be trimmed away
			if text.is_empty() or text.ends_with(" "):
				return _result("none")
			return _append(" ")
		BACKSPACE:
			if text.is_empty():
				return _result("none")
			text = text.substr(0, text.length() - 1)
			return _result("typed")
		ENTER:
			var q: String = text.strip_edges()
			if q.is_empty():
				return _result("none")
			text = ""
			return {"event": "submit", "text": q}
		CANCEL:
			text = ""
			return {"event": "cancel", "text": ""}
	if key.length() == 1 and character_keys().has(key):
		return _append(key)
	return _result("none")


func clear() -> void:
	text = ""


func _append(c: String) -> Dictionary:
	if text.length() >= MAX_CHARS:
		return _result("none")
	text += c
	return _result("typed")


func _result(event: String) -> Dictionary:
	return {"event": event, "text": text}
