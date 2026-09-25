# Desktop automatic updates

The Windows desktop app quietly checks the stable GitHub Releases feed after
startup. An available update is shown to the user; a successful check with no
new version and a failed background check stay out of the way. An update is not
installed without confirmation. Tauri verifies the updater signature before
launching the installer. Reopen the app when installation finishes.

The feed URL is
`https://github.com/aneskurtovic/ContextTrace/releases/latest/download/latest.json`.
The repository and stable release assets must be public so installed apps can
fetch them without credentials. `latest.json` must point to the installer
asset for that exact version rather than to a moving `latest` asset URL.

## First updater-enabled version

Versions 0.1.0 and 0.1.1 predate the updater. Install 0.1.2 manually; it is the
first updater-enabled stable release. Subsequent compatible versions can be
offered in-app.

## Release key handling

The Woodpecker tag workflow requires the protected
`TAURI_SIGNING_PRIVATE_KEY` secret and is restricted to trusted tag builds.
Woodpecker injects it into the packaging step; the packaging script removes it
from the child-process environment before building the production application,
then restores it for NSIS bundling and clears it afterwards. This narrows its
exposure but is not isolation from the trusted runner itself. The key must
remain stable because installed applications trust the corresponding embedded
public key. Do not publish, rotate or replace the key casually. The current
key is unencrypted, so the workflow supplies an empty
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` value.

The optional GitHub Actions fallback requires its own copy of the updater key
as a repository Actions secret. It is not automatically shared with
Woodpecker. See the [release procedure](RELEASING.md) for asset checks and
clean-host acceptance.
