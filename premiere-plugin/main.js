// Imports sonisub's Premiere transcripts (clip.premiere.json next to the media, or in a chosen folder)
// into clips of the open project. Uses Transcript.importFromJSON / createImportTextSegmentsAction (Premiere 25.6+).

const ppro = require("premierepro");
const { localFileSystem: lfs } = require("uxp").storage;

const $ = (id) => document.getElementById(id);
let extraFolder = null;

/** Media clips: the selected ones (bins expanded), or every clip in the project. */
async function clips(project) {
  const selection = await ppro.ProjectUtils.getSelection(project);
  const picked = await selection.getItems();
  const queue = picked.length ? [...picked] : await (await project.getRootItem()).getItems();
  const out = [];
  const seen = new Set(); // a clip selected together with its bin comes up twice
  for (let i = 0; i < queue.length; i++) {
    const item = queue[i];
    const bin = ppro.FolderItem.cast(item);
    if (bin) {
      queue.push(...(await bin.getItems()));
      continue;
    }
    const clip = ppro.ClipProjectItem.cast(item);
    if (clip && (await clip.getContentType()) === ppro.Constants.ContentType.MEDIA) {
      const key = `${clip.name}\n${await clip.getMediaFilePath()}`;
      if (!seen.has(key)) {
        seen.add(key);
        out.push(clip);
      }
    }
  }
  return out;
}

function splitPath(path) {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  const dir = path.slice(0, cut + 1);
  const name = path.slice(cut + 1);
  const dot = name.lastIndexOf(".");
  return { dir, stem: dot > 0 ? name.slice(0, dot) : name };
}

/** Text of a file by native path, or null if it isn't there. */
async function readPath(path) {
  const unix = path.split("\\").join("/");
  // UXP wants a file: URL; which spelling it accepts for Windows drive paths varies, so try both.
  for (const url of ["file:" + unix, "file:/" + unix.replace(/^\/+/, "")]) {
    try {
      const entry = await lfs.getEntryWithUrl(url);
      if (entry && entry.isFile) return await entry.read();
    } catch (_) {
      // not found under this spelling
    }
  }
  return null;
}

async function transcriptFor(clip) {
  const media = await clip.getMediaFilePath();
  if (!media) return null;
  const { dir, stem } = splitPath(media);
  const text = await readPath(`${dir}${stem}.premiere.json`);
  if (text !== null || !extraFolder) return text;
  try {
    const entry = await extraFolder.getEntry(`${stem}.premiere.json`);
    return entry.isFile ? await entry.read() : null;
  } catch (_) {
    return null;
  }
}

function hasTranscript(clip) {
  // Transcript.hasTranscript exists from Premiere 26.3; before that every clip counts as empty.
  return typeof ppro.Transcript.hasTranscript === "function" && ppro.Transcript.hasTranscript(clip);
}

async function importAll() {
  const overwrite = $("overwrite").checked;
  const project = await ppro.Project.getActiveProject();
  if (!project) return "No open project.";
  const all = await clips(project);
  const r = { imported: [], had: 0, missing: 0, failed: [] };
  for (const clip of all) {
    if (!overwrite && hasTranscript(clip)) {
      r.had++;
      continue;
    }
    const text = await transcriptFor(clip);
    if (text === null) {
      r.missing++;
      continue;
    }
    try {
      const segments = ppro.Transcript.importFromJSON(text);
      let ok = false;
      project.lockedAccess(() => {
        ok = project.executeTransaction((tx) => {
          tx.addAction(ppro.Transcript.createImportTextSegmentsAction(segments, clip));
        }, `Import transcript: ${clip.name}`);
      });
      if (ok) r.imported.push(clip.name);
      else r.failed.push(`${clip.name}: Premiere refused the transcript`);
    } catch (e) {
      r.failed.push(`${clip.name}: ${e}`);
    }
  }
  const lines = [
    `${all.length} clip(s): ${r.imported.length} imported, ${r.had} already had a transcript, ${r.missing} without .premiere.json`,
  ];
  if (r.failed.length) lines.push(`${r.failed.length} failed:`, ...r.failed.map((f) => "  " + f));
  if (r.imported.length) lines.push("", "Imported:", ...r.imported.map((n) => "  " + n));
  return lines.join("\n");
}

$("run").addEventListener("click", async () => {
  $("run").disabled = true;
  $("report").textContent = "Importing…";
  try {
    $("report").textContent = await importAll();
  } catch (e) {
    $("report").textContent = `Error: ${e}`;
  } finally {
    $("run").disabled = false;
  }
});

$("pick").addEventListener("click", async () => {
  const folder = await lfs.getFolder();
  if (folder) {
    extraFolder = folder;
    $("folder").textContent = folder.nativePath;
  }
});
