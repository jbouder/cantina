# Fixtures

`di-guitar.wav` is four seconds of a plucked guitar as it leaves the
instrument, before any amp: what `cantina-cli render pedal --di` plays. It
is synthesised by `crates/cli/src/di.rs` (Karplus-Strong strings), not
recorded, and is dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). A test
regenerates it and fails if the file differs; rewrite it with
`UPDATE_DI=1 cargo test -p cantina-cli di`.
