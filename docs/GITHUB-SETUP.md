# Publish the project to GitHub

Status: complete (2026-10-15) — https://github.com/ahmadmirzamasood/weave-css. Historical note: The planning environment could write files but had neither a repository-creation connector nor GitHub CLI. Repository creation is already requested by the user; this is an execution prerequisite, not a new permission requirement.

## Commands for an authenticated local environment

Run from the extracted project directory. Install Git and GitHub CLI first if absent. Inspect existing git state before initialization and do not nest inside an unrelated project.

```bash
gh auth status
gh api user --jq .login
git init -b main
git status --short
git add README.md START-HERE.md CLAUDE.md MEMORY.md PRD.md DEV-PLAN.md SECURITY.md RELEASE.md CONTRIBUTING.md LICENSE CHANGELOG.md .gitignore .github instructions docs prompts
git commit -m "docs: initialize Weave development plan"
gh repo create weave-css --public --source=. --remote=origin --push --description "An independent CSS preprocessor written in Rust"
gh repo view --json nameWithOwner,url,visibility
git rev-parse HEAD
git ls-remote origin refs/heads/main
```

Check name availability first and adjust the repository name if occupied; never reuse an unrelated repository. Do not add `--license` or a generated README to this command: the local files already exist. If git identity is missing, use the user's configured identity; do not invent one. Compare local and remote SHA and verify visibility PUBLIC.

Record URL in README and MEMORY. Enable Issues and private vulnerability reporting in repository settings; inspect whether rulesets/branch protection are available for the account and permissions. Restrict force pushes/deletions on main, require relevant CI after phase 02, and document any unavailable setting. Do not claim that publishing source alone configures security reporting.

Future changes: create `phase/NN-short-name` branches, commit coherent changes, push, and review before merge. Keep all implementation files and reproducibility evidence in GitHub. Keep build artifacts out of git; publish them through releases when authorized.
