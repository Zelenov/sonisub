---
name: review-gate
description: >
  Independent multi-agent review that decides whether a sonisub change (code PR or design doc)
  may merge. Use before opening or merging any agent-made PR, after every fix round, and when
  asked to "review", "gate" or "check if this is ready for main".
---

# Review gate

The author of a change never judges it. Reviewers are subagents started with fresh context: they get
the issue, the design doc (if any), the commit SHA under review, the PR body (for the sample
output), and the diff against `main` — not the author's reasoning. Run them in parallel. A verdict
applies to that SHA only.

## Reviewers

Code PR — all three, every round:

1. **Correctness** — tries to break the change. Reads the diff and the code around it, looks for
   logic errors, panics/`unwrap` in production paths, and in particular:
   - cost and billing: a file is never transcribed twice (a saved `.soniox.json` is reused, a
     create that timed out is not sent again), a retry or loop that sends a paid request twice, a
     batch that goes on after a rejected key, an exhausted balance or budget; prices and the cost
     line against Soniox's published prices and usage logs (cite the page);
   - API key handling: the key never reaches logs, `sonisub.log`, `Debug` output, error messages,
     panics, saved JSON or other files; `--api-key` / `SONIOX_API_KEY` precedence unchanged unless
     asked;
   - Soniox: remote files and transcriptions deleted on success, errors, Ctrl+C and a cancelled
     `CancelToken`; timeouts on every request, retries only where safe and bounded, HTTP status
     and Soniox `error_type` mapped to the right error (fatal ones stop the batch);
   - audio: files with no audio track, digital silence, odd codecs, the ffmpeg fallback, very
     short and very long files, Windows paths and non-ASCII names;
   - output: SRT timing and numbering, cue cutting (sentence ends, punctuation, pauses, never
     mixing speakers, line limits), the Premiere JSON against Adobe's import format, existing
     outputs skipped or overwritten as documented, exit codes;
   - both feature combinations build and are tested where they apply (`cli`, none), regressions
     in untouched callers (frename uses the library with `default-features = false`).
   Runs the local gate (`nightly` step 5). Where it suspects a bug it writes a failing test to
   prove it. Checks
   `git diff origin/main... -- '*.rs' '*.toml' 'rust-toolchain*' '.cargo/**' '.github/**' 'tests/fixtures/**'`
   for weakened gates: removed or loosened asserts, new `#[ignore]`, deleted tests, `cfg` that
   hides a test on the CI platforms, the live test made to pass without checking anything,
   expected fixtures changed without a reason given in the PR, CI steps or flags removed or
   relaxed. Each is a blocker unless the issue requires it. Any change to a guarded file (list in
   `nightly` → Trust) that the issue does not explicitly ask for is a blocker.
2. **Design and quality** — could this be simpler, smaller, more in line with the codebase? Checks
   module boundaries (HTTP in `soniox.rs`, audio in `audio.rs`, cue cutting in `srt.rs`, the
   command line only in `main.rs`/`cli.rs`, nothing clap/ctrlc outside `feature = "cli"`),
   naming, duplication, dead code, new dependencies (needed? optional behind the right feature?
   default features off where possible? still no system libraries and no OpenSSL?), scope creep
   beyond the issue, missing tests for new behaviour, doc comments on new public items.
3. **Product** — does it do what the issue and design ask, from the user's point of view?
   - Command line: option names and defaults consistent with the existing ones, `--help` text,
     stdout for results and stderr for progress, exit codes, the plan (`-n`) and the cost line,
     error messages that say what to do.
   - Output files: subtitles a video editor would accept without fixing, file names and where
     they are written, the `.soniox.json` and `.premiere.json` formats stable (a changed field is
     a breaking change).
   - Library API: easy to call right and hard to call wrong, consistent with `job::process` and
     `Options`, no needless breaking change; a breaking change is named in `version.md` with what
     callers must change, and the PR says what frename must change.
   - Premiere panel (when touched): the flow in `premiere-plugin/README.md` still holds, and the
     PR lists what the owner must check by hand in Premiere Pro.
   - README and `version.md` text (short, user language, per `readme` skill; the README's
     examples match the code).
   - The PR body has `## Sample output` with the exact command and its output for every visible
     change, or "No visible change." when there is none (a missing or wrong sample is a `major`
     finding).

Design doc (advisory only, one round, never blocks; see `nightly` step 3) — reviewers 2 and 3,
judging the design: missing flows, edge cases, cost, simpler alternatives, feasibility (is every
API/format/price claim sourced?).

## Verdict format

Each reviewer returns:

```
VERDICT: APPROVE | CHANGES_REQUIRED
FINDINGS:
- [blocker|major|minor] file:line — problem — concrete failure scenario — suggested fix
```

APPROVE is allowed with only `minor` findings. Any `blocker` or `major` means CHANGES_REQUIRED.
A finding without a concrete failure scenario or a concrete improvement is dropped.

## Loop

1. Fix every blocker/major (and minors that are cheap and clearly right).
2. Re-run the local gate.
3. Start a **new** round with fresh reviewers (never reuse a reviewer that saw an earlier round —
   it anchors on its old findings). Give them the full current diff.
4. Repeat until all three approve in the same round. Four rounds without that → the change is
   finished as far as possible and left unmerged for the owner (`nightly` → "Owner review").

Record every round in the PR description: SHA, each reviewer's verdict, findings count, what was fixed.
