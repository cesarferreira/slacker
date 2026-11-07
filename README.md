<div align="center">
  <h1>slacker</h1>

  <p><strong>Chat on Slack with native terminal media</strong></p>

  <p>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
    <img alt="Rust" src="https://img.shields.io/badge/rust-1.85%2B-orange">
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

Requires [Rust](https://rustup.rs) **1.85+** and `~/.cargo/bin` on your `PATH`.

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
Videos and animated GIFs need `ffmpeg` on your `PATH` (`brew install ffmpeg`
on macOS). PNG, JPEG, and WebP previews use the built-in decoder. Media opens
in a full-screen terminal viewer. `Space` pauses video; `Esc` closes it.
`--graphics off` keeps chat usable in other terminals; `--graphics kitty`
forces the protocol when automatic terminal detection misses it. Automatic
graphics detection is disabled inside tmux; passthrough is not implemented.

### Connect to Slack

Create a Slack app, add **User Token Scopes**, install it to your workspace,
and set its user OAuth token in `SLACK_TOKEN`. Start a new token with these
scopes (your workspace may require administrator approval):

```text
channels:read   groups:read   im:read   mpim:read   users:read
channels:history   groups:history   im:history   mpim:history
chat:write   files:read
```

See Slack's [OAuth guide](https://docs.slack.dev/authentication/installing-with-oauth/)
and [conversation history reference](https://docs.slack.dev/reference/methods/conversations.history/).
A user token is recommended because channel-thread access differs for bot
tokens. Tokens are read from the environment, never saved by slacker.

```bash
# Set SLACK_TOKEN in your environment, then:
slacker
slacker --channel engineering
slacker --refresh 60
```

Only joined channels and existing DMs are listed. Messages load in pages of
15; `PgUp` requests another page. Threads load from the oldest replies forward,
and refresh adds replies after the last loaded timestamp while preserving loaded
messages. Successful sends appear immediately. `R` refreshes manually. Automatic refresh is
opt-in with `--refresh`; intervals below 60 seconds are clamped to 60. Slack
rate-limit responses show the server's retry delay. Sending is never retried
automatically, and failed sends retain the draft. Drafts are kept separately
per conversation and thread for the current session.

### Keyboard and mouse

| Action | Shortcut / mouse |
| --- | --- |
| Select conversation or message | `j` / `k`, arrows, click; wheel moves through the list under the pointer |
| Focus conversations / messages | `h` / `l`, `Tab` |
| Open conversation | `Enter` in the sidebar, or click its name |
| Find a conversation | `Ctrl+K`, type a fuzzy query, `Enter`; click a result |
| Compose | `i`, or click the composer |
| Send | `Enter` in compose mode, or click Send |
| Newline | `Alt+Enter`; bracketed paste preserves newlines |
| Open thread | `t`, `Enter` on a message, or click Thread |
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
updates, notifications, multi-workspace switching, reactions, uploads, and
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
