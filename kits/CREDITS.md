# Stock kit credits

Every sample in these kits is in the public domain under the
[Creative Commons Zero](https://creativecommons.org/publicdomain/zero/1.0/)
dedication. They were recorded or synthesized by their authors on
[Freesound](https://freesound.org), trimmed and collected by the
[Sonic Pi](https://github.com/sonic-pi-net/sonic-pi) project in
`etc/samples` (whose README records the dedication and the links below),
and converted here from FLAC to 16-bit WAV at their original 44.1 kHz with
no other change. Thank you to menegass, looppool, Zajo and Neotone, and to
Sam Aaron for gathering them.

The kit definitions (`kit.json`: pad names, velocity layers, gains, pans
and choke groups) are Cantina's own, under its MIT license.

## Acoustic (`kits/acoustic`)

| File | Sonic Pi sample | Source |
| --- | --- | --- |
| `kick-soft.wav` | `drum_bass_soft` | <https://www.freesound.org/people/menegass/sounds/100052/> |
| `kick-hard.wav` | `drum_bass_hard` | <https://www.freesound.org/people/menegass/sounds/100051/> |
| `snare-soft.wav` | `drum_snare_soft` | <https://www.freesound.org/people/menegass/sounds/100059/> |
| `snare-hard.wav` | `drum_snare_hard` | <https://www.freesound.org/people/menegass/sounds/100058/> |
| `hh-closed.wav` | `drum_cymbal_closed` | <https://www.freesound.org/people/menegass/sounds/100053/> |
| `hh-pedal.wav` | `drum_cymbal_pedal` | <https://www.freesound.org/people/menegass/sounds/100054/> |
| `hh-open.wav` | `drum_cymbal_open` | <https://www.freesound.org/people/menegass/sounds/100055/> |
| `tom-hi-soft.wav` | `drum_tom_hi_soft` | <https://www.freesound.org/people/menegass/sounds/100063/> |
| `tom-hi-hard.wav` | `drum_tom_hi_hard` | <https://www.freesound.org/people/menegass/sounds/100062/> |
| `tom-mid-soft.wav` | `drum_tom_mid_soft` | <https://www.freesound.org/people/menegass/sounds/100067/> |
| `tom-mid-hard.wav` | `drum_tom_mid_hard` | <https://www.freesound.org/people/menegass/sounds/100066/> |
| `tom-lo-soft.wav` | `drum_tom_lo_soft` | <https://www.freesound.org/people/menegass/sounds/100065/> |
| `tom-lo-hard.wav` | `drum_tom_lo_hard` | <https://www.freesound.org/people/menegass/sounds/100064/> |
| `crash-soft.wav` | `drum_splash_soft` | <https://www.freesound.org/people/menegass/sounds/100061/> |
| `crash-hard.wav` | `drum_splash_hard` | <https://www.freesound.org/people/menegass/sounds/100060/> |
| `ride-soft.wav` | `drum_cymbal_soft` | <https://www.freesound.org/people/menegass/sounds/100057/> |
| `ride-hard.wav` | `drum_cymbal_hard` | <https://www.freesound.org/people/menegass/sounds/100056/> |

## Tight rock (`kits/rock`)

| File | Sonic Pi sample | Source |
| --- | --- | --- |
| `kick.wav` | `drum_heavy_kick` | <https://freesound.org/people/Zajo/sounds/4832/> |
| `snare.wav` | `drum_snare_hard` | <https://www.freesound.org/people/menegass/sounds/100058/> |
| `hh-closed.wav` | `drum_cymbal_closed` | <https://www.freesound.org/people/menegass/sounds/100053/> |
| `hh-open.wav` | `drum_cymbal_pedal` | <https://www.freesound.org/people/menegass/sounds/100054/> |
| `tom-hi.wav` | `drum_tom_hi_hard` | <https://www.freesound.org/people/menegass/sounds/100062/> |
| `tom-lo.wav` | `drum_tom_lo_hard` | <https://www.freesound.org/people/menegass/sounds/100064/> |
| `crash.wav` | `drum_splash_hard` | <https://www.freesound.org/people/menegass/sounds/100060/> |
| `ride.wav` | `drum_cymbal_hard` | <https://www.freesound.org/people/menegass/sounds/100056/> |
| `cowbell.wav` | `drum_cowbell` | <https://freesound.org/people/Neotone/sounds/75338/> |

## Electronic (`kits/electronic`)

| File | Sonic Pi sample | Source |
| --- | --- | --- |
| `kick.wav` | `elec_hollow_kick` | <https://www.freesound.org/people/looppool/sounds/13099/> |
| `kick-2.wav` | `elec_soft_kick` | <https://www.freesound.org/people/looppool/sounds/13142/> |
| `snare.wav` | `elec_snare` | <https://www.freesound.org/people/looppool/sounds/13146/> |
| `snare-2.wav` | `elec_mid_snare` | <https://www.freesound.org/people/looppool/sounds/13132/> |
| `clap.wav` | `elec_hi_snare` | <https://www.freesound.org/people/looppool/sounds/13125/> |
| `hh-closed.wav` | `elec_tick` | <https://www.freesound.org/people/looppool/sounds/13113/> |
| `hh-open.wav` | `elec_cymbal` | <https://www.freesound.org/people/looppool/sounds/13143/> |
| `tom-lo.wav` | `elec_fuzz_tom` | <https://www.freesound.org/people/looppool/sounds/13140/> |
| `tom-hi.wav` | `elec_lo_snare` | <https://www.freesound.org/people/looppool/sounds/13145/> |
| `perc.wav` | `elec_pop` | <https://www.freesound.org/people/looppool/sounds/13134/> |
| `blip.wav` | `elec_blip` | <https://www.freesound.org/people/looppool/sounds/13121/> |
| `bell.wav` | `elec_bell` | <https://www.freesound.org/people/looppool/sounds/13116/> |
| `wood.wav` | `elec_wood` | <https://www.freesound.org/people/looppool/sounds/13135/> |

## Metal (`kits/metal`)

No third-party audio: every sample is synthesised by Cantina from
[`recipes/metal.recipe.json`](../recipes/metal.recipe.json) with
`cantina-cli synth-kit recipes/metal.recipe.json kits/metal`, and is under
Cantina's MIT license. Regenerate the folder that way after changing the
recipe; a test fails if the two disagree.

| File | Recipe pad |
| --- | --- |
| `china.wav` | `china` |
| `crash-1.wav` | `crash` |
| `crash-2.wav` | `crash` |
| `hh-closed-1.wav` | `hh-closed` |
| `hh-closed-2.wav` | `hh-closed` |
| `hh-open-1.wav` | `hh-open` |
| `hh-open-2.wav` | `hh-open` |
| `kick-1.wav` | `kick` |
| `kick-2.wav` | `kick` |
| `kick-3.wav` | `kick` |
| `ride-1.wav` | `ride` |
| `ride-2.wav` | `ride` |
| `snare-1.wav` | `snare` |
| `snare-2.wav` | `snare` |
| `snare-3.wav` | `snare` |
| `tom-hi-1.wav` | `tom-hi` |
| `tom-hi-2.wav` | `tom-hi` |
| `tom-lo-1.wav` | `tom-lo` |
| `tom-lo-2.wav` | `tom-lo` |
| `tom-mid-1.wav` | `tom-mid` |
| `tom-mid-2.wav` | `tom-mid` |

## Jazz (brushes) (`kits/jazz`)

No third-party audio: every sample is synthesized by Cantina from
[`recipes/jazz.recipe.json`](../recipes/jazz.recipe.json) with
`cantina-cli synth-kit recipes/jazz.recipe.json kits/jazz`, and is under
Cantina's MIT license. Regenerate the folder that way after changing the
recipe; a test fails if the two disagree.

| File | Recipe pad |
| --- | --- |
| `china.wav` | `china` |
| `crash-1.wav` | `crash` |
| `crash-2.wav` | `crash` |
| `hh-closed-1.wav` | `hh-closed` |
| `hh-closed-2.wav` | `hh-closed` |
| `hh-open-1.wav` | `hh-open` |
| `hh-open-2.wav` | `hh-open` |
| `hh-pedal.wav` | `hh-pedal` |
| `kick-1.wav` | `kick` |
| `kick-2.wav` | `kick` |
| `kick-3.wav` | `kick` |
| `ride-1.wav` | `ride` |
| `ride-2.wav` | `ride` |
| `ride-3.wav` | `ride` |
| `snare-1.wav` | `snare` |
| `snare-2.wav` | `snare` |
| `snare-3.wav` | `snare` |
| `tom-hi-1.wav` | `tom-hi` |
| `tom-hi-2.wav` | `tom-hi` |
| `tom-lo-1.wav` | `tom-lo` |
| `tom-lo-2.wav` | `tom-lo` |
| `tom-mid-1.wav` | `tom-mid` |
| `tom-mid-2.wav` | `tom-mid` |

## Vintage 60s/70s (`kits/vintage`)

No third-party audio: every sample is synthesized by Cantina from
[`recipes/vintage.recipe.json`](../recipes/vintage.recipe.json) with
`cantina-cli synth-kit recipes/vintage.recipe.json kits/vintage`, and is under
Cantina's MIT license. Regenerate the folder that way after changing the
recipe; a test fails if the two disagree.

| File | Recipe pad |
| --- | --- |
| `china.wav` | `china` |
| `crash-1.wav` | `crash` |
| `crash-2.wav` | `crash` |
| `hh-closed-1.wav` | `hh-closed` |
| `hh-closed-2.wav` | `hh-closed` |
| `hh-open-1.wav` | `hh-open` |
| `hh-open-2.wav` | `hh-open` |
| `hh-pedal.wav` | `hh-pedal` |
| `kick-1.wav` | `kick` |
| `kick-2.wav` | `kick` |
| `kick-3.wav` | `kick` |
| `ride-1.wav` | `ride` |
| `ride-2.wav` | `ride` |
| `snare-1.wav` | `snare` |
| `snare-2.wav` | `snare` |
| `snare-3.wav` | `snare` |
| `tom-hi-1.wav` | `tom-hi` |
| `tom-hi-2.wav` | `tom-hi` |
| `tom-lo-1.wav` | `tom-lo` |
| `tom-lo-2.wav` | `tom-lo` |
| `tom-mid-1.wav` | `tom-mid` |
| `tom-mid-2.wav` | `tom-mid` |

## Hip-hop 808 (`kits/hiphop-808`)

No third-party audio: every sample is synthesized by Cantina from
[`recipes/hiphop-808.recipe.json`](../recipes/hiphop-808.recipe.json) with
`cantina-cli synth-kit recipes/hiphop-808.recipe.json kits/hiphop-808`, and is under
Cantina's MIT license. Regenerate the folder that way after changing the
recipe; a test fails if the two disagree.

| File | Recipe pad |
| --- | --- |
| `china.wav` | `china` |
| `clap-1.wav` | `clap` |
| `clap-2.wav` | `clap` |
| `cowbell.wav` | `cowbell` |
| `crash-1.wav` | `crash` |
| `crash-2.wav` | `crash` |
| `hh-closed-1.wav` | `hh-closed` |
| `hh-closed-2.wav` | `hh-closed` |
| `hh-open-1.wav` | `hh-open` |
| `hh-open-2.wav` | `hh-open` |
| `hh-pedal.wav` | `hh-pedal` |
| `kick-1.wav` | `kick` |
| `kick-2.wav` | `kick` |
| `ride-1.wav` | `ride` |
| `ride-2.wav` | `ride` |
| `snare-1.wav` | `snare` |
| `snare-2.wav` | `snare` |
| `tom-hi-1.wav` | `tom-hi` |
| `tom-hi-2.wav` | `tom-hi` |
| `tom-lo-1.wav` | `tom-lo` |
| `tom-lo-2.wav` | `tom-lo` |
| `tom-mid-1.wav` | `tom-mid` |
| `tom-mid-2.wav` | `tom-mid` |

## Metal (triggered) (`kits/metal-triggered`)

No third-party audio: every sample is synthesized by Cantina from
[`recipes/metal-triggered.recipe.json`](../recipes/metal-triggered.recipe.json) with
`cantina-cli synth-kit recipes/metal-triggered.recipe.json kits/metal-triggered`, and is under
Cantina's MIT license. Regenerate the folder that way after changing the
recipe; a test fails if the two disagree.

| File | Recipe pad |
| --- | --- |
| `china.wav` | `china` |
| `crash-1.wav` | `crash` |
| `crash-2.wav` | `crash` |
| `hh-closed-1.wav` | `hh-closed` |
| `hh-closed-2.wav` | `hh-closed` |
| `hh-open-1.wav` | `hh-open` |
| `hh-open-2.wav` | `hh-open` |
| `hh-pedal.wav` | `hh-pedal` |
| `kick.wav` | `kick` |
| `ride-1.wav` | `ride` |
| `ride-2.wav` | `ride` |
| `snare-1.wav` | `snare` |
| `snare-2.wav` | `snare` |
| `snare-3.wav` | `snare` |
| `tom-hi-1.wav` | `tom-hi` |
| `tom-hi-2.wav` | `tom-hi` |
| `tom-lo-1.wav` | `tom-lo` |
| `tom-lo-2.wav` | `tom-lo` |
| `tom-mid-1.wav` | `tom-mid` |
| `tom-mid-2.wav` | `tom-mid` |
