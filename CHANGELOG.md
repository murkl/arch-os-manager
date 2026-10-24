# Changelog

## [2.2.0](https://github.com/murkl/arch-os-manager/compare/2.1.0...2.2.0) (2026-10-24)


### Features

* a clean-up of your choice, and a splash that says what it waits for ([#92](https://github.com/murkl/arch-os-manager/issues/92)) ([65ced7f](https://github.com/murkl/arch-os-manager/commit/65ced7f104cffb4a1a1c25599b42459b700144d0))
* back after a done action skips its page, marked packages are installed and removed in one run, and each repair says under it what it does ([#96](https://github.com/murkl/arch-os-manager/issues/96)) ([59893ff](https://github.com/murkl/arch-os-manager/commit/59893ff4d02c0e42dca984188324c2c45a16433a))
* failed services come first, with what they are, and the run terminal ends with its outcome ([#97](https://github.com/murkl/arch-os-manager/issues/97)) ([696ee9d](https://github.com/murkl/arch-os-manager/commit/696ee9dd361d8af4a838e697f03ac20ee6b7a139))
* input hints and polish ([#101](https://github.com/murkl/arch-os-manager/issues/101)) ([2576ad5](https://github.com/murkl/arch-os-manager/commit/2576ad5278cea8e998c113ec476819e0f07a8aac))
* runs in the terminal, every command shown and every question yours ([#98](https://github.com/murkl/arch-os-manager/issues/98)) ([5fd6204](https://github.com/murkl/arch-os-manager/commit/5fd62044fd71781485204f669398997deba0a550))
* the package brings what it needs on any Arch Linux, counts what is done, and closes the notification before the next ([#95](https://github.com/murkl/arch-os-manager/issues/95)) ([065ff71](https://github.com/murkl/arch-os-manager/commit/065ff7106a6ebbbad35acd4cd99867089a8e0cfc))


### Bug Fixes

* safe commands ([#102](https://github.com/murkl/arch-os-manager/issues/102)) ([08315f3](https://github.com/murkl/arch-os-manager/commit/08315f369185074c070fc31c1ae0438d244f997c))
* the update check waits for a running upgrade ([#100](https://github.com/murkl/arch-os-manager/issues/100)) ([d694a84](https://github.com/murkl/arch-os-manager/commit/d694a84b1f93a6f65937ebfb3073d8c4837eafba))
* the window and the timer keep each other's state, the clean-up leaves the kernels to paccache, and the check runs as arch-os's ([#94](https://github.com/murkl/arch-os-manager/issues/94)) ([dbb7aa6](https://github.com/murkl/arch-os-manager/commit/dbb7aa6beb9bf6a2155bb4ad78a8bd368a09f593))

## [2.1.0](https://github.com/murkl/arch-os-manager/compare/2.0.0...2.1.0) (2026-10-19)


### Features

* an update notification that replaces itself, and an interface that never waits on the disk ([#91](https://github.com/murkl/arch-os-manager/issues/91)) ([dd2bd26](https://github.com/murkl/arch-os-manager/commit/dd2bd2657c03f571d97b5ec3f44b97a2c497e76d))
* leave Flatpak to another app store, with fixes for the window, terminal keys, Rust 1.89 and snapshots removed during the clean-up ([#87](https://github.com/murkl/arch-os-manager/issues/87)) ([f4d3cd8](https://github.com/murkl/arch-os-manager/commit/f4d3cd8771e8f0a3e58a4a3fdd302cb166a73af2))
* show the spinner in every value still being read ([#83](https://github.com/murkl/arch-os-manager/issues/83)) ([0a8c774](https://github.com/murkl/arch-os-manager/commit/0a8c774469b4ae142adba584df0ce8d377b0663d))
* steadier runs and update checks, and one word for each key ([#85](https://github.com/murkl/arch-os-manager/issues/85)) ([4ba1568](https://github.com/murkl/arch-os-manager/commit/4ba15688efffd36141e76bf61c0e0b0dbccd7ee6))
* the terminal's own cursor in text boxes, typed rows and job results ([#81](https://github.com/murkl/arch-os-manager/issues/81)) ([527c654](https://github.com/murkl/arch-os-manager/commit/527c654bc7781ac1adde09dcec3e448633e88c09))


### Bug Fixes

* keep a throwaway run's GSettings out of dconf ([#82](https://github.com/murkl/arch-os-manager/issues/82)) ([6ce2185](https://github.com/murkl/arch-os-manager/commit/6ce21856e5b53163e1690e6af53065d2b2479c1d))
* outdate a foreign icon cache so the app icon shows ([69e9e9e](https://github.com/murkl/arch-os-manager/commit/69e9e9ee2b2731d1b9000c2791cde2320ce06556))
* show the app icon in the notification header, and match the repository description to the package ([#88](https://github.com/murkl/arch-os-manager/issues/88)) ([e1d0d61](https://github.com/murkl/arch-os-manager/commit/e1d0d61bb89abcdbd17d569661d209f530aaf876))

## [2.0.0](https://github.com/murkl/arch-os-manager/compare/1.9.1...2.0.0) (2026-10-17)


### Features

* migrate to a Rust runtime with YAML and shell scripts, and a pipeline ([#70](https://github.com/murkl/arch-os-manager/issues/70)) ([e4a8ee3](https://github.com/murkl/arch-os-manager/commit/e4a8ee3e44ed77cc29b3616f6eb01e98db0be7f8))
