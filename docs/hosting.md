# Plugin hosting

Third-party plugins are Audio Units and CLAP plugins (VST3 is out for
good: its license would force the project's). Both end up as a
`Processor` in a chain, so the chain, the mixer and the project file never
know they are holding a plugin. The code is `crates/hosting`; this page
covers what M4 added on top of the M0 wrappers.

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
