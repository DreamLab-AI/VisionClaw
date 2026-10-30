extends "res://addons/gut/test.gd"
const SpatialVolume := preload("res://scripts/spatial_volume.gd")

# ADR-2158 D5, phase-1 contract §3.2 and §8 (agent B): the shared fit maths.


func test_fit_scale_isotropic() -> void:
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(4.0, 4.0, 4.0)), 0.25, 1.0e-9)
	assert_almost_eq(SpatialVolume.fit_scale(Vector3(2.0, 2.0, 2.0), Vector3.ONE), 2.0, 1.0e-9)


func test_fit_scale_anisotropic_checks_each_axis() -> void:
	# The binding axis moves across x, y and z; the result is always the minimum ratio.
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(10.0, 2.0, 1.0)), 0.1, 1.0e-9, "x binds")
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(1.0, 10.0, 2.0)), 0.1, 1.0e-9, "y binds")
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0, 1.0, 10.0)), 0.1, 1.0e-9, "z binds")
	# Anisotropic bounds: 4/8 = 0.5 on x beats 1/1 on y and 2/1 on z.
	assert_almost_eq(SpatialVolume.fit_scale(Vector3(4.0, 1.0, 2.0), Vector3(8.0, 1.0, 1.0)), 0.5, 1.0e-9)
	var b := Vector3(1.0, 1.5, 0.5)
	var e := Vector3(3.0, 7.0, 0.25)
	var s := SpatialVolume.fit_scale(b, e)
	for i: int in 3:
		assert_true(e[i] * s <= b[i] + 1.0e-9, "axis %d fits" % i)


func test_fit_scale_ignores_axes_below_min_extent() -> void:
	# A flat graph (z extent 0) fits on x and y alone.
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0, 4.0, 0.0)), 0.25, 1.0e-9)
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0, 4.0, 1.0e-7)), 0.25, 1.0e-9)
	# Just above MIN_EXTENT qualifies. (Vector3 is f32: 1e-6 itself rounds to
	# 9.99999997e-7, below the double MIN_EXTENT, so it does not.)
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0e-6, 0.0, 0.0)), 5.0e5, 1.0)


func test_fit_scale_all_axes_below_min_extent_is_one() -> void:
	assert_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3.ZERO), 1.0, "empty")
	assert_eq(SpatialVolume.fit_scale(Vector3(3.0, 2.0, 1.0), Vector3(1.0e-7, 0.0, 9.0e-7)), 1.0, "a single point")
