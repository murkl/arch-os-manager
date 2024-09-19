<div align="center">
  <h1>
    <img src="./logo.svg" width="150" height="150">
    <p><b>Arch OS Manager</b></p>
  </h1>
</div>

<div align="center">
  <p><img src="./screenshots/main.png"></p>

**[ ➜ More Screenshots ](#screenshots)**

  <p>
    <img src="https://img.shields.io/badge/MAINTAINED-YES-green?style=for-the-badge">
    <img src="https://img.shields.io/badge/License-GPL_v2-blue?style=for-the-badge">
  </p>
  <p><sub>100% shellcheck approved</sub></p>
  <p><sub>powered by <a href="https://github.com/murkl/arch-os">Arch OS</a></sub></p>
</div>

## Install from AUR

1. Install Package **[ ➜ AUR/arch-os-manager](https://aur.archlinux.org/packages/arch-os-manager)**

2. Run Arch OS Manager:

```
arch-os
```

**Note:** Add parameter `-k` to start with built-in kitty

## Install from GitHub

1. Clone GitHub Repo:

```
git clone https://github.com/murkl/arch-os-manager.git && cd arch-os-manager
```

2. Install Dependencies:

```
./arch-os --install
```

3. Run Arch OS Manager:

```
./arch-os
```

**Note:** Add parameter `-k` to start with built-in kitty

## Features

- Check and list updates in _checkupdates_ format
- System health info
- Startup notify on available updates
- Notify & show latest Arch Linux news
- Notice on orphaned packages & pacdiff files
- Search and manage packages
- Show system info, packages & logs
- Upgrade system packages
- Remove orphaned packages
- Clear Pacman cache
- Reset Pacman keyring
- Built-in log blacklist filter
- Built-in settings editor
- Built-in kitty support

Install these **optional** dependencies to equip Arch OS Manager with additional features:

- `paru`: Add support for manage AUR packages
- `flatpak`: Add support for manage Flatpak packages
- `reflector`: Add support for refresh Pacman mirrorlist
- `downgrade`: Add support for downgrade packages
- `meld`: Add support for merge pacdiff configurations
- `xdg-utils`: Add support for open news in browser with `xdg-open`

## Usage

```
// App
arch-os [--kitty | -k]              Open main menu
arch-os [--kitty | -k] check        Print package updates like checkupdates
arch-os [--kitty | -k] notify       Notify on new package updates
arch-os [--kitty | -k] settings     Edit settings in built-in ediitor
arch-os [--kitty | -k] version      Print version info
arch-os [--kitty | -k] help         Open help page

// Actions
arch-os [--kitty | -k] search       Search & manage package (pacman, aur)
arch-os [--kitty | -k] info         Show system info (logs, services, health)
arch-os [--kitty | -k] upgrade      System upgrade (news, health, pacman, aur, flatpak)
arch-os [--kitty | -k] orphans      Remove orphaned packages (pacman, aur, flatpak)
arch-os [--kitty | -k] merge        Merge updated configurations (using pacdiff and meld)
arch-os [--kitty | -k] refresh      Refresh pacman mirrorlist (using preconfigured reflector)
arch-os [--kitty | -k] downgrade    Downgrade packages (pacman only)
arch-os [--kitty | -k] cache        Clear package manager cache (pacman, aur)
arch-os [--kitty | -k] reset        Reset pacman (reset pacman keyring)
```

### Install Dependencies _(optional)_

Use parameter `--install` to install and preconfigure **all required packages** from the Arch OS Manager.

#### Pacman Packages

```
git base-devel pacman-contrib reflector flatpak gum kitty libnotify fzf xdg-utils meld
```

#### AUR Packages

These packages are built and installed manually from the AUR-Git repository using `makepkg`:

- paru (see `AUR_MANAGER_REPO` property for more details)
- downgrade

Paru is preconfigured in `/etc/paru.conf`. Old configurations are copied to `/etc/paru.conf.20240919170446`, for example (before editing).

## Settings

Edit the settings with the built-in editor in Arch OS Manager or edit the config file manually.

```
ARCH_UPGRADE_CONFIRM=true         # Enable confirm upgrade system packages (disable: false)
ARCH_DOWNLOAD_TIMEOUT=false       # Disable pacman/paru download timeout (enable: true)
AUR_MANAGER_REPO=paru             # Set AUR manager package for --install (paru, paru-bin, paru-git)
AUR_SUPPORT=true                  # Enable AUR support (disable: false)
AUR_REVIEW=false                  # Disable AUR review (enable: true)
FLATPAK_SUPPORT=true              # Enable flatpak support (disable: false)
FLATPAK_UPGRADE_CONFIRM=false     # Disable confirm flatpak upgrade (enable: true)
ORPHANS_CONFIRM=false             # Disable confirm remove orphans (enable: true)
AUTOSTART_NOTIFY=true             # Enable update notify on system boot (disable: false)
AUTOSTART_DELAY=30                # Delay update check after boot in seconds (min: 10)
NEWS_QUANTITY=3                   # Number of news to shown (disable: 0)
SHOW_UNKNOWN_PKG_LIST=false       # Show unknown package counter instead of name list (enable: true)
```

### Configurations

- Arch OS Manager config: `~/.arch-os/config/settings.conf`
- Built-in kitty config: `~/.arch-os/config/kitty.conf`
- System logs blacklist: `~/.arch-os/config/blacklist.conf`

## Integrate GNOME Extension

1. Install GNOME Extension **[➜ archlinux-updates-indicator](https://extensions.gnome.org/extension/1010/)**

2. Set `Check command`:

```
dconf write /org/gnome/shell/extensions/arch-update/check-cmd "'/usr/bin/arch-os check'"
```

3. Set `Update command`:

```
dconf write /org/gnome/shell/extensions/arch-update/update-cmd "'arch-os --kitty upgrade'"
```

4. Set `Package Manager` (optional):

```
dconf write /org/gnome/shell/extensions/arch-update/package-manager "'arch-os --kitty'"
```

<div align="center">

## Screenshots

<sub><b>This screenshots may outdated.</b></sub>

<p><img src="./screenshots/upgrade.png"></p>
<p><img src="./screenshots/info.png"></p>
<p><img src="./screenshots/search.png"></p>
<p><img src="./screenshots/refresh.png"></p>

</div>

## Credits

- https://archlinux.org
- https://github.com/RaphaelRochet/arch-update
- https://github.com/Morganamilo/paru
- https://github.com/kovidgoyal/kitty
- https://github.com/charmbracelet/gum
- https://github.com/archlinux-downgrade/downgrade
- https://meldmerge.org
