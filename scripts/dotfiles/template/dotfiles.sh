#!/bin/bash
# The functions of every profile, as in a PKGBUILD: one per group of dotfiles.yaml, and helpers for
# init.sh. Set up reads this file before each call, so outside the functions stand only
# definitions, and calls the function of each group switched on, again where it changed. So every
# function must be safe to run twice. One that ends with 75 runs again at the next set up.

# A group, as dotfiles.yaml names it with install: gaming
# gaming() {
#     sudo pacman -S --needed steam
# }
