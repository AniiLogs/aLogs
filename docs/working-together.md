# Working together

How the two developers (and the AI agents they use) share the AniiLogs repositories without stepping on each
other's work, and without spending many tokens on syncing.

## The rules

1. **Claim before you start.** Every piece of work is a GitHub issue; assign yourself before you begin (open one if
   there isn't one). One person per area at a time. If you need to touch the other person's area, ask first.
2. **Start from the latest.** `git pull` before you start and again before you push. Set this once per clone, so a
   pull replays your commits on top instead of making merge commits:

   ```
   git config pull.rebase true
   git config rebase.autoStash true
   ```

3. **Everything lands by pull request, on every repository.** Branch from the latest `main` as `<github-name>/<topic>`
   (`sargeeva/sanctum-minimap`), push it, open a PR that says `Closes #<issue>`, and turn on **auto-merge**. It merges
   itself once the tests pass and the branch is deleted. Nobody has to review it unless a test fails.
4. **Small, short branches.** Merge within a day or two. The longer a branch lives, the more `main` moves under it.
   If `main` moved while you worked, `git pull --rebase origin main` on your branch before pushing again.
5. **Never rewrite shared history.** No force-push to `main` (it's blocked), and don't rebase someone else's branch.
6. **The tests run on GitHub** for every PR (`.github/workflows/tests.yml`). A red check blocks the merge; fixing it is
   the PR author's job.

## Per repository

All three have `main` protected the same way: pull requests only, no approval needed, force-pushes blocked. The
maintainer (organization owner) can bypass it, and does only for release bookkeeping (`app/betas.json` after
publishing a beta) and emergencies. Everything else, the maintainer's work included, goes through a PR.

| Repository | Check a PR must pass | What a merge does |
| --- | --- | --- |
| `aLogs` | `meter` (meter tests) | Nothing reaches players until the maintainer builds and publishes a beta. |
| `aniilogs.github.io` | `site` (syntax, tests, repository boundary) | Deploys the public site right away. |
| `SanctumHelper` | none yet | Its author owns it. |

`.github/CODEOWNERS` says who is asked to look at a PR touching their area.

`AniiLogs/SanctumHelper` is a fork, so `gh pr create` aims at the original repository unless told otherwise: use
`gh pr create --repo AniiLogs/SanctumHelper --base main`.

## Betas first, releases by the maintainer

Merged work goes into the next **beta** of the current version (built and published by the maintainer), never straight
into a release. Testers and both of you run the beta; only the maintainer turns a tested beta into a release. So a
merged change that turns out broken is fixed by another PR before the release, not after.

## Who owns what

- **Maintainer:** releases and betas (`app/betas.json`, `share.mjs`, `release.mjs`), the updater signing key, clearing
  log reports, site roles.
- **Sanctum Helper's author:** `SanctumHelper/` and the Sanctum Helper plug-in.
- **Either of you:** bug fixes from log reports ([log-reports.md](log-reports.md)) and site design, once claimed.

Which parts of aLogs belong to which plug-in: [plugins.md](plugins.md).

## Files that conflict often

- `app/CHANGELOG.md`: add your own line to the right section. On a conflict, keep both sides' lines.
- `app/ui/patch-notes.js` is generated. On a conflict, take either side, then run `npm run notes` in `app/`.
- `app/betas.json`: the maintainer's only. Don't edit it.

## For AI agents: keep it cheap

- Start each task with `gh issue list --assignee @me` (or the issue the developer names), not by reading the repo.
- Catch up with `git log --oneline -20` and `git diff --stat <old>..HEAD`. Open a full diff only for the files you
  are about to touch.
- Search with grep and read the lines you need, not whole files. Most files here are long.
- Don't review the other developer's commits or PRs unless asked. The tests on GitHub do that.
- Open the PR with `gh pr create --fill` and `gh pr merge --auto --squash`, then stop: don't watch the checks.
- Resolve a conflict by reading only the conflicted parts (`git diff --name-only --diff-filter=U`). Ask the developer
  when both sides changed the same logic; don't guess.
- Run the tests for the part you changed, not the whole suite (`bossbar/tests/two_boss.py` is slow). GitHub runs
  the rest, except four that need files kept out of git (the game's fonts, `test_shots`, the built OCR helper):
  `red_level`, `one_char_names`, `ocr_language` and `sim_victory_open_fight` run only on a PC with the game and a
  build. Run those yourself when you touch OCR or the result screens.
- Answer briefly: what changed, the commit, and whether the tests pass.
