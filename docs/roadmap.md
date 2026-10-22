# Roadmap

ukiyo is a personal hub for a Fujifilm X-series camera: a lot of power underneath, a calm and
simple surface on top.

## v0 — Lab (in progress)

The live property mapper: connection core, fast polling, change feed, labelling into
`catalog/<model>.json`, guarded writes, keep-awake while connected.

## Camera hub

- **Overview** of the connected camera: model, firmware, body and lens serials, lenses seen,
  battery, card space.
- **History** per camera, kept on this Mac: shot count and other counters on every connect,
  charted over time (shots per day, per month, since a date).
- **Owner details** editable: copyright and author strings, file-name prefix.

## Film simulation recipes

- A recipe library with an editor for every field the camera stores (film sim, grain, Color
  Chrome, white balance and shift, tones, colour, sharpness, NR, clarity, D-Range).
- Read and write C1-C7 (USB RAW CONV./BACKUP RESTORE mode). Swap whole sets in and out.
- Import from recipe sites (Fuji X Weekly and others) by pasting a link or the recipe text.
- Rebuild a recipe from a photo's metadata.
- A neutral calibration slot for colour-profile shooting.

## Settings

- The camera's menus laid out in Fujifilm's own structure, with every setting editable from
  the Mac (tether mode).
- Search by name, and by meaning: semantic search plus extra keywords per setting, so
  "eye focus" finds FACE/EYE DETECTION SETTING.
- Saved setting presets ("street", "studio") and favourite single settings.
- Snapshots of the whole camera, with a diff between two.

## Look and feel

- Visual identity from *ukiyo* (the floating world) and ukiyo-e prints, in the spirit of
  Ishiguro's *An Artist of the Floating World*. An app icon combining a film camera, Fujifilm's
  X bodies and woodblock-print style.

## Open questions

- Whether recipe-site import can use structured data or needs text parsing per site.
- Which on-device model to use for semantic settings search.
