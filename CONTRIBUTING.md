# Contributing to Inkdown

Thank you for helping. Bug reports, fixes, features, documentation and testing on
platforms the author does not have (macOS, Linux, real Android phones) are all welcome.

## Before you start

- **Bugs and small fixes:** open an issue or go straight to a pull request.
- **Features and larger changes:** open an issue first so the approach can be agreed
  before you spend time on it.
- **Security problems:** do not open a public issue. See [SECURITY.md](SECURITY.md).

## Licence of contributions

Inkdown is licensed under the [GNU General Public License, version 3 or later](LICENSE).
By submitting a contribution you confirm that you wrote it (or have the right to submit
it) and you agree that:

1. it is licensed to everyone under GPL-3.0-or-later, like the rest of the project; and
2. you also grant the project owner a perpetual, worldwide, non-exclusive, royalty-free
   licence to use, modify, sublicense and distribute your contribution under other terms.

The second point lets the owner publish Inkdown in app stores whose terms do not fit the
GPL, and offer other licences, without having to ask every contributor again. You keep
the copyright in your work.

## Set up

You need [Rust](https://rustup.rs) (stable). On Linux also install:

```sh
sudo apt install pkg-config libxkbcommon-dev libwayland-dev libxcb1-dev libgl1-mesa-dev libfontconfig1-dev
```

```sh
git clone https://github.com/debmalyaroy/inkdown.git
cd inkdown
make run        # Windows without make: pwsh build.ps1 run
```

The app is in `native/`. The first build takes a while because the dependencies are
compiled with optimisation; later builds take a minute or two.

## Where things are

| Path | What |
| --- | --- |
| `native/src/app.rs` | Application state, the frame loop and the command dispatcher |
| `native/src/editor.rs`, `buffer.rs`, `edit.rs` | The code editor, text buffer and editing commands |
| `native/src/md.rs`, `preview.rs`, `rich.rs` | Markdown layout, the reader and Write mode |
| `native/src/assets.rs`, `chart.rs`, `designer.rs` | Diagrams, math, charts and the diagram designer |
| `native/src/avro.rs`, `../src/data.rs` | Avro reader; JSON / YAML / TOML conversion |
| `native/src/update.rs` | Update check and in-place update |
| `native/src/shell.rs`, `picker.rs`, `sidebar.rs` | Menus, dialogs, palette and sidebar |
| `packaging/` | Per-platform packaging |

## Checks to run

```sh
make test                  # unit tests, every platform
pwsh build.ps1 uitest      # Windows: end-to-end checks of the real window
```

CI runs the unit tests on Windows, macOS and Linux for every pull request.

The end-to-end suite drives the window with scripted input in an isolated profile
(`INKDOWN_AUTOMATION`, `INKDOWN_DATA_DIR`). When you add a feature, add a check to
`native/tools/uitest.ps1` if it can be observed in a state dump. Always set
`INKDOWN_DATA_DIR` when you run the app from a script, so your own settings are not
touched.

## Conventions

- **Line endings are LF** in every file, on every platform (`.gitattributes` enforces it).
- **No hardcoded absolute paths.** Use environment variables, the executable's own
  location, or paths relative to the script.
- **Keep it small and fast.** No web view, no bundled runtime. Think twice before adding
  a dependency, and prefer pure-Rust crates that also build for Android.
- **Platform code** goes behind `cfg` in the existing platform modules (`platform.rs`,
  `mobile.rs`, the installer), not scattered through the interface code.
- **Commands** are named strings handled in `InkApp::run`. A new command needs an entry
  in `picker::COMMANDS` so that it appears in the palette, and a default key binding in
  `keymap.rs` only if it earns one.
- Format with `cargo fmt` (settings in `native/rustfmt.toml` if present) and keep new
  code free of compiler warnings.
- Write comments that say why, not what.

## Pull requests

- One change per pull request, with a description of what it does and how you tested it.
- Say which platforms you ran it on.
- Add a line to `CHANGELOG.md` under the next version for anything a user would notice.
- Screenshots help for anything visual.

## Releases

Maintainers: set the version in `native/Cargo.toml`, add `docs/releases/v<version>.md`,
then push the tag `v<version>`. The Release workflow builds and publishes everything.
