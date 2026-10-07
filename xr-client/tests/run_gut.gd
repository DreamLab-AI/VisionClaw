extends SceneTree

# ---------------------------------------------------------------------------
# GUT 9.3 headless runner shim (PRD-QE-002 §4.2).
#
# GUT 9.x ships its own headless entry point, `res://addons/gut/gut_cmdln.gd`,
# which extends SceneTree and parses `-g*` command-line flags. The pre-9.x
# programmatic API this script used to drive (GutMain.set_log_level /
# add_directory / test_scripts / the `tests_finished` signal) no longer exists,
# so the old runner parsed as a no-op and hung the CI job. A SceneTree cannot
# host a second SceneTree, so there is nothing useful to "wrap" here.
#
# CANONICAL INVOCATION — call gut_cmdln.gd directly (this is what CI runs):
#
#   godot --headless --xr-mode off --path xr-client \
#     -s res://addons/gut/gut_cmdln.gd \
#     -gdir=res://tests/unit -ginclude_subdirs \
#     -gexit -gjunit_xml_file=res://tests/report/junit.xml
#
# or, equivalently, using the checked-in config so the flags stay in one place:
#
#   godot --headless --xr-mode off --path xr-client \
#     -s res://addons/gut/gut_cmdln.gd -gconfig=res://.gutconfig.json
#
# `-gexit` makes GUT quit with a non-zero status on any failure; the JUnit XML
# lands under res://tests/report/ for CI to collect. GUT 9.7.1 is vendored under
# res://addons/gut/ (pinned upstream tag; see addons/README.md).
#
# This shim exists only so `-s res://tests/run_gut.gd` fails LOUDLY with the
# correct command instead of silently hanging. It never reports a false pass.
# ---------------------------------------------------------------------------

const GUT_CMDLN := "res://addons/gut/gut_cmdln.gd"
const CANONICAL := \
	"godot --headless --path xr-client -s %s -gconfig=res://.gutconfig.json" % GUT_CMDLN


func _init() -> void:
	printerr("run_gut.gd is a shim for GUT 9.3 — do not invoke it directly.")
	if ResourceLoader.exists(GUT_CMDLN):
		printerr("Run GUT's own headless entry point instead:\n    %s" % CANONICAL)
	else:
		printerr("GUT is not installed at %s; install it, then run:\n    %s"
			% [GUT_CMDLN, CANONICAL])
	# Non-zero: a CI step that still targets this script must fail, not pass.
	quit(2)
