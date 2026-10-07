# Godot Addons

Install via Godot AssetLib at first project open. Pinned versions:

| Addon | Version | Source |
|---|---|---|
| GUT (Godot Unit Test) — **vendored** in `addons/gut/` | 9.6.1 (upstream tag `v9.6.1`, commit `c80954f47bed74a0a2c471d472c0389f98e0a8f6`, branch `main` = the Godot 4.6 line; MIT, `gut/LICENSE.md`) | https://github.com/bitwes/Gut |
| Godot OpenXR Vendors | 3.0.x | https://github.com/GodotVR/godot_openxr_vendors |
| Godot OpenXR Loaders (Khronos + Meta + Pico) | bundled with Vendors plugin | (same) |

GUT is committed so the suite runs against the pinned version on the runtime
actually used (Godot 4.6.1). Pick the GUT line for the engine: 9.6.x (`main`)
targets Godot 4.6, 9.7.x (`godot_4_7`) targets Godot 4.7. GUT 9.3.x fails to
compile on Godot ≥ 4.5 (its `Logger` shadows the engine's new native class), and
9.7.1 logs Parse Errors on 4.6.1 (`godot_singletons.gd:5` names the 4.7-only
`AccessibilityServer`; `stub_params.gd:16` returns null as `StringName`), so
GUT 9.7.1 was replaced by 9.6.1 on 2026-10-07. Update it only by replacing
`addons/gut/` wholesale from an upstream release tag and recording the tag and
commit above. The other addons are not committed; CI runs `godot --headless --import` first
which populates `addons/` from AssetLib. After install, the directory contains:

```
addons/
  godot_openxr_vendors/
    plugin.cfg
    config/
    extension/
```

LiveKit Android AAR (PRD-008 §5.5) lands here in a follow-up sprint.
