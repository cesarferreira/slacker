<div align="center">
  <h1>slacker</h1>

  <p><strong>Chat on Slack with native terminal media</strong></p>

  <p>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
    <img alt="Rust" src="https://img.shields.io/badge/rust-1.89%2B-orange">
    <img alt="Edition" src="https://img.shields.io/badge/edition-2024-blue">
    <a href="https://crates.io/crates/slacker"><img alt="crates.io" src="https://img.shields.io/crates/v/slacker.svg"></a>
  </p>

  <p>
    <a href="#install">Install</a>
    &nbsp;·&nbsp;
    <a href="#quickstart">Quickstart</a>
    &nbsp;·&nbsp;
    <a href="#development">Development</a>
  </p>
</div>

---

## Install

Requires [Rust](https://rustup.rs) **1.89+** and `~/.cargo/bin` on your `PATH`.

```bash
cargo install --path . --locked
```

Verify:

```bash
slacker --help
```

<details>
<summary><strong>Build from source</strong> — for development or unreleased changes</summary>

```bash
git clone https://github.com/cesarferreira/slacker.git
cd slacker
cargo install --path . --locked
# or
make install-release
```

Debug install (faster compile, larger binary):

```bash
make install
```

Run without installing:

```bash
make build-release
./target/release/slacker
```

</details>

<a id="quickstart"></a>
## Quickstart

```bash
cargo run -- --demo
cargo run -- doctor
cargo run -- media ~/Pictures/photo.jpg
cargo run -- media ~/Movies/clip.mp4
```

Run in Kitty, Ghostty, or WezTerm for images and silent video playback.
Kitty graphics are enabled by default; `--graphics off` disables them, and
`--graphics auto` opts into terminal-name detection.
Videos and animated GIFs need `ffmpeg` on your `PATH` (`brew install ffmpeg`
on macOS). PNG, JPEG, and WebP previews use the built-in decoder. Media opens
in a full-screen terminal viewer. `Space` pauses video; `Esc` closes it.
`--graphics off` keeps chat usable in other terminals. Automatic
graphics detection is disabled inside tmux; passthrough is not implemented.

The offline demo has distinct conversations in general, engineering, design,
random, and direct messages, with photos, a four-second video, emoji reactions,
and contextual thread replies. Images appear directly in the chat.
Click an image or video to open it, a reaction to toggle it locally, or a replies
link to read and reply in the thread. Demo messages and replies stay available
while switching conversations during the session. No Slack token or network is
needed for the demo.

### Connect to Slack

Sign in to the Slack desktop app, then run:

```bash
slacker
```

On first launch, choose one of your signed-in workspaces. There is no Slack app
to register and no token to copy. On macOS, allow the **Slack Safe Storage**
Keychain prompt if it appears. The selected workspace is remembered; credentials
are read fresh from the desktop session on each launch. Slacker remembers the
workspace choice and caches loaded history, but never saves a duplicate token or
cookie file.

```bash
slacker auth                    # choose or reconnect a workspace
slacker --workspace example      # choose by name, domain, or workspace ID
slacker --channel engineering
slacker --bell                   # optional sound for background messages
slacker --refresh 0              # disable automatic refresh
```

This follows [slk's desktop-session setup](https://github.com/gammons/slk/wiki/Setup).
Desktop sign-in supports macOS Keychain and Linux Secret Service (`secret-tool`),
including native, Flatpak, and Snap profile locations. Windows desktop cookie
decryption and KDE KWallet are not implemented yet. If Slack signs you out,
sign back in to the desktop app and run `slacker auth` again.

Only joined channels and existing DMs are listed. Messages load in pages of 15;
`PgUp` requests another page. Threads load from oldest replies forward, and
refresh adds replies while preserving loaded messages. Failed sends keep the
draft. Drafts are separate for each conversation and thread during the session.
`R` refreshes manually. Automatic refresh runs every 60 seconds; nonzero intervals
below 60 seconds are clamped to 60. Slack rate limits show the server's retry
delay, and sending is never retried automatically.

### Local history cache

Previously loaded conversations, user names, messages, and thread replies are
cached in SQLite. On reopening Slacker, the last conversation appears from the
cache while authentication and fresh history load in the background. Switching
back to a visited conversation or thread also restores its messages and reading
position without clearing the feed. New responses update edited messages and add
new ones; cached older pages remain available. History stays readable if Slack
is slow or offline. `R` retries a failed connection or refreshes the active feed.
After a refresh, `PgUp` may revisit cached pages while following Slack's current
pagination cursor; this avoids skipping messages that arrived while offline.

The cache lives at `~/Library/Caches/slacker/history.sqlite3` on macOS, or
`$XDG_CACHE_HOME/slacker/history.sqlite3` (normally `~/.cache/slacker/history.sqlite3`)
on Linux. It contains **unencrypted message content and profile metadata**, with
owner-only file permissions on Unix. It does not store authentication tokens or
cookies. Each credential and API endpoint has a separate opaque namespace, so
different accounts/workspaces do not share history; rotating credentials starts
a fresh cache. Disk reads and writes run on a separate thread, and cache failures
do not prevent using Slack online.

Only fetched history is cached, not the entire workspace. Retention is limited to
the latest 500 loaded messages per feed and 100 recently used channel/thread
feeds per credential. Unused entries expire after 30 days. Close Slacker and
delete `history.sqlite3` to clear the cache. Demo mode never writes to it.

<details>
<summary>Advanced: use an existing OAuth token instead</summary>

`SLACK_TOKEN` remains an optional override for servers or setups without desktop
session access. Use a Slack user OAuth token with the relevant scopes:

```text
channels:read   groups:read   im:read   mpim:read   users:read
channels:history   groups:history   im:history   mpim:history
chat:write   files:read
```

See Slack's [OAuth guide](https://docs.slack.dev/authentication/installing-with-oauth/).
The override is read from the environment and never saved.

</details>

### Unread title and sound

The terminal title shows the open conversation and workspace. New messages from
other people received while the terminal is unfocused add a count, such as
`(3) slacker · #engineering · My workspace`. Returning to the terminal clears it.
`--bell` also emits a terminal bell once per incoming batch; whether it plays a
sound or flashes follows your terminal settings. The previous title is restored
on exit, and focus reporting is disabled again.

These are session counts for the open channel, observed during refresh. Initial
history, older pages, your own messages, and thread-history pages stay quiet.
They are not Slack's workspace-wide unread totals. Automatic refresh must be
enabled for background alerts; terminals without focus reporting can still
clear the count when you press a key or click.

### Panes and people

Conversations show profile photos beside aligned message blocks, local-time
timestamps, and date separators. Times and dates follow your computer's timezone,
including daylight saving. Profile photos load in the background and are reused
throughout the session. Kitty-compatible terminals show native images;
`--graphics off` uses small pixelated photos. Initials appear while a photo is
loading or unavailable.

Click an underlined link or preview title to open it in your default browser (or
mail app for `mailto:` links). Every wrapped segment keeps the full destination,
so URLs split across lines are still clickable in conversations, threads, and
the full-message reader. Link clicks preserve the active composer and draft.
Slack-provided link previews appear as cards with titles, descriptions, and
metadata such as GitHub website/star fields. Preview metadata is cached with the
message. No extra requests are made to GitHub or linked websites; links without
a Slack-provided preview stay clickable without a card.

Drag the divider beside the sidebar to resize it. Clicking a replies link opens
an independent thread panel on the right; drag its divider to adjust its width.
Channel and thread messages sit above the composer, with empty space above short
conversations. New messages follow automatically only when you're already at the
latest message; scrolling into older history or opening the full-message reader
keeps your reading position.
Channel and thread drafts stay separate, and sending keeps the composer focused.
Scrolling another pane preserves your typing destination.

The pinned People section shows existing direct messages by colleague name, with
online (●), away (○), do not disturb (⊘), or unknown (·) presence and status emoji.
Presence refreshes for visible people at most once per minute. Blue sidebar dots
show Slack unread state when desktop-session unread counts are available.

### Keyboard and mouse

| Action | Shortcut / mouse |
| --- | --- |
| Select conversation or message | `j` / `k`, arrows, click; wheel moves through the list under the pointer |
| Cycle sidebar / channel / thread panes | `h` / `l`, `Tab` |
| Open conversation | `Enter` in the sidebar, or click its name |
| Find a conversation | `Ctrl+K`, type a fuzzy query, `Enter`; click a result |
| Compose | Click the message field, or press `i` |
| Send | `Enter` while typing, or click Send (shown for a nonempty draft) |
| Newline | `Alt+Enter`; bracketed paste preserves newlines |
| Open right thread panel | `t`, `Enter` on a message, or click its replies link |
| Resize panes | Drag the sidebar or thread divider |
| Read a long message | `o` or click Read; `j` / `k`, page keys, or wheel scroll |
| View media | `v` for the first image/video, or click an attachment |
| Pause / resume video | `Space`, or click Pause / play |
| First / last item | `g` / `G`, `Home` / `End` |
| More history / move five messages | `PgUp` / `PgDown` |
| Refresh / reconnect | `R`, or click Refresh |
| Close popup / media / thread, leave compose mode | `Esc`; drafts are preserved |
| Help | `?`, `F1`, or click Help |
| Quit | `q` in navigation mode; `Ctrl+C` everywhere |

Hold `Shift` while selecting text to use your terminal's mouse selection.
While composing, normal letter keys insert text rather than triggering actions.

### Current scope

This is an initial single-workspace client inspired by
[gammons/slk](https://github.com/gammons/slk), implemented independently in Rust
with Ratatui. It uses Slack's Web API and manual/interval refresh. WebSocket live
updates, workspace-wide notifications, multi-workspace switching, changing reactions in live Slack, uploads, and
persistent history are not implemented yet. Image/video attachments use the
[Kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/).
Previews download to temporary files that are removed when the viewer closes;
the download limit is 50 MiB. Videos play at 12 fps, without audio or seeking.

<a id="development"></a>
## Development

Common tasks via the `Makefile`:

```bash
make              # check + build + test
make build        # debug build
make build-release
make install      # install debug binary
make install-release
make run ARGS="--demo"
make check        # cargo check + clippy
make fmt          # format
make lint         # fmt check + clippy
make test
make clean
make demo         # install + show --help
```

Releasing (requires [cargo-release](https://github.com/crate-ci/cargo-release) and [git-cliff](https://github.com/orhun/git-cliff)):

```bash
make release                  # default minor bump
make release LEVEL=patch      # patch bump
make release LEVEL=major      # major bump
```

The pre-release hook regenerates `CHANGELOG.md` with `git-cliff` from your conventional-commit history (grouped into Features, Bug Fixes, etc. per `cliff.toml`) and commits it alongside the version bump. Pushing the resulting `v*` tag triggers the release workflow, which builds the multi-platform binaries and publishes a GitHub Release whose notes are generated by `git-cliff` from the same config.

## License

MIT
