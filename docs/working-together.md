# Working together

How the two developers (and the AI agents they use) share the AniiLogs repositories without stepping on each
other's work, and without spending many tokens on syncing.

## The rules

1. **Claim before you start.** Before working on something, say which files or area you're taking (in our chat, or
   by assigning yourself the GitHub issue). One person per area at a time. If you need to touch the other person's
   area, ask first.
2. **Start from the latest.** `git pull` before you start and again before you push. Set this once per clone, so a
   pull replays your commits on top instead of making merge commits:

   ```
   git config pull.rebase true
   git config rebase.autoStash true
   ```

3. **Small commits, pushed often.** A day's work in one push makes conflicts likely; a few small pushes don't.
4. **Never rewrite shared history.** No `git push --force` to `main`, and no rebasing commits that are already pushed.
5. **The tests run on GitHub.** Every push to `main` and every pull request runs the repository's tests
   (`.github/workflows/tests.yml`). If your push turns them red, fixing it is your next job.

## Per repository

| Repository | How changes land |
| --- | --- |
| `aLogs` | Straight to `main` after `git pull`. Nothing reaches players until the maintainer builds and publishes a beta. |
| `SanctumHelper` | Straight to `main`. Its author owns it. |
| `aniilogs.github.io` | **Pull requests only** (`main` is protected), because a push to `main` deploys the live site and API. Branch as `<your-name>/<topic>`, open a PR, and merge once the tests pass. The maintainer can push directly. |

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

- Catch up with `git log --oneline -20` and `git diff --stat <old>..HEAD`. Open a full diff only for the files you
  are about to touch.
- Search with grep and read the lines you need, not whole files. Most files here are long.
- Don't review the other developer's commits unless asked. The tests on GitHub do that.
- Resolve a conflict by reading only the conflicted parts (`git diff --name-only --diff-filter=U`). Ask the developer
  when both sides changed the same logic; don't guess.
- Run the tests for the part you changed, not the whole suite (`bossbar/tests/two_boss.py` is slow). GitHub runs
  the rest, except four that need files kept out of git (the game's fonts, `test_shots`, the built OCR helper):
  `red_level`, `one_char_names`, `ocr_language` and `sim_victory_open_fight` run only on a PC with the game and a
  build. Run those yourself when you touch OCR or the result screens.
- Answer briefly: what changed, the commit, and whether the tests pass.
