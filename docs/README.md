<div align="center">
  <h1>
    <img src="./logo.svg" width="150" height="150">
    <p><b>Arch OS Manager</b></p>
  </h1>
</div>

<div align="center">
  <p><sub><b>powered by <a href="https://github.com/murkl/arch-os">Arch OS</a></b></sub></p>
  <p><img src="./screenshots/arch-os-manager.png"></p>
  <p><i>Streamlining software management and updates for enhanced usability in Arch Linux</i></p>
  <p>
    <img src="https://img.shields.io/badge/MAINTAINED-YES-green?style=for-the-badge">
    <img src="https://img.shields.io/badge/License-GPL_v2-blue?style=for-the-badge">
  </p>
  <p><sub><b>100% shellcheck approved</b></sub></p>
</div>

## Install from AUR

**[➜ AUR/arch-os-manager](https://aur.archlinux.org/packages/arch-os-manager)**

**Run Arch OS Manager:** `arch-os`

## Install from GitHub

1. **Install Dependencies:** `paru pacman-contrib kitty gum libnotify ttf-firacode-nerd`
2. **Clone GitHub Repo:** `git clone https://github.com/murkl/arch-os-manager.git && cd arch-os-manager`
3. **Run Arch OS Manager:** `./arch-os`

## Settings

Edit settings with the build-in editor in Arch OS Manager or edit the config file manually at `~/.arch-os/arch-os.conf`.

```
FLATPAK_SUPPORT_ENABLED=true            # Enable Flatpak Support (Disable: false)
FLATPAK_UPGRADE_CONFIRM_ENABLED=false   # Disable flatpak upgrade confirm (Enable: true)u
ORPHANS_CONFIRM_ENABLED=false           # Disable confirm remove orphans (Enable: true)
UPGRADE_CONFIRM_ENABLED=true            # Enable confirm upgrade system packages (Disable: false)
AUR_REVIEW_ENABLED=false                # Disable AUR review (Enable: true)
ARCH_NEWS_URL_ENABLED=true              # Show 'More info url' below the news (Disable: false)
ARCH_NEWS_COUNT=3                       # Number of news to shown (Disable: 0)
FILTER_RESULT_SIZE=20                   # Max filter results
PACKAGE_INFO_WIDTH=80                   # Max info width (cut horizontal output)
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

<img width="48%"  style="vertical-align: top;" src="./screenshots/main.png">
<img width="48%"  style="vertical-align: top;" src="./screenshots/search.png">
<img width="48%"  style="vertical-align: top;" src="./screenshots/fetch.png">
<img width="48%"  style="vertical-align: top;" src="./screenshots/upgrade.png">
<img width="48%"  style="vertical-align: top;" src="./screenshots/merge.png">
<img width="48%"  style="vertical-align: top;" src="./screenshots/refresh.png">
<img width="48%"  style="vertical-align: top;" src="./screenshots/downgrade.png">
<img width="48%"  style="vertical-align: top;" src="./screenshots/help.png">

</div>
