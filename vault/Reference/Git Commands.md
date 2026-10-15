---
tags: [reference, git, code]
---
# Git Commands

## Every day

```bash
git status
git add -p
git commit -m "Say what changed and why"
git push
```

## Undoing

| Want to                         | Command                         |
|---------------------------------|---------------------------------|
| Unstage a file                  | `git restore --staged <file>`   |
| Discard changes in a file       | `git restore <file>`            |
| Change the last commit message  | `git commit --amend`            |
| Undo a pushed commit            | `git revert <commit>`           |

## Branches

```bash
git switch -c feature/sitemap
git rebase main
git branch -d feature/sitemap
```

See also [[Rust Cheatsheet]].
