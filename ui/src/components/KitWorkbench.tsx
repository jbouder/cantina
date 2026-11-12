import { useEffect, useId, useState } from "react";
import {
  chooseSamplePath,
  drumDesignAudition,
  drumDesignKitOpen,
  drumDesignKitRecipe,
  drumDesignKitReset,
  drumDesignKitSave,
  drumDesignKitSet,
  drumDesignKitSwap,
  type EngineStatus,
  type KitEditStatus,
  type KitInfo,
  type KitPad,
  kitPads,
  type PadInfo,
} from "../lib/engine";
import { chokeGroups, recipeValues, withKitPad, withRecipePad } from "../lib/kitEdit";
import { Button } from "./Button";
import { Field } from "./Field";
import { Select } from "./Select";
import { Sheet } from "./Sheet";
import { TextInput } from "./TextInput";

type Props = {
  /** The kit the part plays, by id. */
  kitId: string;
  /** Every installed kit, to swap a pad from. */
  kits: KitInfo[];
  /** The pads the designer hears. */
  pads: PadInfo[];
  edit: KitEditStatus | null;
  onStatus: (s: EngineStatus) => void;
  onError: (message: string) => void;
};

/** A slider that changes a number as it moves and commits when let go. */
function Slider({
  id,
  label,
  hint,
  min,
  max,
  step,
  value,
  onCommit,
}: {
  id: string;
  label: string;
  hint: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onCommit: (v: number) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft);
  };
  return (
    <Field label={label} hint={hint} htmlFor={id}>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={draft}
        onChange={(e) => setDraft(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
        className="w-full"
      />
    </Field>
  );
}

/**
 * The kit side of the drum designer: every pad auditions on click, and
 * with the workbench open each pad's level, pan, tune, decay and choke
 * group change, its samples swap for a file or another kit's pad, and a
 * kit made from a recipe synthesizes again from changed settings. The
 * result is heard at once and saved as a kit of your own.
 */
export function KitWorkbench({ kitId, kits, pads, edit, onStatus, onError }: Props) {
  const ids = useId();
  const [chosen, setChosen] = useState<string>(pads[0]?.id ?? "");
  const [fromKit, setFromKit] = useState(kits.find((k) => k.id !== kitId)?.id ?? "");
  const [fromPads, setFromPads] = useState<PadInfo[]>([]);
  const [fromPad, setFromPad] = useState("");
  const [saving, setSaving] = useState(false);
  const [name, setName] = useState("");
  const report = (p: Promise<EngineStatus>) => p.then(onStatus, (e) => onError(String(e)));

  useEffect(() => {
    if (!fromKit) return;
    kitPads(fromKit).then(
      (list) => {
        setFromPads(list);
        setFromPad((p) => (list.some((x) => x.id === p) ? p : (list[0]?.id ?? "")));
      },
      (e) => onError(String(e)),
    );
  }, [fromKit, onError]);

  const kitName = edit?.kit.name ?? kits.find((k) => k.id === kitId)?.name ?? kitId;
  const pad: KitPad | undefined = edit?.kit.pads.find((p) => p.id === chosen);
  const recipePad = edit?.recipe?.pads.find((p) => p.id === chosen);
  const mine = kits.map((k) => k.name.toLowerCase());
  const clash = mine.includes(name.trim().toLowerCase());
  const setPad = (change: Partial<KitPad>) => {
    if (edit) report(drumDesignKitSet(withKitPad(edit.kit, chosen, change)));
  };

  return (
    <div className="grid content-start gap-3">
      <div className="flex items-center gap-2">
        <h3 className="caption m-0 min-w-0 flex-1 truncate">{kitName}</h3>
        {edit ? (
          <>
            <Button
              tier="quiet"
              disabled={!edit.changed}
              onClick={() => report(drumDesignKitReset())}
              title="Put the kit back as it was"
            >
              Reset
            </Button>
            <Button
              onClick={() => {
                setName(edit.kit.name);
                setSaving(true);
              }}
              title="Save the kit as one of yours, in Kits in your Cantina folder"
            >
              Save as kit…
            </Button>
          </>
        ) : (
          <Button
            onClick={() => report(drumDesignKitOpen())}
            title="Change this kit: a copy to tune and swap, heard as you go"
          >
            Change the kit
          </Button>
        )}
      </div>
      <p className="m-0 text-ink-dim text-label">
        Click a pad to hear it{edit ? " and change it" : ""}.
      </p>
      <ul className="m-0 grid list-none grid-cols-2 gap-1 p-0">
        {pads.map((p) => (
          <li key={p.id}>
            <Button
              aria-pressed={p.id === chosen}
              className="w-full"
              title={`Hear ${p.name}`}
              onClick={() => {
                setChosen(p.id);
                drumDesignAudition(p.id, 0.9).catch((e) => onError(String(e)));
              }}
            >
              <span className="min-w-0 truncate">{p.name}</span>
            </Button>
          </li>
        ))}
      </ul>

      {edit && pad ? (
        <>
          <h3 className="caption m-0">{pad.name}</h3>
          <Slider
            id={`${ids}-level`}
            label="Level"
            hint={`${(pad.gainDb ?? 0).toFixed(1)} dB`}
            min={-24}
            max={12}
            step={0.5}
            value={pad.gainDb ?? 0}
            onCommit={(gainDb) => setPad({ gainDb })}
          />
          <Slider
            id={`${ids}-pan`}
            label="Pan"
            hint={
              (pad.pan ?? 0) === 0
                ? "Center"
                : `${Math.round(Math.abs(pad.pan ?? 0) * 100)}% ${(pad.pan ?? 0) < 0 ? "left" : "right"}`
            }
            min={-1}
            max={1}
            step={0.05}
            value={pad.pan ?? 0}
            onCommit={(pan) => setPad({ pan })}
          />
          <Slider
            id={`${ids}-tune`}
            label="Tune"
            hint={`${(pad.tune ?? 0) > 0 ? "+" : ""}${pad.tune ?? 0} semitones`}
            min={-12}
            max={12}
            step={0.5}
            value={pad.tune ?? 0}
            onCommit={(tune) => setPad({ tune })}
          />
          <Slider
            id={`${ids}-decay`}
            label="Decay"
            hint={`${Math.round((pad.decay ?? 1) * 100)}% of each hit rings`}
            min={0.05}
            max={1}
            step={0.05}
            value={pad.decay ?? 1}
            onCommit={(decay) => setPad({ decay })}
          />
          <Select
            id={`${ids}-choke`}
            label="Choke group"
            hint="A hit cuts the others in its group"
            value={pad.choke ?? ""}
            onChange={(e) => setPad({ choke: e.target.value })}
          >
            <option value="">None</option>
            {[...new Set([...chokeGroups(edit.kit), "hat", "cymbal"])].map((g) => (
              <option key={g} value={g}>
                {g}
              </option>
            ))}
          </Select>

          <h3 className="caption m-0">Samples</h3>
          <p
            className="m-0 truncate text-ink-dim text-label"
            title={pad.layers.map((l) => l.file).join(", ")}
          >
            {pad.layers.length} layer{pad.layers.length === 1 ? "" : "s"}:{" "}
            {pad.layers.map((l) => l.file).join(", ")}
          </p>
          <Button
            onClick={() =>
              chooseSamplePath().then(
                (path) => {
                  if (path) report(drumDesignKitSwap(chosen, { from: "file", path }));
                },
                (e) => onError(String(e)),
              )
            }
            title={`Put a WAV file on ${pad.name}`}
          >
            From a file…
          </Button>
          <Select
            id={`${ids}-from-kit`}
            label="From a kit"
            value={fromKit}
            onChange={(e) => setFromKit(e.target.value)}
          >
            {kits.map((k) => (
              <option key={k.id} value={k.id}>
                {k.name}
              </option>
            ))}
          </Select>
          <span className="flex items-center gap-2">
            <span className="relative min-w-0 flex-1">
              <select
                aria-label="Its pad"
                className="surface-field min-h-8 w-full min-w-0 appearance-none py-0 pr-8 pl-2"
                value={fromPad}
                onChange={(e) => setFromPad(e.target.value)}
              >
                {fromPads.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
              <svg
                aria-hidden="true"
                viewBox="0 0 16 16"
                className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 fill-field-fg"
              >
                <path d="M2 5l6 6 6-6z" />
              </svg>
            </span>
            <Button
              disabled={!fromKit || !fromPad}
              onClick={() =>
                report(drumDesignKitSwap(chosen, { from: "kit", kit: fromKit, pad: fromPad }))
              }
              title={`Put that kit's pad, every layer of it, on ${pad.name}`}
            >
              Use
            </Button>
          </span>

          {edit.recipe && recipePad ? (
            <>
              <h3 className="caption m-0">Recipe</h3>
              <p className="m-0 text-ink-dim text-label">
                {kitName} is made from a recipe: change {pad.name}'s sound and synthesize it again.
              </p>
              {(
                [
                  ["pitch", "Pitch", 20, 2000, 1, (v: number) => `${Math.round(v)} Hz`],
                  ["decay", "Length", 0.01, 4, 0.01, (v: number) => `${v.toFixed(2)} s`],
                  ["tone", "Tone", 0, 1, 0.05, (v: number) => `${Math.round(v * 100)}% bright`],
                  ["noise", "Noise", 0, 1, 0.05, (v: number) => `${Math.round(v * 100)}%`],
                ] as const
              ).map(([field, label, min, max, step, say]) => {
                const value = recipeValues(recipePad)[field];
                return (
                  <Slider
                    key={field}
                    id={`${ids}-recipe-${field}`}
                    label={label}
                    hint={say(value)}
                    min={min}
                    max={max}
                    step={step}
                    value={value}
                    onCommit={(v) => {
                      const recipe = edit.recipe;
                      if (recipe) {
                        report(drumDesignKitRecipe(withRecipePad(recipe, chosen, { [field]: v })));
                      }
                    }}
                  />
                );
              })}
            </>
          ) : null}
        </>
      ) : null}

      {saving ? (
        <Sheet
          title="Save as kit"
          onClose={() => setSaving(false)}
          footer={
            <>
              <Button onClick={() => setSaving(false)}>Cancel</Button>
              <Button type="submit" form={`${ids}-save`} tier="primary" disabled={!name.trim()}>
                {clash ? "Replace" : "Save"}
              </Button>
            </>
          }
        >
          <form
            id={`${ids}-save`}
            className="grid gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (!name.trim()) return;
              drumDesignKitSave(name, clash).then(
                (s) => {
                  onStatus(s);
                  setSaving(false);
                },
                (e2) => onError(String(e2)),
              );
            }}
          >
            <TextInput
              id={`${ids}-name`}
              label="Name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
            <p className="m-0 text-ink-dim text-label">
              {clash
                ? `Replaces your kit called ${name.trim()}.`
                : "A kit folder in Kits in your Cantina folder: its kit.json, every sample, and the recipe when it has one. The part plays it from now on."}
            </p>
          </form>
        </Sheet>
      ) : null}
    </div>
  );
}
