# diaryx.share

Live sharing plugin for Diaryx.

This plugin owns the live-share UI and session orchestration. When `diaryx.sync`
is installed and available, it delegates transport/runtime reuse through the
generic `host_plugin_command` bridge. Otherwise it falls back to its own
temporary in-plugin CRDT runtime for the session.

Share is now self-contained with respect to server/auth/workspace resolution:
it reads `server_url`, `auth_token`, and current workspace/provider-link state
from `host_get_runtime_context`, and only falls back to persisted config for
older hosts that do not provide those runtime fields.

When a share session needs a remote workspace, the plugin prefers the current
workspace's generic `provider_links` entry for `diaryx.sync`, then legacy sync
metadata, and only creates a new remote workspace if neither exists.
