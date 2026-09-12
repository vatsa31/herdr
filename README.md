# herdr

> [!NOTE]
> This is a fork of [herdrdev/herdr](https://github.com/herdrdev/herdr). The
> upstream project is a terminal workspace manager for coding agents. This fork
> adds a generic plugin-resource API and a native resource section in the
> sidebar, developed to support the companion
> [Jira sidebar plugin](https://github.com/vatsa31/herdr-jira).

## what this fork adds

Upstream Herdr plugins can provide commands, event hooks, and terminal panes,
but they cannot place live data in Herdr's native sidebar. This fork adds that
missing extension point while keeping the host implementation independent of
Jira or any other service.

```text
Before                            With this fork
────────────────────────          ────────────────────────
Workspaces                        Workspaces
Agents                            Agents
                                  My Jira issues · 8
                                    PROJ-142  Fix timeout
                                              In Progress
```

The implementation includes:

- additive `[[resources]]` plugin manifest entries;
- `plugin.resource.list`, `plugin.resource.refresh`, and
  `plugin.resource.activate` APIs;
- server-owned polling with bounded provider execution, a 60-second default
  refresh, stale-data retention, and protection against obsolete responses;
- a collapsible, independently scrollable sidebar section with mouse activation
  and manual refresh;
- structured activation that lets a plugin open the selected resource in an
  existing or new pane; and
- regression coverage for resource selection, resizing, collapse/reopen, and
  sidebar mouse-hit boundaries.

The work is documented in [PR #1](https://github.com/vatsa31/herdr/pull/1)
and the follow-up interaction fixes in
[PR #2](https://github.com/vatsa31/herdr/pull/2). It has been exercised with
mock data on Linux and with real Jira data on Apple Silicon macOS. The Jira API,
authentication, filtering, and issue rendering remain in the plugin rather than
Herdr core.

### build this fork

The installer, Homebrew formula, release badges, and update channel below refer
to upstream Herdr and do not contain this fork's sidebar changes. Build this
fork from source to use them:

```bash
git clone https://github.com/vatsa31/herdr.git
cd herdr
cargo build --release --locked
./target/release/herdr
```

Building currently requires Rust, platform build tools, and Zig 0.16.0. Pair
this binary with the companion
[`vatsa31/herdr-jira`](https://github.com/vatsa31/herdr-jira) fork. Pin both
revisions for a reproducible installation; running the upstream updater will
replace a custom binary.

## upstream project

<p align="center">
  <img src="assets/logo.png" alt="herdr" width="100" />
</p>

<p align="center">
  <a href="https://herdr.dev">herdr.dev</a> · <a href="#install">install</a> · <a href="https://herdr.dev/docs/quick-start/">quick start</a> · <a href="https://herdr.dev/docs/">docs</a>
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/herdrdev/herdr/releases"><img src="https://img.shields.io/github/downloads/herdrdev/herdr/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/herdrdev/herdr/stargazers"><img src="https://img.shields.io/github/stars/herdrdev/herdr?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/herdrdev/herdr/releases/latest"><img src="https://img.shields.io/github/v/release/herdrdev/herdr?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/herdr"><img src="https://img.shields.io/homebrew/v/herdr?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/herdrdev"><img src="https://img.shields.io/badge/follow-%40herdrdev-000000?logo=x&logoColor=white" alt="follow @herdrdev on X" /></a>
</p>

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**the runtime your coding agents live on.**

- **detach without stopping work** — herdr keeps terminals running in a background server when you close the client or lose your SSH connection. after a server or machine restart, herdr restores the saved layout and can resume supported agent sessions; the original processes do not survive. [session state →](https://herdr.dev/docs/session-state/)
- **several machines, one window** — keep local work and saved ssh machines together, with a combined agent list and independent reconnects. [remote machines →](https://herdr.dev/docs/connecting-machines/)
- **never hunt for the stuck one** — every pane is marked working, blocked, or idle. when an agent stops and needs an answer, herdr says so.
- **agent-native** — agents drive herdr through the cli and socket api: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked. [agent skill →](https://herdr.dev/docs/agent-skill/)
- **runs what you already run** — claude code, codex, cursor, opencode, grok and the rest. herdr doesn't wrap or replace them; it owns their terminals.
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://herdr.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## install

> [!IMPORTANT]
> The commands in this section install upstream Herdr. Use
> [build this fork](#build-this-fork) for the plugin-resource sidebar.

```bash
curl -fsSL https://herdr.dev/install.sh | sh
```

or `brew install herdr` · `mise use -g herdr` · windows: `powershell -ExecutionPolicy Bypass -c "irm https://herdr.dev/install.ps1 | iex"` · [endpoint-protected Windows](https://herdr.dev/docs/windows-beta/) · [binaries](https://github.com/herdrdev/herdr/releases)

then start it where the work lives:

```bash
herdr
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `herdr` reattaches. [quick start →](https://herdr.dev/docs/quick-start/)

## docs

everything lives at [herdr.dev/docs](https://herdr.dev/docs/): [quick start](https://herdr.dev/docs/quick-start/) · [concepts](https://herdr.dev/docs/concepts/) · [supported agents](https://herdr.dev/docs/agents/) · [keyboard](https://herdr.dev/docs/keyboard/) · [configuration](https://herdr.dev/docs/configuration/) · [session state](https://herdr.dev/docs/session-state/) · [connecting machines](https://herdr.dev/docs/connecting-machines/) · [remote](https://herdr.dev/docs/persistence-remote/) · [integrations](https://herdr.dev/docs/integrations/) · [plugins](https://herdr.dev/docs/plugins/) · [socket api](https://herdr.dev/docs/socket-api/)

## thanks

every past sponsor and backer is listed in [SPONSORS.md](./SPONSORS.md) — thank you 🐑

enterprise / partnership: hey@herdr.dev

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## development

```bash
git clone https://github.com/herdrdev/herdr
cd herdr
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
```

## license

Herdr is licensed under the [Apache License 2.0](LICENSE).
