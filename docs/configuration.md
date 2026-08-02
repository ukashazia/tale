# Configuration contract

## Goals

Configuration should be small, inspectable, and safe to share after profile
names and tailnet IDs are redacted. It controls Tale, not Tailscale itself.
Tailscale local preferences and tailnet settings remain live resources managed
through their respective adapters.

There are no automatic configuration migrations, deprecated aliases, or legacy
fallbacks. A release that changes the format fails with a precise validation
error and documents the new shape.

## File locations

The default configuration path is:

- Unix, including macOS: `$XDG_CONFIG_HOME/tale/config.toml`, falling back to
  `$HOME/.config/tale/config.toml` when `XDG_CONFIG_HOME` is unset;
- Windows: `%APPDATA%\tale\config.toml`.

State and logs use:

- Unix: `$XDG_STATE_HOME/tale`, falling back to `$HOME/.local/state/tale`;
- Windows: `%LOCALAPPDATA%\tale`.

Cache uses `$XDG_CACHE_HOME/tale` or the platform cache directory. No file is
created on first read-only launch unless the user explicitly saves a setting or
adds a profile.

`tale config path` prints the resolved paths. `tale config check` validates the
file without starting the TUI.

## Precedence

Highest precedence wins:

1. command-line flags;
2. the narrow environment variables documented below;
3. `config.toml`;
4. built-in defaults.

Environment variables do not map generically onto TOML keys.

| Variable | Purpose |
| --- | --- |
| `TALE_CONFIG_FILE` | use an explicit config path |
| `TALE_PROFILE` | select an admin profile for this run |
| `TALE_ACCESS_TOKEN` | ephemeral access token for the selected profile; never persisted |
| `TALE_TAILSCALE_PATH` | override the local executable for this run |
| `NO_COLOR` | force color mode `none` |
| `VISUAL`, `EDITOR` | external policy editor, in that order |

## Command-line interface

```text
tale [--profile NAME] [--read-only] [--no-local] [--view ROUTE]
     [--config PATH] [--tailscale-path PATH]

tale auth add PROFILE
tale auth remove PROFILE
tale auth status [PROFILE]
tale config path
tale config check
tale doctor
```

- `--read-only` disables all mutations regardless of profile configuration.
- `--no-local` skips local-client detection and is useful on an admin workstation
  without Tailscale installed.
- `--view` accepts only canonical routes and documented aliases.
- `auth remove` removes Tale's stored credential reference; it does not revoke
  the credential at Tailscale.
- `doctor` performs non-mutating local, keyring, API, terminal, and config checks
  and redacts its output.

## TOML schema

The initial complete schema is:

```toml
default_profile = "ops"
read_only = false

[local]
enabled = true
tailscale_path = "tailscale"
refresh_interval = "2s"
command_timeout = "10s"

[admin]
refresh_interval = "30s"
request_timeout = "15s"

[ui]
color = "auto"
symbols = "auto"
mouse = false
detail_pane = "auto"
time_zone = "local"
relative_times = true
show_footer = true

[history]
persist_tasks = false
max_tasks = 200

[profiles.ops]
tailnet = "-"
read_only = false
credential = "ops"

[profiles.audit]
tailnet = "example.com"
read_only = true
credential = "audit"
```

All fields are optional except `tailnet` and `credential` inside a declared
profile. Unknown fields are errors so misspellings cannot silently weaken a
setting.

### Root fields

| Field | Type | Default | Rules |
| --- | --- | --- | --- |
| `default_profile` | string | none | must name an existing profile |
| `read_only` | bool | `false` | global write lock; profiles cannot override `true` |

### Local fields

| Field | Type | Default | Rules |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | when false, do not detect or invoke the CLI |
| `tailscale_path` | path/string | `tailscale` | executable name or absolute path; never a command string |
| `refresh_interval` | duration | `2s` | 500ms–5m |
| `command_timeout` | duration | `10s` | 1s–10m; streaming/interactive commands have explicit policies |

### Admin fields

| Field | Type | Default | Rules |
| --- | --- | --- | --- |
| `refresh_interval` | duration | `30s` | 5s–30m; not used for log streams |
| `request_timeout` | duration | `15s` | 1s–2m |

Tale does not initially expose a custom API base URL. Supporting Headscale or a
mock server is a separate product decision, not an undocumented URL override.
Tests inject their transport without using user configuration.

### UI fields

| Field | Values | Default |
| --- | --- | --- |
| `color` | `auto`, `none`, `ansi16`, `ansi256`, `truecolor` | `auto` |
| `symbols` | `auto`, `ascii`, `unicode` | `auto` |
| `mouse` | bool | `false` |
| `detail_pane` | `auto`, `always`, `never` | `auto` |
| `time_zone` | `local`, `utc` | `local` |
| `relative_times` | bool | `true` |
| `show_footer` | bool | `true` |

`detail_pane = "always"` still yields to the minimum-size safety layout. `auto`
symbol detection may select only characters whose widths are known; ASCII is
used when detection is uncertain.

### History fields

| Field | Type | Default | Rules |
| --- | --- | --- | --- |
| `persist_tasks` | bool | `false` | persistence excludes output and all secret-bearing tasks |
| `max_tasks` | integer | `200` | 20–5000, applied to memory and persisted metadata |

Persisted task metadata contains timestamp, action ID, redacted target label,
duration, and result class. It excludes stdout, stderr, HTTP bodies, policy
documents, tokens, key material, and webhook secrets.

### Profile fields

| Field | Type | Default | Rules |
| --- | --- | --- | --- |
| `tailnet` | string | required | a Tailnet ID or `-`; not a display label |
| `read_only` | bool | `true` | profile write lock |
| `credential` | string | required | keyring record name, not secret material |

Profile names use ASCII letters, digits, `_`, and `-`, are case-sensitive, and
must be unique. The configuration never contains OAuth client secrets, API
tokens, auth keys, or credential-store payloads.

## Credential records

`tale auth add PROFILE` prompts for one of:

- `oauth_client`: client ID, client secret, and requested scopes;
- `access_token`: a pre-generated API access token.

The credential record stores its kind and secret fields in the OS keyring under
service `tale` and the configured credential name. OAuth scopes are not secret
and are also returned by `auth status` so the user can audit the requested
access. Tale refuses a literal token in TOML.

Recommended profiles:

- `audit`: read-only with explicit read scopes;
- `ops`: only the write scopes required for daily device/route/DNS/user work;
- a separate short-lived profile for credential or policy administration.

Tale does not request `all` or add scopes automatically. If an action needs a
missing scope, it names the scope and remains disabled.

## Writes and file permissions

- Create configuration and state directories with user-only permissions where
  the platform supports them.
- Write TOML through a same-directory temporary file, flush it, then atomically
  replace the target.
- Never rewrite the config merely because defaults were applied.
- `auth add` validates credentials before committing config/keyring changes.
- `auth remove` asks whether to remove only the referenced keyring entry or also
  the profile block; neither action revokes a remote credential.
- Tale never edits shell startup files.

## Logs and privacy

Tracing is off at normal verbosity except for a bounded in-memory task stream.
When file logging is enabled by a future explicit setting, it must use structured
redaction at the field boundary.

Never log:

- Authorization headers or OAuth exchanges;
- environment-variable values;
- credentials returned once;
- full policy documents or API response bodies that may contain secrets;
- child-process stdin;
- private certificate keys;
- unredacted generated commands containing authentication material.

Tale has no telemetry by default and no configuration field to enable telemetry
until a concrete, separately reviewed design exists.

## Keybinding and theme configuration

The first release intentionally has fixed contextual keybindings and a semantic
color system. The action registry is designed to support remapping later, but
shipping arbitrary bindings or theme schemas before actions and visual states
stabilize would create a compatibility burden without improving the first
working product.

