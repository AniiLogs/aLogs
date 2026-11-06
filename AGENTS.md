# Notes for AI agents working on aLogs

- Layout, building and testing: see README.md.
- Plug-ins (DPS meter, Sanctum Helper, leaderboards) and what each owns, and what aLogs and Sanctum Helper agree on:
  see docs/plugins.md.
- **Two developers share these repositories:** follow docs/working-together.md: claim a GitHub issue first, work on
  a `<github-name>/<topic>` branch, land it by PR with auto-merge (`main` is protected). Start every session with
  `gh pr list --author @me` and fix your own red or conflicted PRs first. Follow its "keep it cheap" rules
  for agents.
- **Fixing a problem a player reported:** follow docs/log-reports.md. In short: download the report with
  `node tools/log-reports.mjs get <id>`, fix the cause in a branch and open a PR, then **always** leave a note on the
  report with `node tools/log-reports.mjs note <id> "Found: … Changed: … PR: … Verified: …"`. Don't clear reports;
  the maintainer does.
- **Players' logs are private.** Never commit them (`/log-reports/` is git-ignored) or copy their contents, UIDs or
  in-game names into code, commits, issues or PRs. Quote numbers and timings only.
- Never log or send a player's Discord sign-in or other private data from the app or the meter.
