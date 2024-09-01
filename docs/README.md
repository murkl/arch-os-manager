<div align="center">
  <h1>
    <img src="./logo.svg" width="150" height="150">
    <p><b>Arch OS Manager</b></p>
  </h1>
</div>

<div align="center">
  <p><img src="./screenshots/arch-os-manager.png"></p>
  <p>
    <img src="https://img.shields.io/badge/MAINTAINED-YES-green?style=for-the-badge">
    <img src="https://img.shields.io/badge/License-GPL_v2-blue?style=for-the-badge">
  </p>
  <p><sub>100% shellcheck approved</sub></p>
  <p><sub>powered by <a href="https://github.com/murkl/arch-os">Arch OS</a></sub></p>
</div>

## Install from AUR

**[ ➜ AUR/arch-os-manager](https://aur.archlinux.org/packages/arch-os-manager)**

- Run Arch OS Manager:

```
arch-os
```

- **Note:** Add parameter `-k` to start with built-in kitty

## Install from GitHub

- Install Dependencies:

```
sudo pacman -S kitty gum libnotify pacman-contrib
```

- Clone GitHub Repo:

```
git clone https://github.com/murkl/arch-os-manager.git && cd arch-os-manager
```

- Run Arch OS Manager:

```
./arch-os
```

- **Note:** Add parameter `-k` to start with built-in kitty

## Features

- Check and list updates in _checkupdates_ format
- Startup notify on available updates
- Show latest Arch Linux news
- Notice on orphaned packages & pacdiff files
- Search and manage packages
- Show system info, packages & logs
- Upgrade system packages
- Remove orphaned packages
- Reset Pacman keyring
- Built-in log blacklist filter
- Built-in settings editor
- Built-in kitty support

Install these **optional** dependencies to equip Arch OS Manager with more features:

- `paru`: Add support for manage AUR packages
- `flatpak`: Add support for manage Flatpak packages
- `reflector`: Add support for refresh Pacman mirrorlist
- `downgrade`: Add support for downgrade packages
- `meld`: Add support for merge pacdiff configurations

## Usage

```
arch-os [--kitty | -k]              Open main menu
arch-os [--kitty | -k] check        Print package updates like checkupdates
arch-os [--kitty | -k] notify       Notify on new package updates
arch-os [--kitty | -k] settings     Edit settings
arch-os [--kitty | -k] version      Print Arch OS Manager version
arch-os [--kitty | -k] help         Open help page
arch-os [--kitty | -k] info         Show system info (system / package info & logs)
arch-os [--kitty | -k] search       Search & manage package (pacman, aur)
arch-os [--kitty | -k] upgrade      System upgrade (news, pacdiff, pacman, aur, flatpak)
arch-os [--kitty | -k] orphans      Remove orphaned packages (pacman, aur, flatpak)
arch-os [--kitty | -k] merge        Merge updated configurations (using pacdiff and meld)
arch-os [--kitty | -k] refresh      Refresh pacman mirrorlist (using preconfigured reflector)
arch-os [--kitty | -k] downgrade    Downgrade packages (pacman only)
arch-os [--kitty | -k] reset        Reset pacman (refresh pacman keyring)
```

## Settings

Edit the settings with the built-in editor in Arch OS Manager or edit the config file manually.

```
AUTOSTART_NOTIFY=true             # Enable update notify on system boot (disable: false)
AUTOSTART_DELAY=30                # Delay update check after boot in seconds (min: 10)
AUR_SUPPORT=true                  # Enable AUR support (disable: false)
AUR_REVIEW=false                  # Disable AUR review (enable: true)
ARCH_UPGRADE_CONFIRM=true         # Enable confirm upgrade system packages (disable: false)
ARCH_DOWNLOAD_TIMEOUT=false       # Disable pacman/paru download timeout (enable: true)
ARCH_SHOW_SYSTEM_CHECK=true       # Show Arch OS system check (disable: false)
ARCH_SHOW_SYSTEM_INFO=true        # Show system packages info (disable: false)
FLATPAK_SUPPORT=true              # Enable flatpak support (disable: false)
FLATPAK_UPGRADE_CONFIRM=false     # Disable confirm flatpak upgrade (enable: true)
NEWS_QUANTITY=3                   # Number of news to shown (disable: 0)
NEWS_SHOW_URL=true                # Show 'More Information' below the news (disable: false)
ORPHANS_CONFIRM=false             # Disable confirm remove orphans (enable: true)
FILTER_RESULT_SIZE=20             # Max filter results (max: 50)
PACKAGE_INFO_WIDTH=80             # Max info width (cut horizontal output)
```

### Configurations

- Arch OS Manager config: `~/.arch-os/config/settings.conf`
- Built-in kitty config: `~/.arch-os/config/kitty.conf`
- System logs blacklist: `~/.arch-os/config/blacklist.conf`

## Integrate GNOME Extension

- Install GNOME Extension: **[archlinux-updates-indicator](https://extensions.gnome.org/extension/1010/)**
- Set `Check command`:

```
dconf write /org/gnome/shell/extensions/arch-update/check-cmd "'/usr/bin/arch-os check'"
```

- Set `Update command`:

```
dconf write /org/gnome/shell/extensions/arch-update/update-cmd "'arch-os --kitty upgrade'"
```

- Set `Package Manager` (optional):

```
dconf write /org/gnome/shell/extensions/arch-update/package-manager "'arch-os --kitty'"
```

<div align="center">

## Screenshots

<sub><b>This screenshots may outdated.</b></sub>

<img width="48%" style="vertical-align: top;" src="./screenshots/main.png">
<img width="48%" style="vertical-align: top;" src="./screenshots/search.png">
<img width="48%" style="vertical-align: top;" src="./screenshots/fetch.png">
<img width="48%" style="vertical-align: top;" src="./screenshots/upgrade.png">
<img width="48%" style="vertical-align: top;" src="./screenshots/merge.png">
<img width="48%" style="vertical-align: top;" src="./screenshots/refresh.png">
<img width="48%" style="vertical-align: top;" src="./screenshots/downgrade.png">
<img width="48%" style="vertical-align: top;" src="./screenshots/help.png">

</div>

## Credits

- https://archlinux.org
- https://github.com/RaphaelRochet/arch-update
- https://github.com/Morganamilo/paru
- https://github.com/kovidgoyal/kitty
- https://github.com/charmbracelet/gum
- https://github.com/archlinux-downgrade/downgrade
- https://meldmerge.org
