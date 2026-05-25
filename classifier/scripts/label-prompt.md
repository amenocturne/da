# Bash Command Safety Labeling

You are labeling bash commands for training a safety classifier. The classifier will gate bash tool calls in a coding agent runtime.

## Input

The project lives at `/Users/skril/Vault/Projects/personal/agentic-kit/tools/smart-approve/classifier`.

Read `data/commands-public.jsonl` from that directory — each line is `{"command": "..."}`. There are ~13,800 commands.

## Task

Split the commands into chunks of 500. For each chunk, spawn a subagent that processes every command and outputs structured JSONL.

### Per-command output schema

```json
{
  "command": "the sanitized command",
  "label": "safe | needs-approval | dangerous",
  "confidence": 1-5,
  "reason": "one-line rationale",
  "pii": ["original → replacement", ...]
}
```

### Label definitions

**safe** — Read-only, no side effects, no data exfiltration risk. Auto-approve without user prompt.
- Reading files, listing directories, searching, text processing pipelines
- Build/test commands that only affect the project directory (cargo check, npm test, go build)
- Version control reads (git status, git log, git diff)
- System info queries (uname, sw_vers, df, ps)

**needs-approval** — Write operations with bounded blast radius, or commands that could be safe but depend on context. Prompt the user.
- File modifications within a project (sed -i, file writes, mkdir, mv, cp)
- Package installation (pip install, npm install, cargo install)
- Git writes (git add, git commit, git push)
- Running project code (npm run, cargo run, python script.py)
- Docker operations, server connections (ssh)
- Network requests that send data (curl -X POST, wget)
- Process management (kill, pkill)

**dangerous** — Destructive, irreversible, system-level, or security-sensitive. Block by default.
- Recursive deletion outside project dirs (rm -rf /)
- System file modification (chmod on system paths, chown)
- Pipe to shell (curl | bash)
- Force operations (git push --force, git reset --hard)
- Credential/secret exposure
- Obfuscated commands (base64 decode | eval, hex encoding)
- Privilege escalation (sudo on destructive commands)

### Bias toward restriction

When uncertain, label MORE restrictive:
- Uncertain between safe and needs-approval → needs-approval
- Uncertain between needs-approval and dangerous → dangerous
- False positives (rejecting safe commands) are cheap — just one extra user prompt
- False negatives (approving dangerous commands) can cause real harm

### PII detection

Flag any personal identifiers that survived regex pre-sanitization:
- Real hostnames, domain names (except well-known ones like github.com, pypi.org)
- Real usernames, email addresses
- Internal project names that reveal identity
- API keys, tokens, passwords
- Server IPs (except localhost, 0.0.0.0, standard RFC examples)

For each PII found, suggest a replacement that preserves the command's semantic structure.

If no PII found, output `"pii": []`.

### Confidence scoring

- **5**: Unambiguous (ls -la → safe, rm -rf / → dangerous)
- **4**: Clear with minor context dependency
- **3**: Genuinely ambiguous, could go either way depending on context
- **2**: Uncertain, defaulting to more restrictive label
- **1**: Can't determine without more context

## Output

Each subagent writes its chunk to `data/labeled/chunk-NNN.jsonl` inside the classifier directory, one JSON object per line matching the schema above.

After all subagents complete, merge by running:

```bash
cd /Users/skril/Vault/Projects/personal/agentic-kit/tools/smart-approve/classifier && uv run scripts/merge-chunks.py -i data/labeled -o data/labeled.jsonl
```

Report summary stats: label distribution, avg confidence, PII count.

## Execution

1. `cd /Users/skril/Vault/Projects/personal/agentic-kit/tools/smart-approve/classifier`
2. Read `data/commands-public.jsonl`, count total commands
3. Split into chunks of 500
4. Spawn one subagent per chunk (parallel)
5. Each subagent: read its assigned line range from the file, label every command, write `data/labeled/chunk-NNN.jsonl`
6. After all complete: run the merge command above, report stats
