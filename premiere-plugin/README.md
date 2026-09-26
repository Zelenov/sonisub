# sonisub panel for Premiere Pro

Imports the transcripts made by `sonisub -t premiere` into clips of the open project, all at once, instead of
Import Static Transcript clip by clip. Needs Premiere Pro 25.6 or newer.

## Use

1. Make the transcripts: `sonisub E:/Footage -r -t premiere` (next to each video: `clip.premiere.json`).
2. In Premiere: Window > UXP Plugins > sonisub.
3. Select clips or bins in the Project panel (nothing selected = the whole project) and press **Import transcripts**.

- A transcript is looked up next to the clip's media file as `<name>.premiere.json`. If they were written
  elsewhere (`sonisub -d subs`), press **Also look in folder…** and pick that folder.
- Clips that already have a transcript are skipped unless **Replace existing transcripts** is on
  (the check needs Premiere 26.3; on older versions every clip is imported).
- Each import is a separate step in Edit > Undo.

## Install

For yourself, no packaging needed:

1. Install the **UXP Developer Tool** from the Creative Cloud app (turn on developer mode if it asks).
2. With Premiere running: Add Plugin > pick `premiere-plugin/manifest.json` > Load.

To share it: in UXP Developer Tool, … > Package makes a `.ccx`; double-clicking it installs through Creative Cloud.

## Files

| File | Does |
|---|---|
| `manifest.json` | plugin id, Premiere 25.6+, full file-system access (to read the `.json` next to any media) |
| `index.html` | the panel |
| `main.js` | finds clips, reads `<name>.premiere.json`, `Transcript.importFromJSON` + `createImportTextSegmentsAction` |
