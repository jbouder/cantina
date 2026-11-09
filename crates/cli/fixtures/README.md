# Fixtures

`di-guitar.wav` is four seconds of a plucked guitar as it leaves the
instrument, before any amp: what `cantina-cli render pedal --di` plays.
`di-bass.wav` is four seconds of a bass line the same way: what a bass rig
renders through. Both are synthesised by `crates/cli/src/di.rs`
(Karplus-Strong strings), not recorded, and are dedicated to the public
domain under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
A test regenerates them and fails if a file differs; rewrite them with
`UPDATE_DI=1 cargo test -p cantina-cli di`.
