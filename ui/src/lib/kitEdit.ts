// The kit workbench's edits, pure: a pad's settings in a kit, a pad's
// synthesis in a recipe, and what each voice starts from.

import type { KitFile, KitPad, KitRecipe, PadRecipe } from "./engine";

/** The kit with pad `id` changed by `change`. */
export function withKitPad(kit: KitFile, id: string, change: Partial<KitPad>): KitFile {
  return {
    ...kit,
    pads: kit.pads.map((p) => {
      if (p.id !== id) return p;
      const next: KitPad = { ...p, ...change };
      // Defaults stay out of the file, as the shell writes it.
      if (next.choke !== undefined && !next.choke.trim()) delete next.choke;
      if (next.tune === 0) delete next.tune;
      if (next.decay === 1) delete next.decay;
      if (next.gainDb === 0) delete next.gainDb;
      if (next.pan === 0) delete next.pan;
      return next;
    }),
  };
}

/** The recipe with pad `id`'s synthesis changed by `change`. */
export function withRecipePad(
  recipe: KitRecipe,
  id: string,
  change: Partial<PadRecipe>,
): KitRecipe {
  return { ...recipe, pads: recipe.pads.map((p) => (p.id === id ? { ...p, ...change } : p)) };
}

/** What a voice's pitch, decay, tone and noise start from. Mirrors `Voice::defaults`. */
export const VOICE_DEFAULTS: Record<
  PadRecipe["voice"],
  { pitch: number; decay: number; tone: number; noise: number }
> = {
  kick: { pitch: 55, decay: 0.5, tone: 0.5, noise: 0.1 },
  snare: { pitch: 190, decay: 0.25, tone: 0.6, noise: 0.6 },
  hat: { pitch: 400, decay: 0.08, tone: 0.7, noise: 0.5 },
  clap: { pitch: 1200, decay: 0.3, tone: 0.5, noise: 1 },
  tom: { pitch: 120, decay: 0.4, tone: 0.5, noise: 0.15 },
  cowbell: { pitch: 560, decay: 0.3, tone: 0.6, noise: 0 },
};

/** A pad's synthesis as it sounds: its own numbers, else its voice's. */
export function recipeValues(p: PadRecipe): {
  pitch: number;
  decay: number;
  tone: number;
  noise: number;
} {
  const d = VOICE_DEFAULTS[p.voice];
  return {
    pitch: p.pitch ?? d.pitch,
    decay: p.decay ?? d.decay,
    tone: p.tone ?? d.tone,
    noise: p.noise ?? d.noise,
  };
}

/** The choke groups a kit has, for the picker. */
export function chokeGroups(kit: KitFile): string[] {
  return [...new Set(kit.pads.flatMap((p) => (p.choke ? [p.choke] : [])))].sort();
}
