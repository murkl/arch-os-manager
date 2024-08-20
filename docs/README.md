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

**[➜ AUR/arch-os-manager](https://aur.archlinux.org/packages/arch-os-manager)**

Run Arch OS Manager: `arch-os`

## Install from GitHub

1. Install Dependencies: `paru pacman-contrib kitty gum libnotify ttf-firacode-nerd`
2. Clone GitHub Repo: `git clone https://github.com/murkl/arch-os-manager.git && cd arch-os-manager`
3. Run Arch OS Manager: **`./arch-os`**

## Usage

```
arch-os [--kitty | -k]              # Open main menu
arch-os [--kitty | -k] help         # Open Help page
arch-os [--kitty | -k] settings     # Edit Properties
arch-os [--kitty | -k] check        # Check & print package updates (pacman, aur, flatpak)
arch-os [--kitty | -k] notify       # Check, print & notify package updates (pacman, aur, flatpak)
arch-os [--kitty | -k] search       # Search & manage package (pacman, aur)
arch-os [--kitty | -k] fetch        # Fetch package updates (pacman, aur, flatpak)
arch-os [--kitty | -k] upgrade      # System upgrade (pacman, aur, flatpak)
arch-os [--kitty | -k] orphans      # Remove orphaned packages (pacman, aur, flatpak)
arch-os [--kitty | -k] merge        # Merge updated configurations (using pacdiff and meld)
arch-os [--kitty | -k] refresh      # Refresh Pacman mirrorlist (using preconfigured reflector)
arch-os [--kitty | -k] downgrade    # Downgrade packages (pacman only)
arch-os [--kitty | -k] reset        # Reset Pacman (refresh pacman keyring)
arch-os [--kitty | -k] system       # System info (system & services logging)
```

## Settings

Edit settings with the build-in editor in Arch OS Manager or edit the config file manually: `~/.arch-os/arch-os.conf`

```
ARCH_AUR_REVIEW=false            # Disable AUR review (Enable: true)
ARCH_UPGRADE_CONFIRM=true        # Enable confirm upgrade system packages (Disable: false)
FLATPAK_SUPPORT=true             # Enable Flatpak Support (Disable: false)
FLATPAK_UPGRADE_CONFIRM=false    # Disable flatpak upgrade confirm (Enable: true)
NEWS_QUANTITY=3                  # Number of news to shown (Disable: 0)
NEWS_SHOW_URL=true               # Show 'More info url' below the news (Disable: false)
ORPHANS_CONFIRM=false            # Disable confirm remove orphans (Enable: true)
FILTER_RESULT_SIZE=20            # Max filter results
PACKAGE_INFO_WIDTH=80            # Max info width (cut horizontal output)
```

## Integrate GNOME Extension

1. Install GNOME Extension: **[archlinux-updates-indicator](https://extensions.gnome.org/extension/1010/)**
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
- https://github.com/Morganamilo/paru
- https://github.com/kovidgoyal/kitty
- https://github.com/charmbracelet/gum
- https://github.com/RaphaelRochet/arch-update
