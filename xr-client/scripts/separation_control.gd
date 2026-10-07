extends RefCounted

## Write coalescer for the HUD Graph Separation control (ADR-2135).
##
## The desktop slider (Motion › Layout Forces › "Separate Knowledge · Ontology ·
## Memory") writes `graphSeparationX` on every change. A wand drag produces a
## value every frame, and GraphScene's physics path allows one PUT in flight,
## so this object decides WHEN a value goes out; GraphScene does the PUT.
##
##   * While dragging: at most one write every DRAG_INTERVAL_MS (≤ 4 Hz).
##   * On release or a −/+ press: the value goes out as soon as the gate frees.
##   * Only the newest intent is kept, so a busy gate never queues stale values.
##   * A value equal to the server's is never sent.
##
## Time is passed in (`now_ms`) so the throttle is testable without a clock.

## Same range and step as the desktop slider (control-center motion.ts).
const MIN: float = 0.0
const MAX: float = 400.0
const STEP: float = 5.0
## 250 ms = 4 writes a second while the wand drags the slider.
const DRAG_INTERVAL_MS: int = 250

var dragging: bool = false
## Newest value not yet sent (NAN = none).
var _target: float = NAN
## Value sent and awaiting its response (NAN = none).
var _in_flight: float = NAN
## When the throttle last let a value out; a release ignores it.
var _last_sent_ms: int = -DRAG_INTERVAL_MS
## True once the newest intent should go out without waiting for the throttle.
var _immediate: bool = false


## Round to the 5-unit step and clamp to 0–400.
static func snap(v: float) -> float:
	return clampf(roundf(v / STEP) * STEP, MIN, MAX)


## The wand moved the slider; the value goes out on the next 4 Hz tick.
func drag(v: float) -> void:
	dragging = true
	_target = snap(v)


## The wand let go: the final value goes out as soon as the gate frees.
func release(v: float) -> void:
	dragging = false
	_target = snap(v)
	_immediate = true


## A −/+ press. Builds on an unsent or in-flight intent, so quick presses add up.
func step(delta: float, committed: float) -> void:
	var base: float = committed
	if not is_nan(_target):
		base = _target
	elif not is_nan(_in_flight):
		base = _in_flight
	var v: float = snap(base + delta)
	if v == base:
		return   # at a rail: nothing to send
	_target = v
	_immediate = true


## The value to PUT now, or NAN. `gate_busy` is GraphScene's one-in-flight
## physics gate; `committed` is the value the server last confirmed.
func take(now_ms: int, gate_busy: bool, committed: float) -> float:
	if is_nan(_target) or gate_busy:
		return NAN
	if _target == committed and is_nan(_in_flight):
		# Nothing to change on the server. A drag keeps going; anything else is done.
		_target = NAN
		_immediate = false
		return NAN
	if not _immediate and now_ms - _last_sent_ms < DRAG_INTERVAL_MS:
		return NAN
	var v: float = _target
	_target = NAN
	_immediate = false
	_in_flight = v
	_last_sent_ms = now_ms
	return v


## The PUT could not be dispatched: keep the value for the next frame.
func requeue(v: float) -> void:
	if is_nan(_target):
		_target = v
	_in_flight = NAN


## The PUT answered (either outcome). GraphScene commits or discards the value.
func settled() -> void:
	_in_flight = NAN


## True while the operator's intent is ahead of the server (dragging, queued or
## in flight); read-back must not move the slider then.
func holding() -> bool:
	return dragging or not is_nan(_target) or not is_nan(_in_flight)


## What the HUD should show: the newest intent, else the in-flight value, else
## the server's.
func display_value(committed: float) -> float:
	if not is_nan(_target):
		return _target
	if not is_nan(_in_flight):
		return _in_flight
	return committed
