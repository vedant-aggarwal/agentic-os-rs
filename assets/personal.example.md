# Personal hero files (never committed)

His presets and 3D models live on the server PC in `~/.aos/personal/` (or `AOS_PERSONAL`):
`hero3d.json`, `character/stage-01.glb` ... `stage-30.glb` and `companion.glb`. The server serves
them to paired devices at `GET /api/personal/<path>` (device token required), and the React app
caches them on the device (IndexedDB) so the next launch is instant. Nothing personal is bundled
into the web build or the phone app. The Dioxus build can still read a local copy from the
gitignored `assets/personal/` (`build.rs` creates it empty on a fresh clone). With no presets the
hero stays on the 2D art (React) or a faceless mannequin (Dioxus).

| File | What it does |
|---|---|
| `hero3d.json` | Presets: character keyframes per level (fat, muscle, slouch, hair, jaw, height, outfit), skin/hair/eye colours, companion keyframes (`vivid` 0..1 = ghostly to vivid), and optional `overrides`. |
| `*.glb` (optional) | Model overrides named in `hero3d.json`. Compress with `gltfpack -cc -tc` (meshopt + KTX2) or plain meshopt; the loader includes the meshopt decoder. |

`hero3d.json` shape:

```json
{
  "maxLevel": 30,
  "character": {
    "skin": "#a46f55", "hairColor": "#111016", "eyeColor": "#3f2617",
    "keyframes": [
      { "level": 1,  "fat": 1.0, "muscle": 0.0, "slouch": 1.0, "hair": 0.0, "jaw": 0.0, "height": 1.80, "outfit": 0 },
      { "level": 30, "fat": 0.02, "muscle": 1.0, "slouch": 0.0, "hair": 1.0, "jaw": 1.0, "height": 1.83, "outfit": 9 }
    ]
  },
  "companion": {
    "skin": "#e8bb9b", "gown": "#0b3a3a", "eyeColor": "#6b3fa8",
    "keyframes": [ { "level": 1, "vivid": 0.0 }, { "level": 30, "vivid": 1.0 } ]
  },
  "overrides": {
    "character": [ { "fromLevel": 1, "url": "char-stage-01.glb", "height": 1.8 } ],
    "companion": [ { "fromLevel": 1, "url": "companion-01.glb" } ],
    "bike":    { "url": "interceptor.glb", "height": 0.9 },
    "macbook": { "url": "macbook.glb", "height": 0.75 }
  }
}
```

Leave `overrides` empty (`{}`) to use the procedural models. Leave `companion` out to get the crystal.
