# Start here

This is a multi-file development kit, not a finished Rust compiler.

Open a terminal in this extracted directory and start Claude Code. Paste:

```text
Read CLAUDE.md and MEMORY.md. Execute prompts/phase-00-github.md completely.
The project must be public and MIT licensed. Verify the repository URL,
visibility and pushed commit. Do not implement later phases yet.
```

After bootstrap, use this pattern for each phase:

```text
Read CLAUDE.md and MEMORY.md. Execute prompts/phase-01-language-spec.md.
Implement the full requested phase, verify its acceptance gates, update
project memory, commit and push the phase branch, then report the next phase.
```

Replace the prompt path using DEV-PLAN.md. Use one phase at a time, review its outputs, then continue. Do not paste all prompts into one enormous session. Claude should read the files directly so the repository remains the shared source of truth.

If authentication is missing, authenticate GitHub CLI locally with `gh auth login`; do not paste tokens into chat or project files. See docs/GITHUB-SETUP.md. The current preparation environment did not create a repository. Package names and project branding remain provisional.
