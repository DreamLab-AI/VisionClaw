extends "res://addons/gut/test.gd"

# The model behind the HUD's on-screen keyboard (scripts/onscreen_keyboard.gd,
# ADR-2136): key semantics only, no nodes.

const Keyboard := preload("res://scripts/onscreen_keyboard.gd")


func test_rows_are_qwerty_with_digits_and_ten_keys_each() -> void:
	assert_eq(Keyboard.ROWS.size(), 4)
	for row: Array in Keyboard.ROWS:
		assert_eq(row.size(), 10, "ten keys per row: %s" % str(row))
	assert_eq("".join(Keyboard.ROWS[1]), "qwertyuiop")
	assert_eq("".join(Keyboard.ROWS[2]).left(9), "asdfghjkl")
	assert_eq("".join(Keyboard.ROWS[3]).left(7), "zxcvbnm")
	assert_eq("".join(Keyboard.ROWS[0]), "1234567890")
	assert_eq(Keyboard.character_keys().size(), 40)


func test_typing_space_backspace_and_enter() -> void:
	var k := Keyboard.new()
	assert_eq(k.press("space")["event"], "none", "no leading space")
	for c in ["a", "d", "r"]:
		assert_eq(k.press(c)["event"], "typed")
	assert_eq(k.press("space")["text"], "adr ")
	assert_eq(k.press("space")["event"], "none", "no doubled space")
	k.press("2")
	k.press("1")
	assert_eq(k.press("backspace")["text"], "adr 2")
	var r: Dictionary = k.press("enter")
	assert_eq(r, {"event": "submit", "text": "adr 2"})
	assert_eq(k.text, "", "cleared after a search")
	assert_eq(k.press("enter")["event"], "none", "a blank buffer is not searched")
	assert_eq(k.press("backspace")["event"], "none")


func test_cancel_unknown_keys_and_the_length_cap() -> void:
	var k := Keyboard.new()
	k.press("x")
	assert_eq(k.press("cancel"), {"event": "cancel", "text": ""})
	assert_eq(k.press("Q")["event"], "none", "only the keyboard's own keys")
	assert_eq(k.press("ab")["event"], "none")
	for i in Keyboard.MAX_CHARS + 5:
		k.press("a")
	assert_eq(k.text.length(), Keyboard.MAX_CHARS, "capped")
	assert_eq(k.press("enter")["text"].length(), Keyboard.MAX_CHARS)
