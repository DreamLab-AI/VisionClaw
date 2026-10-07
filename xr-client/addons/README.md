# Godot Addons

Install via Godot AssetLib at first project open. Pinned versions:

| Addon | Version | Source |
|---|---|---|
| GUT (Godot Unit Test) — **vendored** in `addons/gut/` | 9.7.1 (upstream tag `v9.7.1`, commit `aeb5d4f3f7f0a6c9b5e178876d6c99b791fda605`; MIT, `gut/LICENSE.md`) | https://github.com/bitwes/Gut |
| Godot OpenXR Vendors | 3.0.x | https://github.com/GodotVR/godot_openxr_vendors |
| Godot OpenXR Loaders (Khronos + Meta + Pico) | bundled with Vendors plugin | (same) |

GUT is committed so the suite runs against the pinned version on the runtime
actually used (Godot 4.6.1): GUT 9.3.x fails to compile on Godot ≥ 4.5 (its
`Logger` shadows the engine's new native class). Update it only by replacing
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
