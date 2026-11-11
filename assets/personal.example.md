# assets/personal/ (gitignored)

Personal 3D data for the hero lives here and is never committed. The app ships this
folder as-is (`asset!("/assets/personal", AssetOptions::folder())`); `build.rs` creates it
empty on a fresh clone. With the folder empty the hero still renders: a faceless
mannequin that morphs with level, and a floating crystal for the Mind goal.

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
