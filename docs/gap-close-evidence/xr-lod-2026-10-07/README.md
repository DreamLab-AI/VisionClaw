# XR LOD and halo — visual evidence (2026-10-07)

Captured on HP (Godot 4.6.1-stable, Compatibility/opengl3, X11, `--xr-mode off`)
from `tests/spatial_visual_fixture.tscn` and scratch close-ups using production
materials. Crops of 1024×768 captures.

| File | What |
|---|---|
| `halo-before-after.png` | Top: gem with the old halo `next_pass` shell. Bottom: gem + `NodesHaloMulti` quad (`node_halo_quad.gdshader`). Plain, fold representative (wider blue ring) and query variable (wider ring). 0.59 % of pixels differ by > 8/255. |
| `impostor-left-gem-right-impostor.png` | Same fixture data: left clusters gem tier, right clusters far-tier impostors (`node_impostor.gdshader`). |
| `edges-left-cylinder-right-ribbon.png` | Same fixture data: left edges near-tier cylinders, right edges far-tier ribbons (`edge_ribbon.gdshader`), incl. dashed subclass/inferred styles. |
| `fixture-before-after.png` | Top: fixture at `9f7ce003f` (before halo/edge changes). Bottom: after. RMSE 0.0048. |

Headset (stereo) look is not certified by these desktop captures.
