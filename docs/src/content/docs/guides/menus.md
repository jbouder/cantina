---
title: "Menus"
description: "What each item in Cantina's menu bar does, with its shortcut."
sidebar:
  order: 4
---

Everything in Cantina's menu bar is also somewhere in the window; the menus
are where a Mac app's commands are expected to be, with their shortcuts
beside them. An item that cannot apply right now, such as **Save** with
nothing to save, is greyed out.

## Cantina

| Item | Shortcut | What it does |
| --- | --- | --- |
| About Cantina | | The version, the license and the credits for the drum samples and impulse responses. |
| Check for Updates… | | Opens Settings and asks for a newer release. If there is one, a banner offers to install it. |
| Settings… | ⌘, | Audio device, buffer size, the latency warning, skins, motion and updates. |
| Hide, Hide Others, Show All, Quit | ⌘H, ⌥⌘H, ⌘Q | As in every Mac app. |

## File

| Item | Shortcut | What it does |
| --- | --- | --- |
| New Project… | ⌘N | Opens **Projects** with the name of a new project ready to type. |
| Open… | ⌘O | Opens **Projects** to pick another project. |
| Open Recent | | The last ten projects you had open, newest first. **Clear Menu** empties it. With unsaved edits, Cantina asks before dropping them. |
| Close Window | ⌘W | Closes the window. |
| Save | ⌘S | Writes the project to its folder. |
| Duplicate… | ⇧⌘S | Copies the project as it is now, unsaved edits and all, to a new name, and switches to the copy. The original stays as it was last saved. Its unsaved edits stay recoverable: it offers them back when you open it. |
| Rename… | | Saves the project and renames its folder. Undo history carries on. |
| Import Audio… | ⇧⌘I | Copies WAV files into the project and puts them on the selected audio track at the playhead, one after another, or on a new track when a drum track is selected. A file at another sample rate is converted in the copy, and one with more than two channels is folded to stereo. The original file is never changed. Undo takes each file back off. |
| Import Pedal… | | Adds a `.pedal.json` to your pedals, and to the selected track's chain. |
| Import Pattern… | | Adds a `.pattern.json` to the project, onto the selected drum track. |
| Import Kit… | | Copies a kit folder (one with a `kit.json`) into `~/Music/Cantina/Kits`, after checking it loads, so drum tracks can pick it. |
| Export Mixdown… | ⌘E | The [export sheet](/guides/mixdown/#export-the-mix). |
| Bounce Track in Place | | [Bounces](/guides/mixdown/#bounce-a-track-in-place) the selected track, as **Bounce** in its editor does. |
| Show Project in Finder | | Opens a Finder window with the project's folder selected. |
| Show Library in Finder | | Opens `~/Music/Cantina`, which holds your projects and the Pedals, Patterns, Kits and IRs folders. |

## Edit

| Item | Shortcut | What it does |
| --- | --- | --- |
| Undo, Redo | ⌘Z, ⇧⌘Z | Undo and redo the last edit to the project. In a text field they undo your typing instead. |
| Cut, Copy, Paste, Select All | ⌘X, ⌘C, ⌘V, ⌘A | In text fields. |

## Transport

| Item | Key | What it does |
| --- | --- | --- |
| Play / Stop | Space | Plays from the playhead, or stops. |
| Record | R | Counts in and records the armed tracks. See [recording](/guides/recording/). |
| Metronome | K | Turns the click on and off; checked while it is on. |

The keys for these items have no ⌘, so they do nothing while you are typing
in a text field. The menu shows them without a shortcut for that reason.

## View and Window

| Item | Key | What it does |
| --- | --- | --- |
| Performance Mode | P | The [stage](/guides/stage/) full screen. |
| Enter Full Screen | ⌃⌘F | The window full screen. |
| Minimize, Zoom, Bring All to Front | ⌘M | As in every Mac app. |
