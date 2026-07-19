# Shaktool 2.0

Discord bot for SM/SMZ3 communities, does random things and more...
Run at your own risk

## Environment

- `DISCORD_TOKEN` — bot token (required)
- `COMMAND_PREFIX` — prefix for text commands (default: `%`)
- `DATABASE_PATH` — path to the SQLite database file (default: `shaktool.db`, created automatically)
- `SPEEDRUN_API_KEY` — API key of a speedrun.com account that moderates the monitored games
  (needed for the review buttons and `auto` mode)
- `QUAD_API_KEY` — optional `quad.samus.link` personal API key (`qr_...`); enables the Quad
  commands to list and roll the key owner's private seed profiles. Official profiles work
  without a key. Keep this server-side and never paste it into a Discord command.

## Background tasks

Background tasks run on a fixed interval and can post to Discord. They are defined in
`src/tasks/` — implement the `Task` trait and register the task in `tasks()` in
`src/tasks/mod.rs`. Tasks get a `TaskContext` with the serenity context (for Discord
access) and the database (for settings and persistent state).

Settings are stored in the database and managed with the admin-only `config` command.
Every setting is either **per-server** (each Discord server has its own value) or
**global** (one value for the whole bot) — the bot routes automatically and says which
kind it changed. Each setting also has a declared value type (id, integer range,
boolean, list, overrides); `%config set` validates input against it up front and, on a
bad value, replies with the reason and a worked example. `%config list` (no scope) shows
all known settings with their scope, type, description, and an example value; unknown
settings are rejected with the same list.

```
%config set <scope> <key> <value>
%config get <scope> <key>
%config unset <scope> <key>
%config list [scope]
```

The Quad randomizer command rolls on `https://quad.samus.link` by default. Extra selectable
sites, such as beta deployments, can be enabled globally:

```
%config set quad sites beta=https://beta-quad.example.com,local=http://localhost:5173
```

Use `/quad-options` or `%quad_options` to show metadata-backed option keys and copyable
examples for the freeform `%quad ... options` argument. Useful examples:

```
/quad-options section:sm
/quad-options section:alttp search:crystal
/quad-options section:alttp page:2
```

Saved profiles can be discovered with `/quad-profiles` (or `%quad_profiles`) and rolled by
copying the displayed internal profile ID into `/quad profile:<id>` (or `%quad profile:<id>`).
Add `revision:<id>` to roll an older revision; otherwise the current revision is used. A
profile roll can override
`seed` and `spoiler`, but cannot be mixed with game toggles or custom setting pairs because
the saved profile supplies the complete configuration. The optional `QUAD_API_KEY` is sent
as a Bearer token and makes the configured key owner's private profiles available.

Seed generation posts a "Generating seed" embed immediately, shows included games and changed
options, and edits that message when the randomizer API returns. When metadata is available,
the bot sends default options too so the seed page has a fuller option summary.

### Speedrun.com queue moderation

Watches the speedrun.com verification queue (every 2 minutes) for the configured games:

- every queue submission is posted to each server's **mod log channel** with
  **Approve**/**Reject** buttons (Reject asks for a reason, which the runner sees on
  speedrun.com). Buttons are usable by administrators and the optional `mod_role` role.
- the queue is **tracked**: runs approved, rejected, or removed on the website itself get
  their mod log messages updated accordingly; approved runs are not tracked further.
- every approval — by button, on the website, or automatic — is announced in each server's
  public **announce channel** with the run's info.

Each submission carries a neutral assessment (score 0-100) built from gathered evidence:

- video links: unrecognized hosts (possible malicious links), deleted/unavailable videos,
  and (for YouTube) whether the actual video title looks related to the run — the resolved
  title and channel are shown in the mod log either way
- player history: established runners (3+ verified runs in the game) effectively always
  pass; first-time or low-history submitters add suspicion, guests more so
- leaderboard context: a would-be top-3 time from a runner with little history

The scoring lives behind a `Judge` trait over a serializable `Evidence` value
(`src/tasks/speedrun/judge.rs`), so the rule-based judge can later be replaced or chained
with an LLM-backed one (see `src/api/llm.rs`) without touching the monitor task.

Configuration — channels and games are per-server, moderation policy is global (a
speedrun.com game has one queue, so actions can only happen once):

```
# Per-server:
%config set speedrun mod_channel <channel id>       (moderation log + buttons)
%config set speedrun announce_channel <channel id>  (public approved-run announcements)
%config set speedrun games supermetroid,smz3
%config set speedrun mod_role <role id>             (optional)

# Global:
%config set speedrun modes supermetroid:auto,supermetroid/100%:manual   (optional)
%config set speedrun threshold 50                                       (optional; default 50)
%config set speedrun thresholds supermetroid/Any%:40                    (optional overrides)
%config set speedrun dry_run true                                       (optional)
```

Game names are speedrun.com abbreviations (the part after `speedrun.com/` in the game URL).
Modes and thresholds can be set per game or per `game/category` (most specific wins):

- `manual` (default) — the bot takes no action on its own; every queue run waits in the
  mod log for a human decision. The assessment is shown as information, not a verdict.
- `auto` — runs scoring below the threshold are auto-approved (and announced); runs at or
  above it are flagged and left in the queue for manual review.

With `dry_run` enabled the bot posts and tracks everything but never touches speedrun.com,
shows what `auto` mode would have done, and works without `SPEEDRUN_API_KEY`. The buttons
on dry-run messages are simulated: Approve/Reject update the mod log and post the
announcement (marked as demo) without any speedrun.com action.

Three admin commands help with testing:

- `%speedrun demo <game> [count]` — posts the latest submissions (any status) to this
  server's mod log through the full pipeline, with working simulated buttons, to demo the
  end-to-end flow and the look of the messages. Never contacts speedrun.com for actions.
- `%speedrun showcase` — posts entirely fabricated submissions covering the interesting
  cases (clean veteran, missing/deleted/suspicious videos, would-be record from a nobody,
  sub-second guest troll, first game/category runs). The fake evidence is scored by the
  real judge; approving the first-run scenarios shows the celebration blurbs in the
  announcement.
- `%speedrun debug <game> [count] [mode]` — dry-runs recent submissions through the
  scoring pipeline and replies with a text report (scores, signals, would-be actions).
