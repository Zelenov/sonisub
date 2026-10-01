# sonisub — agent guide

This file is what an agent needs to build, test and ship sonisub without a human in the loop.

## Layout

- `src/job.rs` — one file: media → audio → Soniox → `.srt` / `.premiere.json`, cleanup;
  `job::process`, `Options`, `Outcome`.
- `src/batch.rs` — folders into a list of files and a plan. `src/usage.rs` — what was spent.
- `src/audio.rs` — audio track → 16 kHz mono FLAC (symphonia, or ffmpeg when it is on `PATH`).
- `src/soniox.rs` — the Soniox REST client, error types, the remote cleanup guard.
- `src/languages.rs` — language codes from Soniox. `src/cancel.rs` — Ctrl+C and `CancelToken`.
- `src/srt.rs` — transcript tokens → cues → SRT. `src/premiere.rs` — → Premiere Pro transcript.
- `src/main.rs`, `src/cli.rs` — the `sonisub` command line (feature `cli`).
- `tests/` — integration tests against a mock Soniox; `tests/fixtures/` — real Soniox output with
  its media and the expected `.srt` / `.premiere.json` (made by `scripts/make-fixture.sh`);
  `tests/live.rs` — the one real Soniox round trip (`#[ignore]`).
- `premiere-plugin/` — a Premiere Pro UXP panel (JavaScript) that imports `.premiere.json`
  transcripts. Nothing tests it automatically; a change to it is checked by the owner by hand.
- `.claude/skills/` — project skills (nightly, review-gate, readme, create-release-version).
- `docs/design/` — design notes for features that needed them (created with the first one).
- `version.md` — release notes; its first line `# X.Y.Z` is the version and must equal
  `version` in `Cargo.toml` (`release.yml` checks). A change to it on `main` makes a GitHub release
  `vX.Y.Z` with Windows, Linux and macOS builds, then publishes the crate to crates.io
  (`.github/workflows/release.yml`). **A crates.io version can never be replaced.** To pause a
  release PR, convert it to a draft.

Features: `cli` (default; clap, ctrlc, the binary), none (the library alone; frename uses this).

## Commands

What CI runs (`.github/workflows/ci.yml`), plus the release build:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo clippy --locked --no-default-features -- -D warnings
cargo test --locked
cargo build --release --locked
```

`release.yml` also runs `cargo test --release --locked` on Windows, Linux and macOS.

`SONIOX_API_KEY` set → `cargo test --test live -- --ignored` transcribes `tests/fixtures/dialog.mp4`
for real (a few seconds of audio, well under a cent); CI runs it when the secret exists.

The Rust toolchain is pinned in `rust-toolchain.toml` so a new stable release cannot turn CI red
overnight. Updating it is its own PR (new lints get fixed there). Formatting follows
`rustfmt.toml`.

No system packages are needed: audio decoding is pure Rust, TLS is rustls.

## Autonomous work

Features are GitHub issues in `Zelenov/sonisub`. The unattended pipeline — picking an issue,
implementing it, independent review, CI, merge and release — is `.claude/skills/nightly/SKILL.md`.
Reviewers follow `.claude/skills/review-gate/SKILL.md`. User-facing text follows
`.claude/skills/readme/SKILL.md`.

The issue is the request. Do exactly what the issue asks (the agent's design notes only fill in
details), nothing beyond it. Ideas of your own become new issues labelled `idea`, never extra code
in the current PR.

## Consumers

frename (`Zelenov/frename`) uses sonisub as a library from crates.io:
`sonisub = { version = "0.3", default-features = false }` in its root `Cargo.toml`.

- A breaking library API change (a removed or renamed public item, a changed signature, a new
  field in a public struct callers build (`job::Options`), a changed `Outcome` or error variant)
  bumps the minor version and is described in `version.md` (`## Changed`, what callers must
  change) so frename can follow.
- The library without default features must keep building: CI checks it.
- Every release moves frename to it, in the same session as the release, once the version is on
  crates.io; not gated on an `approved` label. Set the requirement in frename's `Cargo.toml` to
  the new version (needed for a minor bump), `cargo update -p sonisub`, make whatever code change
  `version.md`'s `## Changed` requires (nothing, for an additive release), build and run
  frename's test suite, then commit (`Cargo.toml` + `Cargo.lock`, plus any adapted call site) and
  open a PR there, following frename's own nightly/review-gate process for it like any other
  change; link it from this release's PR. If frename's repo is outside this session's access,
  file an issue there instead (labelled `feature`, body `🤖 agent:`, the version, what changed,
  what frename must do), so the next session with access can do it without re-deriving anything.

## Labels

| Label | Set by | Meaning |
|---|---|---|
| `P1` `P2` `P3` | owner/agent | Priority (lower number first). |
| `regression` | owner | A release broke something; picked before everything else. |
| `feature`, `process` | owner/agent | Kind of work. |
| `needs-design` | owner/agent | The agent writes its own design notes in `docs/design/` on the feature branch before coding. Not a gate; the owner does not approve designs. |
| `approved` | owner | Makes an `idea` or a non-owner issue implementable. |
| `idea` | agent | Agent's own proposal; not implemented until `approved`. |
| `in-progress` | agent | An agent session is working on it (see heartbeat lock). |
| `awaiting-owner` | agent | Legacy, no longer set: the pipeline never waits for design answers. |
| `needs-owner` | agent | On an issue: agent cannot proceed at all (guarded file, failed release); owner answers and removes it to let the agent retry. |
| `owner-review` | agent | Code review or CI did not converge: the feature is built on its PR but not merged or released. Owner merges it, or removes the label from the PR to hand it back. |
| `hold` | owner | Do not work on / merge this. |
| `blocked`, `rejected` | owner | Not now / never. |
| `agent` | agent | PR opened by the agent pipeline. |
| `release-failed` | agent | A release run failed twice; blocks version bumps until fixed. |

## Owner setup (one-time, GitHub settings)

Settings → Rules → Rulesets → new branch ruleset for `main`, enforcement Active, **no bypass list
(administrators included)**:
- require a pull request before merging (0 approvals: the agent uses the owner's account);
- require status checks `ci-linux` and `ci-windows`, and branches up to date before merging;
- block force pushes; restrict deletions.

Settings → General → Pull Requests: allow squash merging only.

Settings → Secrets and variables → Actions: `SONIOX_API_KEY` (live test) and
`CARGO_REGISTRY_TOKEN` (crates.io publish).

This makes the gates enforceable, not just written down: the agent merges with the owner's account,
so without the ruleset nothing stops a merge on red CI.

## Never

- Commit secrets, or print them in logs, PR bodies or sample output. The Soniox key lives in the
  user's environment (`SONIOX_API_KEY`) or `--api-key` at runtime; in agent sessions it is an
  environment variable, in CI a GitHub Actions secret. `CARGO_REGISTRY_TOKEN` exists only in CI.
- Run `cargo publish` (other than `--dry-run`) or `cargo yank`: publishing is `release.yml`'s.
- Run `sonisub purge --yes` against the owner's Soniox account: it deletes files and
  transcriptions that may not be sonisub's.
- Skip, disable or weaken a test to get CI green, or regenerate fixtures to make a failing test
  pass without explaining the `.srt` diff in the PR.
- Force-push `main` or rewrite its history.
- Merge a PR whose CI is not green on its latest commit.
