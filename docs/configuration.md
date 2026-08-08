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

Credentials live beside it in `credentials.toml`, in the same directory and never
in `config.toml` itself, so the configuration stays shareable. `--config` moves
both together. The credential file is created mode `0600` inside a `0700`
directory and is refused rather than read if it is later widened.

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
| `TALE_TAILSCALE_PATH` | override the local executable for this run |
| `TALE_TAILSCALE_SOCKET` | override the local daemon socket or named pipe for this run |
| `NO_COLOR` | force color mode `none` |
| `VISUAL`, `EDITOR` | external policy editor, in that order |

## Command-line interface

```text
tale [--profile NAME] [--read-only] [--no-local] [--view ROUTE]
     [--config PATH] [--tailscale-path PATH] [--tailscale-socket PATH] [--mock]

tale auth add PROFILE [--tailnet ID] [--kind oauth-client|access-token]
                      [--secret-stdin] [--client-id ID] [--scopes SCOPES]
tale auth remove PROFILE
tale auth status [PROFILE]
tale config path
tale config check
tale doctor [--config PATH] [--mock] [--output PATH]
```

- `--read-only` disables all mutations regardless of profile configuration.
- `--no-local` skips local-client detection and is useful on an admin workstation
  without Tailscale installed.
- `--mock` selects deterministic fictional providers and prevents local process,
  HTTP, and credential-store access. It is incompatible with `--profile`, visibly
  labels the session `mock`, and is never persisted.
- `--view` accepts only canonical routes and documented aliases.
- `auth add` prompts for whatever it is not given. `--secret-stdin` reads the
  secret from standard input instead, which is the only form that works without a
  controlling terminal; in that form every other value must arrive as a flag,
  because no prompt can be answered.
- `auth remove` removes the stored credential; it does not revoke the credential
  at Tailscale. Removing the last profile leaves the file without
  `default_profile`, which no longer selects anything.
- `doctor` performs non-mutating local, credential-store, API, terminal, and config checks
  and redacts its output. `--output` writes the allowlisted Tale 1.0 support
  bundle to an explicit path and never uploads it.

## TOML schema

The initial complete schema is:

```toml
default_profile = "ops"
read_only = false

[local]
enabled = true
tailscale_path = "tailscale"
socket_path = "/var/run/tailscale/tailscaled.sock"
reconcile_interval = "30s"
command_timeout = "10s"

[admin]
refresh_interval = "30s"
request_timeout = "15s"

[ui]
theme = "tailscale-dark"
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
credential_backend = "file"
credential_file = "/home/user/.config/tale/credentials.toml"

[profiles.audit]
tailnet = "example.com"
read_only = true
credential = "audit"
credential_backend = "file"
credential_file = "/home/user/.config/tale/credentials.toml"
```

All fields are optional except `tailnet`, `credential`, `credential_backend`, and
the location that backend requires, inside a declared profile. Unknown fields are errors so misspellings cannot silently weaken a
setting.

`ui.theme` accepts exactly `tailscale-dark` (default), `tailscale-light`, or
`terminal`. There are no aliases or fallback names. `terminal` preserves the
terminal's default foreground/background for neutral surfaces while retaining
semantic accents permitted by `ui.color`. Settings can preview and apply a
theme for the current process; Tale does not edit this key automatically.

`ui.color` accepts `auto`, `truecolor`, `ansi256`, `ansi16`, or `none`.
`NO_COLOR` forces `none` with environment provenance regardless of the theme.
Theme selection never upgrades the resolved capability.

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
| `socket_path` | path/string | platform default | one Unix socket or Windows named pipe; no probing |
| `reconcile_interval` | duration | `30s` | 5s–10m |
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
| `theme` | `tailscale-dark`, `tailscale-light`, `terminal` | `tailscale-dark` |
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

View history and `:` command history are always process-local in this release.
Both are independently bounded to 100 entries and have no configuration key.
Filters persist only when the user explicitly saves a view. Key sequences are
fixed registry data; there is no remapping, macro, plugin, or shell-command
configuration.

### Profile fields

| Field | Type | Default | Rules |
| --- | --- | --- | --- |
| `tailnet` | string | required | a Tailnet ID or `-`; not a display label |
| `read_only` | bool | `true` | profile write lock |
| `credential` | string | required | credential-store record name, not secret material |
| `credential_backend` | string | required | which store holds the secret; `file` is the only supported value |
| `credential_file` | path/string | required for `file` | the file holding this profile's secret |

Profile names use ASCII letters, digits, `_`, and `-`, are case-sensitive, and
must be unique. The configuration never contains OAuth client secrets, API
tokens, auth keys, or credential-store payloads.

## Credential records

`tale auth add PROFILE` records one of:

- `oauth_client`: client ID, client secret, and requested scopes;
- `access_token`: a pre-generated API access token.

Each value is prompted for unless it is supplied as a flag. `--secret-stdin`
takes the secret from standard input — the access token, or the client secret
for `oauth_client` — so the command works in a script, a container, or CI. This
is the only writer to the credential store, and it is the only way back once
`auth remove` has emptied a configuration.

```sh
printf '%s' "$TOKEN" |
  tale auth add ops --tailnet TAILNET_ID --kind access-token --secret-stdin
```

The record is written to `credentials.toml` under its configured credential name.
OAuth scopes are not secret and are also returned by `auth status` so the user
can audit the requested access. Tale refuses a literal token in `config.toml`.

### Which credential is used

A credential is read from exactly one place: the record named by the selected
profile's `credential` field, in the store named by that profile's
`credential_backend`. There is no environment variable and no fallback.

Each profile states its own backend and location, so a configuration says where
its secrets live rather than leaving it implied. Two profiles may use different
files. `credential` names a record within that store and never holds secret
material; it defaults to the profile name, so it only becomes interesting once a
single credential backs several profiles.

`file` is the only backend today. Storage sits behind an interface, so another
can be added without changing the record type or these commands, and existing
profiles keep working because they already say which backend they use.

`auth add` records the backend when it creates a profile and leaves it alone
afterwards, so rotating a secret never relocates it.

A configuration that selects no profile is not an error. Tale starts, local
views work, and the admin views stay inactive until a profile is selected.

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
- `auth add` validates a credential against the API before writing it, so a
  rejected credential leaves both files untouched.
- Credentials are written through a same-directory temporary file created `0600`,
  so the secret is never briefly visible at a wider mode.
- `auth remove` asks whether to remove only the stored credential or also the
  profile block; neither action revokes a remote credential.
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
