---
name: readme
description: >
  How to write and update sonisub's README.md and version.md entries for users. Use whenever a
  change is visible to the user (command line, output files, library API, Premiere panel), when
  editing README.md, or when reviewing user-facing text.
---

# README for users

The README is first for people who download the `sonisub` binary and run it on their footage; it
is also the crate's page on crates.io, so its "As a library" section serves Rust developers
(frename is one). The Premiere Pro panel has its own `premiere-plugin/README.md`, which follows
the same rules.

## Rules

- Plain language, short sentences, second person.
- Organised by what the reader does (download, run on files and folders, choose outputs, read
  costs and errors, use it as a library, build), not by code modules.
- A new option gets a line in "Examples" or one or two sentences where a reader would look for it.
  The full option list is `sonisub --help`; the README does not repeat it.
- Examples and sample output must match what the current build prints.
- The "As a library" section names only public items, and its dependency line matches the
  current minor version.
- Keep it compact: when adding, check whether an older paragraph can be shortened or removed.
  Target: README stays under ~250 lines.
- Technical details that are still worth keeping go to `docs/` or doc comments.
- English only.

## version.md

Follow `.claude/skills/create-release-version/SKILL.md`. One line per user-visible change, written
as what the user can now do or what now behaves differently. A breaking library API change says
what callers must change.
