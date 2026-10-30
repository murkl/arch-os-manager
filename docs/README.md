<div align="center">

<img src="banner.png" alt="System Manager">

<p><b>Update</b>, <b>maintain</b> and <b>repair</b> Arch Linux from one place.</p>

**[➜ Reference](REFERENCE.md)** · **[➜ Contributing](CONTRIBUTING.md)**

<p>
  <a href="https://aur.archlinux.org/packages/arch-os-manager"><img src="https://img.shields.io/aur/version/arch-os-manager?style=flat-square&label=aur&color=5e81ac" alt="AUR version"></a>
  <a href="https://github.com/murkl/arch-os-manager/releases/latest"><img src="https://img.shields.io/github/v/release/murkl/arch-os-manager?style=flat-square&label=release&color=5e81ac" alt="Latest release"></a>
  <a href="../LICENSE"><img src="https://img.shields.io/github/license/murkl/arch-os-manager?style=flat-square&color=5e81ac" alt="License"></a>
</p>

</div>

## Features

- Repository, AUR and Flatpak updates in one run
- Arch Linux news before every update
- Update notifications, count in the GNOME top bar
- Config updates compared, used or merged
- Mirror ranking, firmware updates
- Live CPU, RAM and disk usage
- Install, remove and downgrade packages, AUR included
- Foreign packages without source flagged
- System and user services
- Boot errors, hideable by kind
- [Clean-up](REFERENCE.md#clean-up), Docker and Podman included
- Keyring renewal, package file repair
- Known failures with their fix
- Next steps after every run
- Terminal, console or own window
- Compact or verbose output
- [Themes](REFERENCE.md#themes): Nord, Catppuccin, Tokyo Night, Gruvbox or custom
- English and German

[Optional packages](REFERENCE.md#optional-packages) add features, installed under **Settings › Optional packages**.

## Installation

From the **[AUR](https://aur.archlinux.org/packages/arch-os-manager)**, with paru or yay:

```sh
paru -S arch-os-manager
```

## First Start

```sh
system-manager
```

The first start adds **System Manager** to the app menu, which opens it in a window.

<p align="center"><img src="screenshots/home.png" width="100%" alt="The main menu: CPU, RAM and disk in use, then every task with its state"><br><sub><b>Main menu</b> · system state and tasks</sub></p>

<p align="center"><img src="screenshots/update.png" width="100%" alt="The update page: the news to read first, then what the update brings"><br><sub><b>Update</b> · news, then updates with versions</sub></p>
