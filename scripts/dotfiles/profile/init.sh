#!/bin/bash
# The commands of this machine, in functions as in a PKGBUILD. Set up reads this file, then calls
# init and the function of each group switched on, each again where it changed. So every function
# must be safe to run twice, and outside them stand only definitions. One that ends with 75 runs
# again at the next set up.

init() {
    # sudo pacman -S --needed zsh
    # sudo chsh -s /usr/bin/zsh "$USER"
    :
}

# A group, as dotfiles.yaml names it with install: gaming
# gaming() {
#     sudo pacman -S --needed steam
# }
