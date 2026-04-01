---
name: version-bump-reminder
description: After pushing a PR, remind the user to bump the version and recommend the appropriate semver bump type based on the changes.
user_invocable: false
triggers:
  - after creating or pushing a PR
  - after `gh pr create`
  - after `git push` when a PR exists
---

# Version Bump Reminder

After pushing a PR (either creating a new one or pushing to an existing one), remind the user about version bumping.

## When to trigger

Activate this skill whenever you:
- Create a new PR with `gh pr create`
- Push commits to a branch that has an open PR

## What to do

### 1. Analyze the changes

Look at all commits in the PR (not just the latest) to understand the scope:

```bash
git log --oneline $(git merge-base HEAD master)..HEAD
git diff --stat $(git merge-base HEAD master)..HEAD
```

### 2. Recommend a semver bump

Apply these rules to determine the bump type:

| Bump | When | Examples |
|------|------|----------|
| **Major** (x.0.0) | Breaking changes to CLI flags, config format, exit codes, or public API. Removing or renaming commands/options. Changes that would break existing CI pipelines or scripts using desktest. | Removing a subcommand, changing task JSON schema in a non-backward-compatible way, changing exit code meanings |
| **Minor** (0.x.0) | New features, new subcommands, new CLI flags, new capabilities that are backward-compatible. | Adding a new subcommand, adding a new evaluation metric type, adding a new app config type |
| **Patch** (0.0.x) | Bug fixes, documentation changes, refactors, CI/build changes, dependency updates, performance improvements with no API changes. | Fixing a bug in the agent loop, updating dependencies, improving error messages, build/release process changes |

**Default to patch** when uncertain. Only recommend minor or major when there's clear justification.

### 3. Present the recommendation

Format your reminder like this:

---

**Version bump reminder:** This PR is ready to merge. After merging, you'll want to bump the version.

**Recommendation: [bump type]** (current -> next) — [1-2 sentence justification based on the changes]

To bump after merge:
```sh
git checkout master && git pull
make bump_version VERSION=<next>
```

Then, to trigger the release workflow, push the commit and tag:
```sh
git push && git push --tags
```

Or if you'd prefer a different bump (fill in the computed version number):
- Patch: `make bump_version VERSION=<computed-patch-version, e.g. 0.2.2>`
- Minor: `make bump_version VERSION=<computed-minor-version, e.g. 0.3.0>`
- Major: `make bump_version VERSION=<computed-major-version, e.g. 1.0.0>`

Would you like me to proceed with the [bump type] bump, or do you want a different version?

---

### 4. Wait for user confirmation

Do NOT run `make bump_version` automatically. Always wait for the user to confirm the version. They may:
- Accept your recommendation
- Choose a different bump level
- Skip the bump entirely (e.g. if more PRs are planned before a release)
- Specify an exact version number

### 5. If confirmed, execute the bump

Only after the user confirms:
1. Make sure you're on `master` with latest changes: `git checkout master && git pull`
2. Run `make bump_version VERSION=<confirmed_version>`
3. Ask if they want you to push: `git push && git push --tags`

## Important notes

- Read `Cargo.toml` to get the current version before making recommendations
- The `make bump_version` target handles: updating Cargo.toml, running cargo check, committing, and tagging
- The tag push triggers the release workflow which builds binaries for all 6 targets
- If the PR hasn't been merged yet, remind about bumping but note it should happen post-merge
