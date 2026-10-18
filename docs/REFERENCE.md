# Reference

## How It Works

```
src/                        the runtime: interface, parsers, logging, no system command of its own
scripts/<name>/script.sh    one area of the system, its operations as functions op_<name>
scripts/<name>/script.yaml  what the runtime knows about them
scripts/lib.sh, lib.yaml    sourced first by every script, and the causes every run may hit
themes/<id>.yaml            built-in themes
locales/de.po               German, the English source is in the code
assets/fonts/               Fira Code and the icons, for the window
aur/                        the AUR package arch-os-manager, stamped at a release
```

One binary carries all of it. The scripts are written to `$XDG_RUNTIME_DIR/system-manager-<pid>/` per process, readable by the user only, and removed on exit.

```mermaid
flowchart LR
    R[Runtime] -->|script.sh op args| S[script.sh]
    S -->|stdout| R
    S -->|"fd 3: step, fail"| R
    S -.->|pseudo terminal| T[Terminal view]

    style R fill:#1793d1,stroke:#1793d1,color:#fff
```

## Scripts

`operation` in `lib.sh` runs `op_<operation>`, a dash in the name an underscore in the function. An operation reads in one of two ways:

| Way | Terminal | Questions | Limit | Used for |
|---|---|---|---|---|
| Query | none | none | 120 s | reading state, printing lines the runtime parses |
| Run | its own pseudo terminal | answered in the interface | none | changing the system |

A query may not call `sudo`, and the runtime refuses to start one whose yaml says `root: true`.

### The yaml

| Key | Meaning |
|---|---|
| `ops` | Each operation by name, with the keys below. build.rs makes it `script::op::<SCRIPT>_<NAME>` |
| `errors` | Exit code from 10 on: headline, then explanation. A test holds them against the `exit` lines |
| `hints` | Output line containing the key: `text` as headline, then explanation, `offer` the page that fixes it. `lib.yaml` holds pacman's |
| `probe` | `features` only: each optional package under its group, the `command` that proves it works, what it `adds`, and the fact it `needs`. One with `extension: true` comes from extensions.gnome.org |
| `facts` | `features` only: what a package may need of the system, with why it is not offered without. `fact_<name>` in the script tells whether it holds |

| Operation key | Meaning |
|---|---|
| `title` | What it does, as a gerund: the title of its run, the start of its failure |
| `confirm` | Asked before the run: the question, then what happens |
| `root` | It calls `sudo`, so the password is asked before the run. A test holds this against the function |
| `needs` | Optional packages it cannot run without, shown as unavailable until they are there |
| `steps` | `step <key>` on fd 3 shows this sentence during the run |

Every sentence is English and translated in `locales/de.po`.

### Exit Codes and Events

| Exit | Meaning |
|---|---|
| `0` | Done |
| `1` | Unexpected: lib.sh's trap wrote `fail\t<code>\t<file>\t<line>\t<command>` to fd 3, and the runtime shows that line |
| `2` | No such operation |
| `10` to `99` | Declared in `errors` |

### Environment

| Variable | Value |
|---|---|
| `LC_ALL` | `C.UTF-8`: output and questions in one known wording |
| `SM_AUR` | The AUR helper of the settings, empty for none |
| `SM_FLATPAK` | The `flatpak` setting, `off` without Flatpak |
| `SM_RUN` | The title of a run, unset in a query |
| `SUDO_PROMPT` | A fixed prompt the runtime recognises |
| `TERM` | `xterm-256color` in a run |

`lib.sh` sets `set -Eeuo pipefail`, `inherit_errexit` and `nullglob`, and ignores a hangup, so a closed terminal stops no run. A run holds shutdown and sleep off through logind while it lasts. It offers:

| Function | Does |
|---|---|
| `operation` | Runs `op_<name>`, as the last line of every script |
| `step` | Names the phase of a run |
| `allow` | Lets a command end with the codes named, as `grep` ends with 1 on nothing |
| `scratch` | A folder of the operation's own as `$work`, removed when it ends |
| `fetch` | curl bound to https, redirects too, with time limits |
| `aur` | The helper of `$SM_AUR` |
| `aur_sync` | The helper without the review and menus a run cannot answer |

A function more than one script needs goes into `lib.sh`.

### A New Operation

1. `op_<name>` in `scripts/<area>/script.sh`. A new area is a folder whose `script.sh` starts with the two lines that source `lib.sh` and ends with `operation "$@"`
2. Its entry under `ops` in `script.yaml`, with at least `title`
3. Its sentences in `locales/de.po`

`make check` fails until all three agree.

## The Start Page

Under the header, one row shows in percent what the machine uses, read every two seconds. `usage: false` switches it off:

| Block | In use | Read from |
|---|---|---|
| CPU | Busy ticks of all, over the last two seconds | `/proc/stat` |
| RAM | What `MemAvailable` leaves, as `free` counts it | `/proc/meminfo` |
| Disk | Of `/`, as `df` counts it | `statvfs` |

A share reads in `success` below 75 %, `warning` below 90 % and `error` from there. Short of room the bars go first, then the names where icons say them. The spinner turns in a value still being read, on every row of the start page and in Settings alike, and `?` stands where a value could not be read. Where the menu would not fit under the row, and during a search, the row does not show.

## Runs

A run is named in the header after the action chosen, and shows its step and pacman's progress. **tab** switches to the terminal itself, where every key goes to the program. A question appears in both: in the overview as a choice, in the terminal as typed.

<img src="screenshots/run.png" width="720" alt="An update under way: its step, the progress bar and the package being upgraded">

| Question | Overview |
|---|---|
| `[Y/n]`, `[y/N]` | Yes or no, the default first |
| `Enter a number (default=N):` | The numbered choices, such as providers |
| `Enter a selection` and helper menus ending in `==>` | A text box, where enter keeps the default |
| The sudo prompt | The password |

pacman's `Proceed with installation?`, `Proceed with download?` and `Do you want to remove these packages?` are answered yes: the person confirmed before the run. Conflicts, replacements and keys are always asked.

A run holds shutdown and sleep off while it lasts, and a closed terminal does not stop it.

An update takes the repositories, then the AUR, then Flatpak where `flatpak` is `all`. A failing AUR helper leaves Flatpak still to update and is then named. The list of updates names a source it could not ask, as the AUR during an outage, and shows what the others have.

**esc**, **q** and **ctrl+c** ask whether to stop, and yes sends ctrl+c to the program. A failure shows the declared error or a recognised cause, the source line, the last output and the log, and offers the page the cause names, another try and the way back.

After a run, the manager reads the system again and offers what may follow, none of it a must:

| Offer | When |
|---|---|
| Restart | A new kernel waits |
| Config updates | `.pacnew` or `.pacsave` files are open |
| Services | A service failed |
| Clean up | Unneeded packages, or 1 GiB to free, and not after a clean-up |

<img src="screenshots/done.png" width="720" alt="The end of an update: what may follow, each with its reason">

**Note:** _A question in another shape still waits in the terminal view, which tab reaches._

### The Password

Asked once per session, before the first run that elevates, and proven with `sudo --validate`. `sudo --non-interactive --validate` tells beforehand whether it is needed at all, so with `NOPASSWD` it is never asked. sudo never gets an empty password: pam_faillock counts an unanswered prompt and an empty password as failed attempts, and locks the account after three. It is held in memory only, answers every later sudo prompt of every run, also those of an AUR helper, and is written only while the terminal does not echo. A wrong one is forgotten and asked again.

### Repair and Configuration

**Renew the keyring** removes a database lock no process holds, builds the keyring anew beside the old one and updates. **Check the files** runs `pacman -Qkk` as root and installs every native package again whose file lost its content, type or link, or is missing. Owner, mode and time do not count, as a filesystem such as the vfat of `/boot` keeps them its own way. A configuration changed by hand is a backup file pacman lists apart, and a package of the AUR is only named. Taking over or merging a configuration update first takes a snapshot where snapper keeps the root. A merge is written beside the configuration and renamed over it, and a sudoers file must pass `visudo` first.

## Files

| Path | Content |
|---|---|
| `~/.arch-os/settings.yaml` | What was chosen, an absent key being the default |
| `~/.arch-os/themes/<id>.yaml` | Own themes |
| `~/.arch-os/state.yaml` | News read, updates already announced |
| `~/.arch-os/logs/system-manager.log` | The runtime and every run, rotated at 2 MiB |
| `~/.arch-os/cache/news.xml` | The feed as last loaded, for offline |
| `~/.local/share/applications/system-manager.desktop` | The app menu entry, opening the window |
| `~/.local/share/icons/hicolor/scalable/apps/system-manager.svg` | Its icon, the Arch OS logo |
| `~/.config/systemd/user/system-manager-notify.{service,timer}` | The update notification, as often as `notify` says |
| `~/.local/share/gnome-shell/extensions/arch-update@RaphaelRochet/` | The update indicator, when installed |

`SM_HOME` puts all of it elsewhere, for a throwaway run.

## Settings

| Key | Values | Default |
|---|---|---|
| `language` | `en`, `de` | from `LC_ALL`, `LC_MESSAGES`, `LANG` |
| `theme` | a theme id | `nord` |
| `usage` | `true`, `false` | `true` |
| `aur-helper` | `paru`, `yay`, `none` | paru, else yay |
| `app-menu` | `true`, `false` | `true` |
| `notify` | `hourly`, `daily`, `weekly`, `off` | `hourly` |
| `flatpak` | `all`, `cleanup`, `off` | `all` |
| `hidden-log` | messages hidden from the system log | none |

A file that does not parse is left alone and named under **Settings › About**, and the defaults apply until a setting is changed. The first start or `init` takes the settings of the `arch-os` script before 2.0 over once, where none are chosen, and removes what it left. `scripts/migrate/script.sh` says how to remove this.

Every start and `system-manager init` put the app menu entry and the update notification in place as `app-menu` and `notify` say, for the binary where it lies now, and point the update indicator at it where installed. An installer calls `init` from its chroot as the user, through a private bus of `dbus-run-session` where none answers. The entry needs a Wayland session, as the window does, the notification a desktop and libnotify. The timer announces two minutes after login and then as often as chosen.

`flatpak` leaves Flatpak to another app store, such as GNOME Software: `cleanup` only removes unused runtimes, `off` leaves Flatpak alone. The check, the notification and the update indicator follow it. The setting appears only where Flatpak is installed.

A hidden message is stored with its numbers, hex values and hashes as `#`, so it stays hidden when only those change.

## Themes

```yaml
name: Nord
colors:
  background: "#2E3440"
  surface: "#3B4252"
  muted: "#616E88"
  text: "#D8DEE9"
  accent: "#88C0D0"
  success: "#A3BE8C"
  warning: "#EBCB8B"
  error: "#BF616A"
```

| Slot | Used for |
|---|---|
| `background` | Everything behind |
| `surface` | The track of a bar and of a scrollbar, the row under the cursor |
| `muted` | Hints, secondary values, disabled rows |
| `text` | Labels and text |
| `accent` | Cursor, title, progress, pending updates |
| `success`, `warning`, `error` | States |

The built-in themes are the files in [`themes/`](../themes), listed by name. Each reads on its background at WCAG AA: 4.5 for `text`, 3 for `accent` and the states, and a test holds them to it. A file in `~/.arch-os/themes/` with the id of a built-in replaces it. One that does not parse is skipped and named under **Settings › About**.

The icons are Material Design symbols of the Nerd Fonts. The window carries them, a terminal shows them with a Nerd Font such as `ttf-nerd-fonts-symbols`. The Linux console, told by `TERM=linux`, gets sixteen colours and ASCII in place of every glyph: no icons, `#` for the logo, `>` for the cursor, `*` for a dot, `+` for a mark, the keys named in words in the footer.

## Keys

| Key | Outside a text box | In a text box |
|---|---|---|
| **↑↓**, **j k** | Choose | ↑↓ choose |
| **⇞⇟** | A page further | the same |
| Wheel | As ↑↓ | the same |
| **⏎**, **l**, **→** | Open | ⏎ open |
| **esc** | Back | Close the box |
| **backspace**, **h**, **←** | Back | Delete |
| **f**, **/** | Filter, on the start page a search over everything | typed |
| **g**, **G** | First, last | typed |
| **q** | Quit | typed |
| **ctrl+c** | Quit, in a run ask to stop | the same |
| **tab** | In a run overview or terminal, on the packages page mark | the same |
| **space** | On the news, read or unread | typed |
| **r** | On the start page, read every value anew | typed |

Actions stay under what they are about, so ↑↓ always reaches them. What does not fit above them scrolls with **⇞⇟** and the wheel, while a page's head stays. Held down, a key repeats only to move or to type. A terminal tells a held key only by its pace: the same key again within 80 ms. Pasted text is typed into a text box and does nothing elsewhere.

The footer names what each key does, in the same words on every page. **esc** reads main menu where the start page lies below, back further down. The start screen stands three seconds while the first values load, and any other key skips it.

## Command Line

| Command | Does |
|---|---|
| `system-manager` | The interface in the terminal, or in a window without one |
| `system-manager --window` | In a window |
| `system-manager <page>` | Opens on `upgrade`, `news`, `packages`, `services`, `log`, `cleanup`, `pacnew`, `mirrors`, `firmware`, `repair`, `settings`, `features` |
| `system-manager init` | Puts the app menu entry, the update notification and the update indicator in place |
| `system-manager check` | Pending updates as checkupdates prints them: an AUR package as `aur/name`, a Flatpak with its commits where the versions do not tell the change, an unknown version as `?`, exit 2 without any, and 1 where a source could not be asked and none had updates |
| `system-manager notify` | Announces pending updates, with `--new` only those not announced before, as the timer runs it |

It runs as the user and asks for the sudo password itself. As root it only answers `check`.

## Optional Packages

| Group | Package | Adds |
|---|---|---|
| `packages` | `paru` or `yay` | Builds and updates packages from the AUR |
| `packages` | `pacman-contrib` | Lists updates, trims the cache and finds configuration updates |
| `packages` | `fakeroot` | Lets pacman-contrib list updates without root |
| `packages` | `reflector` | Ranks the mirrors by speed |
| `packages` | `flatpak` | Updates and cleans up Flatpak apps |
| `packages` | `fwupd` | Updates the firmware of the device |
| `desktop` | `gnome-arch-update` | A GNOME Shell extension that shows pending updates in the top bar |
| `desktop` | `libnotify` | Desktop notifications about new updates |
| `desktop` | `meld` | Merges configuration updates side by side |

A package counts when its command answers, as `probe` in `scripts/features/script.yaml` names it. One that `needs` a fact is offered only where it holds:

| Fact | Holds when | Needed by |
|---|---|---|
| `desktop` | A session lies in `/usr/share/wayland-sessions` or `/usr/share/xsessions` | flatpak, meld, libnotify, the notification |
| `wayland` | A session lies in `/usr/share/wayland-sessions` | the app menu entry, which opens the window |
| `gnome` | `gnome-shell` is installed | gnome-arch-update |

**Settings › Optional packages** installs and removes each, removal being `pacman -Rns`:

| Package | Beyond pacman |
|---|---|
| `paru`, `yay` | Built from source with makepkg, as Arch OS does, after `git` and `base-devel`, and the build dependencies go again. Removed is whichever package owns the command, such as `paru-bin` |
| `reflector` | `/etc/xdg/reflector/reflector.conf`: https, synced within 12 hours, the 10 latest, by speed, the country of the time zone where Arch has a mirror there. `reflector.timer` enabled, and disabled on removal. **Refresh now** starts `reflector.service` with the same configuration |
| `fwupd` | `fwupd-refresh.timer` enabled, so the firmware list stays current, and disabled on removal |
| `gnome-arch-update` | The Arch Linux Updates Indicator from extensions.gnome.org in the version for this GNOME Shell, enabled for the user and loaded at the next login. Its `check-cmd`, `update-cmd` and `package-manager` run this binary. Removed with its settings, one from a package with that package |

## Clean-Up and the Recovery

The cache keeps three versions of every installed package and none of what is gone, as `paccache -r` and `paccache -ruk0` choose. On top, every kernel package a snapshot in `/.snapshots` has installed stays: the Recovery of Arch OS rebuilds a rolled back snapshot's boot files from it. A cache it cannot judge, because `/.snapshots` is unreadable, is not trimmed. Unneeded packages go with `pacman -Rns`, AUR build folders of packages no longer installed are removed, and Flatpak drops unused runtimes unless `flatpak` is `off`.

## Building

| Target | Does |
|---|---|
| `make check` | Format, clippy, shellcheck, yamllint, actionlint, zizmor, msgfmt, tests, release build, before every commit |
| `make run` | The interface against a throwaway `target/home` |
| `make install` | `~/.local/bin/system-manager` |
| `make aur-check` | `aur/PKGBUILD` stamped with stand-ins and read by makepkg |
| `docs/screenshots.sh [x.y.z]` | `docs/screenshots/` from example data through the window's painter, showing that version |
| `docs/banner.sh` | `docs/banner.png` from two screenshots, with chromium, imagemagick and Fira Code |
| `tools/icons.sh` | The icon font, cut from the Nerd Fonts symbols to what `src/ui/icon.rs` names |

Tests run on Arch, as the CI does: the scripts against stand-ins for pacman, sudo and friends, the read-only ones against the real pacman. The kernel test of the clean-up needs root, as the CI container has.

build.rs names a pre-release of the patch after the last release in `.release-please-manifest.json`, such as `1.9.2-dev`, unless `SM_VERSION` names the release, as the PKGBUILD does.

How a change becomes a release and reaches the AUR: **[➜ Contributing](CONTRIBUTING.md#workflow)**.
