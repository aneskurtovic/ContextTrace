# Linux desktop builds

Linux x86_64 packaging is in development. The build baseline is Debian 12;
Ubuntu 24.04 is a target for separate acceptance, not a verified compatibility
claim. Debian 12 builds require glibc 2.36 or newer, including the AppImage;
older distributions require a compatible older build base and separate validation. Published Linux assets and clean-host acceptance are not yet available.
Windows remains the supported release surface. See [CI](CI.md) and the
[release procedure](RELEASING.md) for the distinction between development
artifacts, Woodpecker release verification and clean-host testing.

## Build a development handoff

On an x86_64 Linux host install Node.js 22.13 or newer, Rust through rustup,
Git, and the following Debian 12 packages (use your distribution's equivalent
package names elsewhere):

```sh
sudo apt-get update
sudo apt-get install build-essential curl wget file pkg-config libwebkit2gtk-4.1-dev \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  libfuse2 xdg-utils xvfb xauth dbus-x11 webkit2gtk-driver
cargo install tauri-driver --version 2.1.0 --locked
bash scripts/ci/check-linux.sh
```

The script runs formatting, workspace tests and Clippy, the Linux updater feature
release-profile Clippy check, frontend tests, and builds the CLI, deb and
AppImage. It inspects package architecture and desktop entries, then uses WebKitWebDriver under Xvfb and D-Bus to check the rendered app shell
and a real native startup IPC call in an isolated home. Output is placed
in a new `target/linux-handoffs/x64.*` directory with SHA-256 hashes and build
information. Fixture manifest checks require PowerShell and run in the existing
Windows CI lane; run those separately as described in `CLAUDE.md` when available.
Development handoffs may come from a dirty checkout and are not release evidence.

## Run or install

For a development AppImage, verify its SHA-256 against the handoff manifest,
make it executable, and start it from a graphical Linux session:

```sh
sha256sum -c SHA256SUMS.txt
chmod +x ContextTrace-<version>-linux-x64.AppImage
./ContextTrace-<version>-linux-x64.AppImage
```

AppImages normally need FUSE 2 (on Debian 12 the `libfuse2` package). The
AppImage runtime also supports `--appimage-extract-and-run` where mounting is
unavailable. A running desktop display, WebKitGTK dependencies, and a D-Bus
session are needed for the graphical app and desktop integration. Notifications
also need a desktop notification service. AppImage desktop integration depends
on the user's environment or integration tool; verify registration and click
behavior on the actual desktop.

To install a trusted deb on Debian or Ubuntu and let apt resolve dependencies:

```sh
sudo apt install ./ContextTrace-<version>-linux-x64.deb
context-trace
sudo apt remove context-trace
```

Confirm the package name first with `dpkg-deb -f <file>.deb Package` if using an
artifact from another build. Removing the package should preserve user data.
Archives default to `$XDG_DATA_HOME/contexttrace/archive` when XDG_DATA_HOME is
absolute, or `~/.local/share/contexttrace/archive`; the
`CONTEXTTRACE_ARCHIVE` override takes precedence. Archives may contain the only
remaining copies of sessions, so back them up and delete them separately only
when intended. This behavior still needs clean-host uninstall testing.

Linux native resume looks for `gnome-terminal`, `konsole`, or `xterm`, in that
order. Install one and the relevant Codex or Claude CLI to use it; otherwise
copy the shown command into a terminal. Runtime discovery and notifications
also depend on the session and desktop environment.

The deb and development AppImage builds have no built-in updater. A future
opt-in release AppImage is built with the Linux updater feature and signed by
the Windows Woodpecker release pipeline; updater availability and an actual
in-app upgrade must be verified on a clean host. Install a newer deb through apt
rather than relying on the in-app updater. No Linux release is claimed until
the release and acceptance gates pass.

## Separate acceptance

On clean Debian 12 and Ubuntu 24.04 desktop hosts, record distribution,
desktop/session type, architecture, installed prerequisites and artifact hashes.
Check deb install, app launch, log discovery for both agents, upgrade, removal
and archive preservation. Separately run the AppImage, test its updater on a
signed release, protocol registration, notification delivery and click
navigation, local file opening, and resume with each available terminal and
both agent CLIs. A headless CI smoke does not prove these behaviors; record
failures and untested cases independently from Woodpecker results.
