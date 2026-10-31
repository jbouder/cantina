# Plugin hosting

Third-party plugins are Audio Units and CLAP plugins (VST3 is out for
good: its license would force the project's). Both end up as a
`Processor` in a chain, so the chain, the mixer and the project file never
know they are holding a plugin. The code is `crates/hosting`; this page
covers what M4 added on top of the M0 wrappers.

## Two threads, one plugin: the handle

The spike left one decision to M4: a `Processor` is a single object that
moves between the UI thread and the audio thread with the graph swap,
while plugin APIs split a plugin into a main-thread half and an
audio-thread half and expect the host to use both at once (save state,
show an editor, read parameters) while audio runs. The answer is the
second of the two options the spike offered: the engine keeps the
main-thread half on the UI thread and talks to the audio half through
the queues it already has.

Concretely, `Processor::handle()` returns a `cantina_dsp::Handle` once,
before the processor goes into a graph. Stock pedals return nothing: all
there is to them is their parameters, which the engine already tracks.
A plugin returns the UI-thread side of itself. The shell keeps it next to
the node id (`LivePedal` in `src-tauri/src/engine.rs`) for as long as the
pedal is in the chain, and uses it for:

- **state**: `state()` and `set_state()` while the processor runs;
- **latency**: `latency()` as it is now, since a plugin can change it
  after it was prepared;
- **changes the plugin made on its own**: `take_changes()` returns the
  parameters that moved since the last call, from an editor or a preset
  loaded inside the plugin. The shell polls every handle a few times a
  second from the meter pump and records each change as a `SetParam`
  edit (coalesced like a knob drag), so the generic knobs and undo
  follow whatever the plugin's own UI does;
- **the format's own features** through `as_any()`, which is how the
  editor windows reach the Audio Unit instance or the CLAP GUI
  extension.

Nothing on a handle is ever called from the audio thread. For an Audio
Unit the two halves share one `AudioComponentInstance` behind an `Arc`
(property and parameter calls on an AUv2 instance are safe from another
thread while it renders, which is how every host works), and the
instance is disposed when both halves are gone. The processor's own
`state()` and `set_state()` still exist for a processor that is not in a
graph, which is how a project loads: the state goes in before the node
is added.

## Audio Units

`AudioUnitProcessor` instantiates the component for a fixed channel
count, reads the parameter list (name, range, unit, default) so the UI
can show knobs before audio runs, and configures stream formats and the
render callback in `prepare`. `process` copies the input aside, lets the
unit pull it through the callback and renders over the buffer. A unit
that fails to configure or render bypasses and counts the failure; it
never stops the stream.

State is the unit's `kAudioUnitProperty_ClassInfo`, the property list
every host saves as a preset, serialized as a binary plist. Restoring it
sets the property back and notifies parameter listeners, so an editor
view redraws. Latency is `kAudioUnitProperty_Latency` times the output
rate. `AudioUnitHandle::take_changes` compares every parameter against
the value it last reported, which is cheap (a few dozen property reads)
and needs no listener on a run loop.

Apple's AUDelay, which ships with every Mac, is the test subject: an
impulse comes out at the delay time, the plist state round-trips into a
fresh instance, and the handle sees a parameter change made behind its
back while the processor is prepared and rendering.

## State in the project

A plugin's state lives in the project as `plugins/<pedal id>.state`: a
four-byte magic, the pedal's spec, then the plugin's own bytes. The spec
is there so a file left behind by a removed pedal can never land in a
different plugin that later gets the same id. The shell reads every
state file when a project opens, writes them all on Save (after asking
each running plugin for its current state), and refreshes them from the
running plugins whenever the stream closes. A stream rebuild, after a
sample rate or buffer size change or an interface going away, drops the
whole graph and builds it again from the project, so each plugin is
created anew, given its state, then the project's parameter values on
top (those are what undo tracks, so they win), and only then added to
the graph. Files for pedals that are gone stay, like audio files for
deleted clips, so undoing the removal finds the state again.

The project's parameter values and the plugin's state overlap: both hold
the parameters. The polling above keeps the values current, so a save
stores a state and a value list that agree, and a load that applies the
values over the state changes nothing.

## The scanner

Listing what is installed is cheap and runs no plugin code: Audio Units
come from the component registry, CLAP bundles from
`~/Library/Audio/Plug-Ins/CLAP` and `/Library/Audio/Plug-Ins/CLAP`,
searched recursively. Learning anything more (the channel layout, whether
there is an editor, which plugins a CLAP bundle holds) means loading and
instantiating the plugin, and some plugin always crashes doing that. So
the scanner does it in a child process.

The child is the Cantina binary itself, run with `--scan-plugins`, so
nothing extra has to be bundled. The parent (`scan::Scanner`) writes one
JSON **source** per line to the child's stdin: an Audio Unit component
(`type:subtype:manufacturer`, name, version) or a CLAP bundle (path, newest
modification time inside it, total size). The child prints one line per
event, each prefixed with `@cantina ` so a plugin that writes to stdout
while loading cannot be mistaken for a result:

```
@cantina {"begin":"clap:/Library/Audio/Plug-Ins/CLAP/Thing.clap"}
@cantina {"examining":"Thing"}
@cantina {"plugin":{"format":"clap","spec":"clap:/…/Thing.clap#com.vendor.thing","name":"Thing",…}}
@cantina {"end":"clap:/Library/Audio/Plug-Ins/CLAP/Thing.clap"}
```

Each plugin is reported the moment it is described. When the child dies,
the parent knows which source it was in (the `begin` without an `end`)
and which plugin it was instantiating (the last `examining`), records
that as a **failure** with the exit status, and starts a new child on the
rest. What the dead child reported before it died is kept. A child that
prints nothing for thirty seconds is killed and its source recorded the
same way. The hosting crate's tests exercise both: the CLAP test fixture
carries a second plugin that aborts the process when instantiated, and a
`sleep` stands in for a hung child.

### The cache and rescan

The result is a `PluginList`, saved as `plugins.json` in the app's data
folder (`~/Library/Application Support/com.jbouder.cantina/`). Every entry
carries the spec the engine builds it from (`au:aufx:dely:appl`,
`clap:<bundle path>#<plugin id>`), name, vendor, version, main input and
output channel counts, and whether it ships an editor. The list also
remembers every source with the fingerprint it had. A rescan lists the
sources again and hands the child only the ones whose fingerprint
changed or that are new; unchanged sources keep their cached entries and
failures, and sources that went away are dropped. A full rescan hands it
everything. The first launch, with no cache, scans when the UI first asks
for the list.

In the app: the **Plugins…** button on a track opens the browser, grouped
by format and vendor, with Add, Rescan and Full rescan, and the failed
plugins listed underneath with why.
