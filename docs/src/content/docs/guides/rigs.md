---
title: "Rigs"
description: "Rigs are ready-made chains for a style: the stock rigs for guitar and bass, what each is for, and how to write your own."
---

Most of the time you want "a metalcore tone" before you want to know what
a compressor's attack does. A rig is that: a chain of pedals with their
settings for a style, with a name, a genre and a line on what it sounds
like. Cantina ships a rig for each common style, for guitar and for bass,
built from its own stock pedals and cabinets, so every one works on a
fresh install.

## The stock rigs

Every stock rig is leveled against the same DI, so switching from one to
another does not jump in volume: they sit within 6 dB of each other, the
clean ones a little quieter so their peaks have room.

### Guitar

| Rig | Genre | What it sounds like | Chain |
| --- | --- | --- | --- |
| Metalcore | Metal | Tight, gated high gain with a boosted front end and a scooped mid for chugs and breakdowns. | Gate, Overdrive, Amp, 4x12, Reverb |
| Heavy Metal | Metal | Classic heavy metal: a hot distortion into a cranked amp, mids intact, a touch of delay for leads. | Gate, Distortion, Amp, 4x12, Delay, Reverb |
| Thrash | Metal | Fast and dry: deep scooped mids, a tight gate and nothing hanging off the end. | Gate, Overdrive, Amp, 4x12 |
| Djent | Metal | Percussive extended-range gain: low end cut before the amp, mids forward and a sharp gate. | Gate, Overdrive, EQ, Amp, 4x12, Reverb |
| Hard Rock | Rock | An overdrive pushing a crunchy amp: big chords and singing leads. | Overdrive, Amp, 4x12, Reverb |
| Classic Rock | Rock | An amp on the edge of breakup, mids up, through a closed two-twelve in a warm room. | Amp, 2x12, Reverb |
| Alternative | Alternative | A gritty distortion into a mid-gain amp with a little chorus for width. | Distortion, Amp, 2x12, Chorus, Reverb |
| Grunge | Alternative | A dark, wooly fuzz into a dirty amp: loud quiet loud. | Fuzz, Amp, 4x12, Reverb |
| Shoegaze | Alternative | A wall of fuzz washed in chorus, long delays and an enormous reverb. | Fuzz, Amp, 2x12, Chorus, Delay, Reverb |
| Punk | Punk | Bright, aggressive distortion with the mids left in: three chords and the truth. | Distortion, Amp, 2x12 |
| Blues | Blues | A light overdrive into a small combo just breaking up, with a spring-sized reverb. | Compressor, Overdrive, Amp, 1x12, Reverb |
| Funk Clean | Funk | Squashed and sparkly: a fast compressor into a clean amp, mids scooped for scratches. | Compressor, Amp, 1x12, Reverb |
| Jazz Clean | Jazz | Warm and round: a clean amp with the treble rolled off and a soft room. | Compressor, Amp, 1x12, Reverb |
| Country | Country | Snappy chicken pickin': hard compression, a bright clean amp and a slapback echo. | Compressor, Amp, 1x12, Delay, Reverb |
| Ambient | Ambient | Clean swells into long ping-pong delays and a huge, dark reverb. | Compressor, Amp, 2x12, Chorus, Delay, Reverb |

### Bass

| Rig | Genre | What it sounds like | Chain |
| --- | --- | --- | --- |
| Bass Clean | Clean | Round and even: gentle compression into a clean bass amp and a four-ten. | Compressor, Amp, 4x10 |
| Bass Rock | Rock | Growl with the mids pushed so the bass cuts through loud guitars. | Compressor, Amp, 4x10 |
| Bass Metal | Metal | A tight gate, a hard distortion and a clanky top end that locks with the kick. | Gate, Compressor, Distortion, Amp, 4x10 |
| Bass Funk | Funk | Slap-ready: heavy compression, deep lows, scooped mids and a bright top. | Compressor, Amp, 4x10 |

The cabinets are Cantina's synthesised [starter impulse
responses](/guides/pedals/); a rig sounds more like a real amp in a room
through a captured one. Drop your own into `~/Music/Cantina/IRs` with the
same name to replace a stock one everywhere.

## Writing your own

A rig is a JSON file, `<id>.rig.json`, described on the [rig
format](/reference/rig-format/) page: the pedals in order, each with its
parameters by name. Check it and hear it by the numbers with
[`cantina-cli`](/reference/cli/):

```sh
cantina-cli check my-tone.rig.json
cantina-cli render rig my-tone.rig.json      # through the DI of its instrument
```

`render rig` plays a guitar rig through the guitar DI and a bass rig
through the bass DI, and prints the peak, the RMS level, the DC offset and
the chain's latency. Set the amp's `level` so the RMS sits near -19 dBFS
with the peak under -1.5 dBFS, as the stock rigs do, and your rig will
sit with them. A coding agent with the [authoring
skill](/guides/agents/) knows the format and does this loop for you.
